# PROJECT KNOWLEDGE BASE

**Generated:** 2026-03-31 09:10:15Z
**Commit:** 4a4bdbe9
**Branch:** main

## OVERVIEW
High-performance macOS Remote Desktop system. Built with Swift, leveraging native frameworks for ultra-low latency screen capture and rendering.

## STRUCTURE
```
.
├── Sources/
│   ├── HostCore/    # Screen capture, encoding, input injection
│   ├── ClientCore/  # Metal rendering, decoding, input capture
│   ├── Shared/      # Network protocols, common utilities
│   └── App/         # SwiftUI interface and app entry
├── Tests/           # Unit and Integration tests
├── project.yml      # XcodeGen project definition
└── EclipticRD.xcodeproj # Generated (do not edit directly)
```

## WHERE TO LOOK
| Task | Location | Notes |
|------|----------|-------|
| Host Core Logic | `Sources/HostCore` | Video/Input streaming implementation |
| Client Renderer | `Sources/ClientCore` | Metal rendering and user control |
| Protocol Defines | `Sources/Shared` | UDP/TCP payload structures |
| UI/Integration | `Sources/App` | SwiftUI entry point |

## CODE MAP
| Symbol | Type | Location | Role |
|--------|------|----------|------|
| `ServerCore` | class | `Sources/HostCore/ServerCore.swift` | Host session orchestration |
| `VideoEncoder` | class | `Sources/HostCore/VideoEncoder.swift` | H.264/HEVC compression |
| `MetalRenderer` | class | `Sources/ClientCore/MetalRenderer.swift` | GPU-accelerated video display |
| `UDPChannel` | class | `Sources/Shared/UDPChannel.swift` | Low-latency data transmission |

## CONVENTIONS
- **macOS Native**: Prefer `Network.framework` over sockets, `Metal` over CoreGraphics.
- **XcodeGen**: Modification to project settings MUST be done in `project.yml`.
- **Async/Await**: Use Swift Structured Concurrency for networking and background tasks.

## ANTI-PATTERNS (THIS PROJECT)
- Avoid manual socket pointers; use `NWConnection` or `NWListener`.
- Do not use `NSView` for high-frequency updates; use `MetalRenderer`.
- Never commit `EclipticRD.xcodeproj` manually if it conflicts with `project.yml`.

## COMMANDS
```bash
# Generate Xcode project
xcodegen generate

# Run tests
xcodebuild test -scheme EclipticRD -destination 'platform=macOS'
```

## NOTES
- Requires **macOS 13.0+** for ScreenCaptureKit and newer Metal APIs.
- Ensure "Hardened Runtime" is enabled with proper entitlements for input/screen recording.
