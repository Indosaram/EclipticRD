# 원격 접속·페어링 코드 리뷰 및 개선계획

- 작성일: 2026-09-10
- 기준 커밋: `71f8b05f5a0e9d53ce0249beb46ca864b7a836f8`
- 검토 기준: 위 커밋에 작업 트리 변경을 포함한 소스. 커밋 자체에 대한 리뷰가 아니다.
- 산출물 범위: 코드 리뷰, 제한된 프론트엔드 테스트·재현, 개선 설계와 검증 계획. 구현·배포 완료 보고서가 아니다.
- 경로 표기: 아래 코드 경로는 저장소 루트 기준이다.

## 1. 결론과 우선순위

**접속 UI 개선보다 먼저, 페어링의 권한 방향과 승인 상태, UDP 수신 주소의 인증을 바로잡아야 한다.** 현재는 인증 정보를 저장하고 재사용하는 기능이 존재하지만, 호스트의 인바운드 허용 목록과 클라이언트의 아웃바운드 자격증명이 같은 기본 파일을 사용한다. 또한 bootstrap TLS가 호스트 승인을 거치지 않고 기존 페어링 ID로 애플리케이션 인증을 완료할 수 있다.

프론트엔드에서는 저장된 페어링의 재사용 기준이 데스크톱과 iOS에서 다르고, iOS는 연결 종료와 오류 정리를 UI 상태만으로 판단한다. 따라서 정상 연결 테스트가 통과해도 재접속, 원격 종료, 정리 실패, 연결 취소에서 문제가 남는다.

| ID | 우선순위 | 발견사항 | 증거 수준 |
| --- | --- | --- | --- |
| R1 | P1 | 호스트·클라이언트 기본 저장소 공유로 권한 방향 혼동 | 소스 경로·소비 지점 확인 |
| R2 | P1 | bootstrap TLS에서 호스트 승인 및 승인받은 ID 결속 누락 | 인증 상태 전이·입력 처리 확인 |
| R3 | P1 | 미인증 UDP 패킷이 영상 수신 주소를 결정 | 등록·송신 경로 확인 |
| R4 | P1 | 페어링 목록 IPC가 장기 비밀키를 반환 | 반환 타입·직렬화·명령 등록 확인 |
| R5 | P1 | iOS가 TCP 종료를 세션 상태에 반영하지 않음 | 이벤트 생산·소비 경로 확인 |
| R6 | P1 | iOS 오류 후 네이티브 정리 누락 및 정리 실패 후 idle 보고 | 실제 JS 모듈 + invoke 대역으로 재현 |
| R7 | P1 | iOS 연결 중 취소 모달이 숨겨진 부모 안에 위치 | HTML·상태 분기·CSS 확인 |
| R8 | P2 | 데스크톱이 이름·접두사·테스트 장비 IP로 재접속 키 선택 | 검색·선택·인증 호출 확인 |
| R9 | P2 | iOS 발견 카드 및 IP 직접 입력에서 저장 페어링 재사용 단절 | 소스 확인 + 카드 JS 경로 재현 |
| R10 | P2 | Apple scoped IPv6 link-local 발견 결과가 공통 검증에서 탈락 | 주소 선택·검증·게시 경로 확인 |
| R11 | P1 | 동시 작업에서 고정 기본 PIN과 자동 재페어링 도입 | 리뷰 중 추가된 작업 트리 변경 확인 |

P1은 다음 배포 전에 해결해야 하는 권한·비밀정보·주요 접속 수명주기 문제다. P2는 실패 조건이 제한되거나 다른 접속 경로가 있는 기능 결함이다. 실서비스 침해, 미디어 평문 유출, 실기기 재현 성공을 주장하지 않는다.

### 작업 트리 경계

리뷰 시작 시 `clients/rust/tauri-shell/src-tauri/src/lib.rs`와 `discovery_tests.rs`에 기존 수정이 있었다. 리뷰 중 `clients/rust/erd-host/src/main.rs`, 데스크톱 `ui/index.html`, `lib.rs`의 추가 변경을 관찰했다. 이 리뷰 작업은 해당 파일을 수정하지 않았다.

R11은 시작 시점에는 없던 동시 변경이다. 문서 저장 전 관련 코드를 다시 읽어 반영했다. 데스크톱 `lib.rs`와 `index.html`의 줄 번호는 이 재확인 시점을 기준으로 한다. 이후 동시 변경으로 줄 번호가 이동하면 함께 적힌 함수명을 기준으로 찾는다. 추가된 WebGL 변경의 렌더링 품질은 이번 접속·페어링 리뷰 대상에 포함하지 않았다.

## 2. 상세 발견사항

### R1. [P1] 아웃바운드 페어링이 로컬 호스트의 인바운드 권한이 될 수 있음

**근거**

