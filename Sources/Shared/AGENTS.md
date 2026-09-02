# Shared Knowledge Base

<!-- Score: 22 | Domain: Binary wire protocols, network transport channels, discovery, NAT traversal -->

## OVERVIEW
Shared protocol definitions, binary packet serialization, network transport abstractions, clipboard synchronization, and diagnostic logging.

## WHERE TO LOOK
| Task | File | Key Symbol |
|------|------|------------|
| Stream & Clipboard Protocol | `ProtocolFoundation.swift` | `StreamConfiguration`, `ClipboardSyncRequestPayload` |
| Packet Framing | `PacketHeader.swift` | `PacketHeader`, `PacketType` |
| Video Packet Payloads | `FramePayloads.swift` | `FrameHeaderPayload`, `FrameChunkPayload` |
| Input Event Payloads | `InputPayloads.swift` | `InputEventPayload`, `ModifierFlags` |
| Control Messaging | `ControlTypes.swift` | `ControlMessage`, `ControlMessageType` |
| Handshake & Capabilities | `HandshakePayload.swift` | `HandshakePayload`, `HandshakeCapabilities` |
| TCP Transport | `TCPChannel.swift` | `TCPChannel` |
| UDP Transport | `UDPChannel.swift` | `UDPChannel` |
| Signaling Server Client | `SignalingClient.swift` | `SignalingClient`, `SessionCandidate` |
| STUN NAT Traversal | `STUNClient.swift` | `STUNClient` |
| Bonjour Discovery | `Bonjour.swift` | `BonjourBrowser`, `DiscoveredHost` |
| Clipboard Sync Monitor | `ClipboardMonitor.swift` | `ClipboardMonitor` |
| Logging & Constants | `ERDLog.swift`, `ERDConstants.swift` | `ERDLog`, `ERDConstants` |

## CONVENTIONS
- **Binary Wire Alignment**: Payload structs must use deterministic memory layouts and explicit endianness conversion.
- **Protocol Stability**: Add fields additively with capability negotiation flags; do not break backward compatibility.
- **Separation of Concerns**: Keep `Shared` free of Metal, ScreenCaptureKit, or AppKit/SwiftUI view dependencies.

## ANTI-PATTERNS
- Avoid raw socket implementations; always utilize Apple's `Network.framework` (`NWConnection`, `NWListener`).
- Do not allocate unbound memory buffers during packet deserialization.
