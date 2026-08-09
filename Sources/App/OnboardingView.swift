import SwiftUI
import AppKit
import ApplicationServices
import CoreGraphics

struct OnboardingView: View {
    let onContinue: () -> Void

    @AppStorage("screenRecordingRequested") private var screenRecordingRequested = false
    @State private var screenRecordingGranted = false
    @State private var accessibilityGranted = false
    @State private var relaunchRequired = false

    private let permissionPoller = Timer.publish(every: 1.5, on: .main, in: .common).autoconnect()

    var body: some View {
        ZStack {
            ERDAppBackground()

            permissionsPanel
                .frame(maxWidth: 560)
                .padding(ERDTheme.Spacing.screen)
        }
        .frame(minWidth: 560, minHeight: 480)
        .preferredColorScheme(.dark)
        .onAppear {
            refreshPermissionState()
        }
        .onReceive(permissionPoller) { _ in
            refreshPermissionState()
        }
    }



    private var permissionsPanel: some View {
        ERDPanel(padding: ERDTheme.Spacing.panelLarge) {
            HStack(spacing: ERDTheme.Spacing.small) {
                ERDStatusPill(title: "EclipticRD", systemImage: "sparkles", tint: ERDTheme.blue)
                ERDStatusPill(title: "Setup", systemImage: "hand.raised.fill", tint: ERDTheme.teal)
            }

            ERDSectionHeader(
                eyebrow: "Welcome",
                title: "Prepare this Mac",
                detail: "Grant the two macOS permissions below to enable low-latency screen capture and remote input."
            )

            VStack(spacing: ERDTheme.Spacing.section) {
                PermissionCard(
                    icon: "display.2",
                    title: "Screen Recording",
                    description: "Required to capture and stream the desktop during remote sessions.",
                    isGranted: screenRecordingGranted,
                    action: grantScreenRecording,
                    statusText: screenRecordingStatusText,
                    accent: ERDTheme.blue
                )

                PermissionCard(
                    icon: "cursorarrow.motionlines",
                    title: "Accessibility",
                    description: "Required to send keyboard and mouse input to the Mac you are controlling.",
                    isGranted: accessibilityGranted,
                    action: grantAccessibility,
                    statusText: nil,
                    accent: ERDTheme.teal
                )
            }

            statusFooter
        }
    }

    private var statusFooter: some View {
        VStack(alignment: .leading, spacing: ERDTheme.Spacing.small) {
            Text("WORKSPACE ACCESS")
                .font(ERDTheme.Typography.eyebrow)
                .tracking(1.2)
                .foregroundStyle(ERDTheme.blue)

            ERDRowCard(
                alignment: .center,
                spacing: ERDTheme.Spacing.section,
                padding: ERDTheme.Spacing.section,
                chrome: .surface(
                    fill: ERDTheme.surface,
                    stroke: ERDTheme.panelBorder,
                    cornerRadius: ERDTheme.Radius.rowCard
                )
            ) {
                ERDIconBadge(
                    systemImage: statusIcon,
                    tint: statusTint,
                    size: ERDTheme.Layout.rowIconSize,
                    chrome: .elevated(
                        stroke: statusTint.opacity(0.24),
                        cornerRadius: ERDTheme.Radius.tile
                    )
                )
            } content: {
                VStack(alignment: .leading, spacing: ERDTheme.Spacing.row) {
                    HStack(spacing: ERDTheme.Spacing.small) {
                        ERDStatusPill(
                            title: bothGranted ? "Ready" : "Setup Required",
                            systemImage: bothGranted ? "checkmark.circle.fill" : "slider.horizontal.3",
                            tint: bothGranted ? ERDTheme.green : ERDTheme.amber
                        )

                        if relaunchRequired {
                            ERDStatusPill(
                                title: "Restart Needed",
                                systemImage: "arrow.clockwise.circle",
                                tint: ERDTheme.red
                            )
                        }
                    }

                    Text(onboardingMessage)
                        .font(ERDTheme.Typography.bodyEmphasized)
                        .foregroundStyle(ERDTheme.strongText)
                        .fixedSize(horizontal: false, vertical: true)
                }
            } trailing: {
                footerAction
            }
        }
    }