- `clients/rust/erd-app/src/pairing.rs:216-255`: 데스크톱 클라이언트의 기본 파일은 사용자 데이터 디렉터리의 `EclipticRD/pairing-keys.json`이다. iOS는 별도 Keychain을 사용한다.
- `clients/rust/erd-host/src/session.rs:115-120`: 호스트도 사용자 데이터 디렉터리의 같은 파일을 사용한다.
- `clients/rust/erd-app/src/session.rs:258-266`: 원격 호스트가 발급한 ID·키를 클라이언트가 저장한다.
- `clients/rust/erd-host/src/session.rs:2078-2085,2420-2435`: 호스트는 해당 저장소의 레코드를 인바운드 TLS PSK 및 handshake 인증에 사용한다.

**조건과 영향:** 같은 OS 계정으로 클라이언트와 호스트를 기본 설정에서 실행하는 경우다. 로컬 클라이언트가 원격 호스트 B에 페어링하면 B가 발급해 알고 있는 키가 로컬 호스트의 허용 자격증명으로도 사용될 수 있다. 로컬 호스트가 B의 인바운드 접근을 승인한 적이 없어도 발생한다. 사용자 지정 저장소를 이미 분리한 구성과 iOS Keychain에 같은 파일 충돌을 주장하지 않는다.

**개선:** 호스트 허용 목록과 클라이언트 자격증명을 서로 다른 저장소·파일·임시 파일 이름으로 분리한다. 예: `host-authorizations.json`, `client-pairings.json`. 레코드의 역할과 발급 주체도 명시한다. 기존 파일만으로는 레코드의 권한 방향을 복구할 수 없으므로, 모호한 기존 레코드를 호스트 허용 목록에 자동 이관하지 않는다.

**완료 조건:** 격리된 사용자 데이터 디렉터리에 클라이언트 경로로 저장한 키가 호스트 인증에서는 거부되어야 한다. 호스트가 별도 승인한 키만 성공하고, 클라이언트 레코드 삭제가 호스트 권한을 삭제하지 않아야 한다. OS별 기본 경로 검증은 환경변수를 공유하는 병렬 테스트 대신 격리 프로세스로 실행한다.

### R2. [P1] bootstrap TLS가 호스트 승인을 건너뛸 수 있음

**근거**

- `clients/rust/erd-host/src/session.rs:381-392`: `PreAuth`에서 `Handshake`를 허용하고, `Authenticated`에서도 재차 허용한다.
- `clients/rust/erd-host/src/session.rs:2405-2418`: 승인 후 레코드를 발급하지만 연결 상태에는 승인받은 ID를 보관하지 않는다.
- `clients/rust/erd-host/src/session.rs:2420-2459`: 요청의 pairing ID를 읽고 bootstrap TLS identity이면 승인 여부나 방금 승인한 ID와의 일치를 요구하지 않은 채 `Authenticated`로 전환한다.
- `clients/rust/erd-host/src/session.rs:2485-2490`: 이 상태의 TCP 입력은 입력 주입기로 전달된다.
- `clients/rust/erd-net/src/tls_psk.rs:52-61,157-168`: pairing ID는 TLS PSK identity에 실리는 식별자다. 비밀키 자체와 동등한 비밀로 취급할 수 없다.

**조건과 영향:** 유효한 현재 PIN과 기존 pairing ID를 아는 피어가 bootstrap TLS를 연 뒤 `PairingRequest` 없이 해당 ID의 `Handshake`를 보내는 경우다. 해당 페어링 키를 몰라도 호스트 승인 없이 인증된 TCP 입력 경로에 도달할 수 있다. 이 사실만으로 UDP 영상 복호화까지 가능하다고 결론내리지 않는다.

**개선:** 인증 상태에 출처를 담는다. 저장 키 기반 TLS는 협상한 ID와 요청 ID가 같아야 한다. bootstrap TLS는 이 연결에서 승인·발급한 정확한 ID만 사용할 수 있어야 한다. 인증 완료 후 두 번째 handshake는 거부하고 캡처·암호 상태를 다시 만들지 않는다.

**완료 조건:** 실제 loopback TLS와 대역 입력 주입기를 사용해 다음을 검증한다.

1. 기존 A가 있어도 bootstrap 연결에서 승인 없이 `Handshake(A)`를 보내면 종료되고 입력·캡처가 시작되지 않는다.
2. B를 승인받은 연결에서 `Handshake(A)`를 보내면 거부된다.
3. 승인된 B 및 저장 PSK로 인증한 A의 정상 경로는 성공한다.
4. 인증 후 중복 handshake로 캡처 작업이나 cipher가 교체되지 않는다.

### R3. [P1] 영상 수신 주소가 현재 세션 인증에 결속되지 않음

**근거**

- `clients/rust/erd-host/src/session.rs:2645-2661`: TCP 피어와 IP가 같은 `0xff` 패킷을 인증 없이 등록한다. 다른 패킷도 `open_datagram`의 실패를 무시하고 `udp_peer`를 변경한다.
- `clients/rust/erd-host/src/session.rs:2184-2220`: 최초 등록 주소를 송신 스레드가 값으로 캡처한다. 이후 `udp_peer` 변경이 실제 송신 목적지에 반영되지 않는다.
- `clients/rust/erd-app/src/session.rs:839`: 정상 클라이언트도 평문 probe를 사용한다.

