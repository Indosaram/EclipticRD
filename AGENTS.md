# PROJECT KNOWLEDGE BASE

**Generated:** 2026-08-31 13:32:57Z
**Commit:** 9d89884
**Branch:** main

## OVERVIEW
Ultra-low latency macOS Remote Desktop system written in Swift, leveraging ScreenCaptureKit, VideoToolbox hardware codecs, Metal GPU rendering, and Network.framework transports.

## STRUCTURE
```
.
├── Sources/
│   ├── HostCore/    # ScreenCaptureKit capture, VideoToolbox encoding, CGEvent input
│   ├── ClientCore/  # Metal MTKView rendering, VideoToolbox decoding, audio playback
│   ├── Shared/      # Packet payloads, NWConnection TCP/UDP channels, STUN/Bonjour
│   └── App/         # SwiftUI interface, ConnectionManager coordinator, ERDTheme
├── Tests/           # TestCLI target for protocol and codec unit tests
├── E2ETests/        # E2ETest target for loopback streaming verification
└── project.yml      # XcodeGen project definition
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Host Streaming & Capture | `Sources/HostCore/` | `ServerCore.swift`, `ScreenCapture.swift`, `VideoEncoder.swift` |
| Client Decoding & Metal Canvas | `Sources/ClientCore/` | `ClientCore.swift`, `MetalRenderer.swift`, `VideoDecoder.swift` |
| Wire Protocols & Transport | `Sources/Shared/` | `ProtocolFoundation.swift`, `TCPChannel.swift`, `UDPChannel.swift` |
| UI & State Orchestration | `Sources/App/` | `ConnectionManager.swift`, `HomeView.swift`, `RemoteDesktopView.swift` |

## CODE MAP
| Symbol | Type | Location | Refs | Role |
|--------|------|----------|------|------|
| `ServerCore` | class | `Sources/HostCore/ServerCore.swift` | 6 | Host session coordinator and streaming manager |
| `ClientCore` | class | `Sources/ClientCore/ClientCore.swift` | 32 | Client session coordinator and telemetry engine |
| `MetalRenderer` | class | `Sources/ClientCore/MetalRenderer.swift` | 7 | GPU YUV/RGB texture renderer with cursor overlay |
| `ConnectionManager` | class | `Sources/App/ConnectionManager.swift` | 12 | App-level state machine and preset coordinator |
| `StreamConfiguration` | struct | `Sources/Shared/ProtocolFoundation.swift` | 24 | Negotiated session quality, codec, and resolution config |
| `UDPChannel` | class | `Sources/Shared/UDPChannel.swift` | 8 | Low-latency packet transmission wrapper over NWConnection |
| `TCPChannel` | class | `Sources/Shared/TCPChannel.swift` | 9 | Reliable control and handshake stream wrapper over NWConnection |
| `ERDTheme` | enum | `Sources/App/AppTheme.swift` | 530 | Design system tokens and semantic color definitions |

## CONVENTIONS
- **macOS Native**: Prefer `Network.framework` over BSD sockets, `Metal` over CoreGraphics / AppKit drawing.
- **XcodeGen**: All Xcode project modifications MUST be made in `project.yml`, never edited directly in `.xcodeproj`.
- **Async/Await**: Use Swift Structured Concurrency for network lifecycle and background workers.
- **Thread Affinity**: All SwiftUI UI modifications must dispatch on `@MainActor`; video/audio pipelines run on dedicated QoS queues.

## ANTI-PATTERNS (THIS PROJECT)
- Avoid raw pointer socket operations; use `NWConnection` or `NWListener`.
- Do not use `NSImageView` or `NSView` for high-frequency frame presentation; use `MetalRenderer`.
- Never commit `EclipticRD.xcodeproj` manually if it conflicts with `project.yml`.
- Never perform frame compression, decompression, or network I/O on the main dispatch queue.

## COMMANDS
```bash
# Generate Xcode project from project.yml
xcodegen generate

# Build App Target
xcodebuild -scheme EclipticRD -destination 'platform=macOS' build

# Headless integration tests: TestCLI is a TOOL, not a test bundle —
# `xcodebuild -scheme TestCLI test` does NOT work. Build it, then run the binary.
xcodebuild -scheme TestCLI -destination 'platform=macOS' build
"$(xcodebuild -scheme TestCLI -destination 'platform=macOS' -showBuildSettings build 2>/dev/null | awk '/ BUILT_PRODUCTS_DIR/{print $3}')/TestCLI"

# Unit tests (XCTest: crypto, wire protocol, pairing)
xcodebuild -scheme EclipticRDLogicTests -destination 'platform=macOS' test
```

## NOTES
- Requires **macOS 13.0+** for ScreenCaptureKit and modern Metal texture caching APIs.
- Requires Hardened Runtime with Screen Recording and Accessibility permissions enabled for capture and input injection.
