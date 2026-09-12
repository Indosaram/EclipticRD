# MahoRD AI Agent Remote Desktop Architecture & Capability Audit

> 기준 리비전: 2026-09-09 현재 작업 트리  
> 조사 범위: 활성 Rust 워크스페이스 `clients/rust/`의 `maho-app`, `maho-host`, `maho-net`, `maho-proto`, `tauri-shell` 및 관련 검증 문서  
> 목적: AI 에이전트가 MahoRD를 통해 원격 데스크톱을 인지하고 조작하는 전체 경로를 코드 기준으로 분석하고, 기능 공백·안전성·신뢰성·에이전트 사용성 문제를 우선순위화합니다.

## 1. 결론 요약

MahoRD는 이미 **에이전트가 GUI 클라이언트를 띄우지 않고 원격 화면을 보고 입력을 보낼 수 있는 실질적인 headless client 경로**를 갖추고 있습니다. `maho-client --agent-server`는 loopback HTTP API를, `maho-client --mcp`는 stdio MCP를 제공하며, 둘 다 Tauri/WebKit 없이 `maho-app`의 `ClientSession`에 직접 연결됩니다. 원격 전송은 TLS 1.2 PSK 기반 TCP control/input과 세션 키에서 파생된 UDP media 암호화로 구성되어 있고, 8자리 PIN bootstrap은 PBKDF2-HMAC-SHA256 600,000회와 5회 실패 lockout을 사용합니다. macOS ScreenCaptureKit, Windows DXGI Desktop Duplication, Linux wlroots screencopy와 각 OS 입력 주입기도 실제 구현되어 있습니다.

다만 현재 형태는 **“video frame + blind coordinate injection”을 에이전트 표면에 얹은 1세대 computer-use interface**에 가깝습니다. 안정적인 자율 에이전트 플랫폼으로 사용하려면 다음 문제가 우선 해결되어야 합니다.

1. **Critical — loopback HTTP 제어 API에 호출자 인증이 없습니다.** `127.0.0.1` 제한은 있지만 bearer/capability token, peer credential, Origin 검증이 없고 응답에는 `Access-Control-Allow-Origin: *`가 붙습니다. 로컬 다른 프로세스/사용자 또는 브라우저 기반 공격 표면이 원격 PC 입력 권한으로 이어질 수 있습니다.
2. **Major — “입력 성공”이 실제 OS 입력 성공을 의미하지 않습니다.** 호스트는 `SendInput`, macOS Accessibility, Linux uinput 주입 실패를 로그만 남기고 클라이언트에 ACK하지 않습니다. 에이전트는 UIPI/권한/rate-limit 실패에도 성공 응답을 받을 수 있습니다.
3. **Major — high-DPI에서 screenshot pixel 좌표와 agent screen-info 좌표가 불일치할 수 있습니다.** HandshakeAck는 logical width/height를 제공하지만 headless screenshot은 decode된 physical frame 크기를 반환합니다. Windows 125%, Retina 2x 같은 환경에서 vision 모델이 본 픽셀 좌표를 그대로 클릭하면 오프셋 위험이 있습니다.
4. **Major — 멀티 모니터가 agent-facing 개념으로 모델링되어 있지 않습니다.** monitor list/select API가 없으며 Windows는 기본 capture가 display 0인데 input은 기본적으로 전체 virtual desktop에 매핑되고, Linux는 선택 output의 global origin/desktop geometry를 input injector에 전달하지 않습니다.
5. **Major — Unicode/IME 입력이 없습니다.** `TypeText`는 ASCII만 물리 keycode로 합성하며 한글·일본어·이모지 등은 조용히 건너뜁니다. `paste_mode`, `delay_ms`, `hold_ms`, drag `duration_ms`도 현재 동작하지 않습니다.
6. **Major — 입력 안전장치가 불완전합니다.** 5초 `InputStateTracker::check_timeout()`는 정의되어 있지만 호출되지 않으며, Tauri disconnect는 tracker를 먼저 `clear()`한 뒤 `Reset`만 보내므로 실제 held key를 잃을 수 있습니다. macOS host에서 `Reset` 자체는 no-op입니다.
7. **Major — 에이전트 피드백 루프가 약합니다.** action response는 이벤트 전송 개수만 반환하며 cursor 위치, 실제 injection ACK, 새 frame id, screen diff, wait-for-change를 제공하지 않습니다.
8. **Major — 현재 session model은 단일 host/단일 active session입니다.** Tauri는 connect 전에 기존 session을 disconnect하고, headless `maho-client`도 CLI에서 host를 고정합니다. HostServer도 connection을 직렬 처리합니다. 자동 reconnect/backoff와 agent-facing host discovery/session switch API도 없습니다.
9. **Improvement — 접근성 트리, 파일 전송, agent audio, agent clipboard, OCR/semantic hit target 같은 현대 computer-use 보조 채널이 없습니다.** 코드 검색상 Windows UI Automation 및 Linux AT-SPI 연동은 없고, clipboard/audio 기반 기능은 내부 전송 능력이 있어도 agent MCP/HTTP 표면에는 노출되지 않습니다.

즉, **원격 전송·캡처·입력이라는 기반은 충분히 존재하지만, 에이전트에게 필요한 좌표 계약, 확정적 action acknowledgement, semantic perception, session orchestration, 안전한 local control plane이 아직 부족합니다.**

---

## 2. 심각도 기준

| 등급 | 의미 |
| --- | --- |
| **Critical** | 보안 경계 우회, 원격 조작 권한 노출, 광범위한 안전 문제처럼 배포 전 차단이 필요한 항목 |
| **Major** | 에이전트가 잘못된 화면을 클릭하거나 성공을 오판하고 stuck input을 만들 수 있는 핵심 신뢰성/기능 문제 |
| **Minor** | 특정 환경의 오류, API 일관성, 운영/디버깅 품질 문제 |
| **Improvement** | 현재 기능은 동작하지만 modern computer-use 수준으로 올리기 위해 권장되는 확장 |

---

## 3. 전체 아키텍처

### 3.1 Agent-facing 경로

```text
AI Agent
  ├─ stdio MCP
  │    └─ maho-client --mcp
  │         └─ mcp_stdio.rs -> mcp_dispatch.rs -> AgentServerBackend
  │
  ├─ loopback HTTP/1.1
  │    └─ maho-client --agent-server [PORT]
  │         └─ agent_server.rs -> AgentServerBackend
  │
  └─ Tauri IPC
       └─ tauri-shell commands
            ├─ agent_execute_action
            ├─ agent_get_screen_info
            ├─ agent_capture_screen
            └─ agent_release_all

AgentServerBackend / Tauri AppState
  └─ maho_app::ClientSession
       ├─ TCP: TLS-PSK pairing, handshake, input, control, clipboard, heartbeat
       └─ UDP: encrypted video/audio/cursor datagrams

Remote maho-host
  ├─ macOS: ScreenCaptureKit + CoreGraphics input
  ├─ Windows: DXGI Desktop Duplication + SendInput
  └─ Linux: wlroots screencopy + /dev/uinput
```

주요 코드 위치:

- CLI/headless lifecycle: `clients/rust/maho-app/src/bin/maho_client.rs`
- HTTP agent server: `clients/rust/maho-app/src/agent_server.rs`
- Agent action translation/screenshot encoding: `clients/rust/maho-app/src/agent_input.rs`
- MCP schema/dispatch/stdio: `clients/rust/maho-app/src/mcp_server.rs`, `mcp_dispatch.rs`, `mcp_stdio.rs`
- Client transport/session: `clients/rust/maho-app/src/session.rs`
- Wire input/handshake: `clients/rust/maho-proto/src/input.rs`, `handshake.rs`
- TLS-PSK/bootstrap: `clients/rust/maho-net/src/tls_psk.rs`
- Host lifecycle/media/input: `clients/rust/maho-host/src/session.rs`
- macOS capture/input: `capture_macos.rs`, `inject_macos.rs`
- Windows capture/input/geometry: `capture_windows.rs`, `inject_windows.rs`, `windows_logic.rs`
- Linux capture/input: `capture_linux.rs`, `inject_linux.rs`
- Tauri orchestration/discovery: `clients/rust/tauri-shell/src-tauri/src/lib.rs`

