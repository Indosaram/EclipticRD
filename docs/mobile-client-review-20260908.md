# 모바일 → 데스크톱 접속 코드리뷰 및 개선안

작성일: 2026-09-08

## 결론

현재 코드는 **iOS/Android에서 데스크톱에 접속하는 완성된 앱이 아니다.**
`maho-mobile`은 Rust 라이브러리와 정책·입력 변환 로직이다. 리뷰 시작 시
연결·영상·오디오 경로가 실제 처리 없이 성공을 반환했으며, 이번 수정으로
해당 허위 성공을 명시적 미지원 오류 또는 출력량 0으로 바꿨다.
실제 세션·네이티브 미디어 연결과 APK/IPA 앱 프로젝트는 여전히 없다.
기존 데스크톱 클라이언트의 동작이나 Linux에서의
모바일 라이브러리 테스트 통과를 모바일 제품 완성의 근거로 사용할 수 없다.

Android APK 및 iOS 앱으로 완성하여 개인 휴대폰에 설치하는 것은 기술적으로
가능하다. 그러나 현재 트리를 그대로 빌드해서 원격 데스크톱 앱으로 사용하는
것은 불가능하며, 아래의 세션 연결·미디어·앱 패키징 작업이 먼저 필요하다.

## 검토 범위와 제한

- 검토 대상은 작업 시작 시점의 미커밋 변경을 포함한
  `clients/rust/maho-mobile`, 관련 Rust 크레이트와 Tauri 설정이다.
- 기존 미커밋 변경은 보존한다. 이 작업은 커밋·배포 승인이 아니다.
- 최신 사용자 지시: **코드 수준에서만 검증한다. 실기기·에뮬레이터는 별도
  승인 전 사용하지 않는다.** 설치, 실행, 입력 조작, 화면 캡처를 검증에
  포함하지 않는다.
- 기존 제약에 따라 `cargo check/test/build`를 포함하는 컴파일은
  Omarchy Linux에서만 수행한다. iOS 네이티브 빌드는 macOS/Xcode가 필요하므로
  실제 수행하려면 이 빌드 위치 제약에 대한 별도 예외 승인이 필요하다.
- 아래 줄 번호는 수정 전 리뷰 기준이다. 수정 결과 및 테스트 증거는 문서
  끝의 검증 결과에 별도로 기록한다.

## 심각도별 발견 사항

### F1 · 치명적: 실제 연결·입력 전송 없이 성공 반환

근거: `clients/rust/maho-mobile/src/bridge.rs:21-89`.

`maho_mobile_create`는 호스트 문자열과 포트를 구조체에 넣고
`is_connected: true`를 설정한다. TLS 연결, 페어링, handshake, UDP 세션 생성은
없다. `maho_mobile_send_touch`는 `_event`를 만들어 버리고 `Ok`를 반환한다.
호스트가 존재하지 않아도 테스트가 성공하는 이유다. 테스트의 포트 `19735`도
실제 스트리밍 TCP 기본 포트 `19730`이 아니라 에이전트 HTTP API 포트다.

개선:

1. 현재 단계에서는 핸들 생성과 네트워크 연결 성공을 구별하고, 지원되지 않는
   전송은 명시적으로 실패시킨다. 생성 실패 시 출력 핸들도 일관되게 처리한다.
2. 완성 단계에서는 `maho-app::ClientSession`의 인증·페어링·Ready 상태를
   재사용하고 실제 전송 성공/실패를 C ABI에 연결한다.
3. Rust/네이티브 경계의 포인터 소유권, 유효 기간, 동시 접근 및 파괴 계약을
   문서화한다. 잘못된 임의 포인터를 안전하게 검증할 수 있다고 주장하지 않는다.

### F2 · 치명적: MediaCodec/VideoToolbox/오디오가 실제 구현이 아님

근거:

- `clients/rust/maho-mobile/src/android.rs:47-80,104-119`
- `clients/rust/maho-mobile/src/ios.rs:45-75,99-112`

Android 디코더는 비어 있지 않은 바이트열에 대해 `frames_decoded`만 증가시킨다.
iOS도 `frames_rendered`만 증가시킨다. Surface와 Metal layer는 실제 객체가
아니라 boolean이다. AudioTrack/AudioEngine 역시 샘플을 출력하지 않고
카운터를 증가시킨다. 테스트의 임의 NAL 바이트가 통과해도 프레임 출력의
증거가 되지 않는다.

