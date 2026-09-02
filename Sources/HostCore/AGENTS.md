# HostCore Knowledge Base

<!-- Score: 16 | Domain: Host capture, hardware encoding, OS input injection -->

## OVERVIEW
Host-side remote desktop engine orchestrating ScreenCaptureKit capture, VideoToolbox hardware encoding, and CGEvent input injection.

## WHERE TO LOOK
| Task | File | Key Symbol |
|------|------|------------|
| Host Orchestration | `ServerCore.swift` | `ServerCore` |
| Screen Capture | `ScreenCapture.swift` | `ScreenCapture`, `ScreenCaptureError` |
| Video Compression | `VideoEncoder.swift` | `VideoEncoder`, `VideoEncoderError` |
| Input Simulation | `InputReceiver.swift` | `InputReceiver` |
| Frame Chunking | `FrameSender.swift` | `FrameSender` |

## KEY INVARIANTS
- **Keyframe Generation**: IDR frames are forced upon initial client connection and whenever packet loss exceeds recovery threshold.
- **Color Space & Scaling**: Stream configuration defaults to Rec.709 with hardware-accelerated color conversion.
- **Packet Fragmentation**: Frames exceeding MTU (1200 bytes) are split into `FrameChunkPayload` packets with monotonic sequence IDs.

## CONVENTIONS
- **Buffer Management**: Use `CVPixelBufferPool` for efficient VTCompressionSession allocation without frame copying.
- **Capture Timing**: ScreenCaptureKit frames are handled on a high-priority serial dispatch queue; avoid locking or blocking.
- **Accessibility**: Verify system input injection permissions before attempting `CGEventPost`.

## ANTI-PATTERNS
- Never use legacy `CGDisplayCreateImage` or `AVFoundation` display capture; SCKit is required for low-latency 60+ FPS.
- Do not perform frame encoding or network I/O on the main thread.
- Avoid dropping keyframes without notifying `ServerCore` to schedule an immediate IDR refresh.