### 3.2 Capture → perception 경로

```text
Host desktop
 -> native capture (BGRA)
 -> NV12 / encoder
 -> encrypted UDP fragments
 -> ClientSession assembly
 -> decoder
 -> latest NV12 frame cache
 -> PNG/JPEG encoder
 -> base64 HTTP JSON OR MCP image content
 -> vision model
```

중요한 점은 `/screen/screenshot`과 `remote_take_screenshot`이 **호스트에 새 screenshot을 요청하는 API가 아니라, 연속 video stream에서 가장 최근에 decode된 frame을 다시 이미지로 인코딩하는 API**라는 점입니다.

### 3.3 Action → input 경로

```text
AgentAction JSON/MCP args
 -> convert_agent_action_to_events()
 -> normalized wire InputEvent
 -> ClientSession::send_input()
 -> TLS TCP frame
 -> maho-host InputEvent::decode()
 -> platform injector
 -> OS event subsystem
```

현재 성공 응답은 마지막 단계인 **OS가 이벤트를 실제 적용했는지 확인하지 않습니다.** 이 차이는 자율 에이전트 신뢰성에서 가장 중요한 구조적 gap 중 하나입니다.

---

## 4. 원격 접속 및 세션 라이프사이클

### 4.1 Headless client 지원

**현황 — 지원됨.**

`clients/rust/maho-app/Cargo.toml`은 Tauri 의존성이 없고 `maho-client` binary를 직접 빌드합니다. `maho_client.rs`는 연결/decoder/media worker를 시작한 뒤 `--agent-server`면 자체 Tokio runtime의 HTTP listener를, `--mcp`면 별도 thread에서 stdio MCP를 실행합니다. 따라서 에이전트 실행 측에는 Tauri/WebKit/desktop UI가 필요하지 않습니다.

**제약 — remote host까지 완전한 GUI-independent headless는 아닙니다.**

- macOS host는 Screen Recording + Accessibility 권한과 로그인된 GUI session이 필요합니다.
- Windows Desktop Duplication 및 SendInput도 interactive desktop/session과 integrity level 영향을 받습니다.
- Linux는 Wayland compositor와 wlroots screencopy가 필요하며 `/dev/uinput` 권한이 필요합니다.
- Linux의 `--output`/Hyprland headless output 조합은 서버용으로 사용할 수 있지만 compositor 자체는 필요합니다.

**평가: Improvement**

권장사항:

- `maho-client agent serve`와 `maho-host service`를 명확히 분리한 headless deployment guide를 제공합니다.
- startup preflight API에서 capture/input capability와 필요한 OS permission을 구조화하여 반환합니다.
- Linux에는 XDG Desktop Portal/PipeWire fallback을 실제 구현합니다. 현재 `capture_linux.rs`는 unsupported compositor에서 `PortalRequired` 오류를 정의하지만 fallback 자체는 없습니다.

### 4.2 Pairing 및 TLS-PSK

**현황 — 설계가 비교적 견고합니다.**

- `maho-net/src/tls_psk.rs`
  - TLS 1.2 PSK cipher: `PSK-AES128-GCM-SHA256`, `PSK-AES256-GCM-SHA384`
  - bootstrap identity `maho-b1`, paired identity `maho-p1.<pairing-id>`
  - 8자리 PIN을 고정 salt `erd/bootstrap/v3`와 PBKDF2-HMAC-SHA256 600,000회로 stretch 후 HKDF로 TLS PSK를 생성합니다.
  - `MAX_PAIRING_ATTEMPTS = 5`, 60초 failure window, lockout 300초입니다.
- `maho-host/src/session.rs`
  - 기본 pairing window는 `PAIRING_WINDOW = 300s`입니다.
  - bootstrap identity는 pairing window와 lockout이 모두 허용할 때만 TLS PSK set에 포함됩니다.
  - PairingRequest 후 host consent channel 승인이 필요하고, 승인되면 random 32-byte key와 UUID pairing ID를 저장합니다.
  - 이후 Handshake에서 TLS-negotiated identity와 payload pairing ID를 교차 검증합니다.

**한계:** `BootstrapLockout::record_failure()`는 TLS accept 실패 전체를 bootstrap failure처럼 기록합니다. 이미 paired client의 잘못된 PSK/손상 handshake도 pairing lockout counter에 영향을 줄 가능성이 있으므로 bootstrap identity 실패인지 구분하는 것이 더 정확합니다.

**평가: Minor**

권장사항:

- TLS callback 단계에서 offered identity가 bootstrap일 때만 bootstrap failure를 카운트합니다.
- pairing/audit log에 reason code를 남기되 PIN/PSK는 절대 기록하지 않습니다.

### 4.3 인증 실패의 agent error 전달

**현황:** `maho-client`는 먼저 pairing/reconnect/handshake를 완료한 뒤에만 AgentServerBackend와 MCP/HTTP worker를 시작합니다.

따라서 잘못된 PIN, pairing not found, TLS failure, handshake timeout은 좋은 Rust/CLI error string으로는 나타나지만 **agent가 연결된 MCP/HTTP endpoint에서 구조화된 session/auth error로 받을 수는 없습니다.** 서버 자체가 아직 열리지 않았기 때문입니다.

**평가: Major**

권장사항:

- agent daemon lifecycle을 “server 먼저 실행 → `disconnected/connecting/pairing/ready/error` state machine”으로 바꿉니다.
- MCP에 `remote_session_status`, `remote_connect`, `remote_pair`, `remote_disconnect`를 제공합니다.
- HTTP에는 `/api/v1/sessions`, `/api/v1/sessions/{id}/connect` 형태의 orchestration 계층을 둡니다.
- error response에 stable code (`PAIRING_REJECTED`, `PAIRING_NOT_FOUND`, `TLS_AUTH_FAILED`, `HANDSHAKE_TIMEOUT`)와 retryability를 제공합니다.

### 4.4 Reconnect

**현황:** 저장된 pairing을 이용한 명시적 reconnect는 구현되어 있습니다. `maho-client` startup과 Tauri `connect()`가 PairingStore에서 record를 찾아 `ClientSession::reconnect()`를 호출합니다.

**부족한 점:** 연결이 끊긴 뒤 automatic reconnect/backoff/resume은 없습니다. TCP runtime/UDP receiver 오류 후 agent가 session을 계속 보유하면서 다시 dial하는 state machine이 없습니다.

또한 CLI와 Tauri에는 테스트 환경으로 보이는 특정 host/IP 이름 매칭이 코드에 하드코딩되어 있습니다.

- `maho-app/src/bin/maho_client.rs`: `100.91.254.71` ↔ `indo`
- `tauri-shell/src-tauri/src/lib.rs`: 위 매칭과 별도 `100.126.171.58`/`DESKTOP` 매칭

이 로직은 다른 사용자의 실제 host와 잘못된 pairing record를 연결할 수 있고 product code에 환경 의존성을 남깁니다.

**평가: Major**

권장사항:

- 하드코딩된 IP/name 예외를 제거합니다.
- pairing record에 stable host identity/public fingerprint를 저장하고 discovery advertisement와 결합합니다.
- reconnect policy: exponential backoff + jitter, max elapsed time, network-change trigger, session generation counter를 둡니다.
- reconnect 후 화면 geometry/capabilities를 반드시 재협상하고 이전 input tracker를 폐기합니다.

### 4.5 Discovery: mDNS/Bonjour 및 Tailscale