**조건과 영향:** 같은 클라이언트 머신 또는 같은 NAT 외부 IP의 다른 송신자가 다른 포트에서 먼저 패킷을 보내거나, 이전 연결의 probe가 소켓에 남아 있는 경우다. TCP 접속은 성공해도 암호화된 영상이 잘못된 UDP 포트로 계속 전송될 수 있다. 평문 노출이 아니라 목적지 오등록·접속 실패 문제로 분류한다.

**개선:** handshake에서 정한 현재 세션의 client-to-host 키로 인증한 등록 패킷만 수신 주소를 설정하도록 클라이언트·호스트를 함께 변경한다. 복호화·인증 실패는 어떤 endpoint 상태도 변경하지 않아야 한다. 초기 구현에서는 세션 내 주소 고정을 권장하며, 주소 이동 지원이 필요하면 인증된 변경을 실제 송신기까지 전달하는 계약을 별도로 둔다.

**완료 조건:** 잘못된 키, 평문 probe, 이전 세션 패킷을 먼저 소비시켜도 endpoint가 미등록 상태여야 한다. 현재 세션의 유효한 등록 후에만 올바른 소켓으로 제어된 테스트 프레임이 도착해야 한다. 등록 이후의 위조 패킷도 목적지를 바꾸지 못해야 한다.

### R4. [P1] 목록 조회가 장기 페어링 비밀키를 WebView에 노출함

**근거**

- `clients/rust/tauri-shell/src-tauri/src/lib.rs:1438-1444`: `list_pairings`가 `Vec<erd_app::PairingRecord>` 전체를 반환한다.
- `clients/rust/erd-app/src/pairing.rs:13-19`: `PairingRecord`는 `Serialize` 대상이며 `key`도 직렬화한다.
- `clients/rust/tauri-shell/src-tauri/src/lib.rs:1908`: 해당 명령이 invoke handler에 등록된다.
- 데스크톱의 두 `tauri.conf.json`은 `withGlobalTauri: true`, `security.csp: null`이다.

**조건과 영향:** 이 명령을 호출할 수 있는 프론트엔드 JS에 장기 키가 전달된다. 목록 UI에는 필요 없는 비밀정보 경계 확장이다. 외부 사이트에서 바로 이 명령을 호출할 수 있다거나 현재 XSS가 존재한다는 주장은 아니다.

**개선:** 목록용 `PairingSummary`에는 ID, 표시 이름, 생성 시각, 재인증 필요 상태 등 공개 메타데이터만 포함한다. 키는 네이티브 저장소와 인증 코드에서만 사용한다. 사용하지 않는 명령이라면 외부 등록 제거를 우선 검토한다. CSP는 보조 방어로 다루고, CSP만 추가해 반환 타입 결함을 해결했다고 판단하지 않는다.

**완료 조건:** 실제 명령의 직렬화 결과에 허용된 메타데이터만 있어야 한다. 키 필드 및 키 값이 응답에 없어야 하며, 네이티브 재접속은 계속 성공해야 한다. 문구가 아니라 JSON 필드 계약을 테스트한다.

### R5. [P1] iOS TCP 종료가 UI의 연결 상태에 반영되지 않음

**근거**

- `clients/rust/erd-app/src/session.rs:513-540`: TCP runtime 오류를 이벤트로 전달하고 공통 세션을 `Disconnected`로 바꾼다.
- `clients/rust/ios-shell/src/state.rs:545-565`: runtime을 만들고 저장하지만 이 파일의 실행 경로는 `RuntimeEvents`를 소비하지 않는다.
- `clients/rust/ios-shell/src/state.rs:188-206`: `stats()`는 별도의 `inner.state`를 반환하며 공통 세션 종료 상태를 확인하지 않는다.
- `clients/rust/ios-shell/src/state.rs:658-659,775-778`: 미디어 worker는 UDP를 읽고 UDP timeout은 계속 대기한다.

**조건과 영향:** 호스트가 TCP를 종료해도 iOS 네이티브 상태는 `Ready`로 남을 수 있다. UDP 수신 대기가 이어지면 UI도 스트리밍 또는 영상 대기 상태를 유지한다. 사용자가 원격 종료와 단순 정지 화면을 구별하지 못하고 재접속 경로로 돌아오지 못한다.

**개선:** 세션 수명주기를 소유하는 한 경로에서 TCP terminal 이벤트를 소비하고, 종료 이유와 세션 generation을 포함해 iOS 상태에 반영한다. 오류 후 작업 종료·입력 release·미디어 정리는 R6과 같은 종료 경로를 사용한다. stats 값만 임의로 덮어써 실제 작업을 남기는 수정은 피한다.

**완료 조건:** 인증된 테스트 세션의 TCP를 서버 쪽에서 닫으면 terminal 이벤트를 통해 UI가 접속 종료를 인식하고 모든 소유 작업이 끝나야 한다. UDP 패킷 유무나 타이밍 운에 의존해서는 안 된다. 이전 세션의 늦은 종료 이벤트가 새 세션을 종료하면 안 된다.

### R6. [P1] iOS 오류 상태에서는 정리를 생략하고, 정리 실패도 idle로 표시함

**근거**