개선:

- 미구현 네이티브 백엔드는 명시적 unsupported 오류로 구별하고, 실제 처리량
  카운터로 오인할 성공 값을 반환하지 않는다.
- Android는 실제 MediaCodec 입력/출력 버퍼 및 Surface 생명주기와 오디오
  출력 어댑터가 필요하다. iOS는 VideoToolbox 세션, 영상 버퍼 소유권,
  Metal 표시 및 오디오 출력 어댑터가 필요하다.
- H.264/HEVC 협상과 지원 여부, 키프레임 복구, 화면 회전 및 출력 대상 재생성,
  오디오 포커스/인터럽트 처리를 연결해야 한다.

### F3 · 높음: 설치 가능한 Android/iOS 앱 대상이 없음

근거: `clients/rust/maho-mobile/Cargo.toml:19-21`,
`clients/rust/tauri-shell/Cargo.toml`,
`clients/rust/tauri-shell/tauri.conf.json`.

`lib/cdylib/staticlib`는 라이브러리 산출물이지 설치 가능한 앱이 아니다.
`clients` 파일 목록에 AndroidManifest, Gradle 프로젝트, Kotlin/Java 앱 진입점,
iOS 앱 프로젝트, Info.plist 및 모바일 entitlement 파일이 없다.
기존 Tauri 설정은 데스크톱 윈도우 설정이며 `maho-mobile` 의존성도 없다.
다만 `clients/rust/tauri-shell/src-tauri/src/lib.rs:1650-1652`에는
`#[cfg_attr(mobile, tauri::mobile_entry_point)]`가 이미 있다. 따라서 모바일
진입점 표식조차 없다는 판단은 부정확하다. 이 표식은 생성된 앱 프로젝트,
타깃 의존성 설정 및 실제 모바일 미디어 통합을 대체하지 않는다.

개선: 기존 Rust 코어를 유지하면서 Tauri v2 모바일 앱 셸 또는 작은 플랫폼
셸을 선택하고, 기존 진입점과 실제 모바일 라이브러리 호출 경로를 연결한다.
퇴역한 Swift 데스크톱 앱을 복원하는 방식은 사용하지 않는다.
Tauri 생성 프로젝트가 필요하다는 사실과 과거 Swift 앱을 복원한다는 것은
서로 다른 문제다.

### F4 · 높음: 데스크톱 미디어 의존성이 모바일 라이브러리에 그대로 유입

근거: `clients/rust/maho-mobile/Cargo.toml:9-13`,
`clients/rust/maho-app/Cargo.toml:9-12`,
`clients/rust/maho-decode/Cargo.toml:9-16`,
`clients/rust/maho-render/Cargo.toml:9-13`.

`maho-mobile`의 직접 의존성뿐 아니라 `maho-app`을 통해서도 기본 FFmpeg,
CPAL 경로가 활성화된다. `maho-mobile` 한 곳에서만
`default-features = false`를 설정해도 전이 의존성의 feature 활성화가 남는다.
Linux에서 FFmpeg 7을 찾는 빌드 환경은 Android/iOS용 바이너리와 링커 설정을
제공하지 않는다. `maho-net`의 vendored OpenSSL도 타깃별 C 툴체인이 필요하다.

개선: 공통 세션·프로토콜과 데스크톱 미디어를 feature 또는 크레이트 경계로
분리하고, Android/iOS 타깃의 네이티브 미디어 어댑터를 선택한다.
데스크톱 기본 기능을 유지하면서 `cargo tree -e features`와 타깃별
컴파일로 검증한다. 의존성 이름만 제거해 모바일 지원 완료로 표시하지 않는다.

### F5 · 높음: 멀티터치 및 모드 전환에서 마우스 버튼 상태 불일치

근거: `clients/rust/maho-mobile/src/touch.rs:128-197`.

모든 손가락의 Began이 LeftMouseDown, 모든 Ended/Cancelled가 LeftMouseUp을
발생시킨다. 첫 손가락으로 드래그하는 중 두 번째 손가락을 떼면 첫 드래그가
풀린다. 시작한 적 없는 ID의 Ended도 Up을 만든다. `set_mode`는 활성 터치를
지우지만 원격 버튼을 해제하는 이벤트를 반환하지 않는다.