**현황:** `maho-net/src/discovery.rs`는 Apple 플랫폼에서 DNSService 계열, Linux/Windows에서 mdns-sd를 사용하여 `_maho-rd._tcp`를 발견합니다. TXT에는 protocol, name, os, udp_port가 포함됩니다.

Tauri의 `list_hosts_internal()`은 LAN 결과와 `tailscale status --json` 결과를 병합합니다. 코드 주석도 명시하듯 Tailscale 결과는 **Tailscale peer가 존재한다는 신호일 뿐 MahoRD service readiness 증명이 아닙니다.** Tailscale 결과에는 MahoRD tcp/udp port가 없고 기본 port 사용을 기대합니다.

**Agent gap:** 이 discovery surface가 `maho-client --mcp`나 HTTP API에는 노출되지 않습니다. headless agent는 실행 시 `--host`를 이미 알아야 합니다.

**평가: Major**

권장사항:

- MCP `remote_list_hosts`, `remote_probe_host`를 추가합니다.
- Tailscale peer마다 짧은 authenticated service probe 또는 signed MahoRD discovery record를 사용합니다.
- LAN/Tailscale merge key를 IP가 아니라 stable host identity로 바꿉니다.
- discovery result에 `discovered`, `reachable`, `paired`, `authenticated`, `last_seen`, ports를 구분합니다.

### 4.6 Multi-host / multi-session

**현황:** 현재 client model은 단일 active `ClientSession`입니다.

- Tauri `connect()`는 lifecycle lock을 잡고 `disconnect_internal()`을 먼저 호출합니다.
- headless `maho-client`도 하나의 CLI `host`와 하나의 backend만 구성합니다.
- `HostServer::serve()`는 `serve_next()`를 호출하고, accepted connection을 같은 thread에서 `handle_connection()`이 끝날 때까지 처리한 뒤 다음 `accept()`로 갑니다. 즉 host 역시 실질적으로 동시 client session을 하나만 처리합니다.

**평가: Major**

권장사항:

- agent daemon에 `SessionManager<HashMap<SessionId, ClientSession>>`를 도입합니다.
- 모든 agent tool에 optional `session_id`를 둡니다.
- host는 accept loop와 session worker를 분리하고 max concurrent session 정책을 명시합니다.
- 한 host의 concurrent control ownership을 허용할지, observer/control lease로 나눌지 정의합니다.

---

## 5. 시각 인지 및 Screen Capture

### 5.1 Screenshot API의 실제 의미

**HTTP:** `GET /api/v1/screen/screenshot[?format=jpeg]`  
**Tauri:** `agent_capture_screen`  
**MCP:** `remote_take_screenshot`

모두 `latest decoded NV12 frame`을 사용합니다. 새 캡처를 동기 요청하지 않습니다.

`agent_input.rs`의 `encode_nv12_screenshot()`은 NV12를 RGB로 전체 변환 후 `image` crate를 사용하여 PNG 또는 JPEG로 인코딩합니다.

- PNG: 기본 HTTP/MCP 포맷
- JPEG: quality 80 고정
- raw NV12 agent output 없음
- WebP/AVIF 없음
- region crop 없음
- max dimensions/downscale 없음
- JPEG quality parameter 없음

**평가: Major**

이 설계는 3840×1600/4K에서 model 입력 비용과 CPU encode 비용을 불필요하게 키웁니다. HTTP의 base64 JSON은 바이너리보다 약 33% payload overhead도 추가합니다.

권장사항:

```text
capture_screen(
  session_id,
  monitor_id?,
  max_width=1280,
  max_height=1280,
  format="jpeg",
  quality=70,
  region={x,y,w,h}?,
  include_cursor=true,
  min_frame_id?,
  max_age_ms=250?
)
```

- vision용 기본값은 1280~1600급 downscale을 권장합니다.
- 클릭 정밀도가 필요한 경우 crop/region을 사용해 원본 pixel detail을 유지합니다.
- 응답에 `frame_id`, `captured_at`, `decoded_at`, `age_ms`, `pixel_width/height`, `logical_width/height`, `scale_x/y`를 포함합니다.
- HTTP는 binary image response 또는 multipart를 지원하여 base64를 선택 사항으로 만듭니다.

### 5.2 NV12 → RGB 비용 및 color fidelity

현재 변환은 CPU nested loop로 YUV를 RGB로 바꿉니다. 매 screenshot마다 full-resolution RGB buffer와 encoded output을 새로 만듭니다. 연속 screenshot polling 시 video decode와 별도로 CPU/메모리 bandwidth가 커집니다.

또한 변환 상수는 color primaries/range metadata를 입력받지 않습니다. BT.601/BT.709, limited/full range 차이를 explicit하게 처리하지 않으므로 색상 기반 UI 판단의 fidelity가 보장되지 않습니다.

**평가: Minor**

권장사항:

- decoder output에 color space/range metadata를 보존합니다.
- SIMD/GPU conversion 또는 libyuv 계열 최적화를 고려합니다.
- 동일 frame_id에 대한 PNG/JPEG 결과를 짧게 cache합니다.

### 5.3 DPI / logical vs physical coordinate mismatch

**핵심 문제입니다.**

Host HandshakeAck는 `DisplayInfo.logical_width`, `logical_height`, `scale`을 보냅니다. 반면 agent screenshot은 실제 decode frame의 `width`, `height`를 사용합니다.

- macOS `capture_macos.rs`: main display의 CoreGraphics logical bounds와 native pixel dimensions를 별도로 계산합니다. Retina에서는 scale > 1입니다.
- Windows `windows_logic.rs`: DXGI output desktop coordinates를 logical size로, duplication ModeDesc를 physical pixel size로 분리합니다. 저장된 검증 문서에서도 125% DPI의 3072×1280 logical / 3840×1600 physical 사례가 확인되었습니다.
- CLI `ClientBackend::get_screen_info()`는 HandshakeAck logical dimensions를 반환합니다.
- CLI screenshot은 decoded physical frame dimensions를 반환합니다.

따라서 `remote_get_screen_info = 3072×1280`인데 `remote_take_screenshot = 3840×1600`이 될 수 있습니다.

**평가: Major**

권장사항:

- `ScreenGeometry` 계약을 다음처럼 명확히 바꿉니다.

```json
{
  "monitor_id": "primary",
  "logical": {"width": 3072, "height": 1280},
  "physical": {"width": 3840, "height": 1600},
  "scale": {"x": 1.25, "y": 1.25},
  "input_space": "logical",
  "screenshot_space": "physical",
  "origin": "top-left"
}
```

- 더 좋은 방법은 agent-facing input을 **screenshot pixel space**로 통일한 뒤 transport 직전 platform logical/absolute로 변환하는 것입니다.
- 모든 screenshot 응답에 해당 frame의 exact geometry snapshot을 넣어 display mode 변경 중 stale geometry를 방지합니다.

### 5.4 Y-axis 규약

`agent_input.rs::normalize_agent_coordinates()`는 agent top-left Y를 wire에서 `1.0 - y`로 뒤집습니다. `maho-proto::normalize_client_coordinates()`도 legacy macOS bottom-left wire convention을 문서화합니다.

- Windows `normalize_absolute_pointer()`는 wire Y를 다시 뒤집어 top-left absolute desktop으로 사용합니다.
- Linux `map_normalized_to_output()`도 다시 뒤집습니다.
- macOS CoreGraphics 경로는 wire convention을 그대로 host pixel mapping에 사용합니다.

현재 각 backend가 이 convention을 알고 있어 단일 display에서는 동작하지만, “wire가 macOS-origin 관례를 강제”하는 구조는 새로운 platform/semantic layer가 추가될수록 실수 가능성을 높입니다.

**평가: Minor**

권장사항:

- protocol vNext에서 좌표 origin을 top-left로 표준화합니다.
- backward-compatible decode adapter에서만 legacy Y flip을 처리합니다.
- geometry object에 origin/coordinate-space를 명시합니다.

