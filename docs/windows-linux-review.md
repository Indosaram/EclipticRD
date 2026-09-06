# erd-host Windows/Linux 코드 리뷰 및 개선 방안

**리뷰 일자:** 2026-09-04
**대상:** `clients/rust/erd-host/src/` 내 Windows·Linux 플랫폼 모듈
**범위:** `capture_windows.rs`, `capture_linux.rs`, `encode_windows.rs`, `encode_linux.rs`, `inject_windows.rs`, `inject_linux.rs`, `clipboard_windows.rs`, `clipboard_linux.rs`, `audio_linux.rs`, `windows_logic.rs`, `session.rs` 플랫폼 분기

---

## 1. 요약

전체적으로 플랫폼 분리(`#[cfg(target_os)]`)가 잘 지켜지고, 블로킹 캡처/인코딩을 전용 스레드로 분리하는 등 프로젝트 안티패턴을 잘 따르고 있다. 그러나 **캡처→인코드 파이프라인의 제로카피/스레딩 아키텍처**, **인코더 백엔드의 동적 제어 부재**, **오디오 경로의 미구현/미배선** 등이 주요 개선 포인트다.

### 우선순위 요약

| 우선순위 | 항목 | 대상 |
|----------|------|------|
| P0 | 프레임당 메모리 할당 및 복사 비용 | Linux/Windows 캡처·인코드 |
| P0 | Windows 비동기 MFT 및 NVENC 통합 부재 | encode_windows |
| P0 | Linux 오디오 캡처가 세션에 연결되지 않음 | session.rs, audio_linux |
| P1 | 입력 인젝션 좌표/키 매핑 일관성 | inject_windows, inject_linux |
| P1 | 클립보드 동기화의 구조적 중복 및 제한 | clipboard_windows/linux |
| P2 | 단위 테스트 커버리지 확대 | 전체 |

---

## 2. Windows 캡처 (`capture_windows.rs`)

### 2.1 현재 상태
- DXGI Desktop Duplication을 통해 프레임을 `Vec<u8>` BGRA 버퍼로 반환.
- `primary_output_geometry()`는 세션 협상 전에 한 프레임을 캡처해 해상도만 읽고 duplication 인터페이스를 버린다.
- `acquire_next_frame()`은 dirty/move rects, 포인터 위치 등을 함께 반환.

### 2.2 발견 사항
1. **프레임마다 힙 할당 + GPU→CPU 복사**
   - `capture_frame_components_with_metadata()` 낶에서 매 프레임 `Vec<u8>`을 새로 생성하고, staging texture를 통해 CPU로 복사한다.
   - 60fps 4K 기준으로 매 프레임 약 32MB의 힙 트래픽이 발생한다.
2. **`primary_output_geometry()`의 부작용**
   - 해상도를 얻기 위해 한 프레임을 실제로 캡처하고 버리는 방식은 비효율적이다.
3. **타임아웃 재설정 패턴**
   - `acquire_next_frame()` 낶에서 `self.set_timeout(timeout)`을 매번 호출한다. `DXGIManager`의 낶 구현이 이를 다시 설정하는 비용이 있다면 프레임마다 오버헤드가 될 수 있다.

### 2.3 개선 방안
- **재사용 가능한 BGRA 버퍼 풀 도입**
  - `CapturedFrame`의 `bgra: Vec<u8>`을 `Arc<Vec<u8>>` 또는 재사용 풀에서 대여한 `BytesMut`로 변경.
  - 또는 DXGI 출력을 직접 NV12로 변환하는 GPU shader를 적용해 CPU 복사 자체를 줄인다.
- **`primary_output_geometry()` 최적화**
  - DXGI 출력 열거(`IDXGIFactory::EnumAdapters`/`EnumOutputs`)만으로 해상도를 얻도록 수정. duplication 인터페이스 생성/파괴를 피한다.
- **타임아웃 상수화**
  - 생성 시점에 timeout을 설정하고, `acquire_next_frame`은 재설정 없이 사용하도록 `DXGIManager` 사용 방식 재검토.

---

## 3. Linux 캡처 (`capture_linux.rs`)

