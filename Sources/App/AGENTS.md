# App Knowledge Base

<!-- Score: 24 | Domain: SwiftUI application lifecycle, UI views, design system, connection management -->

## OVERVIEW
Main SwiftUI application layer containing the user interface, session state coordination, connection flows, telemetry HUD, and design system tokens.

## WHERE TO LOOK
| Task | File | Key Symbol |
|------|------|------------|
| App Lifecycle | `EclipticApp.swift` | `EclipticApp` |
| Session & State Coordination | `ConnectionManager.swift` | `ConnectionManager`, `State`, `StreamQualityPreset` |
| Main Dashboard | `HomeView.swift` | `HomeView`, `HomeBonjourBrowserManager` |
| Remote Session Canvas & HUD | `RemoteDesktopView.swift` | `RemoteDesktopView`, `StatsOverlayView`, `MetalViewRepresentable` |
| Manual Connection Sheet | `HomeView.swift` | `remotePINCard`, `discoveredWorkspaceCard` |
| Permission Onboarding | `OnboardingView.swift` | `OnboardingView`, `PermissionCard` |
| Design Tokens & UI Chrome | `AppTheme.swift` | `ERDTheme`, `Spacing`, `Radius`, `Typography`, `ERDCardChrome` |

## KEY INVARIANTS
- **State Transitions**: `ConnectionManager` enforces linear state progression (`idle` -> `connecting` -> `connected` -> `disconnecting` -> `idle`) with cancellation guards.
- **HUD Performance Telemetry**: `StatsOverlayView` renders real-time FPS sparklines, RTT graphs, and bitrate meters without adding view invalidation overhead to the video canvas.
- **Permission Flow**: `OnboardingView` proactively checks Screen Recording and Accessibility status using TCC APIs before allowing session start.

## CONVENTIONS
- **Single Source of Truth**: All connection state transitions, quality presets, and telemetry feed through `ConnectionManager`.
- **Design System Consistency**: Use semantic tokens from `ERDTheme` (colors, typography, radii, spacing, card chrome) rather than hardcoded UI constants.
- **Main Thread UI**: All state updates that affect SwiftUI views must be published on `@MainActor`.

## ANTI-PATTERNS
- Avoid placing networking or video decoding logic inside SwiftUI view structs.
- Never block `@MainActor` with synchronous discovery, network I/O, or key event processing.
- Do not instantiate secondary `ClientCore` or `ServerCore` instances outside `ConnectionManager`.