### 5.5 Multi-monitor capture

#### macOS

`MacScreenCapture`는 `CGDisplay::main()`과 main display ID를 선택합니다. agent가 monitor 목록을 보거나 다른 display로 전환하는 API가 없습니다.

#### Windows

`WindowsMediaSource`는 `display_index: 0`으로 생성됩니다. `WindowsCapture` 자체는 `select_display()` 기능이 있지만 agent surface에서 사용되지 않습니다.

더 심각하게 `handle_connection()`의 `WindowsInputInjector::new(None)`은 target을 **전체 virtual desktop**으로 설정합니다. 영상은 display 0인데 `(0..1, 0..1)` 입력은 virtual desktop 전체에 매핑될 수 있습니다. 두 모니터가 좌우로 있을 경우 screenshot 중앙 클릭이 두 모니터 합성 desktop의 중앙으로 갈 수 있습니다.

#### Linux

Capture는 `--output`/`MAHO_OUTPUT`/Hyprland focused output으로 선택할 수 있습니다. 그러나 input injector는 `OutputGeometry::single_output(pixel_width, pixel_height)`로 생성되어 실제 output의 compositor global `x/y`와 전체 desktop dimensions를 잃습니다. non-origin monitor 선택 시 잘못된 output으로 입력이 갈 수 있습니다.

**평가: Major**

권장사항:

- protocol에 `DisplayDescriptor {id,name,logical_rect,physical_rect,scale,rotation,primary}` 목록을 추가합니다.
- `remote_list_displays`, `remote_select_display`를 제공합니다.
- Windows injector에 captured `TargetDisplay`를 전달합니다.
- Linux Hyprland/wl_output geometry의 global origin과 total desktop size를 `OutputGeometry`에 전달합니다.
- display rotation도 구현합니다. 현재 Windows metadata resolver는 90/180/270 회전을 명시적으로 unsupported 처리합니다.

### 5.6 Cursor visibility / position

플랫폼별 상태가 다릅니다.

- **macOS:** ScreenCaptureKit config에서 `setShowsCursor(true)`이므로 cursor가 screenshot pixels에 합성됩니다.
- **Linux:** screencopy `CaptureConfig::default().overlay_cursor = true`이므로 기본 화면에는 cursor가 합성됩니다.
- **Windows:** DXGI frame은 pointer position/visibility metadata를 별도로 받고 host가 `MediaEvent::Cursor(CursorUpdate)`로 보냅니다. 영상 BGRA에 cursor shape를 합성하는 코드는 없습니다.

Tauri는 `latest_cursor`를 유지하고 `get_cursor_position` command도 있습니다. 하지만 headless `maho-client` UDP receive loop는 `SessionEvent::Frame`과 `Ping`만 처리하고 `Cursor`는 `Ok(_) => {}`로 버립니다. 따라서 Windows MCP/HTTP agent는 cursor 위치를 알 방법이 없고 screenshot에도 cursor가 없습니다.

또한 Tauri raw-frame buffer 끝에 cursor x/y/type을 append하지만 `agent_capture_screen`의 screenshot encoder는 NV12 길이만 소비하므로 cursor가 이미지로 합성되는 것은 아닙니다.

**평가: Major**

권장사항:

- headless backend도 `latest_cursor`를 보관합니다.
- `remote_get_pointer_state` / `/api/v1/pointer`를 추가합니다.
- screenshot option `include_cursor=true`에서 cursor bitmap/position을 compositing합니다.
- Windows는 DXGI pointer shape metadata까지 캡처해 실제 shape를 합성합니다. 현재는 type=1 정도의 단순 상태만 전송합니다.

### 5.7 화면 변화 감지 / wait-for-change

Host는 지속 video stream을 보내고 client가 최신 frame을 유지하지만 agent API는 **polling snapshot only**입니다.

없음:

- `wait_until_frame_changes`
- previous frame hash/diff
- dirty-region return
- frame sequence/freshness constraint
- WebSocket/SSE frame/event stream

Windows DXGI에는 dirty/move rectangles가 이미 있고 Linux screencopy에도 damage rect가 있지만 이 정보는 encoder/agent layer까지 전달되지 않습니다.

`maho-client --nudge-ms`는 static compositor가 frame을 잘 내지 않을 때 작은 mouse move를 주기적으로 보내는 보조 장치입니다. agent perception 자체의 change notification은 아닙니다.

**평가: Major**

권장사항:

- decoded frame마다 monotonic `frame_id`와 content hash를 생성합니다.
- `wait_for_screen_change(after_frame_id, timeout_ms, min_changed_ratio)`를 구현합니다.
- platform dirty/damage metadata를 가능하면 보존하고, 없으면 downscaled luminance diff를 계산합니다.
- action API에 `wait_after_action` 옵션을 두어 “입력 → 변화 대기 → screenshot”을 서버에서 한 round trip으로 묶습니다.

### 5.8 실제 latency 자료

저장소의 `docs/agent-first-verification-20260909.md` 및 `docs/remaining-performance-verification-20260909.md`에는 실제 장비 측정이 있습니다.

- Linux 120-frame run: host encode p95 약 63~82 ms, decode p95 약 6.6 ms였고 active run에서 13개 sequence gap이 관측된 과거 자료가 있습니다.
- 후속 MTU-safe 300-frame run에서는 1200-byte UDP payload limit으로 fragmentation 없이 packet gap 0이 관측되었습니다.
- Windows 300-frame finite run: content age p95 약 29.9 ms, fresh residence p95 약 16.9 ms, NV12 conversion p95 약 15.0 ms, encode wall p95 약 24.4 ms입니다.

이 수치는 **capture-to-agent screenshot latency 전체**가 아닙니다. model inference와 screenshot re-encode, MCP/HTTP serialization, action round trip, 다음 frame까지의 waiting은 별도입니다.

**평가: Improvement**

권장사항:

- 하나의 `AgentLoopTrace`에 `capture_at -> encode -> send -> receive -> decode -> screenshot_encode -> tool_response -> action_send -> host_inject -> next_frame` timestamp를 연결합니다.
- `remote_get_metrics`로 p50/p95와 현재 frame age를 agent/operator에게 제공합니다.

---

## 6. 원격 조작 및 Input Control

### 6.1 지원 액션

`AgentAction`이 지원하는 표면:

- MouseMove
- MouseDown / MouseUp
- Click (`left/right/middle`, count)
- Drag (start/end/button/steps/duration_ms)
- Scroll (dx/dy, optional position)
- KeyDown / KeyUp
- KeyPress (hold_ms)
- Hotkey
- TypeText (delay_ms, paste_mode)
- ReleaseAll

MCP는 이 중 고수준 10개 도구만 노출합니다.

- `remote_mouse_click`
- `remote_mouse_move`
- `remote_mouse_drag`
- `remote_mouse_scroll`
- `remote_key_press`
- `remote_hotkey`
- `remote_type_text`
- `remote_release_all`
- `remote_get_screen_info`
- `remote_take_screenshot`

HTTP `/input/action`은 raw `AgentAction`을 받으므로 MCP보다 세밀한 down/up이 가능합니다.

### 6.2 Hotkey 지원

`parse_hotkey_string()`은 Ctrl/Shift/Alt/Win-Cmd/Meta와 Enter/Esc/navigation/F1-F12/ASCII key를 조합할 수 있습니다. 따라서 논리적으로 다음은 표현 가능합니다.

- `Ctrl+Shift+Esc`
- `Win+R`
- `Cmd+Space`

그러나 실제 성공 여부는 OS 보안 경계와 keyboard layout에 달려 있습니다. Windows Secure Attention Sequence (`Ctrl+Alt+Del`) 같은 것은 일반 `SendInput`으로 구현할 수 없습니다.

**평가: Minor**

