import SwiftUI

class HomeBonjourBrowserManager: ObservableObject {
    @Published var hosts: [BonjourBrowser.DiscoveredHost] = []
    private let browser = BonjourBrowser()

    init() {
        browser.onHostsChanged = { [weak self] hosts in
            DispatchQueue.main.async { self?.hosts = hosts }
        }
    }

    func startBrowsing() { browser.startBrowsing() }
    func stopBrowsing() { browser.stopBrowsing() }
}

struct HomeView: View {
    @EnvironmentObject var connectionManager: ConnectionManager
    @State private var isServerRunning = ServerCore.shared.isRunning
    @State private var serverPIN = String(format: "%06d", Int.random(in: 0..<1_000_000))
    @State private var manualPIN = ""
    
    // Sidebar Navigation Tab enum
    enum Tab {
        case computers
        case settings
    }
    @State private var activeTab: Tab = .computers

    @StateObject private var browserManager = HomeBonjourBrowserManager()
    @State private var showDiscoveryEmptyState = false
    @State private var discoveryStateTask: Task<Void, Never>?

    @AppStorage("preferredCodec") private var preferredCodec = "HEVC"
    @AppStorage("preferredFPS") private var preferredFPS = 60
    @AppStorage("hostAudioEnabled") private var hostAudioEnabled = true

    private var isManualPINValid: Bool {
        manualPIN.trimmingCharacters(in: .whitespacesAndNewlines).count == 6
    }

    var body: some View {
        Group {
            switch connectionManager.state {
            case .connected:
                RemoteDesktopView()
            case .connecting:
                connectingContent
            case .error(let message):
                errorContent(message: message)
            case .disconnected:
                navigationDashboardLayout
            }
        }
        .preferredColorScheme(.dark)
        .onAppear {
            isServerRunning = ServerCore.shared.isRunning
            ServerCore.shared.onRunningChanged = { running in
                DispatchQueue.main.async {
                    isServerRunning = running
                }
            }
            beginDiscoveryPresentation()
        }
        .onDisappear {
            ServerCore.shared.onRunningChanged = nil
            discoveryStateTask?.cancel()
            discoveryStateTask = nil
            browserManager.stopBrowsing()
        }
    }

