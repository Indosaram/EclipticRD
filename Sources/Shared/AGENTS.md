# Shared Knowledge Base

## OVERVIEW
Common utilities, network protocol definitions, and data structures shared between Host and Client targets.

## STRUCTURE
- `FramePayloads.swift`: Byte-level definitions for video packets and control messages.
- `UDPChannel.swift` / `TCPChannel.swift`: Wrapper logic around `NWConnection` for reliable and unreliable streams.
- `BonjourService.swift`: Service discovery and advertisement logic.
- `STUNManager.swift`: ICE-like hole punching for NAT traversal.

## WHERE TO LOOK
- **Protocol Changes**: Update `FramePayloads.swift` (ensure byte alignment).
- **Network Troubleshooting**: Check `UDPChannel.swift` connection state handlers.

## CONVENTIONS
- **Protocols First**: Any change to shared payloads MUST be backward compatible or require a protocol version bump.
- **Low Latency**: Favor `UDP` for video frames; use `TCP` for critical control events.

## ANTI-PATTERNS
- Do not include Host-only or Client-only dependencies (like Metal or SCKit) in this module.
- Avoid large buffers in `Shared`; use streaming mechanisms where possible.