권장사항: tool result에 `unsupported_secure_sequence`, `permission_boundary`를 구분하는 platform-specific error를 반환합니다.

### 6.3 Drag 구현 오류 및 timing parameter 무시

`convert_agent_action_to_events()`에서:

- Drag의 모든 intermediate event가 `InputEventType::LeftMouseDragged`로 고정됩니다. `button=right`여도 right drag event가 아닙니다.
- `duration_ms`는 pattern에서 `..`로 버려집니다. step events는 즉시 연속 전송됩니다.
- `KeyPress.hold_ms`도 `..`로 무시되어 down/up가 즉시 연속됩니다.
- `TypeText.delay_ms`, `paste_mode`도 `..`로 무시됩니다.
- click count에도 inter-click timing이 없습니다.

macOS injector는 200 events/s, burst 400의 rate limit을 가지고 있어 큰 drag/text burst가 실제 host에서 rate-limit될 수 있는데 agent는 host injection failure ACK를 받지 못합니다.

**평가: Major**

권장사항:

- action conversion과 dispatch scheduling을 분리합니다.
- `TimedInputSequence {event, delay_after}`를 만들어 duration/hold/delay를 실제 실행합니다.
- drag event type은 button별 `LeftMouseDragged`/`RightMouseDragged`를 선택하고 middle drag protocol variant를 추가합니다.
- double click은 OS threshold 안에 들어가도록 지정 interval을 사용합니다.

### 6.4 Unicode / IME / keyboard layout

`TypeText`는 `synthesize_ascii_char()`만 사용합니다. 지원되지 않는 char는 error가 아니라 `None`으로 **조용히 무시**됩니다.

예: `"안녕하세요"`, `"日本語"`, emoji는 0 events가 될 수 있고 tool은 성공으로 보고할 수 있습니다.

각 host 역시 physical/logical key 위주입니다.

- macOS: `CGEvent::new_keyboard_event(... key_code ...)`
- Windows: `KEYBDINPUT { wVk, wScan: 0 }`, `KEYEVENTF_UNICODE` 미사용
- Linux: legacy macOS key code → evdev key mapping

따라서 US keyboard punctuation mapping도 실제 remote keyboard layout이 달라지면 다른 문자가 입력될 수 있습니다.

**평가: Major**

권장사항 우선순위:

1. **텍스트 클립보드 + paste hotkey**를 agent `type_text`의 기본 Unicode path로 사용합니다.
2. Windows에는 `KEYEVENTF_UNICODE` 기반 fallback을 추가합니다.
3. macOS에는 CGEvent Unicode string 설정 또는 pasteboard path를 사용합니다.
4. Linux Wayland/uinput은 Unicode를 직접 보내기 어렵기 때문에 clipboard/IME-aware helper를 주 경로로 둡니다.
5. unsupported char를 절대 조용히 버리지 말고 `characters_accepted`, `characters_rejected`를 반환합니다.

### 6.5 Modifier tracker bug

`InputStateTracker::record_key_down(key_code, modifiers)`는 `active_modifiers |= modifiers`를 수행하지만 `record_key_up()`은 key set에서 code만 제거하고 modifier를 제거하지 않습니다.

예를 들어 explicit `KeyDown("Ctrl")` → `KeyUp("Ctrl")` 후에도 tracker의 CONTROL bit가 남을 수 있으며 이후 mouse/key event의 `modifiers`에 섞입니다. 대문자 key down처럼 SHIFT modifier가 부가되는 경우도 비슷한 오염 가능성이 있습니다.

**평가: Major**

권장사항:

- modifier key를 별도 `HashSet<ModifierKey>`로 추적합니다.
- `KeyUp`에서 해당 modifier를 명시적으로 제거합니다.
- physical key state와 “이 event에 적용할 modifiers”를 분리합니다.
- key-down/up mixed tests를 추가합니다.

### 6.6 OS permission barriers

#### Windows

`SendInput`은 UIPI/integrity level 제약을 받습니다. host module 주석도 UAC elevation과 synthetic input을 거부하는 게임에 대한 native QA 필요성을 명시합니다. 일반 권한 host는 elevated app에 입력하지 못할 수 있습니다.

**권장:** privileged broker/service와 user-session agent를 분리하거나, 필요한 경우 host가 같은/elevated integrity로 실행되었는지 capability에 명시합니다. Task Scheduler elevation을 자동 전제로 두면 안 됩니다.

#### macOS

매 input마다 `AXIsProcessTrusted()`를 확인합니다. ScreenCaptureKit은 Screen Recording permission이 필요합니다. headless deployment에서는 UI prompt를 띄울 수 없으므로 사전 TCC provisioning이 필요합니다.

#### Linux

`/dev/uinput` 권한이 필요하고 코드 문서에는 `uinput` group + udev rule 사용을 권장합니다. root/setuid 실행은 권장하지 않습니다.

**평가: Major**

공통 권장사항:

```json
{
  "capture": {"available": true, "permission": "granted"},
  "input": {"available": false, "reason": "UIPI_LOWER_INTEGRITY"},
  "unicode_input": false,
  "clipboard": true
}
```

처럼 session capability/preflight를 agent가 읽을 수 있게 합니다.

### 6.7 Host injection failure가 agent에게 전파되지 않음

`maho-host/src/session.rs::inject_input()`은 platform injector error를 `warn!`만 하고 호출자에게 반환하지 않습니다. Client는 TCP write가 성공하면 `send_input` 성공으로 봅니다.

따라서 다음이 모두 false positive가 될 수 있습니다.

- macOS Accessibility permission denied
- macOS rate limited
- Windows SendInput UIPI/partial send 실패
- Linux uinput/device error
- backend unsupported event

HTTP는 `events_sent`, MCP는 “successfully dispatched”를 반환하므로 agent가 다음 판단을 잘못할 수 있습니다.

**평가: Major**

권장사항:

- wire protocol에 `InputCommand {request_id, events}` / `InputResult {request_id, accepted, injected, failures[]}`를 추가합니다.
- host injector가 `Result`를 swallow하지 말고 ACK를 작성합니다.
- ACK timeout과 “transport accepted / OS injected / visually confirmed” 세 단계를 구분합니다.
- batch는 per-event result 또는 first failure index를 포함합니다.

### 6.8 Input safety / stuck keys

좋은 점:

- `InputStateTracker`가 active buttons/keys를 추적합니다.
- `release_all()`은 tracked up events 후 `Reset`을 보냅니다.
- HTTP disconnect는 input gate를 닫고 release를 시도합니다.
- Windows/Linux Reset은 주요 mouse buttons/modifiers를 release합니다.

문제:

1. `check_timeout()`는 5초 기본 timeout을 구현하지만 workspace 검색 기준 실제 호출처가 없습니다.
2. Tauri `reset_session_inputs()`는 tracker를 `clear()`한 뒤 `Reset`만 보냅니다. tracked arbitrary key를 release할 정보가 사라집니다.
3. macOS injector는 `InputEventType::Reset`을 no-op 처리합니다.
4. Linux Reset은 buttons와 modifiers만 release하며 임의의 held non-modifier key를 알 수 없습니다.
5. HTTP action dispatch가 중간 `backend.send_input_event()` 실패로 끝날 때 MCP처럼 즉시 release cleanup을 호출하지 않습니다.
6. Unix `maho-client` SIGINT 경로는 명시적인 async graceful handler가 아니라 process 기본 종료에 기대는 부분이 있어 abnormal termination 시 release 보장이 어렵습니다.

**평가: Major**

권장사항:

- periodic watchdog에서 `check_timeout()`를 실제 호출합니다.
- session teardown은 tracker의 `release_all()` 결과를 보낸 뒤 tracker를 clear합니다.
- macOS Reset을 host-side maintained physical key/button state를 기반으로 실제 release하도록 구현합니다.
- host도 자체 key/button state를 추적해 client crash 시 heartbeat timeout에서 release합니다.
- HTTP partial failure에도 best-effort release를 수행하고 response에 cleanup 결과를 포함합니다.
- safety release를 protocol heartbeat/session lease와 연결합니다.