개선: 원격 포인터를 소유하는 한 터치를 명확하게 추적하고, 다른 터치나
알 수 없는 종료가 그 버튼을 해제하지 못하게 한다. 모드 변경·취소 시
정확히 한 번 해제 이벤트를 호출자에게 전달한다. 핀치·두 손가락 스크롤은
별도 제스처 구현 범위로 남기고 지원된다고 표시하지 않는다.

### F6 · 높음: 비유한 입력과 오류가 터치 상태를 오염시킴

근거: `clients/rust/maho-mobile/src/touch.rs:65-97,133-177`.

`set_zoom`은 중심점의 NaN/Infinity를 검사하지 않고 먼저 상태를 바꾼다.
공개 viewport 필드가 잘못된 값이어도 `transform_to_host`는 일부만 검사한다.
상대 모드의 입력 좌표/차분은 유한성 검사가 없고, 직접 모드도 변환 실패 전에
활성 터치 상태를 갱신한다. 오류 후 다음 이벤트가 오염된 기준점으로 처리된다.

개선: 입력/viewport 경계에서 유한성·크기·zoom을 검사하고, 계산 결과가 유효한
경우에만 상태를 반영한다. 실패한 이벤트는 기존 상태를 보존해야 한다.
취소 이벤트는 잘못된 마지막 좌표 때문에 버튼 해제가 누락되지 않아야 한다.

### F7 · 중간: 발열 제한이 오히려 비트레이트를 올리거나 overflow 발생

근거: `clients/rust/maho-mobile/src/power.rs:72-96`.

Fair 상태의 `(target_bitrate * 4 / 5).max(1_000)`는 기본/절약 한도가
1,000 kbps보다 작으면 그 한도를 초과한다. 공개 설정에 큰 u32 값을 넣으면
곱셈이 overflow된다.

개선: overflow 없는 연산을 사용하고 발열 조정이 기존 한도를 절대 올리지
않게 한다. 작은 상한과 `u32::MAX`를 회귀 테스트로 다룬다.

### F8 · 높음: 보안 저장 및 앱 수명주기는 아직 실서비스와 연결되지 않음

근거: `clients/rust/maho-mobile/src/storage.rs:18-58,61-108`,
`clients/rust/maho-mobile/src/lifecycle.rs`,
`clients/rust/maho-mobile/src/keyboard.rs`.

SecureStorageBackend의 제공 구현은 메모리 HashMap인 MockSecureStorage뿐이다.
이름이 Mock이므로 실제 암호화 저장이라고 해석하면 안 된다. lifecycle은
상태/boolean/재연결 횟수를 바꾸지만 세션 중단, 키 해제, 소켓 재연결,
RequestKeyFrame 전송을 수행하지 않는다. IME도 문자열 조합 모델이며 원격
텍스트 전송이나 플랫폼 입력기 연결이 아니다.

개선: Keychain/Android Keystore 연동과 앱 샌드박스 저장 위치를 먼저 정하고,
세션 구성에 주입한다. 플랫폼 lifecycle 콜백을 입력 해제 → 미디어 정지 →
연결 정리/재연결 → 키프레임 요청에 연결한다. 한글 IME commit을 실제 원격
텍스트 입력으로 전달하는 경로와 커서 위치의 단위도 명시해야 한다.

## 이번 수정 범위와 이후 구현 순서

이번에는 Gemini 3.8 Flash에 F1/F2의 허위 성공 제거 및 ABI 계약,
F5/F6의 터치 상태·좌표 처리, F7의 전력 정책 결함을 위임해 수정했다.
미구현 플랫폼 기능을 unsupported로 구별하는 수정은 기능 완성이 아니다.
앱 셸과 네이티브 미디어를 새로 완성했다고 보고하지 않는다.

후속 모바일 제품 구현 순서:

1. 공통 세션 코어의 데스크톱 의존성 분리 및 타깃 컴파일 경계 확립.
2. 실제 페어링·인증·입력 전송·수신 취소를 연결한 모바일 세션 API.
3. Android 앱 셸과 네이티브 영상/오디오 출력, Keystore 저장.
4. iOS 앱 셸과 네이티브 영상/오디오 출력, Keychain 저장.
5. 모바일 연결 UI, 터치/키보드/IME, 화면 회전, background 및 네트워크 변경.
6. 패키지 서명·배포 설정. 코드 검증 이후 사용자 승인으로 기기/에뮬레이터 검증.