    @ViewBuilder
    private var footerAction: some View {
        if relaunchRequired {
            Button("Quit App") {
                NSApplication.shared.terminate(nil)
            }
            .buttonStyle(ERDActionButtonStyle(tint: ERDTheme.red))
        } else {
            Button("Continue") {
                onContinue()
            }
            .buttonStyle(ERDActionButtonStyle(tint: ERDTheme.blue))
            .disabled(!canContinue)
        }
    }

    private var statusTint: Color {
        if canContinue {
            return ERDTheme.green
        }
        if relaunchRequired {
            return ERDTheme.red
        }
        return ERDTheme.amber
    }

    private var statusIcon: String {
        if canContinue {
            return "checkmark.circle.fill"
        }
        if relaunchRequired {
            return "arrow.clockwise.circle"
        }
        return "lock.shield"
    }

    private var bothGranted: Bool {
        screenRecordingGranted && accessibilityGranted
    }

    private var canContinue: Bool {
        bothGranted && !relaunchRequired
    }

    private var onboardingMessage: String {
        if relaunchRequired {
            return "Please allow Screen Recording in System Settings, then Quit and Reopen EclipticRD."
        }
        if bothGranted {
            return "All permissions granted. You can continue into the main workspace."
        }
        return "Grant both permissions to continue."
    }

    private var screenRecordingStatusText: String? {
        if relaunchRequired {
            return "Access requested. Reopen the app after allowing screen recording."
        }
        if screenRecordingGranted {
            return "Granted."
        }
        return nil
    }

    private func refreshPermissionState() {
        screenRecordingGranted = CGPreflightScreenCaptureAccess()
        accessibilityGranted = AXIsProcessTrusted()
    }

    private func grantScreenRecording() {
        screenRecordingRequested = true
        if CGPreflightScreenCaptureAccess() {
            screenRecordingGranted = true
            relaunchRequired = false
            return
        }

        let granted = CGRequestScreenCaptureAccess()
        if granted {
            screenRecordingGranted = true
            relaunchRequired = false
        } else {
            relaunchRequired = true
            if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture") {
                NSWorkspace.shared.open(url)
            }
            screenRecordingGranted = CGPreflightScreenCaptureAccess()
        }
    }

    private func grantAccessibility() {
        let opts = [kAXTrustedCheckOptionPrompt.takeUnretainedValue(): true] as CFDictionary
        let success = AXIsProcessTrustedWithOptions(opts)
        if !success {
            if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility") {
                NSWorkspace.shared.open(url)
            }
        }
        accessibilityGranted = success
    }
}

private struct PermissionCard: View {
    let icon: String
    let title: String
    let description: String
    let isGranted: Bool
    let action: () -> Void
    let statusText: String?
    let accent: Color

    var body: some View {
        ERDRowCard(
            alignment: .center,
            spacing: ERDTheme.Spacing.section,
            padding: ERDTheme.Spacing.card,
            chrome: .surface(
                fill: ERDTheme.subduedSurface,
                stroke: isGranted ? accent.opacity(0.28) : ERDTheme.softBorder,
                cornerRadius: ERDTheme.Radius.rowCard
            )
        ) {
            ERDIconBadge(
                systemImage: icon,
                tint: ERDTheme.strongText,
                size: ERDTheme.Layout.rowIconSize,
                chrome: .elevated(
                    stroke: accent.opacity(isGranted ? 0.30 : 0.22),
                    cornerRadius: ERDTheme.Radius.tile
                )
            )
        } content: {
            VStack(alignment: .leading, spacing: ERDTheme.Spacing.row) {
                HStack(alignment: .center, spacing: ERDTheme.Spacing.small) {
                    Text(title)
                        .font(ERDTheme.Typography.bodyEmphasized)
                        .foregroundStyle(ERDTheme.strongText)

                    if isGranted {
                        ERDStatusPill(
                            title: "Granted",
                            systemImage: "checkmark",
                            tint: ERDTheme.green
                        )
                    }
                }

                Text(description)
                    .font(ERDTheme.Typography.body)
                    .foregroundStyle(ERDTheme.mutedText)
                    .fixedSize(horizontal: false, vertical: true)

                if let statusText {
                    Label(statusText, systemImage: "info.circle")
                        .font(ERDTheme.Typography.captionMedium)
                        .foregroundStyle(ERDTheme.mutedText)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        } trailing: {
            if !isGranted {
                Button("Grant Access") {
                    action()
                }
                .buttonStyle(ERDActionButtonStyle(tint: accent))
            }
        }
    }
}
