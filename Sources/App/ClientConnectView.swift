import SwiftUI

class BonjourBrowserManager: ObservableObject {
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

struct ClientConnectView: View {
    @StateObject private var browserManager = BonjourBrowserManager()
    @EnvironmentObject var connectionManager: ConnectionManager
    @State private var manualPIN = ""
    @State private var showDiscoveryEmptyState = false
    @State private var discoveryStateTask: Task<Void, Never>?
    @Environment(\.dismiss) var dismiss

    private var isManualPINValid: Bool {
        manualPIN.trimmingCharacters(in: .whitespacesAndNewlines).count == 6
    }

    var body: some View {
        ZStack {
            ERDAppBackground()

            VStack(alignment: .leading, spacing: ERDTheme.Spacing.panel) {
                header
                discoveryPanel
                pinPanel
                footer
            }
            .padding(ERDTheme.Spacing.screen)
        }
        .frame(width: 560, height: 440)
        .preferredColorScheme(.dark)
        .onAppear {
            beginDiscoveryPresentation()
        }
        .onDisappear {
            discoveryStateTask?.cancel()
            discoveryStateTask = nil
            browserManager.stopBrowsing()
        }
    }

    private var header: some View {
        ERDSectionHeader(
            eyebrow: "Client",
            title: "Open Remote Session",
            detail: "Choose a nearby host or enter a PIN to start a trusted EclipticRD connection."
        )
    }

    private var discoveryPanel: some View {
        ERDPanel {
            HStack(alignment: .top, spacing: ERDTheme.Spacing.section) {
                ERDSectionHeader(
                    eyebrow: "Discovery",
                    title: "Local Network",
                    detail: "Nearby hosts advertising over Bonjour appear here and can be connected to instantly."
                )

                Spacer(minLength: ERDTheme.Spacing.section)

                if !browserManager.hosts.isEmpty {
                    ERDStatusPill(
                        title: browserManager.hosts.count == 1 ? "1 Host" : "\(browserManager.hosts.count) Hosts",
                        systemImage: "desktopcomputer",
                        tint: ERDTheme.blue
                    )
                }
            }

            Group {
                if browserManager.hosts.isEmpty {
                    discoveryStateCard
                } else {
                    VStack(alignment: .leading, spacing: ERDTheme.Spacing.small) {
                        Text("Available Hosts")
                            .font(ERDTheme.Typography.bodyEmphasized)
                            .foregroundStyle(ERDTheme.strongText)

                        VStack(spacing: ERDTheme.Spacing.small) {
                            ForEach(browserManager.hosts) { host in
                                HostRow(host: host) {
                                    connectionManager.connectBonjour(endpoint: host.endpoint, name: host.name)
                                    dismiss()
                                }
                            }
                        }
                    }
                }
            }
            .animation(.spring(response: 0.35, dampingFraction: 0.82), value: browserManager.hosts)
        }
    }

    @ViewBuilder
    private var discoveryStateCard: some View {
        if showDiscoveryEmptyState {
            ERDRowCard(alignment: .center, chrome: .elevated(stroke: ERDTheme.softBorder, cornerRadius: ERDTheme.Radius.rowCard)) {
                ERDIconBadge(
                    systemImage: "wifi.slash",
                    tint: ERDTheme.amber,
                    chrome: .elevated(stroke: ERDTheme.amber.opacity(0.22), cornerRadius: ERDTheme.Radius.iconCompact)
                )
            } content: {
                VStack(alignment: .leading, spacing: ERDTheme.Spacing.micro) {
                    Text("No hosts found yet")
                        .font(ERDTheme.Typography.bodyEmphasized)
                        .foregroundStyle(ERDTheme.strongText)

                    Text("Keep the host Mac open, broadcasting, and connected to the same local network. Bonjour discovery stays active while this sheet is open.")
                        .font(ERDTheme.Typography.detail)
                        .foregroundStyle(ERDTheme.mutedText)
                        .fixedSize(horizontal: false, vertical: true)
                }
            } trailing: {
                ERDStatusPill(title: "Scanning", systemImage: "dot.radiowaves.left.and.right", tint: ERDTheme.amber)
            }
        } else {
            ERDRowCard(alignment: .center, chrome: .elevated(stroke: ERDTheme.strongBorder, cornerRadius: ERDTheme.Radius.rowCard)) {
                ERDScanningBadge()
            } content: {
                VStack(alignment: .leading, spacing: ERDTheme.Spacing.micro) {
                    Text("Searching for available hosts")
                        .font(ERDTheme.Typography.bodyEmphasized)
                        .foregroundStyle(ERDTheme.strongText)

                    Text("Make sure the host Mac is broadcasting on the same local network. Nearby Bonjour sessions will appear automatically.")
                        .font(ERDTheme.Typography.detail)
                        .foregroundStyle(ERDTheme.mutedText)
                        .fixedSize(horizontal: false, vertical: true)
                }
            } trailing: {
                ProgressView()
                    .controlSize(.regular)
                    .tint(.white.opacity(0.88))
            }
        }
    }