## 내 휴대폰에 설치할 수 있는가

| 구분 | Android | iOS |
|---|---|---|
| 현재 코드로 동작하는 앱 생성 | 불가: 앱 대상과 실제 플랫폼 경로 없음 | 불가: 앱 대상과 실제 플랫폼 경로 없음 |
| 완성 후 개인 기기 설치 | 서명된 APK를 직접 설치 가능 | Xcode 개발 서명 및 해당 기기용 provisioning 필요 |
| 스토어를 거쳐야 하는가 | 개인 APK 설치에 Play 등록은 불필요 | 개인 개발 설치와 TestFlight/App Store 배포는 별도 경로 |
| 배포용 산출물 | APK 또는 Play용 AAB | 서명된 앱/IPA, 배포 방식에 맞는 profile |
| 빌드 환경 | Linux에서 SDK/NDK/JDK 및 Rust 타깃으로 가능 | macOS + Xcode 및 iOS SDK 필요 |
| 이 작업의 검증 상태 | 코드 검증만, 설치/실행 안 함 | 코드 검증만, 설치/실행 안 함 |

Mac에는 Xcode, Android SDK/NDK 디렉터리와 Rust 모바일 타깃이 존재한다.
이것은 설치 가능성을 높이는 준비 상태일 뿐, 앱이 빌드된다는 증거는 아니다.
서명 identity 존재도 앱 Bundle ID·팀·기기와 일치하는 유효 profile 확보를
증명하지 않는다. Omarchy의 Android 툴체인 완비 여부는 아직 검증하지 않았다.

Apple 공식 안내에 따르면 무료 Personal Team도 개인 기기 테스트가 가능하나,
현재 안내에는 profile 7일 만료 등 제한이 있다. TestFlight/App Store 배포는
Developer Program 및 적절한 서명·프로비저닝 구성이 필요하다.
Android는 앱 서명이 필요하지만 Play 배포와 개인 APK 설치는 구분해야 한다.

통신은 같은 LAN 또는 휴대폰에도 구성한 Tailscale 등의 실제 라우팅이 있어야
한다. PC의 Tailscale IP만 안다고 휴대폰에서 자동으로 접속되는 것은 아니다.
셀룰러/외부망 직결은 NAT·방화벽 및 신호/릴레이 연결 경로를 별도 검증해야 한다.
모바일 앱 패키징만으로 외부망 연결 문제가 해결되지는 않는다.

## 공식 참고 자료

