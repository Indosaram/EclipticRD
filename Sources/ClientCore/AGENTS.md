# ClientCore Knowledge Base

## OVERVIEW
Client-side logic for the Remote Desktop viewer: Metal-based rendering, video decoding, and capture of local user inputs.

## STRUCTURE
- `ClientController.swift`: Orchestrates the connection state and feedback loop to the host.
- `MetalRenderer.swift`: MTKView-based GPU rendering pipeline for YUV/RGB buffers.
- `InputCapture.swift`: Local event monitor to package keyboard/mouse events.
- `SessionCoordinator.swift`: Logic for Bonjour discovery and initial handshake.

## WHERE TO LOOK
- **Rendering Pipeline**: `MetalRenderer.swift` (Check MTKViewDelegate).
- **Input Forwarding**: `InputCapture.swift` (Packaging `InputPayload`).
- **Connection Logic**: `ClientController.swift`.

## CONVENTIONS
- **GPU Synchronization**: Be mindful of triple buffering in Metal to avoid frame drops.
- **Input Lag**: Package and send input events immediately; do not batch if delay exceeds 16ms.

## ANTI-PATTERNS
- Avoid `UIImageView` or `NSImage` for video stream rendering.
- Do not block the render loop with networking tasks.
