import SwiftUI
import MetalKit
import Network
import ApplicationServices
import CoreGraphics

@main
struct EclipticApp: App {
    @StateObject private var connectionManager = ConnectionManager()
    @State private var permissionsGranted = Self.shouldShowHome()

    var body: some Scene {
        WindowGroup {
            Group {
                if permissionsGranted {
                    HomeView()
                        .environmentObject(connectionManager)
                } else {
                    OnboardingView {
                        permissionsGranted = Self.shouldShowHome()
                    }
                }
            }
            .preferredColorScheme(.dark)
            .frame(minWidth: 880, minHeight: 520)
        }
    }

    private static func shouldShowHome() -> Bool {
        return AXIsProcessTrusted() && CGPreflightScreenCaptureAccess()
    }
}