- [Tauri 모바일 개발 전제 조건](https://v2.tauri.app/start/prerequisites/)
- [Tauri Android APK/AAB 생성 및 Play 배포](https://v2.tauri.app/distribute/google-play/)
- [Tauri iOS 서명](https://v2.tauri.app/distribute/sign/ios/)
- [Tauri iOS 패키징 및 App Store 배포](https://v2.tauri.app/distribute/app-store/)
- [Apple 계정/Personal Team 제한](https://developer.apple.com/help/account/basics/about-your-developer-account)

## 수정 및 코드 검증 결과

### 반영된 수정

실제 동작 변경과 회귀 테스트는 **Gemini 3.8 Flash**
(`mahoquot/gemini-3.8-flash-high`, 다른 모델로의 CLI fallback 비활성화)에
위임했다. 주 에이전트는 패치를 리뷰하고, 수정 지시·기계적 포맷 적용·
Omarchy 검증을 수행했다. 도중의 API 502 오류는 저장된 부분 패치를 보존해
같은 모델에서 이어갔다.

| 항목 | 수정 후 상태 | 최종 코드 근거 |
|---|---|---|
| F1 허위 접속/전송 성공 | 핸들 생성은 로컬 할당만 의미. 가짜 연결 플래그 제거, 유효한 전송 요청은 `BackendUnavailable = 5` 반환. 실패 출력 슬롯 null 초기화 | `clients/rust/maho-mobile/src/bridge.rs:12-110` |
| F2 허위 영상·오디오 출력 | 임의 바이트의 decode/render/write 성공 제거. 실제 출력 카운터 증가 없음. iOS start는 명시적 오류 반환 | `clients/rust/maho-mobile/src/android.rs:73-127`, `clients/rust/maho-mobile/src/ios.rs:65-110` |
| F5 터치 소유권 | 두 모드 모두 한 터치가 포인터를 소유. 보조 터치·중복 종료가 버튼 상태를 훼손하지 않음. 모드 전환은 해제 이벤트 반환 | `clients/rust/maho-mobile/src/touch.rs:215-347` |
| F6 좌표/취소 복구 | 좌표·viewport·계산 결과 검사. 오류 시 기준점 보존. 두 모드의 취소는 비유한 좌표 검사보다 먼저 처리 | `clients/rust/maho-mobile/src/touch.rs:72-155,238-265` |
| F7 발열 제한 | u32 몫/나머지 연산으로 overflow 제거, 이미 계산한 비트레이트 상한을 넘지 않음 | `clients/rust/maho-mobile/src/power.rs:72-99` |

공개 API 변경:

- 기존 C ABI 상태값 0~4는 유지하고 `BackendUnavailable = 5`를 추가했다.
- `MobileSessionHandle::is_connected`를 제거했다.
- `IosAudioEnginePlayer::start()`는 `Result<(), IosMediaError>`를 반환한다.
- `TouchGestureHandler::set_mode()`는 `Option<InputEvent>`를 반환한다.
  호출자는 반환된 버튼 해제 이벤트를 실제 전송 경로에 전달해야 한다.

### 검증 결과

모든 Rust 컴파일·테스트·Clippy·라이브러리 빌드는 **Omarchy Linux**의 격리된
소스 스냅샷에서 수행했다. 주 작업공간이나 배포된 호스트 데몬을 덮어쓰지
않았다. 다음 표는 모바일 크레이트에 대한 코드 검증이며 전체 제품 승인이나
Android/iOS 타깃 빌드 성공을 뜻하지 않는다.

| 검사 | 결과 |
|---|---|
| 최초 RED | 회귀 24개 중 23개 실패, 1개 통과. 실제 결함으로 실패함을 확인 |
| 추가 취소 경계 RED | 회귀 32개 중 상대 터치 NaN 취소 1개 실패, 31개 통과 |
| 최종 `cargo test -p maho-mobile` | 단위 27개 + 공개 API 회귀 32개 = **59개 통과**, 실패/무시 0 |
| 최종 `cargo clippy -p maho-mobile --all-targets --no-deps -- -D warnings` | **종료 코드 0**, 경고 억제 없음 |
| 최종 `cargo build -p maho-mobile` | **종료 코드 0**, Linux 라이브러리 빌드 |
| 수정된 Rust 파일의 `rustfmt --check` | **종료 코드 0**. Mac에서 포맷만 검사했으며 컴파일하지 않음 |
| 실기기·에뮬레이터 설치/실행 | 사용자 승인 범위 밖이므로 미실시 |
| APK/IPA 생성·네이티브 codec 검증 | 미실시. 관련 제품 구현/패키징이 아직 없음 |
| LSP 진단 | 로컬 daemon 연결 실패로 사용 불가. Omarchy compiler/Clippy로 코드 진단 수행 |
| Miri/모바일 타깃 교차 컴파일 | 미실시. Omarchy에 nightly/Miri가 없으며 모바일 네이티브 빌드 구성도 미완성 |

검증 명령과 실패/통과 증거:

- [RED 증거](../.omo/mobile-review-20260908/red-evidence.md)
- [최종 코드 게이트 증거](../.omo/mobile-review-20260908/final-evidence.md)
- [Flash 구현 인계](../.omo/mobile-review-20260908/flash-implementation.md)
- [독립 게이트 리뷰](../.omo/evidence/mobile-client-review-gate-review.md):
  `omo-senpi-gate-reviewer` / Gemini 3.8 Flash가 **APPROVE, 차단 이슈 0개**로
  판정했다. 승인 범위는 이 문서와 공통 코드의 한정된 수정이며 모바일 제품
  출시·설치 승인이 아니다.

**남은 제품 작업:** F1의 실제 네트워크 세션, F2의 실제 네이티브 미디어,
F3의 앱 패키징, F4의 의존성 분리, F8의 안전한 영구 저장·lifecycle·IME
연결이다. 따라서 이번 결과는 **모바일 코드 결함 수정 및 구현 현황 검증**이며
**휴대폰에서 사용할 수 있는 원격 데스크톱 앱 완성**은 아니다.