- `clients/rust/ios-shell/ui/connection-state.js:410-430`: `disconnect()`는 `connecting`, `waiting-video`, `streaming`만 `wasBusy`로 인정하고 `error`에서는 네이티브 disconnect를 호출하지 않는다. `finally`는 정리 실패 여부와 무관하게 idle로 바꾼다.
- 같은 파일의 `pollStats`와 `dismissError`: 네이티브 오류를 `error`로 표시한 뒤 이를 idle로 숨길 수 있다.
- `clients/rust/ios-shell/src/state.rs:608-618`: 오디오 오류는 상태를 Error로 바꾸지만 해당 분기에서 전체 세션을 정리하지 않는다.

**재현:** 실제 `createConnectionManager` 모듈을 로드하고 invoke만 대체했다. 연결 후 stats에 `{state: "error", last_error: "fixture audio error"}`를 반환하고 disconnect를 호출했을 때 호출 목록은 `stop_discovery`, `connect`, `stats`뿐이었다. 네이티브 disconnect 없이 최종 idle이었다. 별도 재현에서 네이티브 disconnect를 실패시키면 `lastError`는 남지만 상태는 역시 idle이었다. 실제 iPhone 자원 누수를 측정한 결과는 아니다.

**개선:** UI 표시 상태와 네이티브 자원 소유 여부를 분리한다. 단일 pending cleanup을 공유하고, 오류 중에도 소유한 세션은 종료한다. 실패 시 `cleanup-failed` 또는 이에 준하는 상태에서 재시도와 새 연결 차단을 유지한다. 데스크톱 `connection-state.js`의 generation·pending cleanup 계약을 기준으로 두 플랫폼의 외부 동작을 맞춘다.

**완료 조건:** 오류 후 Disconnect 및 오류 dismiss 경로에서 자원이 남지 않아야 한다. 동시 종료 요청은 하나의 정리 작업을 공유해야 하고, 정리 실패를 성공으로 표시하면 안 된다. 지연된 이전 connect 완료·실패가 새 연결을 되살리거나 끊지 않아야 한다.

### R7. [P1] iOS 연결 중 Cancel이 숨겨진 부모 아래에 있음

**근거**

- `clients/rust/ios-shell/ui/index.html:74-85`: `modal-connecting`은 `view-session` 안에 있다.
- `clients/rust/ios-shell/ui/app.js:274-296`: `connecting`, `disconnecting`에서는 `sessionView.hidden = true`지만 자식 모달은 표시하려 한다. 부모는 `waiting-video`, `streaming`에서만 열린다.
- `clients/rust/ios-shell/ui/styles.css:56-58`: `[hidden]`을 `display: none !important`로 처리한다.

**영향:** PIN 승인·연결 대기 중에는 자식 모달의 hidden을 해제해도 부모가 숨겨져 Cancel과 진행 상태가 표시되지 않는다. HTML·CSS·상태 분기로 확인한 결함이며, 이번 리뷰에서 실기기 화면을 촬영해 확인한 것은 아니다.

**개선:** 연결 상태 모달을 connect/session 화면과 독립된 최상위 레이어로 이동하거나, 화면 선택과 모달 선택을 같은 상태 표에서 결정한다. Cancel에 포커스를 주고 배경 입력을 차단하며 종료 후 포커스를 돌려준다. 정리 중에는 중복 연결을 막되 진행 상태는 계속 보이게 한다.

**완료 조건:** 실제 DOM에서 connect promise를 보류한 상태로 진행 안내와 Cancel이 표시·접근 가능해야 한다. Cancel 후 정리 중 화면도 사라지지 않아야 한다. physical iPhone에서 터치 및 접근성 동작을 확인한다.

### R8. [P2] 데스크톱의 자격증명 선택이 인증된 호스트 식별과 분리되어 있음

**근거**

- `clients/rust/tauri-shell/src-tauri/src/lib.rs:1059-1064`: 이름 대소문자 무시, 접두사, 발견된 이름, 두 테스트 장비 IP 예외로 저장 레코드를 선택한다.
- 같은 파일 `merge_discovery_results:918-945`: LAN과 Tailscale을 이름 또는 IP로 합치고 pairing 표시를 상속한다.
- `clients/rust/tauri-shell/ui/index.html:697-702`: `paired` 힌트로 PIN 없는 즉시 접속과 PIN 입력 경로를 나눈다.

**조건과 영향:** 동명이인 호스트, 이름 접두사 충돌, 주소 변경에서 다른 키를 고르거나 같은 컴퓨터를 새 장비로 취급한다. PSK가 다른 호스트에 실제 인증을 성공시키는 것은 별개이며, 이름 충돌만으로 인증 우회가 된다고 주장하지 않는다. 다만 잘못된 키 선택으로 재접속이 실패하고, R11의 새 fallback이 이를 신규 페어링 시도로 바꾼다.

**개선:** 발견 결과의 표시 이름, 연결 endpoint, 로컬 저장 credential ID, 인증된 피어 신원을 분리한다. connect 요청은 로컬에서 선택한 credential ID를 명시적으로 전달하고 백엔드는 해당 레코드만 사용한다. mDNS 이름·TXT·IP만으로 trusted 상태를 만들지 않는다. 처음 인증에 성공한 뒤에만 endpoint 별칭을 저장하고, 모호한 경우 사용자에게 재인증을 요구한다. 테스트 장비 예외와 접두사 매칭은 제거한다.

