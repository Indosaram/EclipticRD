# iOS 앱 구현 및 실기기 검증 상태

## LAN 탐색 후속 배포

2026-09-09 후속 작업에서 Bonjour LAN 탐색과 모바일 기기 목록을 구현했다.
업데이트 앱은 실제 iPhone 16 Plus에 설치·실행됐으며 Windows와 Linux
호스트도 재배포됐다. **아이폰에서 실제 목록 표시와 영상·입력 접속 검증은
아직 미완료**다. 상세 증거와 남은 권한 조건은
[LAN 탐색 배포 기록](lan-discovery-deployment-20260909.md)을 참조한다.
아래는 최초 빌드 당시의 기록이다.

## 현재 결과

Gemini 3.8 Flash에 실제 iOS 구현을 위임하여 `clients/rust/ios-shell`
(`erd-ios`, `com.eclipticrd.ios`)을 추가했다. 실제 iPhone용 ARM64 앱 빌드,
개발 서명, IPA 생성이 완료됐다. **실기기 설치·접속 검증은 아직 완료되지 않았다.**

생성된 IPA:

`clients/rust/ios-shell/gen/apple/build/arm64/EclipticRD.ipa`

생성 앱은 `iphoneos` 대상이다. 에뮬레이터나 시뮬레이터를 실행하거나 그
타깃으로 테스트하지 않았다. 사용자 승인에 따라 iOS 타깃 컴파일·빌드만
Mac/Xcode에서 수행했고, 공통 Rust/JS 테스트는 Omarchy에서 수행했다.

## 구현 범위

- 기존 `ClientSession`의 실제 인증·페어링·TCP/UDP 수신 경로를 사용하는
  iOS 전용 Tauri 셸.
- FFmpeg 전이 의존성과 분리된 iOS VideoToolbox H.264/HEVC 디코더.
  CoreMedia 샘플과 실제 NV12 출력 버퍼 처리.
- CPAL 오디오 스트림을 별도 스레드에서 초기화·소유하는 출력 경로.
- iOS PairingStore의 Keychain 저장 경로.
- 실제 NV12 WebGL 표시, 호스트/PIN 연결 화면, 직접 터치·트랙패드,
  보조 키보드, 음소거, 연결 해제, 프레임 표시 보고.
- 백그라운드 전환 시 실제 연결 해제, 입력 큐 순서 보존과 이전 세대 입력
  폐기, 취소·오류 전파 처리.
- Tauri Apple 프로젝트 및 필요한 네이티브 프레임워크 링크 구성.

이 목록은 구현된 코드 경로를 설명한다. **iPhone에서 영상·오디오·입력이
정상 동작했다는 실측 결과는 아직 없다.**

## 확인한 검증

| 검사 | 결과 |
|---|---|
| 기존 iOS 교차 컴파일 장애 | FFmpeg pkg-config 교차 컴파일 오류 재현 |
| VideoToolbox 단독 iOS 타깃 검사 | 종료 코드 0, 마지막 검사 경고 0 |
| iOS 앱 라이브러리/테스트 타깃 검사 | Flash 실행 결과 둘 다 종료 코드 0, 경고 0 |
| Omarchy 공통 Rust 회귀 | `erd-app`, `erd-decode`, `erd-mobile`: 199개 통과 |
| Omarchy UI 회귀 | 수명주기·입력 큐 실패 수정 후 29개 통과 |
| iPhone ARM64 전체 빌드 | `BUILD SUCCEEDED`, 최종 명령 종료 코드 0 |
| 앱 코드 서명 확인 | `codesign --verify --deep --strict`: 종료 코드 0 |
| IPA 생성 | 위 경로에 1개 iOS Bundle 생성, 종료 코드 0 |
| iPhone 12 Pro 설치 | 실패: 기기 잠금으로 개발자 디스크 마운트 차단 |
| iPhone 원격 접속/영상/오디오/입력/화면 QA | 미실시, 실기기 설치 후 필요 |

빌드:

```sh
cd clients/rust/ios-shell
ulimit -n 8192
RUSTC_WRAPPER= CARGO_BUILD_JOBS=4 CARGO_TERM_COLOR=never \
  IPHONEOS_DEPLOYMENT_TARGET=16.0 \
  APPLE_DEVELOPMENT_TEAM=5DUM8WPB4C \
  CARGO_TARGET_DIR=/Volumes/T9-Mac/project/EclipticRD-Rewrite/clients/rust/target-ios-device \
  cargo tauri ios build --debug --target aarch64 --ci \
    --export-method debugging \
    --config '{"bundle":{"iOS":{"developmentTeam":"5DUM8WPB4C","minimumSystemVersion":"16.0"}}}'
```

빌드 모니터: `mon_1KYA0ZHR5T73AXKE` / `bash_38`.
서명: Apple Development, Team `5DUM8WPB4C`.
Xcode의 SDK 프레임워크 누락 링크 오류는 필요한 프레임워크를
`bundle.iOS.frameworks`와 생성 프로젝트 입력에 추가하여 해결했다.
로컬 sccache 파일 핸들 한도 오류는 해당 iOS 빌드에서만 래퍼를 제외해 해결했다.

## 실기기 설치 차단

대상: ‘개발용’ iPhone 12 Pro, iOS 26.6.
CoreDevice 식별자: `DBB7A424-6196-5A4D-88EA-CE441C4B8132`.

기기 정보 조회는 성공했으나 잠금 상태는 `passcodeRequired: true`였다.
`unlockedSinceBoot: true`는 부팅 후 한 번 해제했다는 뜻이며 현재 잠금 해제
상태를 의미하지 않는다.

실제 설치 시도 (`mon_SRTNSGNJEV495XW0` / `bash_39`)는 종료 코드 1:

```text
The developer disk image could not be mounted on this device.
CoreDeviceError 12040
kAMDMobileImageMounterDeviceLocked: The device is locked.
```

사용자가 아이폰 잠금을 풀고 개발 연결을 유지하면 같은 서명 앱의 설치,
실행, 실제 데스크톱 연결 및 영상·입력·오디오 검증을 계속해야 한다.
이 문서는 최종 제품 승인이나 실기기 동작 성공 보고가 아니다.

## 구현 인계

- `.omo/ios-device-20260908/contract.md`
- `.omo/ios-device-20260908/native-handoff.md`
- `.omo/ios-device-20260908/app-handoff.md`
- `.omo/ios-device-20260908/ui-handoff.md`

로컬 LSP daemon 연결은 실패하여 iOS compiler 진단과 Omarchy 테스트를
사용했다. Miri 검증, Android 앱 구현, 스토어 배포, git 커밋은 수행하지 않았다.