### 3.1 현재 상태
- wlr-screencopy v3를 사용해 `wl_shm` BGRA 버퍼로 캡처.
- 캐시된 SHM 버퍼를 재사용하지만, `finish_capture()`에서 매번 `vec![0_u8; total_bytes]`로 출력 버퍼를 새로 할당한다.
- Portal fallback은 `PortalRequired` 에러로 위임하고 자동 전환은 하지 않는다.

### 3.2 발견 사항
1. **출력 버퍼 재할당**
   - `finish_capture()`의 `bgra` 버퍼가 매 프레임 새로 할당된다. 캡처 스레드에서 인코드 스레드로 전송 후 재사용되지 않는다.
2. **CPU 기반 알파 채널 패치**
   - `for alpha in bgra[3..].iter_mut().step_by(4) { *alpha = u8::MAX; }` 는 4K 기준 약 8백만 픽셀을 매 프레임 순회한다.
3. **Y-inverted 프레임 처리**
   - y-inverted일 때 추가로 `raw_shm_buf`를 읽고 행 단위 복사를 수행한다. 대부분의 wl_shm BGRA 프레임이 y-inverted이므로 이 경로가 자주 사용된다.

### 3.3 개선 방안
- **제로카피 프레임 전달**
  - 캡처 스레드에서 인코드 스레드로 `Vec<u8>` 소유권을 이동시키는 대신, `Arc<Mutex<FrameBuffer>>` 또는 `crossbeam::queue` 기반 버퍼 풀을 사용해 재사용한다.
  - 더 이상적으로는 Linux DMA-BUF 지원(`zwlr_screencopy_frame_v1::linux_dmabuf`)을 활성화해 GPU 메모리 직접 공유 경로를 만든다. 문서상으로는 v3의 DMA-BUF 확장이 이미 인식되고 있으므로 단계적 적용이 가능하다.
- **알파 패치 최적화**
  - XRGB8888 포맷인 경우에만 알파 패치를 수행하고, ARGB8888인 경우는 생략한다.
  - 또는 `u32` 단위로 `| 0xFF000000` 연산을 적용해 4배 빠르게 처리한다.
- **Y-inversion 처리 통합**
  - Y-inverted 프레임도 `read_exact`로 한 번에 읽은 뒤, 행 역순 복사를 SIMD-friendly한 `chunks_exact`/`par_chunks` (rayon)으로 가속한다.

---

## 4. Windows 인코더 (`encode_windows.rs`)

### 4.1 현재 상태
- Media Foundation 동기 MFT만 사용. 주석에 명시된 대로 하드웨어 MFT(NVIDIA 등)는 비동기 이벤트 기반이므로 의도적으로 제외된다.
- NVENC은 `NvencAvailability::probe()`로 가용성만 확인하고 실제 인코더로는 통합되지 않았다.
- `MediaFoundationEncoder::encode_nv12()`는 프레임마다 `MFCreateMemoryBuffer`로 새 버퍼를 생성하고 `ptr::copy_nonoverlapping`으로 복사한다.

### 4.2 발견 사항
1. **하드웨어 인코더 부재**
   - 대부분의 Windows 호스트에서 NVIDIA/Intel/AMD 하드웨어 MFT가 비동기로만 제공되므로, 현재 구현은 Microsoft Software Encoder로 폴타게 된다. 4K 60fps에서 CPU 부하가 크고 레이턴시가 높아진다.
2. **프레임 입력 버퍼 복사**
   - `sample_from_bytes()`에서 NV12 데이터를 MF 버퍼로 복사한다. DXGI 캡처와 연결하면 BGRA→NV12 변환 + NV12→MF 버퍼 복사로 2번의 CPU 복사가 발생한다.
3. **`force_key_frame`/`update_bitrate`가 세션에서 무시됨**
   - `WindowsMediaHandle::force_key_frame()`과 `update_bitrate()`는 `Ok(())`만 반환한다. ABR 및 키프레임 요청이 인코더에 전달되지 않는다.
4. **단일 스레드 동기 파이프라인**
   - `encode_nv12()`가 `ProcessInput`/`ProcessOutput`을 동기 호출하므로, 소프트웨어 인코더의 경우 한 프레임 인코딩 시간이 다음 캡처를 지연시킬 수 있다.

