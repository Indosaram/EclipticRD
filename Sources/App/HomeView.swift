import SwiftUI

struct HomeView: View {
    @State private var isServerRunning = false
    @State private var serverPIN = String(format: "%06d", Int.random(in: 0..<1_000_000))
    @State private var showClientConnect = false

    var body: some View {
        VStack(spacing: 24) {
            Text("EclipticRD")
                .font(.largeTitle.bold())
                .padding(.top, 20)

            Divider()

            // Server section
            GroupBox {
                VStack(spacing: 12) {
                    HStack {
                        Image(systemName: "display")
                        Text("EclipticRD Server v1.0")
                            .font(.headline)
                    }

                    if isServerRunning {
                        HStack {
                            Text("PIN:")
                                .foregroundColor(.secondary)
                            Text(serverPIN)
                                .font(.system(.title2, design: .monospaced).bold())
                                .foregroundColor(.blue)
                        }

                        Text("Streaming started:")
                            .foregroundColor(.green)
                            .font(.caption)

                        Button("Stop Server") {
                            Task {
                                await ServerCore.shared.stop()
                                isServerRunning = false
                            }
                        }
                        .buttonStyle(.borderedProminent)
                        .tint(.red)
                        .accessibilityIdentifier("stopServer")
                    } else {
                        Button("Start Server") {
                            Task {
                                await ServerCore.shared.start(pin: serverPIN)
                                isServerRunning = true
                            }
                        }
                        .buttonStyle(.borderedProminent)
                        .accessibilityIdentifier("startServer")
                    }
                }
                .padding()
            }

            // Client section
            GroupBox {
                VStack(spacing: 12) {
                    HStack {
                        Image(systemName: "laptopcomputer")
                        Text("Connect to a Mac")
                            .font(.headline)
                    }

                    Button("Open Client") {
                        showClientConnect = true
                    }
                    .buttonStyle(.borderedProminent)
                    .tint(.green)
                    .accessibilityIdentifier("openClient")
                }
                .padding()
            }

            Spacer()
        }
        .padding()
        .frame(minWidth: 400, minHeight: 450)
        .sheet(isPresented: $showClientConnect) {
            ClientConnectView()
        }
    }
}
