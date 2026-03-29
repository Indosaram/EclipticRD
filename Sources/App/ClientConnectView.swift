import SwiftUI
import Network

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
    @State private var manualPIN = ""
    @State private var isConnected = false
    @Environment(\.dismiss) var dismiss

    var body: some View {
        VStack(spacing: 20) {
            Text("EclipticRD Client")
                .font(.title.bold())

            Divider()

            // Bonjour section
            GroupBox {
                VStack(alignment: .leading, spacing: 8) {
                    Label("Local Network (Bonjour)", systemImage: "wifi")
                        .font(.headline)

                    if browserManager.hosts.isEmpty {
                        Text("[Client] Searching for servers via Bonjour...")
                            .font(.caption)
                            .foregroundColor(.secondary)
                    }

                    ForEach(browserManager.hosts) { host in
                        HStack {
                            Image(systemName: "desktopcomputer")
                            Text(host.name)
                            Spacer()
                            Button("Connect") {
                                ClientCore.shared.start(endpoint: host.endpoint, name: host.name)
                                isConnected = true
                                dismiss()
                            }
                            .buttonStyle(.borderedProminent)
                            .tint(.blue)
                        }
                        .padding(.vertical, 2)
                    }
                }
                .padding()
            }

            // PIN section
            GroupBox {
                VStack(spacing: 12) {
                    Label("Internet Connect (PIN)", systemImage: "globe")
                        .font(.headline)

                    HStack {
                        TextField("Enter 6-digit PIN", text: $manualPIN)
                            .textFieldStyle(.roundedBorder)
                            .frame(width: 200)

                        Button("Connect") {
                            guard manualPIN.count == 6 else { return }
                            ClientCore.shared.startICE(pin: manualPIN)
                            isConnected = true
                            dismiss()
                        }
                        .buttonStyle(.borderedProminent)
                        .tint(.orange)
                        .disabled(manualPIN.count != 6)
                    }
                }
                .padding()
            }

            Spacer()

            Button("Cancel") { dismiss() }
                .buttonStyle(.bordered)
        }
        .padding()
        .frame(width: 450, height: 400)
        .onAppear { browserManager.startBrowsing() }
        .onDisappear { browserManager.stopBrowsing() }
    }
}