### 4.3 개선 방안
- **비동기 MFT 지원 추가**
  - `IMFMediaEventGenerator`를 사용해 `METransformNeedInput`/`METransformHaveOutput` 이벤트를 처리하는 비동기 파이프라인을 구현한다.
  - `MFT_ENUM_FLAG_HARDWARE`를 포함한 열거를 다시 활성화하고, 비동기 MFT인지 확인 후 별도 경로로 처리한다.
- **NVENC 백엔드 통합**
  - `nvenc` crate을 사용해 동적 로드된 NVENC 인코더를 `MediaFoundationEncoder`와 동일한 `encode_nv12`/`EncodedFrame` 인터페이스로 제공한다.
  - DXGI 캡처와 NVENC 간 `ID3D11Texture2D` 공유로 GPU 내 제로카피를 구현한다.
- **입력 샘플 재사용**
  - `MFCreateMemoryBuffer`를 미리 생성된 풀에서 재사용하거나, `IMFSample`에 외부 메모리를 어태치하는 `MFCreateMediaBufferFromMemory` 방식을 검토한다.
- **세션-인코더 제어 연결**
  - `WindowsMediaHandle`이 인코더 스레드로 제어 메시지(키프레임 요청, 비트레이트 변경)를 전달하는 채널을 추가한다.

---

## 5. Linux 인코더 (`encode_linux.rs`)

### 5.1 현재 상태
- NVENC → VAAPI → x264 순으로 폴타.
- NVENC/VAAPI 경로에서 BGRA→NV12 변환을 CPU에서 수행한다 (`encode_bgra()` 낶 루프).
- x264 경로는 swscale로 BGRA→YUV420P 변환.

### 5.2 발견 사항
1. **CPU 기반 색변환**
   - 4K 기준 매 프레임 약 8백만 픽셀에 대해 BT.601 limited-range 변환을 CPU에서 수행한다. 이는 캡처 레이턴시의 큰 부분을 차지한다.
   - x264 폴타 경로는 swscale을 사용하지만, NVENC/VAAPI 경로는 수동 루프라서 swscale/SIMD 최적화를 활용하지 못한다.
2. **VAAPI 프레임 풀 크기 고정**
   - `initial_pool_size = 4`로 고정되어 있다. 60fps에서는 부족할 수 있고, 해상도가 클수록 더 많은 표면이 필요하다.
3. **제어 메시지 부재**
   - Windows와 마찬가지로 `LinuxMediaHandle::force_key_frame()`과 `update_bitrate()`가 no-op이다. LinuxVideoEncoder에는 `force_key_frame()`이 있지만 세션에서 호출되지 않는다.
4. **`EncoderBackend::Nvenc` 로그 불일치**
   - `LinuxVideoEncoder::new()`에서 NVENC 성공 시 `tracing::info!`를 출력하지만, VAAPI/x264 경로는 로그가 없다.

### 5.3 개선 방안
- **GPU 색변환 도입**
  - BGRA→NV12 변환을 GPU shader(OpenGL/Vulkan compute) 또는 VAAPI의 `vpp` (Video Post Processing)를 사용해 수행한다.
  - 단기적으로는 swscale의 `SWS_BILINEAR` 대신 SIMD 최적화된 루틴을 사용하거나, `libyuv` crate 연동을 검토한다.
- **프레임 버퍼 풀링**
  - `frame::Video::new()`를 매번 호출하는 대신, NV12 소프트웨어 프레임과 VAAPI 하드웨어 프레임을 풀링한다.
- **VAAPI 풀 크기 동적화**
  - `initial_pool_size = fps * 2` 또는 해상도 기반으로 계산한다.
- **세션-인코더 제어 연결**
  - Windows와 동일하게 인코더 스레드로 `force_key_frame`/`update_bitrate` 메시지를 전달하는 채널을 추가한다.
- **로그 일관성**
  - 모든 백엔드 선택 시점에 `tracing::info!`를 출력하도록 통일한다.

---

## 6. 입력 인젝션 (`inject_windows.rs`, `inject_linux.rs`)

### 6.1 Windows (`inject_windows.rs`)
- `SendInput`을 사용해 절대 좌표 마우스 및 키보드 이벤트를 전송한다.
- macOS keycode → Windows VK 매핑 테이블이 `windows_logic.rs`에 있다.
- 모디파이어 상태를 추적해 변경된 모디파이어만 전송한다.