    // Modern Sidebar + Content Area Navigation Layout
    private var navigationDashboardLayout: some View {
        HStack(spacing: 0) {
            sidebarPanel
            
            Divider()
                .background(ERDTheme.panelBorder)

            contentPane
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .frame(minWidth: 880, minHeight: 520)
        .background(ERDAppBackground())
    }

    // Beautiful Parsec-style Sidebar
    private var sidebarPanel: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Sleek Application Logo
            HStack(spacing: ERDTheme.Spacing.micro * 1.5) {
                Text("Ecliptic")
                    .font(.system(size: 18, weight: .bold, design: .rounded))
                    .foregroundStyle(ERDTheme.strongText)
                Text("RD")
                    .font(.system(size: 18, weight: .light, design: .rounded))
                    .foregroundStyle(ERDTheme.blue)
                
                Spacer()
            }
            .padding(.horizontal, ERDTheme.Spacing.section)
            .padding(.top, 24)
            .padding(.bottom, 36)

            // Sidebar Tabs
            VStack(spacing: ERDTheme.Spacing.micro) {
                sidebarButton(
                    title: "Computers",
                    systemImage: "desktopcomputer",
                    isActive: activeTab == .computers
                ) {
                    activeTab = .computers
                }

                sidebarButton(
                    title: "Settings",
                    systemImage: "gearshape.fill",
                    isActive: activeTab == .settings
                ) {
                    activeTab = .settings
                }
            }
            .padding(.horizontal, ERDTheme.Spacing.compact)

            Spacer()

            // Host Connection Live Status Capsule
            HStack(spacing: ERDTheme.Spacing.compact) {
                Circle()
                    .fill(isServerRunning ? ERDTheme.green : ERDTheme.mutedText.opacity(0.40))
                    .frame(width: 6, height: 6)
                    .shadow(color: isServerRunning ? ERDTheme.green.opacity(0.50) : Color.clear, radius: 4)

                Text(isServerRunning ? "Host Live" : "Host Offline")
                    .font(ERDTheme.Typography.captionMedium)
                    .foregroundStyle(isServerRunning ? ERDTheme.strongText : ERDTheme.mutedText)
            }
            .padding(.horizontal, ERDTheme.Spacing.section)
            .padding(.vertical, ERDTheme.Spacing.compact)
            .background(
                Capsule()
                    .fill(ERDTheme.subduedSurface.opacity(0.40))
            )
            .padding(.horizontal, ERDTheme.Spacing.section)
            .padding(.bottom, ERDTheme.Spacing.panelLarge)
        }
        .frame(width: 200)
        .background(ERDTheme.subduedSurface)
    }

    private func sidebarButton(
        title: String,
        systemImage: String,
        isActive: Bool,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            HStack(spacing: ERDTheme.Spacing.row) {
                Image(systemName: systemImage)
                    .font(.system(size: 13, weight: isActive ? .semibold : .regular))
                    .foregroundStyle(isActive ? ERDTheme.blue : ERDTheme.mutedText)
                    .frame(width: 18)

                Text(title)
                    .font(isActive ? ERDTheme.Typography.bodyEmphasized : ERDTheme.Typography.body)
                    .foregroundStyle(isActive ? ERDTheme.strongText : ERDTheme.mutedText.opacity(0.80))

                Spacer()
            }
            .padding(.horizontal, ERDTheme.Spacing.section)
            .padding(.vertical, ERDTheme.Spacing.small * 1.2)
            .background(
                RoundedRectangle(cornerRadius: ERDTheme.Radius.control, style: .continuous)
                    .fill(isActive ? ERDTheme.elevatedSurface : Color.clear)
            )
        }
        .buttonStyle(.plain)
    }

    // Dynamic Navigation Content Switching Pane
    @ViewBuilder
    private var contentPane: some View {
        switch activeTab {
        case .computers:
            computersTabView
        case .settings:
            settingsTabView
        }
    }

    // --- Tab 1: Computers View (Local Host + Discovered Machines + PIN Connect) ---
    private var computersTabView: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: ERDTheme.Spacing.panelLarge) {
                // Header Title
                ERDSectionHeader(
                    eyebrow: "Workspace",
                    title: "Computers",
                    detail: ""
                )

                HStack(alignment: .top, spacing: ERDTheme.Spacing.card * 1.5) {
                    // Left Column: This Mac (Host Control) & Remote Access PIN Connection
                    VStack(spacing: ERDTheme.Spacing.panel) {
                        hostWorkspaceCard
                        remotePINCard
                    }
                    .frame(maxWidth: .infinity)

                    // Right Column: Discovered Computers (Real-Time Bonjour List)
                    discoveredWorkspaceCard
                        .frame(maxWidth: .infinity)
                }
            }
            .padding(ERDTheme.Spacing.screen * 1.2)
        }
    }

    // Simplified Host Controller
    private var hostWorkspaceCard: some View {
        ERDPanel {
            ERDSectionHeader(
                eyebrow: "Host Control",
                title: "This Mac",
                detail: ""
            )

            VStack(spacing: ERDTheme.Spacing.row) {
                ERDMetricTile(
                    title: "Status",
                    value: isServerRunning ? "Broadcasting" : "Offline",
                    systemImage: isServerRunning ? "display.and.arrow.down" : "display",
                    tint: isServerRunning ? ERDTheme.green : ERDTheme.softBorder
                )

                ERDMetricTile(
                    title: "Access PIN",
                    value: formatPIN(serverPIN),
                    systemImage: "number.square",
                    tint: ERDTheme.blue
                )
            }

            Spacer(minLength: ERDTheme.Spacing.section)

            Button(isServerRunning ? "Stop Broadcasting" : "Start Hosting") {
                Task {
                    if isServerRunning {
                        await ServerCore.shared.stop()
                    } else {
                        await ServerCore.shared.start(pin: serverPIN)
                    }
                }
            }
            .buttonStyle(ERDActionButtonStyle(tint: isServerRunning ? ERDTheme.red : ERDTheme.blue))
        }
    }

    // Minimal Remote Access via PIN Card
    private var remotePINCard: some View {
        ERDPanel {
            ERDSectionHeader(
                eyebrow: "Direct Connection",
                title: "Connect with PIN",
                detail: ""
            )

            VStack(alignment: .leading, spacing: ERDTheme.Spacing.small) {
                HStack(alignment: .center, spacing: ERDTheme.Spacing.compact) {
                    TextField("Enter 6-digit PIN", text: $manualPIN)
                        .textFieldStyle(.plain)
                        .erdInputField()
                        .frame(maxWidth: .infinity)

                    Button("Connect") {
                        connectionManager.connectPIN(pin: manualPIN.trimmingCharacters(in: .whitespacesAndNewlines))
                    }
                    .buttonStyle(ERDActionButtonStyle(tint: ERDTheme.amber))
                    .disabled(!isManualPINValid)
                }

                HStack {
                    Text(isManualPINValid ? "Ready to establish stream" : "Awaiting temporary code")
                        .font(ERDTheme.Typography.caption)
                        .foregroundStyle(isManualPINValid ? ERDTheme.green : ERDTheme.mutedText)
                    
                    Spacer()
                    
                    Text("\(manualPIN.count) / 6")
                        .font(ERDTheme.Typography.caption)
                        .foregroundStyle(isManualPINValid ? ERDTheme.green : ERDTheme.mutedText)
                        .monospacedDigit()
                }
                .padding(.horizontal, 4)
            }
            .padding(.top, ERDTheme.Spacing.micro)
        }
    }

    // Bonjour Browser integrated directly into the layout
    private var discoveredWorkspaceCard: some View {
        ERDPanel {
            HStack(alignment: .center) {
                ERDSectionHeader(
                    eyebrow: "Network Discovery",
                    title: "Local Machines",
                    detail: ""
                )

                Spacer()

                if !browserManager.hosts.isEmpty {
                    ERDStatusPill(
                        title: browserManager.hosts.count == 1 ? "1 Host" : "\(browserManager.hosts.count) Hosts",
                        systemImage: "desktopcomputer",
                        tint: ERDTheme.blue
                    )
                }
            }

            VStack(alignment: .leading, spacing: ERDTheme.Spacing.small) {
                if browserManager.hosts.isEmpty {
                    computersDiscoveryEmptyState
                } else {
                    VStack(spacing: ERDTheme.Spacing.small) {
                        ForEach(browserManager.hosts) { host in
                            ERDRowCard(alignment: .center) {
                                ERDIconBadge(systemImage: "desktopcomputer", tint: ERDTheme.blue)
                            } content: {
                                VStack(alignment: .leading, spacing: ERDTheme.Spacing.micro) {
                                    Text(host.name)
                                        .font(ERDTheme.Typography.bodyEmphasized)
                                        .foregroundStyle(ERDTheme.strongText)

                                    Text(host.endpointDescription)
                                        .font(ERDTheme.Typography.detail)
                                        .foregroundStyle(ERDTheme.mutedText)
                                        .lineLimit(1)
                                        .truncationMode(.middle)
                                }
                            } trailing: {
                                Button("Connect") {
                                    connectionManager.connectBonjour(endpoint: host.endpoint, name: host.name)
                                }
                                .buttonStyle(ERDActionButtonStyle(tint: ERDTheme.blue))
                            }
                        }
                    }
                }
            }
            .animation(.spring(response: 0.35, dampingFraction: 0.82), value: browserManager.hosts)
            .padding(.top, ERDTheme.Spacing.micro)
            
            Spacer()
        }
    }

    @ViewBuilder
    private var computersDiscoveryEmptyState: some View {
        if showDiscoveryEmptyState {
            ERDRowCard(alignment: .center, chrome: .elevated(stroke: ERDTheme.softBorder, cornerRadius: ERDTheme.Radius.rowCard)) {
                ERDIconBadge(
                    systemImage: "wifi.slash",
                    tint: ERDTheme.amber,
                    chrome: .elevated(stroke: ERDTheme.amber.opacity(0.22), cornerRadius: ERDTheme.Radius.iconCompact)
                )
            } content: {
                VStack(alignment: .leading, spacing: ERDTheme.Spacing.micro) {
                    Text("No local computers found")
                        .font(ERDTheme.Typography.bodyEmphasized)
                        .foregroundStyle(ERDTheme.strongText)

                    Text("Ensure another EclipticRD host is open and broadcasting on this network.")
                        .font(ERDTheme.Typography.detail)
                        .foregroundStyle(ERDTheme.mutedText)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        } else {
            ERDRowCard(alignment: .center, chrome: .elevated(stroke: ERDTheme.strongBorder, cornerRadius: ERDTheme.Radius.rowCard)) {
                computersScanningBadge
            } content: {
                VStack(alignment: .leading, spacing: ERDTheme.Spacing.micro) {
                    Text("Scanning local network")
                        .font(ERDTheme.Typography.bodyEmphasized)
                        .foregroundStyle(ERDTheme.strongText)

                    Text("Active Bonjour search is finding nearby machines...")
                        .font(ERDTheme.Typography.detail)
                        .foregroundStyle(ERDTheme.mutedText)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
    }

    private var computersScanningBadge: some View {
        ZStack {
            ProgressView()
                .controlSize(.regular)
                .tint(.white.opacity(0.70))
        }
        .frame(width: 32, height: 32)
    }

    // --- Tab 2: Settings View ---
    private var settingsTabView: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: ERDTheme.Spacing.panelLarge) {
                // Header Title
                ERDSectionHeader(
                    eyebrow: "Configuration",
                    title: "Settings",
                    detail: ""
                )

                VStack(spacing: ERDTheme.Spacing.panel) {
                    // Visual/Video Tuning Card
                    ERDPanel {
                        ERDSectionHeader(
                            eyebrow: "Video Preferences",
                            title: "Streaming Settings",
                            detail: ""
                        )

                        VStack(alignment: .leading, spacing: ERDTheme.Spacing.section) {
                            // Preferred Video Codec
                            VStack(alignment: .leading, spacing: ERDTheme.Spacing.micro) {
                                Text("PREFERRED VIDEO CODEC")
                                    .font(ERDTheme.Typography.eyebrow)
                                    .foregroundStyle(ERDTheme.mutedText)

                                Picker("", selection: $preferredCodec) {
                                    Text("H.265 (HEVC)").tag("HEVC")
                                    Text("H.264").tag("H.264")
                                }
                                .pickerStyle(.segmented)
                                .labelsHidden()
                            }

                            // Frame Rate FPS
                            VStack(alignment: .leading, spacing: ERDTheme.Spacing.micro) {
                                Text("STREAMING FRAME RATE")
                                    .font(ERDTheme.Typography.eyebrow)
                                    .foregroundStyle(ERDTheme.mutedText)

                                Picker("", selection: $preferredFPS) {
                                    Text("60 FPS (Fluid)").tag(60)
                                    Text("30 FPS (Efficient)").tag(30)
                                }
                                .pickerStyle(.segmented)
                                .labelsHidden()
                            }
                        }
                        .padding(.vertical, ERDTheme.Spacing.micro)
                    }

                    // Audio Settings Card
                    ERDPanel {
                        ERDSectionHeader(
                            eyebrow: "Audio Preferences",
                            title: "System Sound Settings",
                            detail: ""
                        )

                        VStack(alignment: .leading, spacing: ERDTheme.Spacing.small) {
                            Text("AUDIO STREAMING")
                                .font(ERDTheme.Typography.eyebrow)
                                .foregroundStyle(ERDTheme.mutedText)

                            Toggle(isOn: $hostAudioEnabled) {
                                HStack(spacing: ERDTheme.Spacing.compact) {
                                    Image(systemName: hostAudioEnabled ? "speaker.wave.2.fill" : "speaker.slash.fill")
                                        .font(ERDTheme.Typography.bodyEmphasized)
                                        .foregroundStyle(hostAudioEnabled ? ERDTheme.green : ERDTheme.mutedText)
                                    
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text("Broadcast System Audio")
                                            .font(ERDTheme.Typography.bodyEmphasized)
                                            .foregroundStyle(ERDTheme.strongText)
                                        Text("Stream stereo sound to clients")
                                            .font(ERDTheme.Typography.caption)
                                            .foregroundStyle(ERDTheme.mutedText)
                                    }
                                }
                            }
                            .toggleStyle(.switch)
                        }
                        .padding(.vertical, ERDTheme.Spacing.micro)
                    }
                }
            }
            .padding(ERDTheme.Spacing.screen * 1.2)
        }
    }

    private func formatPIN(_ pin: String) -> String {
        guard pin.count == 6 else { return pin }
        let index3 = pin.index(pin.startIndex, offsetBy: 3)
        return "\(pin[..<index3]) \(pin[index3...])"
    }

    // State surface layout for connecting/error sheets (remains uniform)
    private func stateSurfaceLayout<Content: View>(@ViewBuilder content: () -> Content) -> some View {
        ZStack {
            ERDAppBackground()

            VStack(alignment: .leading, spacing: ERDTheme.Spacing.panelLarge) {
                HStack(alignment: .center) {
                    HStack(spacing: ERDTheme.Spacing.compact) {
                        Text("Ecliptic")
                            .font(.system(size: 20, weight: .bold, design: .rounded))
                            .foregroundStyle(ERDTheme.strongText)
                        Text("RD")
                            .font(.system(size: 20, weight: .light, design: .rounded))
                            .foregroundStyle(ERDTheme.blue)
                    }
                    Spacer()
                }
                .padding(.bottom, ERDTheme.Spacing.micro)

                content()
            }
            .frame(maxWidth: ERDTheme.Layout.dashboardMaxWidth, alignment: .leading)
            .padding(ERDTheme.Spacing.screen)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
        }
        .frame(minWidth: 880, minHeight: 520)
    }

    private var connectingContent: some View {
        stateSurfaceLayout {
            ERDConnectionStateSurface(
                accent: ERDTheme.blue,
                icon: "bolt.horizontal.circle.fill",
                title: "Establishing secure session",
                message: "Connecting to \(connectionManager.serverName ?? "server") and preparing the remote desktop stream.",
                footer: "This usually takes a moment while the stream and control channel come online.",
                actions: {
                    Button("Cancel") {
                        connectionManager.disconnect()
                    }
                    .buttonStyle(ERDActionButtonStyle(tint: ERDTheme.blue, isProminent: false))
                },
                supporting: {
                    stateSupportGrid(
                        primaryTitle: "Session target",
                        primaryValue: connectionManager.serverName ?? "Awaiting host name",
                        primaryTint: ERDTheme.blue,
                        secondaryTitle: "Status",
                        secondaryValue: "Negotiating video, input, and trust handshake",
                        secondaryTint: ERDTheme.mutedText
                    )
                },
                accessory: {
                    ERDStateAccessoryBadge(tint: ERDTheme.blue) {
                        ProgressView()
                            .controlSize(.large)
                            .tint(.white.opacity(0.92))
                            .scaleEffect(1.1)
                    }
                }
            )
        }
    }

    private func errorContent(message: String) -> some View {
        stateSurfaceLayout {
            ERDConnectionStateSurface(
                accent: ERDTheme.red,
                icon: "wifi.exclamationmark",
                title: "Connection interrupted",
                message: message,
                footer: "Check the host availability, then retry or return to the workspace.",
                actions: {
                    HStack(spacing: ERDTheme.Spacing.row) {
                        Button("Back") {
                            connectionManager.disconnect()
                        }
                        .buttonStyle(ERDActionButtonStyle(tint: ERDTheme.red, isProminent: false))

                        Button("Retry") {
                            connectionManager.retry()
                        }
                        .buttonStyle(ERDActionButtonStyle(tint: ERDTheme.red))
                    }
                },
                supporting: {
                    stateSupportGrid(
                        primaryTitle: "Recovery",
                        primaryValue: "Reconnect to the same session without reopening the client sheet.",
                        primaryTint: ERDTheme.strongText,
                        secondaryTitle: "Fallback",
                        secondaryValue: "Return to the workspace to choose another host or restart broadcasting.",
                        secondaryTint: ERDTheme.mutedText
                    )
                },
                accessory: {
                    ERDStateAccessoryBadge(tint: ERDTheme.red) {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .font(.system(size: 34, weight: .semibold))
                            .foregroundStyle(ERDTheme.red)
                    }
                }
            )
        }
    }

    private func stateSupportGrid(
        primaryTitle: String,
        primaryValue: String,
        primaryTint: Color,
        secondaryTitle: String,
        secondaryValue: String,
        secondaryTint: Color
    ) -> some View {
        HStack(alignment: .top, spacing: ERDTheme.Spacing.row) {
            stateSupportCard(title: primaryTitle, value: primaryValue, tint: primaryTint)
            stateSupportCard(title: secondaryTitle, value: secondaryValue, tint: secondaryTint)
        }
    }

    private func stateSupportCard(title: String, value: String, tint: Color) -> some View {
        VStack(alignment: .leading, spacing: ERDTheme.Spacing.compact) {
            Text(title)
                .font(ERDTheme.Typography.captionMedium)
                .foregroundStyle(ERDTheme.mutedText)

            Text(value)
                .font(ERDTheme.Typography.bodyEmphasized)
                .foregroundStyle(tint)
                .fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(ERDTheme.Spacing.field)
        .erdCardBackground(.subdued)
    }

    private func beginDiscoveryPresentation() {
        showDiscoveryEmptyState = false
        discoveryStateTask?.cancel()
        browserManager.startBrowsing()

        discoveryStateTask = Task {
            try? await Task.sleep(nanoseconds: 2_000_000_000)
            guard !Task.isCancelled else { return }

            await MainActor.run {
                discoveryStateTask = nil
                if browserManager.hosts.isEmpty {
                    showDiscoveryEmptyState = true
                }
            }
        }
    }
}