**완료 조건:** 같은 이름의 서로 다른 호스트가 자동 합쳐지거나 키를 공유하면 안 된다. 동일 호스트의 LAN/Tailscale 전환은 로컬에 확인된 매핑에 한해서 재접속되어야 한다. 위조 발견 메타데이터는 인증 실패 또는 명시적 재인증으로 끝나야 한다.

### R9. [P2] iOS 저장 페어링이 발견 카드와 IP 재접속에 연결되지 않음

**근거**

- `clients/rust/ios-shell/ui/connection-state.js:307-350`: 선택한 발견 호스트는 항상 `isPaired: false`이며 `connectSelectedHost`는 PIN 없이는 호출을 거부한다.
- `clients/rust/ios-shell/ui/app.js:357-367`: 선택한 카드 주소이면 별도로 PIN을 무조건 요구한다.
- `clients/rust/ios-shell/src/state.rs:823-833`: PIN 없는 연결은 입력 host를 `find_by_host`에 넘긴다.
- `clients/rust/erd-app/src/pairing.rs:297-306`: 검색 대상은 레코드의 이름 또는 ID다.
- `clients/rust/erd-app/src/session.rs:258-266`: 저장되는 이름은 접속 IP가 아니라 `PairingGrant.host_name`이다.

**조건과 영향:** IP로 처음 페어링한 뒤 같은 IP로 PIN 없이 직접 연결하면 저장 레코드를 못 찾는다. 발견 카드를 통해서는 키가 저장되어 있어도 PIN 없이 네이티브 조회를 시도하지 않는다. JS 재현에서도 선택 객체의 `isPaired`는 false였고 PIN 없는 연결의 native connect 호출은 0회였다.

**개선:** R8과 공통인 공개 메타데이터·credential 선택 계약을 iOS에도 적용한다. 광고된 `paired` 값을 믿도록 바꾸는 것은 해결책이 아니다. Keychain 저장 결과와 로컬 신뢰 매핑으로 `saved`, `unknown`, `reauth-required`를 판단한다. iOS 명령은 IP와 키 조회용 식별자를 혼용하지 않아야 한다.

**완료 조건:** 최초 승인 후 앱을 재시작해도 같은 호스트에 PIN 없이 재접속할 수 있어야 한다. IP 변경·이름 충돌·키 폐기 시에는 잘못된 키를 쓰거나 자동 승인하지 않고 명시적 재인증으로 전환해야 한다.

### R10. [P2] scoped IPv6 link-local 주소가 게시 전에 탈락함

**근거**

- `clients/rust/erd-net/src/discovery/apple.rs:159-184`: 유효한 scope ID가 있는 link-local 주소를 `fe80::1%5` 형식으로 선택할 수 있다.
- 같은 파일 `:193-214`: 공통 parser에는 scope를 잃은 `IpAddr`를 넘기고 성공한 뒤에만 문자열에 scope를 복구한다.
- `clients/rust/erd-net/src/discovery.rs:25-35,170-176`: 공통 검증은 link-local IPv6를 모두 배제한다.

**조건과 영향:** IPv4 또는 global/ULA IPv6 없이 유효한 scoped link-local 주소만 있는 Apple 발견 결과가 목록에서 사라진다.

**개선:** scope를 보존하는 endpoint 타입으로 선택·검증·표시 경로를 통일한다. scope 없는 link-local 주소는 계속 거부한다. 문자열 포맷 함수만 수정하지 말고 `decide_service_state_action`의 최종 게시 결과를 고친다.

**완료 조건:** 유효한 TXT/SRV와 `(fe80::1, Some(5))`만 주어졌을 때 `Publish`가 되어야 한다. scope가 없거나 0이면 거부되어야 하고 유효한 IPv4 우선순위는 유지되어야 한다. 실제 접속 resolver까지 scope를 보존하는지도 별도로 확인한다.

### R11. [P1] 동시 변경: 고정 기본 PIN 및 재접속 실패의 자동 재페어링

**근거**

- `clients/rust/erd-host/src/main.rs:87-93`: 명시적 PIN 옵션이 없으면 무작위 PIN 대신 `12345678`을 사용하도록 리뷰 중 변경됐다. `--pin generate`는 여전히 별도 경로다.
- `clients/rust/tauri-shell/src-tauri/src/lib.rs:1067-1081`: 저장 키 재접속의 모든 실패 또는 저장 레코드 부재에서 `pair_with_pin("12345678")`를 실행한다.

**조건과 영향:** 기본 옵션의 bootstrap PIN이 예측 가능해진다. R2가 남은 경우 유효한 PIN을 알아야 한다는 전제가 크게 약해진다. 재접속 실패는 네트워크 오류, 키 폐기, 잘못된 대상 선택 등 원인이 다른데 모두 새 페어링 시도로 바뀐다. 사용자가 PIN을 입력하지 않았는데 호스트 승인 요청 또는 추가 인증 시도가 발생할 수 있다.