**발견 사항:**
1. **확장 키 플래그 불완전**
   - `is_extended_key()`가 수동으로 범위를 관리한다. 누락된 확장 키가 있을 수 있고, Windows 버전별 차이를 반영하기 어렵다.
2. **마우스 버튼과 이동의 원자성 부재**
   - `MouseMove`와 `LeftMouseDown`이 별도 `SendInput` 호출로 전송되면, 원격 데스크톱 특성상 버튼 다운 위치와 실제 클릭 위치가 어긋날 수 있다.
3. **UAC/보안 데스크톱**
   - 문서에 언급된 대로 UAC 프롬프트나 보안 데스크톱에서는 `SendInput`이 실패한다. 현재는 에러를 그대로 전파한다.

**개선 방안:**
- 마우스 이동 + 버튼 이벤트를 하나의 `INPUT` 배열로 묶어 `SendInput` 한 번으로 전송한다.
- `is_extended_key`를 `MapVirtualKey` 결과와 비교 검증하는 테스트를 추가한다.
- `SendInput` 실패 시 `ERROR_ACCESS_DENIED`를 구분해 상위에서 사용자에게 "관리자 권한 필요" 피드백을 제공한다.

### 6.2 Linux (`inject_linux.rs`)
- `/dev/uinput`을 사용해 가상 키보드/마우스를 생성한다.
- Wayland compositor와 무관하게 동작하므로 Hyprland에서 안정적이다.
- macOS keycode → evdev KeyCode 매핑이 있다.

**발견 사항:**
1. **절대 좌표 해상도 제한**
   - `ABSOLUTE_AXIS_MAX = 65_535`로 고정되어 있으므로, 8K 이상 또는 멀티모니터 가상 데스크톱에서는 좌표 정밀도가 떨어진다.
2. **Y축 반전**
   - 프로토콜이 macOS 호환을 위해 Y축을 반전해서 전송하므로, Linux에서 다시 반전시키는 코드가 있다. 이는 Windows와 다른 특수 케이스이므로 주석과 함께 유지하되, 프로토콜 차원에서 통일하는 것이 장기적으로 바람직하다.
3. **모디파이어 동기화**
   - KeyDown/KeyUp 이벤트에서 모디파이어를 먼저 동기화하고 키를 전송한다. 만약 클라이언트가 모디파이어 변경 없이 키만 별도로 본낼 경우, 이전 모디파이어 상태가 유지되므로 의도치 않은 조합이 발생할 수 있다.

**개선 방안:**
- `ABSOLUTE_AXIS_MAX`를 가상 데스크톱 크기에 맞춰 동적으로 설정한다 (uinput은 max를 32비트로 지원).
- 프로토콜 v4에서 Y축 반전을 표준화하고, 호스트별 특수 케이스를 제거한다.
- 모디파이어 동기화를 `FlagsChanged` 이벤트에만 의존하도록 클라이언트-호스트 계약을 명확히 문서화한다.

---

## 7. 클립보드 (`clipboard_windows.rs`, `clipboard_linux.rs`)

### 7.1 공통 발견 사항
- 두 모듈 모두 `ClipboardEchoSuppressor`라는 유사한 상태 머신을 별도로 구현하고 있다. FNV-1a 해시와 로직이 사실상 동일하므로 중복이다.
- 4KiB 제한은 프로토콜 계약이지만, 현대 클립보드 사용 패턴(코드 스니펫, 이미지 경로 등)에서는 매우 작다.

### 7.2 Windows (`clipboard_windows.rs`)
- Win32 `GetClipboardSequenceNumber`를 사용해 에코를 억제한다. 이는 macOS `NSPasteboard.changeCount`와 유사한 좋은 접근이다.
- 클립보드 히스토리/클우드 제외 포맷을 등록해 원격 쓰기를 로컬 히스토리에서 제외한다.

**개선 방안:**
- `ClipboardEchoSuppressor`를 `windows_logic.rs`에서 `erd-proto` 또는 공통 모듈로 이동해 Linux와 공유한다.

### 7.3 Linux (`clipboard_linux.rs`)
- `wl-copy`/`wl-paste` 또는 `xclip`을 외부 프로세스로 실행한다. 폴타 로직이 명확하다.
- Wayland의 concealed/transient 타입을 감지해 민감한 클립보드 내용을 전송하지 않는다.