---

## 7. MCP / HTTP / Tauri Ergonomics

### 7.1 MCP transport

**현황:** stdio newline-delimited JSON-RPC입니다. MCP `initialize`, `tools/list`, `tools/call`을 구현하며 protocol version은 `2024-11-05`로 고정되어 있습니다. HTTP/SSE MCP server는 없습니다.

장점:

- stdio는 local process capability boundary가 명확하고 MCP client가 child process를 소유하기 좋습니다.
- stdout JSON과 stderr diagnostics 분리 및 EOF cleanup이 구현되어 있습니다.

한계:

- 한 stdio reader loop에서 request를 순차 처리하므로 4K screenshot encode가 다른 tool call을 막을 수 있습니다.
- network MCP/SSE/Streamable HTTP가 없으므로 외부 orchestrator가 daemon 하나에 연결하기 어렵습니다.

**평가: Improvement**

권장사항:

- stdio는 기본으로 유지하되 MCP Streamable HTTP를 별도 authenticated endpoint로 제공합니다.
- read/dispatch를 request-id 기반 concurrent worker로 분리하되 input ordering은 session queue에서 보장합니다.

### 7.2 MCP schema 품질

10개 tool은 이름은 이해하기 쉽지만 computer-use agent 관점에서 몇 가지 ambiguity가 있습니다.

- 좌표가 “pixels or normalized”이고 `normalized=false`가 기본이라 0.5 같은 값의 의미가 명확하지 않습니다.
- screenshot schema가 full-screen 중심이며 crop/downscale/freshness가 없습니다.
- drag `duration_ms`는 MCP schema에도 없습니다.
- type text는 Unicode 제한을 tool description에서 충분히 강하게 표현하지 않으며 implementation은 `delay_ms`도 무시합니다.
- MCP에는 mouse down/up, key down/up, batch, cursor query, session status/disconnect, clipboard가 없습니다.

**평가: Major**

권장사항:

- agent tool 좌표는 기본적으로 normalized만 받거나 `coordinate_space: "screenshot_px" | "normalized"` enum을 요구합니다.
- schema에 min/max, `additionalProperties:false`, maximum text length/steps/count를 명시합니다.
- tool description에 platform limitations를 capability-driven으로 제공합니다.

### 7.3 HTTP 구현은 일반 WebSocket API가 아님

README는 “HTTP/WebSocket API”를 언급하지만 `maho-app`의 AgentServer에는 WebSocket upgrade/handshake 코드가 없습니다. 구현은 직접 `TcpListener`에서 HTTP/1.1 request line/Content-Length를 파싱하고 응답 후 `Connection: close`하는 형태입니다.

지원 route:

- `GET /api/v1/health`
- `GET /api/v1/screen/info`
- `GET /api/v1/screen/screenshot`
- `POST /api/v1/input/action`
- `POST /api/v1/input/batch`
- `POST /api/v1/input/reset`
- `POST /api/v1/session/disconnect`

**평가: Minor**

권장사항:

- README를 현재 사실에 맞게 수정하거나 실제 WebSocket/SSE event API를 구현합니다.
- 일반 HTTP stack(axum/hyper 등)으로 옮겨 headers, content-type, auth, keep-alive, cancellation, structured middleware를 표준화하는 것이 장기적으로 안전합니다.

### 7.4 Critical: HTTP caller authentication 없음

`maho-client --agent-server`는 `127.0.0.1`에만 bind되어 외부 NIC 노출은 막습니다. 그러나 `agent_server.rs`에는 `Authorization` 검증이 없고 모든 응답에 `Access-Control-Allow-Origin: *`가 포함됩니다. request Content-Type도 검증하지 않습니다.

이 endpoint는 단순 status API가 아니라 **원격 host에서 keyboard/mouse를 실행하는 authority**를 갖습니다. 같은 머신의 다른 user/process 또는 localhost 접근이 가능한 악성 웹 context가 이 port를 호출할 수 있는 threat를 고려해야 합니다.

**평가: Critical**

권장사항:

1. 기본 transport를 Unix domain socket / Windows named pipe로 전환하고 peer credential을 확인합니다.
2. TCP를 유지한다면 startup 시 random 256-bit bearer token을 생성하고 모든 non-health endpoint에 요구합니다.
3. CORS `*`를 제거하고 browser use가 필요할 때만 explicit Origin allowlist를 사용합니다.
4. `Content-Type: application/json`을 POST에 강제합니다.
5. `/health`도 민감한 session detail은 반환하지 않습니다.
6. token은 CLI argv가 아니라 inherited fd/env/permission-0600 file 등 노출이 적은 경로로 전달합니다.

### 7.5 HTTP blocking 및 error semantics

장점:

- screenshot/agent backend 작업은 `spawn_blocking`을 사용합니다.
- concurrent blocking jobs와 screenshot을 semaphore로 제한합니다.
- request/work timeout이 5초로 bounded되어 있습니다.

문제:

- backend blocking timeout이 발생하면 일부 경로는 정상 JSON error response 대신 connection-level `io::Error`로 끝날 수 있습니다.
- action batch가 중간 실패하면 partial sent count를 반환하지 않습니다.
- health는 단순 `{"status":"ok"}`이며 remote session/frame freshness와 무관합니다.
- HTTP screenshot busy는 429지만 retry-after가 없습니다.

**평가: Minor**

권장사항:

- 공통 error envelope: `{code,message,retryable,session_id,request_id,details}`.
- `/health/live`, `/health/ready` 분리: listener liveness vs authenticated remote/frame readiness.
- partial batch failure에 `sent`, `failed_index`, `cleanup`를 포함합니다.

### 7.6 Tauri agent surface의 geometry 문제

Tauri `agent_get_screen_info`는 latest frame이 있으면 frame pixel width/height를 사용하지만 scale을 1.0, host name을 `remote-host`로 하드코딩합니다. frame이 없으면 1920×1080 `unconnected`라는 plausible geometry를 반환합니다.

이는 에이전트가 실제로 미연결인데도 정상 화면 크기로 오인할 수 있습니다.

**평가: Major**

권장사항:

- unconnected이면 error/state를 반환합니다.
- ReadySession의 실제 host metadata와 scale을 AppState에 유지합니다.
- Tauri와 CLI가 동일 `AgentSessionFacade`를 공유하여 geometry contract가 갈라지지 않게 합니다.

---

## 8. Feedback Loop 품질

현재 action feedback은 대략 다음 수준입니다.

```text
agent -> send action
      <- events_sent=N
agent -> take screenshot
      <- latest frame (age unknown)
```

에이전트가 진짜 원하는 것은 다음과 같습니다.

```text
agent -> action(request_id, expected_target?)
      <- host accepted
      <- OS injection result
      <- cursor now at x/y
      <- frame_id advanced / changed region
      <- screenshot or structured state
```

현재 빠진 핵심 정보:

- host injection ACK
- pointer current position
- input request ID correlation
- frame ID / screenshot age
- post-action wait-for-change
- changed rectangle / diff ratio
- active window/process identity
- permission change event

**평가: Major**

### 권장 closed-loop primitive

`remote_act_and_observe`를 추가하는 것이 tool-call overhead를 크게 줄일 수 있습니다.

```json
{
  "action": {"type":"click","x":812,"y":455,"space":"screenshot_px"},
  "observe": {
    "wait_for":"screen_change",
    "timeout_ms":1500,
    "min_change_ratio":0.002,
    "screenshot":{"max_width":1280,"format":"jpeg","quality":70}
  }
}
```

응답:

```json
{
  "request_id":"...",
  "transport":"sent",
  "injection":"confirmed",
  "pointer":{"x":812,"y":455,"space":"screenshot_px"},
  "before_frame_id":1044,
  "after_frame_id":1046,
  "changed":true,
  "change_ratio":0.018,
  "screenshot":{...}
}
```

---

## 9. Modern computer-use 대비 기능 격차

### 9.1 Accessibility / UI tree

활성 Rust workspace에서 Windows UI Automation 또는 Linux AT-SPI 구현은 검색되지 않습니다. macOS Accessibility API는 **permission check/input 권한**에만 쓰이며 AX UI tree를 agent perception으로 제공하지 않습니다.

따라서 에이전트는 버튼 이름, role, bounds, enabled/focused state, text field value를 시각적으로 추론해야 합니다.

**평가: Improvement (효과는 매우 큼)**

권장사항:

- Windows: UI Automation client layer
- macOS: AXUIElement tree
- Linux: AT-SPI2, Wayland app support 가능 범위 명시
- `remote_get_ui_tree`, `remote_find_element`, `remote_invoke_element`, `remote_set_value` 제공
- vision coordinates와 accessibility bounds를 하나의 screenshot coordinate space로 정규화

순수 vision fallback은 유지하되 semantic target이 있으면 구조화 API를 우선 사용합니다.

### 9.2 Clipboard

Underlying protocol에는 `TEXT_CLIPBOARD_SYNC` capability와 `ClipboardSyncUpdate`가 있고 Windows/Linux host 및 macOS client-side monitor 코드도 존재합니다. `ClientSession::send_clipboard_text()`도 있습니다.

그러나 agent MCP/HTTP에는 clipboard get/set tool이 없습니다.

**평가: Major**

이것은 Unicode 입력과 긴 텍스트/URL/script 전달을 가장 빠르게 개선할 수 있는 low-hanging fruit입니다.

권장 MCP:

- `remote_clipboard_get`
- `remote_clipboard_set(text)`
- `remote_paste(text)` — set clipboard + platform paste hotkey + optional restore previous clipboard

Capability negotiation도 host ack에 실제 clipboard capability를 일관되게 광고하도록 점검해야 합니다. 현재 host HandshakeAck는 `STREAM_CONFIGURATION`만 설정하는 경로가 있어 capability representation이 구현 능력을 충분히 반영하지 않습니다.

### 9.3 File transfer

`FILE_TRANSFER` capability나 agent-facing file upload/download protocol은 현재 검색되지 않습니다.

**평가: Improvement**

권장사항:

- 별도 authenticated file channel을 사용합니다. clipboard에 binary/base64를 얹지 않습니다.
- upload에는 destination policy, max size, checksum, overwrite policy, quarantine/consent를 둡니다.
- agent가 script를 실행해야 한다면 “upload”와 “execute” 권한을 분리합니다.

### 9.4 Audio perception

Host는 audio capture 및 UDP AudioFrame을 지원하고 Tauri는 playback path가 있습니다. 그러나 headless `maho-client` UDP loop는 Frame/Ping 외 event를 버리므로 agent가 audio sample, transcript, sound event를 받을 수 없습니다.

**평가: Improvement**

권장사항:

- `remote_capture_audio(duration_ms)`보다는 우선 event classifier/STT plugin 경계를 둡니다.
- notification sound, meeting speech 등 명확한 use case가 있을 때 opt-in capability로 제공합니다.
- 개인정보 영향이 크므로 별도 permission 및 visible indicator를 둡니다.

### 9.5 Semantic process/window context

현재 agent screen info에는 dimensions/scale/host 정도만 있고 active window, process, title, secure desktop 여부가 없습니다.

**평가: Improvement**

권장사항:

- `remote_get_foreground_window`
- process/title/bounds/desktop/monitor
- secure/elevated/protected surface 여부
- input permission compatibility

를 제공하면 agent가 “왜 클릭이 안 되는지”를 훨씬 잘 진단할 수 있습니다.

---

## 10. 플랫폼별 기능/위험 매트릭스

| 항목 | macOS | Windows | Linux |
| --- | --- | --- | --- |
| Capture | ScreenCaptureKit main display | DXGI display index 0 | wlroots screencopy selected output |
| Cursor in pixels | **예** (`showsCursor`) | **아니오**, separate metadata | 기본 **예** (`overlay_cursor`) |
| Agent cursor API | headless 없음 | headless 없음 | headless 없음 |
| Input | CoreGraphics | SendInput | `/dev/uinput` |
| Absolute coordinates | logical display | default virtual desktop | configured output geometry |
| Multi-monitor risk | main-only | capture display 0 vs input virtual desktop mismatch | selected output global origin loss |
| Unicode typing | 없음 | 없음 (`KEYEVENTF_UNICODE` 미사용) | 없음 |
| Permission barrier | Screen Recording + Accessibility | UIPI/UAC/integrity | uinput group + compositor protocol |
| Reset quality | `Reset` no-op | buttons + common modifiers release | buttons + modifiers release |
| Audio host | 지원 | 지원 | 지원 경로 있음 |
| Agent audio | 없음 | 없음 | 없음 |
| Clipboard internals | client monitor 중심 | host/client sync 구현 | host sync 구현 |
| Agent clipboard tool | 없음 | 없음 | 없음 |

---

## 11. 우선순위별 개선 로드맵

### P0 — 배포 전 안전/정확성

1. **HTTP control plane 인증**
   - bearer capability token 또는 Unix socket/named pipe + peer credential
   - CORS `*` 제거
2. **Input ACK protocol**
   - host injection result를 request ID와 함께 반환
3. **좌표 계약 통일**
   - physical/logical geometry를 동시에 노출
   - screenshot pixel space를 agent input 기본 space로 권장
4. **Input safety 수정**
   - timeout watchdog 실제 실행
   - disconnect 시 tracked key/button release 후 clear
   - macOS Reset 구현
5. **현재 명백한 input bugs 수정**
   - right drag event
   - modifier key-up tracker
   - ignored `duration_ms`/`hold_ms`/`delay_ms`
   - non-ASCII silent drop 금지

### P1 — 자율 에이전트 신뢰성

1. `frame_id`, age, timestamp를 screenshot에 추가
2. `wait_for_screen_change` + `act_and_observe`
3. headless cursor state API 및 Windows cursor compositing
4. agent clipboard get/set/paste 및 Unicode typing
5. session status/connect/disconnect/reconnect/backoff
6. host discovery 및 stable host identity
7. monitor enumeration/selection + 정확한 per-monitor input mapping

### P2 — 모델 비용/지연 최적화

1. screenshot downscale/crop/JPEG quality
2. binary HTTP image response
3. screenshot result cache keyed by frame_id/params
4. dirty-region/diff 기반 observation
5. end-to-end loop telemetry

### P3 — modern computer-use parity

1. Windows UIA / macOS AX / Linux AT-SPI semantic tree
2. file transfer
3. active window/process/permission introspection
4. optional audio perception/STT
5. multi-session orchestration/control lease

---

## 12. 세부 Finding 목록