**미확정 정책:** 이 동시 변경이 사용자에게 별도 승인받은 개발용 요구인지 이번 리뷰 대화만으로 확인하지 못했다. 변경을 되돌리거나 승인 사실을 추정하지 않았다. 명시적인 로컬 개발 모드 요구라면 적용 범위와 종료 조건을 분리해야 한다.

**개선 제안:** 제품 기본값은 무작위·유효기간 있는 PIN으로 유지하고, 저장 키 인증 실패는 원인별 오류와 명시적 재페어링 행동으로 연결한다. 고정 PIN이 필요하면 기본 접속 경로가 아닌 명시적 개발 설정으로 격리한다. 어떤 정책을 택하더라도 R2의 승인 결속은 반드시 필요하다.

**완료 조건:** timeout·폐기 키·잘못된 호스트 각각에서 bootstrap 인증이 자동 발생하지 않아야 한다. 신규 페어링은 사용자가 선택한 동작과 PIN 정책에 따라 수행되고, 개발 설정이 일반 배포 기본값으로 활성화되지 않아야 한다.

## 3. 현재 흐름과 유지할 부분

현재 흐름은 다음과 같다.

```text
LAN/Tailscale 발견 또는 직접 주소 입력
  -> 프론트엔드 접속 요청
  -> Tauri 네이티브 명령
  -> PIN 기반 bootstrap TLS 또는 저장 키 기반 TLS
  -> 최초 페어링이면 호스트 승인 및 PairingGrant 저장
  -> 애플리케이션 Handshake / HandshakeAck
  -> 방향별 UDP 키 생성
  -> UDP 수신 주소 등록
  -> 영상·오디오·입력 및 종료 처리
```

유지할 구현도 있다. 데스크톱 상태 머신은 pending connect 정산 후 cleanup을 수행하고 generation으로 늦은 결과를 무효화한다. 관련 테스트는 실제로 통과했다. 호스트는 preauthentication의 절대 deadline을 두고, TLS PIN 형식·pairing key 크기 검증과 방향별 UDP 암호화를 이미 사용한다. 이를 새 프레임워크로 교체하기보다, 현재 경계의 권한 결속과 상태 소유권을 보강하는 것이 적절하다.

## 4. 권장 설계 계약

| 경계 | 결정할 계약 |
| --- | --- |
| 저장소 | 인바운드 권한과 아웃바운드 자격증명은 물리적으로 분리한다. 기존 모호한 레코드는 자동 인바운드 승인하지 않는다. |
| 발견 | 발견은 endpoint 후보 및 표시 메타데이터다. 이름·IP·mDNS 서비스 ID는 인증된 피어 신원 자체가 아니다. |
| 페어링 UI | 로컬 메타데이터로만 저장 상태를 표시한다. 키는 IPC에 반환하지 않는다. 저장 키 부재와 거부·폐기 상태를 구별한다. |
| 연결 요청 | endpoint와 선택한 로컬 credential ID를 별도 필드로 전달한다. PIN은 신규 페어링 요청에서만 명시적으로 전달한다. |
| 인증 | TLS에서 증명한 자격과 애플리케이션 handshake의 ID가 일치해야 한다. bootstrap은 같은 연결의 승인 결과에 결속한다. |
| UDP | 현재 세션에 인증된 등록 패킷만 송신 목적지를 결정한다. 잘못된 인증은 상태 불변이다. |
| 종료 | 네이티브 세션 owner가 TCP·미디어·입력·worker의 terminal 상태를 통합한다. UI 표시 상태로 자원 소유 여부를 추측하지 않는다. |
| 오류 | `pairing-required`, `pairing-denied`, `credential-rejected`, `timeout`, `remote-closed`, `cleanup-failed` 등 machine-readable code와 단계를 전달한다. 문자열 포함 검사로 재페어링 여부를 결정하지 않는다. |
| 취소 | 요청 generation/ID를 기준으로 취소한다. UI 취소와 native 취소의 계약을 연결하고 늦은 완료를 폐기한다. 기존 직렬화 lock을 없애는 대신 취소 가능한 작업 경계를 둔다. |

이름이 비슷하다는 이유로 새 추상 계층을 여러 개 만들 필요는 없다. 공통 Rust 타입과 테스트 가능한 JS 수명주기 계약부터 정하고, 실제 중복이 확인된 부분만 공유한다. 오류 code 세트와 저장소 버전은 구현 시작 전에 확정할 machine-consumed 계약이다.

## 5. 단계별 개선계획

### 단계 A. 신뢰·승인 경계 수정

- 대상: R1, R2, R4, R11.
- 순서: 인바운드/아웃바운드 저장소 분리와 이관 정책 확정 -> bootstrap 승인 ID 결속 -> 목록 IPC 공개 DTO -> PIN·재페어링 정책 적용.
- 변경 위치: `erd-app/src/pairing.rs`, `erd-host/src/session.rs`, `erd-host/src/main.rs`, 데스크톱 명령·관련 tests.
- 이관 원칙: 기존 파일을 삭제하지 않는다. 모호한 레코드는 보존하고 호스트 재승인 대상으로 분류한다. 새 허용 목록을 예전의 공용 파일로 다시 합치는 rollback은 하지 않는다.
- 단계 종료: R1/R2의 실제 loopback TLS 거부·허용 테스트, 목록 JSON 계약, 인증 오류별 무자동재시도 테스트가 모두 통과한다.

