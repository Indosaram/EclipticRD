# ClientCore Knowledge Base

<!-- Score: 18 | Domain: Client rendering, hardware decoding, input capture, audio playback -->

## OVERVIEW
Client-side remote desktop engine managing low-latency video decoding, Metal GPU rendering, audio playback, and local input event dispatch.

## WHERE TO LOOK
| Task | File | Key Symbol |
|------|------|------------|
| Client Orchestration | `ClientCore.swift` | `ClientCore`, `StreamStats` |
| Metal Video Rendering | `MetalRenderer.swift` | `MetalRenderer`, `TrackingMTKView` |
| Video Decompression | `VideoDecoder.swift` | `VideoDecoder` |
| Frame Reassembly | `FrameReceiver.swift` | `FrameReceiver`, `FrameAssembly`, `LossEntry` |
| Input Event Capture | `InputSender.swift` | `InputSender` |
| Low-latency Audio | `ClientAudioPlayer.swift` | `ClientAudioPlayer` |

## KEY INVARIANTS
- **Frame Assembly**: Incomplete frames with missing sequence numbers trigger NACK retransmits or IDR keyframe requests via control channel.
- **Metal Texture Pipeline**: `CVMetalTextureCache` maps incoming `CVPixelBuffer` YUV/RGB planes directly to Metal textures without intermediate memory copies.
- **Rolling Telemetry**: `StreamStats` maintains rolling window calculations for current FPS, decode latency, network jitter, and packet loss percentage.

## CONVENTIONS
- **GPU Synchronization**: Render `CVPixelBuffer` textures using double/triple buffering to prevent GPU stalls.
- **Immediate Input**: Dispatch user input payloads immediately without debounce or batching exceeding 16ms.
- **Loss Recovery**: `FrameReceiver` tracks chunk loss and signals missing frames to trigger keyframe recovery.

## ANTI-PATTERNS
- Never use `NSImageView` or `NSImage` for high-frequency video stream presentation; use `MetalRenderer`.
- Do not perform decompression or texture allocation on the main thread.
- Avoid blocking the MTKView draw loop with synchronization locks or network calls.