| ID | 심각도 | Finding | 주요 코드 |
| --- | --- | --- | --- |
| F-01 | **Critical** | Loopback HTTP remote-control API에 caller auth 없음 + CORS `*` | `maho-app/src/agent_server.rs` |
| F-02 | **Major** | Host input injection failure를 swallow하여 agent success가 authoritative하지 않음 | `maho-host/src/session.rs::inject_input` |
| F-03 | **Major** | Headless screen info logical size와 screenshot physical size 불일치 | `maho_client.rs`, `capture_macos.rs`, `windows_logic.rs` |
| F-04 | **Major** | Windows capture display 0 vs input virtual desktop mismatch | `session.rs`, `inject_windows.rs` |
| F-05 | **Major** | Linux selected output global geometry가 input injector에 전달되지 않음 | `session.rs`, `inject_linux.rs` |
| F-06 | **Major** | Multi-monitor list/select agent API 없음 | MCP/AgentServer/Tauri agent commands |
| F-07 | **Major** | `TypeText` ASCII-only, unsupported Unicode silent drop | `agent_input.rs` |
| F-08 | **Major** | drag duration/key hold/type delay/paste mode가 schema에만 있고 동작하지 않음 | `agent_input.rs` |
| F-09 | **Major** | drag intermediate event가 항상 `LeftMouseDragged` | `agent_input.rs` |
| F-10 | **Major** | `InputStateTracker::check_timeout` 미사용 | `agent_input.rs` |
| F-11 | **Major** | modifier key-up가 `active_modifiers`를 정리하지 않음 | `agent_input.rs` |
| F-12 | **Major** | Tauri disconnect가 tracker clear 후 Reset만 전송; macOS Reset no-op | `tauri-shell/lib.rs`, `inject_macos.rs` |
| F-13 | **Major** | Screenshot freshness/frame ID/wait-for-change 없음 | `agent_server.rs`, MCP dispatch |
| F-14 | **Major** | Windows cursor는 separate metadata인데 headless agent가 버림 | `maho-host/session.rs`, `maho_client.rs` |
| F-15 | **Major** | Auto reconnect/session resume 없음 | `maho-app/session.rs`, `maho_client.rs` |
| F-16 | **Major** | Headless agent discovery/session switch API 없음 | discovery/Tauri vs MCP gap |
| F-17 | **Major** | HostServer가 connections를 직렬 처리, client도 single session | `maho-host/session.rs::serve` |
| F-18 | **Major** | 코드에 특정 Tailscale IP/name pairing 예외 하드코딩 | `maho_client.rs`, Tauri `lib.rs` |
| F-19 | **Major** | Tauri unconnected agent screen info가 1920×1080 fake geometry를 반환 | Tauri `lib.rs` |
| F-20 | **Major** | agent clipboard tool 부재로 Unicode/paste workflow 활용 불가 | `ClientSession`, MCP/HTTP gap |
| F-21 | **Minor** | README의 WebSocket API 주장과 실제 raw HTTP/1.1 구현 불일치 | README, `agent_server.rs` |
| F-22 | **Minor** | HTTP health가 session/frame readiness를 반영하지 않음 | `agent_server.rs` |
| F-23 | **Minor** | HTTP batch partial failure의 sent count/cleanup 정보 부족 | `agent_server.rs` |
| F-24 | **Minor** | Wire coordinate가 legacy bottom-left convention이라 platform adapter 복잡 | `maho-proto/src/input.rs` |
| F-25 | **Minor** | NV12 screenshot 변환에 color-space metadata/optimized path 없음 | `agent_input.rs` |
| F-26 | **Improvement** | Accessibility/UI tree 없음 | workspace 전체 |
| F-27 | **Improvement** | File transfer 없음 | protocol/agent surface |
| F-28 | **Improvement** | Agent audio perception 없음 | headless UDP loop/MCP |
| F-29 | **Improvement** | screenshot crop/downscale/quality controls 없음 | `agent_input.rs`, API schemas |
| F-30 | **Improvement** | multi-session/control lease 없음 | ClientSession/HostServer architecture |

---

## 13. 권장 목표 아키텍처

```text
                +---------------------------+
                | Agent Control Daemon      |
                |---------------------------|
MCP stdio ----> | Auth / capability policy  |
MCP HTTP -----> | SessionManager            |
Local IPC ----> | DisplayManager            |
                | InputScheduler + watchdog |
                | Observation cache/diff    |
                +------------+--------------+
                             |
              +--------------+---------------+
              | per-session ClientSession    |
              | stable host identity         |
              | reconnect/backoff            |
              +-------+---------------+-------+
                      |               |
                TLS control       encrypted UDP
                      |               |
                +-----v---------------v-------+
                | Remote Host                 |
                |-----------------------------|
                | capture + display metadata  |
                | input injector + state      |
                | request-id injection ACK    |
                | clipboard/file capabilities |
                | accessibility provider      |
                +-----------------------------+
```

Agent layer가 직접 `ClientSession` primitive를 조합하기보다 `Agent Control Daemon`이 **session, geometry, input safety, observation freshness를 소유**하도록 만드는 것이 가장 중요합니다.

---

## 14. 권장 API 예시

### Session

- `remote_list_hosts()`
- `remote_connect(host_id)`
- `remote_session_status(session_id)`
- `remote_disconnect(session_id)`

### Perception

- `remote_list_displays(session_id)`
- `remote_capture_screen(session_id, display_id?, region?, max_size?, quality?, min_frame_id?)`
- `remote_wait_for_change(session_id, after_frame_id, timeout_ms, threshold?)`
- `remote_get_pointer_state(session_id)`
- `remote_get_ui_tree(session_id, display_id?, depth?, region?)`

### Action

- `remote_click(... coordinate_space="screenshot_px")`
- `remote_drag(... duration_ms)`
- `remote_type_text(text, method="auto")`
- `remote_clipboard_set(text)`
- `remote_paste(text)`
- `remote_act_and_observe(action, observation_policy)`

모든 action response는 최소 다음을 가져야 합니다.

```json
{
  "request_id":"uuid",
  "session_id":"uuid",
  "transport_accepted":true,
  "host_injected":true,
  "events_injected":3,
  "pointer": {"x":400,"y":200,"space":"screenshot_px"},
  "frame_before":120,
  "frame_after":121,
  "warnings":[]
}
```

---

## 15. 종합 평가

### 이미 강한 부분

- 실제 GUI 없이 동작하는 `maho-client` headless agent mode
- stdio MCP와 loopback HTTP라는 두 automation entry point
- TLS-PSK + persisted pairing + bootstrap stretching/lockout/host consent
- TCP control / encrypted UDP media separation
- macOS/Windows/Linux native capture/input backend
- request/work bounds, input event budget, screenshot concurrency limit
- video stream에서 latest frame을 유지하여 screenshot latency를 capture startup과 분리
- 실제 물리 장비 기반 성능/packet verification 자료가 저장소에 남아 있음

### 현재 agent productization을 막는 핵심

- local control plane authentication 부족
- actual OS input ACK 부족
- screenshot/input coordinate contract 불명확
- Unicode/IME와 clipboard agent surface 부족
- input safety watchdog/Reset semantics 불완전
- cursor/screen freshness/change feedback 부족
- single host/session 및 agent discovery 부재
- monitor abstraction 부재
- vision-only perception

### 최종 판단

MahoRD는 **“AI가 원격 PC를 볼 수 있고 기본 mouse/keyboard를 보낼 수 있는 기술적 기반”은 이미 갖췄습니다.** 특히 headless client, 실제 native capture, pairing/암호화는 실험용 mock 수준을 넘어섭니다.

하지만 현재 인터페이스만으로는 장시간 자율 computer-use에서 요구되는 **정확성, 안전한 권한 경계, action observability, 다국어 입력, multi-monitor/session orchestration**을 충분히 보장하기 어렵습니다. P0/P1 항목을 먼저 해결하면 Anthropic-style computer-use/OSWorld류 workload에서 실패 원인을 “vision model의 추론 실패”와 “remote desktop transport/input 실패”로 분리할 수 있고, agent가 자기 행동의 성공 여부를 스스로 검증할 수 있는 구조로 발전할 수 있습니다.

가장 높은 ROI의 구현 순서는 다음과 같습니다.

> **HTTP auth → input ACK → geometry contract → input safety/Unicode fixes → frame-id/wait-for-change → cursor/clipboard → monitor/session manager → accessibility tree**

이 순서로 진행하면 기존 transport/capture 코드를 대규모로 갈아엎지 않고도 MahoRD를 훨씬 신뢰할 수 있는 agent-native remote desktop substrate로 확장할 수 있습니다.
