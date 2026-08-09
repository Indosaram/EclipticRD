# HostCore Knowledge Base

## OVERVIEW
Implementation of the host-side Remote Desktop logic: screen capture, hardware video encoding, and OS-level input injection.

## STRUCTURE
- `ServerCore.swift`: Entry point for hosting. Manages client connections and sub-modules.
- `ScreenCapture.swift`: Uses `ScreenCaptureKit` for high-performance frame retrieval.
- `VideoEncoder.swift`: Wraps `VideoToolbox` for hardware-accelerated H.264/HEVC encoding.
- `InputReceiver.swift`: Converts incoming control payloads into `CGEvent` system calls.
- `FrameSender.swift`: Orchestrates frame transmission timing and flow control.

## WHERE TO LOOK
- **Input Injection**: `InputReceiver.swift` (Requires Accessibility Permissions).
- **Encoding Pipeline**: `VideoEncoder.swift` handles the transition from `CVPixelBuffer` to H.264 CMSampleBuffers.
- **Performance**: Monitor `FrameSender.swift` for congestion control logic.

## CONVENTIONS
- **SCKit Integration**: Always check stream configuration constraints (e.g., color space).
- **Buffer Recycling**: Ensure `CVPixelBufferPool` is used for efficient memory handling in VideoEncoder.

## ANTI-PATTERNS
- Do not perform expensive image processing on the main thread; use dedicated background dispatch queues.
- Never use `CoreGraphics` screen capture (`CGDisplayCreateImage`); it is too slow for 60fps streaming.
