import SwiftUI
import MetalKit
import AppKit

struct RemoteDesktopView: View {
    @EnvironmentObject var connectionManager: ConnectionManager
    @State private var showStats = false
    @State private var controlsExpanded = false
    @State private var titleBarInset: CGFloat = 0
    @State private var sessionWindow: NSWindow?

    var body: some View {
        GeometryReader { proxy in
            ZStack {
                MetalViewRepresentable()
                    .ignoresSafeArea()

                WindowChromeInsetReader(topInset: $titleBarInset)
                    .frame(width: 0, height: 0)
                    .allowsHitTesting(false)

                SessionWindowReader(
                    window: $sessionWindow,
                    onFullscreenChanged: { isFullscreen in
                        connectionManager.updateFullscreenState(isFullscreen)
                        if !isFullscreen {
                            restoreWindowPresentationIfNeeded()
                        }
                    }
                )
                    .frame(width: 0, height: 0)
                    .allowsHitTesting(false)

                // Top-Right Panel: Session Controls
                VStack(alignment: .trailing) {
                    SessionControlsView(
                        serverName: connectionManager.serverName ?? "Remote Session",
                        sessionControls: connectionManager.sessionControls,
                        showStats: $showStats,
                        isExpanded: $controlsExpanded,
                        toggleImmersiveMode: toggleImmersiveMode,
                        selectQuality: { preset in
                            connectionManager.requestStreamConfiguration(quality: preset)
                        },
                        selectResolution: { preset in
                            connectionManager.requestStreamConfiguration(resolution: preset)
                        }
                    ) {
                        connectionManager.disconnect()
                    }
                }
                .padding(.top, overlayTopInset(safeAreaTop: proxy.safeAreaInsets.top))
                .padding(.trailing, ERDTheme.Spacing.card)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)

                // Bottom-Left Panel: Session Telemetry HUD
                if showStats {
                    StatsOverlayView()
                        .frame(maxWidth: ERDTheme.Layout.dashboardSidebarWidth, alignment: .leading)
                        .transition(.move(edge: .bottom).combined(with: .opacity))
                        .padding(.bottom, overlayBottomInset(safeAreaBottom: proxy.safeAreaInsets.bottom))
                        .padding(.leading, ERDTheme.Spacing.card)
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomLeading)
                }

                // Floating Clipboard Sync Toast
                if connectionManager.showClipboardToast {
                    VStack {
                        HStack(spacing: ERDTheme.Spacing.small) {
                            Image(systemName: "doc.on.clipboard.fill")
                                .font(ERDTheme.Typography.detailMedium)
                                .foregroundStyle(ERDTheme.green)
                            
                            Text(connectionManager.clipboardToastText)
                                .font(ERDTheme.Typography.detailMedium)
                                .foregroundStyle(ERDTheme.strongText)
                        }
                        .padding(.horizontal, ERDTheme.Spacing.section)
                        .padding(.vertical, ERDTheme.Spacing.small)
                        .erdCardBackground(
                            .surface(
                                fill: ERDTheme.elevatedSurface.opacity(0.96),
                                stroke: ERDTheme.green.opacity(0.38),
                                cornerRadius: ERDTheme.Radius.panel
                            )
                        )
                        .transition(.move(edge: .top).combined(with: .opacity))
                        .padding(.top, overlayTopInset(safeAreaTop: proxy.safeAreaInsets.top))
                        
                        Spacer()
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                    .allowsHitTesting(false)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .animation(.spring(response: 0.24, dampingFraction: 0.88), value: controlsExpanded)
        .animation(.easeInOut(duration: 0.18), value: showStats)
        .preferredColorScheme(.dark)
        .onAppear {
            KeyboardShortcutMonitor.shared.register { showStats.toggle() }
        }
        .onDisappear {
            KeyboardShortcutMonitor.shared.unregister()
            restoreWindowPresentationIfNeeded()
        }
    }

    private func overlayTopInset(safeAreaTop: CGFloat) -> CGFloat {
        max(
            ERDTheme.Spacing.card,
            safeAreaTop + ERDTheme.Spacing.small,
            titleBarInset + ERDTheme.Spacing.small
        )
    }

    private func overlayBottomInset(safeAreaBottom: CGFloat) -> CGFloat {
        max(
            ERDTheme.Spacing.card,
            safeAreaBottom + ERDTheme.Spacing.small
        )
    }

    private func toggleImmersiveMode() {
        guard let window = sessionWindow else { return }

        let shouldEnable = !connectionManager.sessionControls.isImmersiveModeEnabled || !connectionManager.sessionControls.isFullscreen
        connectionManager.setImmersiveModeEnabled(shouldEnable)
        window.collectionBehavior.insert(.fullScreenPrimary)

        if shouldEnable {
            window.titleVisibility = .hidden
            window.titlebarAppearsTransparent = true
            NSApplication.shared.presentationOptions = [.autoHideToolbar, .autoHideMenuBar, .fullScreen]
            if !window.styleMask.contains(.fullScreen) {
                window.toggleFullScreen(nil)
            }
        } else {
            restoreWindowPresentation(window: window)
            if window.styleMask.contains(.fullScreen) {
                window.toggleFullScreen(nil)
            }
        }
    }

    private func restoreWindowPresentationIfNeeded() {
        guard let window = sessionWindow else { return }
        restoreWindowPresentation(window: window)

        if window.styleMask.contains(.fullScreen) {
            window.toggleFullScreen(nil)
        }
    }

    private func restoreWindowPresentation(window: NSWindow) {
        NSApplication.shared.presentationOptions = []
        window.titleVisibility = .visible
        window.titlebarAppearsTransparent = false
    }
}

private struct SessionControlsView: View {
    @EnvironmentObject var connectionManager: ConnectionManager
    let serverName: String
    let sessionControls: ConnectionManager.SessionControlState
    @Binding var showStats: Bool
    @Binding var isExpanded: Bool
    let toggleImmersiveMode: () -> Void
    let selectQuality: (ConnectionManager.StreamQualityPreset) -> Void
    let selectResolution: (ConnectionManager.StreamResolutionPreset) -> Void
    let disconnect: () -> Void

    @State private var isHoveringAnchor = false

    var body: some View {
        VStack(alignment: .trailing, spacing: ERDTheme.Spacing.small) {
            if !isExpanded {
                Button {
                    withAnimation(.spring(response: 0.28, dampingFraction: 0.82)) {
                        isExpanded = true
                    }
                } label: {
                    Image(systemName: "display.and.arrow.down")
                        .font(.system(size: 16, weight: .semibold))
                        .foregroundStyle(isHoveringAnchor ? ERDTheme.blue : ERDTheme.strongText)
                        .frame(width: 42, height: 42)
                        .background(.ultraThinMaterial)
                        .clipShape(Circle())
                        .overlay(
                            Circle()
                                .strokeBorder(isHoveringAnchor ? ERDTheme.blue.opacity(0.48) : ERDTheme.panelBorder.opacity(0.6), lineWidth: 1.2)
                        )
                        .shadow(color: Color.black.opacity(0.3), radius: 6, x: 0, y: 3)
                }
                .buttonStyle(.plain)
                .onHover { hovering in
                    isHoveringAnchor = hovering
                }
                .transition(.scale.combined(with: .opacity))
            } else {
                Button {
                    withAnimation(.spring(response: 0.28, dampingFraction: 0.82)) {
                        isExpanded = false
                    }
                } label: {
                    HStack(alignment: .center, spacing: ERDTheme.Spacing.small) {
                        ERDIconBadge(
                            systemImage: "display.and.arrow.down",
                            tint: ERDTheme.blue,
                            size: ERDTheme.Layout.featureIconSize,
                            chrome: .elevated(
                                stroke: ERDTheme.blue.opacity(0.24),
                                cornerRadius: ERDTheme.Radius.iconCompact
                            )
                        )

                        VStack(alignment: .leading, spacing: ERDTheme.Spacing.micro) {
                            Text(serverName)
                                .font(ERDTheme.Typography.detailMedium)
                                .foregroundStyle(ERDTheme.strongText)
                                .lineLimit(1)

                            HStack(spacing: ERDTheme.Spacing.fine) {
                                Circle()
                                    .fill(anchorTint)
                                    .frame(width: ERDTheme.Spacing.fine, height: ERDTheme.Spacing.fine)

                                Text(anchorSubtitle)
                                    .font(ERDTheme.Typography.caption)
                                    .foregroundStyle(ERDTheme.mutedText)
                                    .lineLimit(1)
                            }
                        }
                        .frame(maxWidth: .infinity, alignment: .leading)

                        Button {
                            connectionManager.isAudioMuted.toggle()
                        } label: {
                            Image(systemName: connectionManager.isAudioMuted ? "speaker.slash.fill" : "speaker.wave.2.fill")
                                .font(ERDTheme.Typography.captionMedium)
                                .foregroundStyle(connectionManager.isAudioMuted ? ERDTheme.red : ERDTheme.green)
                                .frame(width: ERDTheme.Layout.featureIconSize, height: ERDTheme.Layout.featureIconSize)
                                .erdCardBackground(
                                    .elevated(
                                        stroke: (connectionManager.isAudioMuted ? ERDTheme.red : ERDTheme.green).opacity(0.22),
                                        cornerRadius: ERDTheme.Radius.iconCompact
                                    )
                                )
                        }
                        .buttonStyle(.plain)

                        Image(systemName: "chevron.up")
                            .font(ERDTheme.Typography.captionMedium)
                            .foregroundStyle(ERDTheme.blue)
                            .frame(width: ERDTheme.Layout.featureIconSize, height: ERDTheme.Layout.featureIconSize)
                            .erdCardBackground(
                                .elevated(
                                    stroke: ERDTheme.blue.opacity(0.22),
                                    cornerRadius: ERDTheme.Radius.iconCompact
                                )
                            )
                    }
                    .padding(.leading, ERDTheme.Spacing.small)
                    .padding(.trailing, ERDTheme.Spacing.row)
                    .padding(.vertical, ERDTheme.Spacing.small)
                    .frame(width: 300, alignment: .leading)
                    .erdCardBackground(
                        .surface(
                            fill: ERDTheme.surface.opacity(0.98),
                            stroke: ERDTheme.blue.opacity(0.24),
                            cornerRadius: ERDTheme.Radius.rowCard
                        )
                    )
                }
                .buttonStyle(.plain)
                .transition(.scale.combined(with: .opacity))

                VStack(alignment: .leading, spacing: ERDTheme.Spacing.section) {
                    HStack(alignment: .top, spacing: ERDTheme.Spacing.row) {
                        VStack(alignment: .leading, spacing: ERDTheme.Spacing.fine) {
                            Text("SESSION")
                                .font(ERDTheme.Typography.eyebrow)
                                .tracking(1.1)
                                .foregroundStyle(ERDTheme.blue)

                            Text(serverName)
                                .font(ERDTheme.Typography.bodyEmphasized)
                                .foregroundStyle(ERDTheme.strongText)
                                .lineLimit(1)

                            Text("Tune the live stream profile, keep telemetry nearby, or switch into an immersive fullscreen session.")
                                .font(ERDTheme.Typography.detail)
                                .foregroundStyle(ERDTheme.mutedText)
                                .fixedSize(horizontal: false, vertical: true)
                        }

                        Spacer(minLength: ERDTheme.Spacing.section)

                        ERDStatusPill(
                            title: statusPillTitle,
                            systemImage: statusPillIcon,
                            tint: statusTint
                        )
                    }

                    SessionProfileSummaryCard(sessionControls: sessionControls)

                    VStack(alignment: .leading, spacing: ERDTheme.Spacing.small) {
                        SessionControlSectionLabel(title: "Quality")

                        LazyVGrid(columns: controlGridColumns, alignment: .leading, spacing: ERDTheme.Spacing.small) {
                            ForEach(ConnectionManager.StreamQualityPreset.allCases) { preset in
                                SessionPresetButton(
                                    title: preset.title,
                                    detail: preset.detail,
                                    isSelected: sessionControls.selectedQuality == preset,
                                    isActive: sessionControls.activeQuality == preset,
                                    tint: ERDTheme.blue,
                                    action: { selectQuality(preset) }
                                )
                                .disabled(sessionControls.isRequestPending)
                            }
                        }
                    }

                    VStack(alignment: .leading, spacing: ERDTheme.Spacing.small) {
                        SessionControlSectionLabel(title: "Resolution")

                        LazyVGrid(columns: controlGridColumns, alignment: .leading, spacing: ERDTheme.Spacing.small) {
                            ForEach(ConnectionManager.StreamResolutionPreset.allCases) { preset in
                                SessionPresetButton(
                                    title: preset.title,
                                    detail: preset.detail,
                                    isSelected: sessionControls.selectedResolution == preset,
                                    isActive: sessionControls.activeResolution == preset,
                                    tint: ERDTheme.teal,
                                    action: { selectResolution(preset) }
                                )
                                .disabled(sessionControls.isRequestPending)
                            }
                        }
                    }

                    VStack(alignment: .leading, spacing: ERDTheme.Spacing.small) {
                        SessionControlSectionLabel(title: "Audio Streaming")

                        ERDRowCard(
                            alignment: .center,
                            chrome: .surface(
                                fill: ERDTheme.subduedSurface,
                                stroke: (connectionManager.isAudioMuted ? ERDTheme.red : ERDTheme.green).opacity(0.18),
                                cornerRadius: ERDTheme.Radius.rowCard
                            )
                        ) {
                            ERDIconBadge(
                                systemImage: connectionManager.isAudioMuted ? "speaker.slash.fill" : "speaker.wave.2.fill",
                                tint: connectionManager.isAudioMuted ? ERDTheme.red : ERDTheme.green,
                                chrome: .elevated(stroke: (connectionManager.isAudioMuted ? ERDTheme.red : ERDTheme.green).opacity(0.18), cornerRadius: ERDTheme.Radius.iconCompact)
                            )
                        } content: {
                            VStack(alignment: .leading, spacing: ERDTheme.Spacing.micro) {
                                Text("Host Audio Output")
                                    .font(ERDTheme.Typography.bodyEmphasized)
                                    .foregroundStyle(ERDTheme.strongText)

                                Text(connectionManager.isAudioMuted ? "Audio transmission is muted." : "Stereo Float32 LPCM 48kHz active.")
                                    .font(ERDTheme.Typography.detail)
                                    .foregroundStyle(ERDTheme.mutedText)
                                    .fixedSize(horizontal: false, vertical: true)
                            }
                        } trailing: {
                            Toggle("", isOn: Binding(
                                get: { !connectionManager.isAudioMuted },
                                set: { connectionManager.isAudioMuted = !$0 }
                            ))
                            .toggleStyle(.switch)
                        }
                    }

                    ERDRowCard(
                        alignment: .center,
                        chrome: .surface(
                            fill: ERDTheme.subduedSurface,
                            stroke: statusTint.opacity(0.18),
                            cornerRadius: ERDTheme.Radius.rowCard
                        )
                    ) {
                        ERDIconBadge(
                            systemImage: sessionControls.isImmersiveModeEnabled ? "arrow.down.right.and.arrow.up.left" : "rectangle.inset.filled.and.person.filled",
                            tint: statusTint,
                            chrome: .elevated(stroke: statusTint.opacity(0.18), cornerRadius: ERDTheme.Radius.iconCompact)
                        )
                    } content: {
                        VStack(alignment: .leading, spacing: ERDTheme.Spacing.micro) {
                            Text("Immersive mode")
                                .font(ERDTheme.Typography.bodyEmphasized)
                                .foregroundStyle(ERDTheme.strongText)

                            Text(sessionControls.isFullscreen ? "The session is filling the display with menu and toolbar chrome hidden." : "Enter fullscreen with desktop chrome suppressed while keeping the floating controls accessible.")
                                .font(ERDTheme.Typography.detail)
                                .foregroundStyle(ERDTheme.mutedText)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    } trailing: {
                        Button(sessionControls.isFullscreen ? "Exit" : "Enter") {
                            toggleImmersiveMode()
                        }
                        .buttonStyle(ERDActionButtonStyle(tint: ERDTheme.teal, isProminent: !sessionControls.isFullscreen))
                    }

                    HStack(spacing: ERDTheme.Spacing.small) {
                        Button(showStats ? "Hide Stats" : "Show Stats") {
                            showStats.toggle()
                        }
                        .buttonStyle(ERDActionButtonStyle(tint: ERDTheme.blue, isProminent: false))

                        Button(action: disconnect) {
                            Label("Disconnect", systemImage: "xmark.circle.fill")
                        }
                        .buttonStyle(ERDActionButtonStyle(tint: ERDTheme.red))
                    }

                    Text("Shortcuts: ⇧⌘S toggles stats · immersive mode keeps this overlay floating over fullscreen video")
                        .font(ERDTheme.Typography.caption)
                        .foregroundStyle(ERDTheme.mutedText)
                }
                .padding(ERDTheme.Spacing.field)
                .frame(maxWidth: 300, alignment: .leading)
                .erdCardBackground(
                    .surface(
                        fill: ERDTheme.surface.opacity(0.96),
                        stroke: ERDTheme.panelBorder,
                        cornerRadius: ERDTheme.Radius.rowCard
                    )
                )
                .transition(.move(edge: .top).combined(with: .opacity))
            }
        }
        .frame(maxWidth: ERDTheme.Layout.dashboardSidebarWidth, alignment: .trailing)
    }

    private var controlGridColumns: [GridItem] {
        [GridItem(.flexible(), spacing: ERDTheme.Spacing.small), GridItem(.flexible(), spacing: ERDTheme.Spacing.small)]
    }

    private var anchorSubtitle: String {
        if sessionControls.isRequestPending {
            return "Updating · \(sessionControls.selectedResolution.title) · \(sessionControls.selectedQuality.title)"
        }

        if sessionControls.isFullscreen {
            return "Immersive · \(sessionControls.activeSummary)"
        }

        return "Connected · \(sessionControls.activeSummary)"
    }

    private var anchorTint: Color {
        switch sessionControls.status {
        case .pending:
            return ERDTheme.amber
        case .rejected:
            return ERDTheme.red
        default:
            return sessionControls.isFullscreen ? ERDTheme.teal : ERDTheme.green
        }
    }

    private var statusTint: Color {
        switch sessionControls.status {
        case .pending:
            return ERDTheme.amber
        case .rejected:
            return ERDTheme.red
        case .applied:
            return ERDTheme.green
        case .ready:
            return sessionControls.isFullscreen ? ERDTheme.teal : ERDTheme.blue
        }
    }

    private var statusPillTitle: String {
        if sessionControls.isRequestPending {
            return "Updating"
        }

        if sessionControls.isFullscreen {
            return "Immersive"
        }

        switch sessionControls.status {
        case .applied:
            return "Applied"
        case .rejected:
            return "Rejected"
        case .pending:
            return "Updating"
        case .ready:
            return showStats ? "HUD On" : "Live"
        }
    }

    private var statusPillIcon: String {
        if sessionControls.isFullscreen {
            return "arrow.up.left.and.arrow.down.right"
        }

        switch sessionControls.status {
        case .pending:
            return "dial.medium.fill"
        case .rejected:
            return "xmark.octagon.fill"
        case .applied:
            return "checkmark.circle.fill"
        case .ready:
            return showStats ? "chart.xyaxis.line" : "dot.radiowaves.left.and.right"
        }
    }
}

private struct SessionProfileSummaryCard: View {
    let sessionControls: ConnectionManager.SessionControlState

    var body: some View {
        ERDRowCard(
            alignment: .center,
            chrome: .surface(
                fill: ERDTheme.subduedSurface,
                stroke: tint.opacity(0.18),
                cornerRadius: ERDTheme.Radius.rowCard
            )
        ) {
            ERDIconBadge(
                systemImage: icon,
                tint: tint,
                chrome: .elevated(stroke: tint.opacity(0.18), cornerRadius: ERDTheme.Radius.iconCompact)
            )
        } content: {
            VStack(alignment: .leading, spacing: ERDTheme.Spacing.micro) {
                Text(sessionControls.statusTitle)
                    .font(ERDTheme.Typography.bodyEmphasized)
                    .foregroundStyle(ERDTheme.strongText)

                Text(sessionControls.statusMessage)
                    .font(ERDTheme.Typography.detail)
                    .foregroundStyle(ERDTheme.mutedText)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private var tint: Color {
        switch sessionControls.status {
        case .pending:
            return ERDTheme.amber
        case .rejected:
            return ERDTheme.red
        case .applied:
            return ERDTheme.green
        case .ready:
            return sessionControls.isFullscreen ? ERDTheme.teal : ERDTheme.blue
        }
    }

    private var icon: String {
        switch sessionControls.status {
        case .pending:
            return "hourglass"
        case .rejected:
            return "exclamationmark.triangle.fill"
        case .applied:
            return "slider.horizontal.3"
        case .ready:
            return sessionControls.isFullscreen ? "rectangle.inset.filled.and.person.filled" : "display"
        }
    }
}

private struct SessionControlSectionLabel: View {
    let title: String

    var body: some View {
        Text(title.uppercased())
            .font(ERDTheme.Typography.eyebrow)
            .tracking(1.0)
            .foregroundStyle(ERDTheme.blue)
    }
}

private struct SessionPresetButton: View {
    let title: String
    let detail: String
    let isSelected: Bool
    let isActive: Bool
    let tint: Color
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(alignment: .leading, spacing: ERDTheme.Spacing.micro) {
                HStack(alignment: .center, spacing: ERDTheme.Spacing.fine) {
                    Text(title)
                        .font(ERDTheme.Typography.detailMedium)
                        .foregroundStyle(ERDTheme.strongText)
                        .lineLimit(1)

                    if isActive {
                        Image(systemName: "checkmark.circle.fill")
                            .font(ERDTheme.Typography.captionMedium)
                            .foregroundStyle(tint)
                    }
                }

                Text(detail)
                    .font(ERDTheme.Typography.caption)
                    .foregroundStyle(ERDTheme.mutedText)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, ERDTheme.Spacing.small)
            .padding(.vertical, ERDTheme.Spacing.row)
            .erdCardBackground(
                .surface(
                    fill: fillColor,
                    stroke: strokeColor,
                    cornerRadius: ERDTheme.Radius.control
                )
            )
        }
        .buttonStyle(.plain)
        .opacity(isSelected || isActive ? 1 : 0.94)
    }

    private var fillColor: Color {
        if isSelected || isActive {
            return ERDTheme.elevatedSurface
        }
        return ERDTheme.subduedSurface
    }

    private var strokeColor: Color {
        if isSelected {
            return tint.opacity(0.42)
        }

        if isActive {
            return tint.opacity(0.24)
        }

        return ERDTheme.softBorder
    }
}

struct StatsOverlayView: View {
    @State private var stats: ClientCore.StreamStats = ClientCore.StreamStats(fps: 0, bitrate: 0, frameLoss: 0, framesReceived: 0)
    @State private var fpsHistory: [Double] = []
    @State private var timer: Timer?

    private static let numberFormatter: NumberFormatter = {
        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        return formatter
    }()

    var body: some View {
        VStack(alignment: .leading, spacing: ERDTheme.Spacing.card) {
            HStack(alignment: .top, spacing: ERDTheme.Spacing.section) {
                VStack(alignment: .leading, spacing: ERDTheme.Spacing.fine) {
                    Text("Session Telemetry")
                        .font(ERDTheme.Typography.bodyEmphasized)
                        .foregroundStyle(ERDTheme.strongText)

                    Text("Live stream performance sampled from the remote desktop pipeline.")
                        .font(ERDTheme.Typography.detail)
                        .foregroundStyle(ERDTheme.mutedText)
                        .fixedSize(horizontal: false, vertical: true)
                }

                Spacer(minLength: ERDTheme.Spacing.section)

                ERDStatusPill(title: "HUD", systemImage: "chart.xyaxis.line", tint: ERDTheme.blue)
            }

            LazyVGrid(columns: [GridItem(.flexible(), spacing: ERDTheme.Spacing.small), GridItem(.flexible(), spacing: ERDTheme.Spacing.small)], spacing: ERDTheme.Spacing.small) {
                StatCard(
                    icon: "play.rectangle.fill",
                    label: "FPS",
                    value: String(format: "%.1f", stats.fps),
                    color: fpsColor(stats.fps)
                )
                StatCard(
                    icon: "arrow.up.arrow.down.square.fill",
                    label: "Bitrate",
                    value: formatBitrate(stats.bitrate),
                    color: .white.opacity(0.92)
                )
                StatCard(
                    icon: "exclamationmark.triangle.fill",
                    label: "Loss",
                    value: String(format: "%.1f%%", stats.frameLoss * 100),
                    color: lossColor(stats.frameLoss)
                )
                StatCard(
                    icon: "number.square.fill",
                    label: "Frames",
                    value: formatNumber(stats.framesReceived),
                    color: .white.opacity(0.92)
                )
            }

            // Beautiful, elegant Sparkline Chart
            VStack(alignment: .leading, spacing: ERDTheme.Spacing.compact) {
                Text("FPS TREND (30S)")
                    .font(ERDTheme.Typography.eyebrow)
                    .tracking(0.8)
                    .foregroundStyle(ERDTheme.mutedText)

                ZStack {
                    VStack {
                        Divider().background(ERDTheme.softBorder.opacity(0.4))
                        Spacer()
                        Divider().background(ERDTheme.softBorder.opacity(0.4))
                    }

                    ERDSparklineView(values: fpsHistory, color: ERDTheme.blue)
                        .padding(.vertical, 4)
                }
                .frame(height: 50)
                .padding(ERDTheme.Spacing.field)
                .erdCardBackground(.subdued)
            }

            Text("Shortcut: ⇧⌘S")
                .font(ERDTheme.Typography.caption)
                .foregroundStyle(ERDTheme.mutedText)
        }
        .padding(ERDTheme.Spacing.card)
        .frame(maxWidth: 300, alignment: .leading)
        .erdCardBackground(
            .surface(
                fill: ERDTheme.surface.opacity(0.96),
                stroke: ERDTheme.panelBorder,
                cornerRadius: ERDTheme.Radius.panel
            )
        )
        .onAppear {
            refreshStats()
            timer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { _ in
                refreshStats()
            }
        }
        .onDisappear {
            timer?.invalidate()
            timer = nil
        }
    }

    private func refreshStats() {
        let newStats = ClientCore.shared.getStats()
        stats = newStats
        fpsHistory.append(newStats.fps)
        if fpsHistory.count > 60 {
            fpsHistory.removeFirst()
        }
    }

    private func fpsColor(_ fps: Double) -> Color {
        if fps >= 50 { return ERDTheme.green }
        if fps >= 30 { return ERDTheme.amber }
        return ERDTheme.red
    }

    private func lossColor(_ loss: Double) -> Color {
        if loss < 0.02 { return ERDTheme.green }
        if loss < 0.10 { return ERDTheme.amber }
        return ERDTheme.red
    }

    private func formatBitrate(_ bitrate: Int) -> String {
        let mbps = Double(bitrate) / 1_000_000.0
        return String(format: "%.1f Mbps", mbps)
    }

    private func formatNumber(_ num: Int) -> String {
        Self.numberFormatter.string(from: NSNumber(value: num)) ?? "\(num)"
    }
}

struct ERDSparklineView: View {
    let values: [Double]
    let color: Color

    var body: some View {
        GeometryReader { geo in
            let w = geo.size.width
            let h = geo.size.height

            if values.count > 1 {
                let maxVal = max(60.0, values.max() ?? 60.0)
                let minVal = 0.0
                let range = maxVal - minVal
                let stepX = w / CGFloat(values.count - 1)

                ZStack {
                    // Line Path
                    Path { path in
                        for i in 0..<values.count {
                            let x = CGFloat(i) * stepX
                            let normY = CGFloat((values[i] - minVal) / range)
                            let y = h - (normY * h)

                            if i == 0 {
                                path.move(to: CGPoint(x: x, y: y))
                            } else {
                                path.addLine(to: CGPoint(x: x, y: y))
                            }
                        }
                    }
                    .stroke(color, style: StrokeStyle(lineWidth: 2, lineCap: .round, lineJoin: .round))

                    // Fill Path (gradient under the line)
                    Path { path in
                        path.move(to: CGPoint(x: 0, y: h))
                        for i in 0..<values.count {
                            let x = CGFloat(i) * stepX
                            let normY = CGFloat((values[i] - minVal) / range)
                            let y = h - (normY * h)
                            path.addLine(to: CGPoint(x: x, y: y))
                        }
                        path.addLine(to: CGPoint(x: w, y: h))
                        path.closeSubpath()
                    }
                    .fill(
                        LinearGradient(
                            gradient: Gradient(colors: [color.opacity(0.24), color.opacity(0.0)]),
                            startPoint: .top,
                            endPoint: .bottom
                        )
                    )
                }
            } else {
                Text("Collecting data...")
                    .font(ERDTheme.Typography.captionMedium)
                    .foregroundStyle(ERDTheme.mutedText)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
            }
        }
    }
}

struct StatRow: View {
    let icon: String
    let label: String
    let value: String
    let color: Color

    var body: some View {
        HStack(alignment: .center, spacing: ERDTheme.Spacing.row) {
            ERDIconBadge(
                systemImage: icon,
                tint: color,
                size: ERDTheme.Layout.featureIconSize,
                font: ERDTheme.Typography.captionMedium,
                chrome: .elevated(stroke: color.opacity(0.18), cornerRadius: ERDTheme.Radius.iconCompact)
            )

            VStack(alignment: .leading, spacing: ERDTheme.Spacing.micro) {
                Text(label.uppercased())
                    .font(ERDTheme.Typography.eyebrow)
                    .tracking(0.8)
                    .foregroundStyle(ERDTheme.mutedText)

                Text(value)
                    .font(ERDTheme.Typography.bodyEmphasized)
                    .monospacedDigit()
                    .foregroundStyle(color)
            }

            Spacer(minLength: ERDTheme.Spacing.section)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(ERDTheme.Spacing.field)
        .erdCardBackground(
            .elevated(stroke: color.opacity(0.18), cornerRadius: ERDTheme.Radius.tile)
        )
    }
}

struct StatCard: View {
    let icon: String
    let label: String
    let value: String
    let color: Color

    var body: some View {
        VStack(alignment: .leading, spacing: ERDTheme.Spacing.compact) {
            HStack(alignment: .center, spacing: ERDTheme.Spacing.compact) {
                Image(systemName: icon)
                    .font(ERDTheme.Typography.detailMedium)
                    .foregroundStyle(color)
                
                Text(label.uppercased())
                    .font(ERDTheme.Typography.eyebrow)
                    .tracking(0.6)
                    .foregroundStyle(ERDTheme.mutedText)
            }

            Text(value)
                .font(ERDTheme.Typography.bodyEmphasized)
                .monospacedDigit()
                .foregroundStyle(color)
        }
        .padding(.horizontal, ERDTheme.Spacing.row)
        .padding(.vertical, ERDTheme.Spacing.small)
        .frame(maxWidth: .infinity, alignment: .leading)
        .erdCardBackground(
            .surface(
                fill: ERDTheme.subduedSurface,
                stroke: ERDTheme.softBorder,
                cornerRadius: ERDTheme.Radius.tile
            )
        )
    }
}

class KeyboardShortcutMonitor {
    static let shared = KeyboardShortcutMonitor()
    private var eventMonitor: Any?
    private var action: (() -> Void)?

    func register(action: @escaping () -> Void) {
        self.action = action
        eventMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { event in
            if event.modifierFlags.contains(.command) &&
               event.modifierFlags.contains(.shift) &&
               event.keyCode == 1 {
                action()
                return nil
            }
            return event
        }
    }

    func unregister() {
        if let monitor = eventMonitor {
            NSEvent.removeMonitor(monitor)
            eventMonitor = nil
        }
        action = nil
    }
}

private struct WindowChromeInsetReader: NSViewRepresentable {
    @Binding var topInset: CGFloat

    func makeNSView(context: Context) -> WindowChromeInsetView {
        let view = WindowChromeInsetView()
        view.onUpdate = updateTopInset(window:)
        return view
    }

    func updateNSView(_ nsView: WindowChromeInsetView, context: Context) {
        nsView.onUpdate = updateTopInset(window:)
        nsView.reportWindowMetrics()
    }

    private func updateTopInset(window: NSWindow) {
        let measuredInset = max(0, window.frame.height - window.contentLayoutRect.height)

        guard abs(measuredInset - topInset) > 0.5 else { return }

        DispatchQueue.main.async {
            topInset = measuredInset
        }
    }
}

private struct SessionWindowReader: NSViewRepresentable {
    @Binding var window: NSWindow?
    let onFullscreenChanged: (Bool) -> Void

    func makeNSView(context: Context) -> SessionWindowReaderView {
        let view = SessionWindowReaderView()
        view.onWindowUpdate = updateWindow
        view.onFullscreenChanged = onFullscreenChanged
        return view
    }

    func updateNSView(_ nsView: SessionWindowReaderView, context: Context) {
        nsView.onWindowUpdate = updateWindow
        nsView.onFullscreenChanged = onFullscreenChanged
        nsView.reportWindow()
    }

    private func updateWindow(_ window: NSWindow) {
        guard self.window !== window else { return }

        DispatchQueue.main.async {
            self.window = window
        }
    }
}

private final class WindowChromeInsetView: NSView {
    var onUpdate: ((NSWindow) -> Void)?

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        reportWindowMetrics()
    }

    override func viewDidMoveToSuperview() {
        super.viewDidMoveToSuperview()
        reportWindowMetrics()
    }

    override func layout() {
        super.layout()
        reportWindowMetrics()
    }

    func reportWindowMetrics() {
        if let window {
            onUpdate?(window)
        }
    }
}

private final class SessionWindowReaderView: NSView {
    var onWindowUpdate: ((NSWindow) -> Void)?
    var onFullscreenChanged: ((Bool) -> Void)?

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        NotificationCenter.default.removeObserver(self)

        if let window {
            NotificationCenter.default.addObserver(
                self,
                selector: #selector(handleDidEnterFullscreen),
                name: NSWindow.didEnterFullScreenNotification,
                object: window
            )
            NotificationCenter.default.addObserver(
                self,
                selector: #selector(handleDidExitFullscreen),
                name: NSWindow.didExitFullScreenNotification,
                object: window
            )
        }

        reportWindow()
    }

    override func viewDidMoveToSuperview() {
        super.viewDidMoveToSuperview()
        reportWindow()
    }

    override func layout() {
        super.layout()
        reportWindow()
    }

    deinit {
        NotificationCenter.default.removeObserver(self)
    }

    @objc private func handleDidEnterFullscreen() {
        onFullscreenChanged?(true)
    }

    @objc private func handleDidExitFullscreen() {
        onFullscreenChanged?(false)
    }

    func reportWindow() {
        if let window {
            onWindowUpdate?(window)
            onFullscreenChanged?(window.styleMask.contains(.fullScreen))
        }
    }
}

class TrackingMTKView: MTKView {
    override var acceptsFirstResponder: Bool { true }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        for area in trackingAreas {
            removeTrackingArea(area)
        }
        let area = NSTrackingArea(
            rect: bounds,
            options: [.mouseMoved, .activeInKeyWindow, .inVisibleRect],
            owner: self,
            userInfo: nil
        )
        addTrackingArea(area)
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        window?.makeFirstResponder(self)
    }
}

struct MetalViewRepresentable: NSViewRepresentable {
    func makeNSView(context: Context) -> TrackingMTKView {
        let mtkView = TrackingMTKView()
        mtkView.device = MTLCreateSystemDefaultDevice()
        mtkView.preferredFramesPerSecond = ERDConstants.defaultFPS
        mtkView.enableSetNeedsDisplay = false
        mtkView.isPaused = false
        mtkView.colorPixelFormat = .bgra8Unorm

        ClientCore.shared.setupRenderer(mtkView: mtkView)

        DispatchQueue.main.async {
            ClientCore.shared.startInput(in: mtkView)
        }

        return mtkView
    }

    func updateNSView(_ nsView: TrackingMTKView, context: Context) {}

    static func dismantleNSView(_ nsView: TrackingMTKView, coordinator: ()) {
        nsView.delegate = nil
    }
}