    private var pinPanel: some View {
        ERDPanel {
            HStack(alignment: .top, spacing: ERDTheme.Spacing.section) {
                ERDSectionHeader(
                    eyebrow: "Remote Access",
                    title: "Connect with PIN",
                    detail: "Enter the temporary six-digit session code shown on the host."
                )

                Spacer(minLength: ERDTheme.Spacing.section)

                ERDStatusPill(
                    title: isManualPINValid ? "Ready" : "6 Digits",
                    systemImage: isManualPINValid ? "checkmark.circle.fill" : "number.square",
                    tint: isManualPINValid ? ERDTheme.green : ERDTheme.amber
                )
            }

            VStack(alignment: .leading, spacing: ERDTheme.Spacing.small) {
                HStack(alignment: .center, spacing: ERDTheme.Spacing.row) {
                    TextField("Enter 6-digit PIN", text: $manualPIN)
                        .textFieldStyle(.plain)
                        .erdInputField()
                        .frame(maxWidth: .infinity)

                    Button("Connect") {
                        connectionManager.connectPIN(pin: manualPIN.trimmingCharacters(in: .whitespacesAndNewlines))
                        dismiss()
                    }
                    .buttonStyle(ERDActionButtonStyle(tint: ERDTheme.amber))
                    .disabled(!isManualPINValid)
                }

                HStack {
                    Text(isManualPINValid ? "Ready to connect" : "Awaiting 6-digit code")
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
        }
    }

    private var footer: some View {
        HStack {
            Spacer()
            Button("Cancel") {
                dismiss()
            }
            .buttonStyle(ERDActionButtonStyle(tint: ERDTheme.blue, isProminent: false))
        }
    }

    private func beginDiscoveryPresentation() {
        showDiscoveryEmptyState = false
        discoveryStateTask?.cancel()
        browserManager.startBrowsing()

        discoveryStateTask = Task {
            try? await Task.sleep(nanoseconds: 1_800_000_000)
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

private struct HostRow: View {
    let host: BonjourBrowser.DiscoveredHost
    let connect: () -> Void

    var body: some View {
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
                connect()
            }
            .buttonStyle(ERDActionButtonStyle(tint: ERDTheme.blue))
        }
    }
}

private struct ERDScanningBadge: View {
    @State private var animate = false

    var body: some View {
        ZStack {
            // Ripple Circle 1
            Circle()
                .stroke(ERDTheme.blue.opacity(0.36), lineWidth: 1.5)
                .scaleEffect(animate ? 1.85 : 1.0)
                .opacity(animate ? 0.0 : 1.0)

            // Ripple Circle 2
            Circle()
                .stroke(ERDTheme.blue.opacity(0.18), lineWidth: 1.5)
                .scaleEffect(animate ? 2.35 : 1.0)
                .opacity(animate ? 0.0 : 1.0)

            ERDIconBadge(systemImage: "magnifyingglass", tint: ERDTheme.blue)
        }
        .onAppear {
            withAnimation(.easeInOut(duration: 1.8).repeatForever(autoreverses: false)) {
                animate = true
            }
        }
    }
}