### 단계 B. 미디어 등록 인증

- 대상: R3. 단계 A와 모듈 작업은 분리할 수 있지만 통합 검증은 A 이후 수행한다.
- 변경 위치: `erd-app/src/session.rs`, `erd-host/src/session.rs`, 필요 시 `erd-proto` 등록 메시지와 `erd-net` datagram 계약.
- compatibility 결정: 클라이언트와 호스트를 함께 갱신한다. 구형 평문 probe를 몰래 허용하는 fallback을 두지 않는다. wire 변경의 capability 또는 protocol-version 협상 방식은 구현 전 확정한다.
- 단계 종료: 위조·이전 세션 등록은 모두 거부되고 새 세션의 올바른 수신 소켓에서 프레임이 관찰된다.

### 단계 C. 저장 페어링과 호스트 선택 통합

- 대상: R8, R9, R10.
- 선행 조건: 단계 A의 저장소 역할 및 공개 DTO가 정해져야 한다.
- 변경 위치: 데스크톱 `list_hosts`/`connect`, iOS discovery/commands/state, `erd-app` 저장 메타데이터, `erd-net` Apple endpoint 처리.
- UX: 새 호스트는 PIN 입력과 승인 대기, 저장 호스트는 키로 재접속, 폐기된 키는 이유와 명시적 재페어링 행동을 제공한다. 발견 실패가 직접 접속을 막으면 안 된다.
- 선택된 호스트의 주소·포트는 제출 시 최신 발견 레코드와 대조한다. 광고 정보만으로 기존 키를 다른 피어에 결속하지 않는다.
- 단계 종료: 앱 재시작·주소 변경·동명 호스트·LAN/Tailscale 전환·IPv6 scope 조건의 키 선택과 접속 결과가 계약에 맞는다.

### 단계 D. 접속·취소·종료 상태 정합성

- 대상: R5, R6, R7.
- 단계 A와 일부 병렬 작업 가능. 공통 오류 DTO는 단계 A/C와 공유한다.
- 변경 위치: iOS native session owner, 두 플랫폼 connection-state 계약, iOS `app.js`·`index.html`.
- 상태 흐름: `idle -> connecting -> awaiting-consent -> waiting-video -> streaming -> disconnecting -> idle`. 오류는 종료 이유와 자원 소유 상태를 보존하며, `cleanup-failed`는 재시도 전까지 성공으로 취급하지 않는다.
- UI: 연결 중 Cancel을 항상 보이게 하고 포커스·입력 소유권을 분리한다. PIN은 요청 후 UI에서 제거하며 상태 snapshot·로그에 남기지 않는다.
- 단계 종료: connect 대기, 승인 거절, 원격 TCP 종료, native error, 정리 실패, 중복 종료, 늦은 이전 generation 완료의 결정적 테스트가 통과한다.

### 단계 E. 실제 표면 통합 검증

- A-D 완료 후 Omarchy에서 관련 Rust tests, workspace tests, 대상 build를 수행한다. Rust `cargo check/test/build`는 Omarchy 전용이라는 기존 제약을 유지한다.
- UI는 Bun으로 실제 사용하는 runner 계약을 실행한다. 순수 상태 테스트와 실제 DOM 테스트를 분리한다.
- Linux·Windows 호스트 및 Mac 클라이언트의 접속·재접속·입력·스크린샷은 CLI/API/MCP 경로로 확인한다. 지속 배포와 게시 여부는 별도 승인 범위다.
- iOS는 physical iPhone으로 최초 페어링, 승인 대기/취소, 앱 재시작 후 재접속, 원격 종료, 오디오 오류 후 정리, 네트워크 변경을 확인한다. simulator/emulator는 사용하지 않는다. iOS 컴파일·서명은 기존 실기기용 Mac 예외 범위에서 수행한다.
- 각 검증에는 소스 버전, 플랫폼, 명령과 exit code, 실제 받은 이벤트/프레임, 종료 후 잔여 작업 여부를 기록한다.

## 6. 회귀·통합 검증 매트릭스