**발견 사항:**
1. **프로세스 스폰 오버헤드**
   - 500ms마다 `wl-paste`/`xclip`을 스폰하는 것은 지속적인 프로세스 생성 비용이 있다.
2. **에코 억제의 시간 기반 제한**
   - `DEFAULT_ECHO_PERIOD = 2초`는 원격 쓰기 직후 로컬에서 동일 내용을 복사하면 전송이 누락될 수 있다.
3. **X11 concealed 타입 미지원**
   - 문서에 명시된 대로 X11에는 `org.nspasteboard.ConcealedType`에 해당하는 표준이 없다.

**개선 방안:**
- Wayland에서는 `zwlr_data_control_manager_v1`을 사용해 네이티브 클립보드 모니터링으로 전환한다. 프로세스 스폰 없이 선택 변경 이벤트를 받을 수 있다.
- 에코 억제를 시간 기반이 아닌 sequence/transaction ID 기반으로 재설계한다. Wayland data-control은 serial을 제공한다.
- `MAX_CLIPBOARD_BYTES`를 프로토콜 협상 가능한 값으로 확장하고, 청크 분할을 지원한다.

---

## 8. 오디오 (`audio_linux.rs`)

### 8.1 현재 상태
- `pw-record` 또는 `parec`을 스폰해 48kHz/스테레오/f32 PCM을 캡처한다.
- `fragment_audio()`로 v3 UDP 프래그먼트로 분할한다.

### 8.2 발견 사항
1. **세션에 연결되지 않음**
   - `session.rs`에서 `LinuxAudioCapture`를 전혀 참조하지 않는다. `capture_audio: false`로 설정되어 있고, `MediaEvent::Audio`는 Windows/Linux에서 사용되지 않는다.
2. **프로세스 기반 캡처의 레이턴시**
   - 파이프를 통한 PCM 읽기는 버퍼링 레이턴시를 추가한다. `pw-record`의 기본 버퍼 크기가 크면 20-50ms의 추가 지연이 발생할 수 있다.
3. **Windows 오디오 부재**
   - `session.rs` 449행 주석에 명시된 대로 Windows 오디오 소스가 없다.

### 8.3 개선 방안
- **Linux 오디오를 미디어 파이프라인에 연결**
  - `LinuxMediaSource`에 오디오 캡처 스레드를 추가하고, `MediaEvent::Audio`를 통해 UDP sender로 전달한다.
  - `--no-audio` CLI 플래그를 Linux에서도 지원한다.
- **네이티브 PipeWire API 사용**
  - `pw-record` 대신 `libpipewire` Rust binding을 사용해 직접 스트림을 생성하면 버퍼 크기와 레이턴시를 정밀 제어할 수 있다.
- **Windows WASAPI 루프백 캡처 추가**
  - `wasapi` crate을 사용해 시스템 오디오 루프백을 캡처하고, Linux와 동일한 48kHz/stereo/f32 포맷으로 맞춘다.

---

## 9. 세션 플랫폼 분기 (`session.rs`)

### 9.1 발견 사항
1. **Windows 캡처→인코드 채널이 unbounded**
   - `let (frame_tx, frame_rx) = channel::<PipelineFrame>();` 는 `std::sync::mpsc::channel` (unbounded)이다. 인코더가 느려지면 메모리가 무한히 증가할 수 있다.
   - Linux는 `sync_channel(2)`로 올바르게 제한되어 있다.
2. **`PipelineFrame::Stop` 미사용**
   - Windows와 Linux 모두 `Stop` variant를 정의하지만 실제로는 채널이 닫히는 것으로 종료를 감지한다. `Stop`은 dead code다.
3. **타이밍 정보 부정확**
   - Windows `VideoFrame`의 `capture_at`, `encode_started_at`, `encode_completed_at`이 모두 인코드 완료 시점의 `Instant::now()`로 설정된다. 실제 캡처 시점과 인코드 지연을 측정할 수 없다.
   - Linux는 `captured_at`은 캡처 시점이지만 `encode_started_at`/`encode_completed_at`은 여전히 동일한 `now`이다.
4. **에러 발생 시 파이프라인 중단**
   - 캡처 또는 인코드 에러 발생 시 `MediaEvent::Error`를 전송하고 스레드를 종료한다. 일시적인 DXGI access loss 등에서는 재시도 로직이 필요하다.