| 시나리오 | 검증 계층 | 통과 조건 |
| --- | --- | --- |
| 아웃바운드 키를 로컬 호스트에서 사용 | 실제 store + loopback TLS | 인바운드 인증 거부 |
| bootstrap에서 승인 없이 기존 ID 제시 | loopback TLS + consent/input 관찰 | 승인 우회·입력·캡처 시작 없음 |
| 승인한 ID와 다른 handshake | 프로토콜/호스트 통합 | terminal 거부 |
| 현재·이전 세션 UDP 등록 혼재 | 실제 UDP 소켓 | 현재 세션의 인증된 endpoint만 선택 |
| 목록 IPC 호출 | native 명령 직렬화 | 비밀키 없는 메타데이터 |
| 저장 키 재접속 거부·timeout | client 통합 | 원인 보존, 자동 bootstrap 없음 |
| 동명 호스트·주소 변경 | 저장소 + discovery + connect | 키 선택의 모호함을 자동 승인하지 않음 |
| iOS 앱 재시작 후 같은 호스트 재접속 | 실기기 | 저장 키 사용, 불필요한 PIN 재입력 없음 |
| TCP 종료 후 UDP 무응답 | runtime + mobile state | 정확한 terminal 전파와 정리 |
| stats error 후 disconnect/dismiss | JS 상태 + native owner | 세션 소유 중이면 정리 수행 |
| cleanup 거부·중복 호출·늦은 connect | deferred promise 및 명시적 이벤트 | 한 정리 owner, 실패 보존, 새 세션 오염 없음 |
| connecting/disconnecting 모달 | 실제 DOM + 실기기 | 표시·Cancel·포커스·배경 입력 차단 |
| scoped IPv6-only 발견 | parser-to-publish + 실제 resolver | scope 보존, 유효한 호스트 표시·접속 |

시간 자체가 검증 대상이 아닌 테스트에는 고정 sleep이나 반복 polling을 넣지 않는다. 네트워크·worker·UI 전이는 트리거 전에 완료 이벤트를 구독하고 bounded timeout 안에 관찰한다. 가짜 invoke의 성공은 실제 TLS·Keychain·영상·터치 성공으로 계산하지 않는다.

## 7. 이번 리뷰에서 실행한 검증과 한계

### 실행 결과

```bash
bun test clients/rust/tauri-shell/ui/connection-state.test.mjs \
  clients/rust/tauri-shell/ui/library.test.mjs
# 24 pass, 0 fail

bun test clients/rust/ios-shell/ui/test/connection-state.test.mjs \
  clients/rust/ios-shell/ui/test/discovery.test.mjs
# 28 pass, 0 fail, exit 0
```

데스크톱 테스트는 복합 명령의 첫 단계에서 통과했다. 이어 실행한 아래 Node 명령은 테스트 본문에 진입하기 전에 실패해 복합 명령 전체 exit code는 1이었다.

```bash
node --test clients/rust/ios-shell/ui/test/connection-state.test.mjs \
  clients/rust/ios-shell/ui/test/discovery.test.mjs
```

Node.js `v22.22.3`에서 CommonJS의 named export 탐지가 실패했다.

```text
SyntaxError: Named export 'createConnectionManager' not found.
SyntaxError: Named export 'attachLifecycle' not found.
```

같은 두 파일을 Bun `v1.4.0`으로 다시 실행해 28개 모두 통과했다. 따라서 Node 실패는 이번 문서 변경에 의한 회귀나 iOS 실행 결과가 아니라 기존 테스트 모듈/runner 호환성 문제다. 계획의 검증 명령은 Bun으로 명시한다.

추가로 eval에서 실제 iOS JS 모듈을 import하여 R6의 오류 후 정리 생략, 정리 실패 후 idle, R9의 PIN 없는 카드 접속 거부를 재현했다. 파일에 회귀 테스트를 추가하거나 제품 코드를 변경하지는 않았다.

### 검증하지 않은 사항

- Rust compile/test 및 전체 build는 실행하지 않았다. 이번 요청은 리뷰·계획서이며, 코드 변경 검증을 수행한 작업이 아니다.
- LSP 심볼 조회는 `LSP daemon unreachable`로 실패했다. 원문 파일과 호출 경로를 직접 읽어 대조했으며, LSP diagnostics 통과를 주장하지 않는다.
- 실제 호스트 공격·침투 재현, 영상 전송·입력 E2E, Keychain 실기기 재접속, 모달 스크린샷 QA는 수행하지 않았다.
- iOS 모달 결함은 정적 HTML/CSS/상태 증거다. JS 대역 재현과 실제 iPhone 런타임 증거를 구분한다.
- 동시 작업의 WebGL 수정은 검증하지 않았다. 기존 성능 측정의 UDP 손실·Windows 인코딩 지연을 R3의 결과라고 연결하지 않는다.

## 8. 구현 전 확정할 결정

1. 기존 공용 페어링 파일의 이관: 모호한 권한은 자동 승인하지 않고, 기존 키 보존 및 호스트 재승인 절차를 제공하는 안을 권장한다.
2. R11의 고정 PIN 요구: 제품 기본 정책인지 명시적 개발 환경 정책인지 확인해야 한다. 기본 자동 재페어링은 권장하지 않는다.
3. UDP 등록 변경: capability/version 협상과 구형 클라이언트의 명시적 실패 방식을 정한다.
4. 호스트 신원: 로컬 credential ID와 인증 후 확인한 endpoint 별칭을 최소 계약으로 삼고, 광고 이름을 신뢰 근거로 사용하지 않는다. 장기 호스트 공개키 체계 도입 여부는 별도 설계 범위다.

권장 착수 순서는 **A -> B -> C/D 통합 -> E**다. C와 D는 인터페이스 합의 후 병렬로 진행할 수 있다. 이 문서는 구현 승인이나 배포 승인을 대신하지 않으며, 위 완료 조건을 충족하기 전에는 문제를 해결된 것으로 표시하지 않는다.