### 9.2 개선 방안
- **Windows 채널 bounded화**
  - `sync_channel(2)`로 변경하고, `try_send` 실패 시 프레임 드롭 또는 최신 프레임으로 교체한다.
- **타이밍 계측 정확화**
  - `PipelineFrame`에 `captured_at`을 포함하고, 인코더 스레드에서 `encode_started_at = Instant::now()`를 기록한 뒤 `encode_completed_at`을 완료 시점으로 설정한다.
- **에러 복구 메커니즘**
  - DXGI `AccessLost`는 이미 재생성하지만, 인코더 에러나 캡처 스레드 panic에 대한 재시작 로직을 추가한다.
- **불필요한 `Stop` variant 제거 또는 활용**
  - 채널 종료로 충분하므로 제거하거나, 명시적 종료 신호로 활용한다.

---

## 10. 공통/교차 플랫폼 개선

### 10.1 색공간 일관성
- Windows `bgra_to_nv12`는 BT.601 limited-range를 사용한다.
- Linux `encode_linux`의 수동 변환도 BT.601 limited-range를 사용한다.
- 클라이언트(`tauri-shell`)는 BT.709로 렌더링한다. 호스트-클리언트 간 색공간 불일치가 있다.

**개선:** 호스트와 클라이언트의 색공간을 BT.709 full-range로 통일하거나, 세션 협상에서 색공간을 명시한다.

### 10.2 프레임 크기 검증
- Windows `bgra_to_nv12`는 홀수 해상도를 거부한다.
- Linux `LinuxVideoEncoder`도 NV12 변환을 위해 짝수 해상도를 요구한다.
- 세션 협상에서 홀수 해상도를 미리 거부하거나, 캡처 시점에 패딩을 추가한다.

### 10.3 테스트 커버리지
- `capture_linux.rs`, `capture_windows.rs`에는 단위 테스트가 없다.
- `inject_linux.rs`의 `map_normalized_to_output`와 `windows_logic.rs`의 `normalize_absolute_pointer`는 좋은 테스트를 가지고 있다.

**개선:** 캡처 모듈의 좌표 변환, 에러 매핑, 버퍼 크기 검증 로직을 `#[cfg(test)]`로 추출해 테스트한다.

---

## 11. 권장 작업 순서

1. **Windows 비동기 MFT + NVENC 통합** (P0)
   - 가장 큰 성능 향상이 기대된다. 하드웨어 인코더 사용 시 CPU 사용률과 레이턴시가 크게 감소한다.
2. **Linux 오디오 파이프라인 연결** (P0)
   - `audio_linux.rs`가 이미 구현되어 있으므로 세션 연결만으로 기능을 완성할 수 있다.
3. **프레임 버퍼 풀링 및 제로카피** (P0/P1)
   - Windows/Linux 공통으로 캡처→인코드 경로의 메모리 할당을 제거한다.
4. **세션-인코더 제어 채널** (P1)
   - ABR과 키프레임 요청이 실제 인코더에 전달되도록 한다.
5. **클립보드 공통화 및 Wayland 네이티브화** (P1/P2)
   - 중복 제거와 프로세스 스폰 오버헤드 제거.
6. **Windows WASAPI 오디오** (P2)
   - Linux 오디오 연결 후 동일한 패턴으로 구현한다.

---

## 12. 참고: 발견된 사소한 문제들

| 파일 | 문제 | 심각도 |
|------|------|--------|
| `encode_linux.rs` | NVENC 성공 시만 로그 출력 | Low |
| `session.rs` | Windows `PipelineFrame::Stop`이 dead code | Low |
| `session.rs` | `VideoFrame` 타임스탬프가 부정확 | Medium |
| `capture_linux.rs` | Y-inverted 프레임 처리가 느림 | Medium |
| `inject_linux.rs` | `ABSOLUTE_AXIS_MAX`가 16비트 고정 | Medium |
| `clipboard_linux.rs` | 2초 에코 억제 기간이 부정확할 수 있음 | Medium |
| `session.rs` | Windows 캡처→인코드 채널이 unbounded | High |
| `encode_windows.rs` | 하드웨어 MFT 미지원으로 CPU 인코딩 강제 | High |
| `audio_linux.rs` | 세션에 연결되지 않음 | High |
