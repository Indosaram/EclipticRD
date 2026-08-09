import SwiftUI
import Network

@MainActor
final class ConnectionManager: ObservableObject {
    enum State {
        case disconnected
        case connecting
        case connected
        case error(String)
    }

    enum StreamQualityPreset: String, CaseIterable, Identifiable {
        case efficient
        case balanced
        case fluid

        var id: String { rawValue }

        var title: String {
            switch self {
            case .efficient: return "Efficient"
            case .balanced: return "Balanced"
            case .fluid: return "Fluid"
            }
        }

        var detail: String {
            switch self {
            case .efficient: return "4 Mbps · 30 fps"
            case .balanced: return "8 Mbps · 60 fps"
            case .fluid: return "12 Mbps · 90 fps"
            }
        }

        var bitrate: Int {
            switch self {
            case .efficient: return 4_000_000
            case .balanced: return ERDConstants.defaultBitrate
            case .fluid: return 12_000_000
            }
        }

        var framesPerSecond: Int {
            switch self {
            case .efficient: return 30
            case .balanced: return ERDConstants.defaultFPS
            case .fluid: return 90
            }
        }
    }

    enum StreamResolutionPreset: String, CaseIterable, Identifiable {
        case native
        case retina1440
        case desktop1080
        case compact720

        var id: String { rawValue }

        var title: String {
            switch self {
            case .native: return "Native"
            case .retina1440: return "1440p"
            case .desktop1080: return "1080p"
            case .compact720: return "720p"
            }
        }

        var detail: String {
            switch self {
            case .native: return "Match host display"
            case .retina1440: return "Sharp desktop fit"
            case .desktop1080: return "Balanced clarity"
            case .compact720: return "Bandwidth saver"
            }
        }

        func dimensions(for nativeWidth: Int, nativeHeight: Int) -> (width: Int, height: Int) {
            switch self {
            case .native:
                return Self.evenDimensions(width: nativeWidth, height: nativeHeight)
            case .retina1440:
                return Self.fittedDimensions(nativeWidth: nativeWidth, nativeHeight: nativeHeight, maxWidth: 2560, maxHeight: 1440)
            case .desktop1080:
                return Self.fittedDimensions(nativeWidth: nativeWidth, nativeHeight: nativeHeight, maxWidth: 1920, maxHeight: 1080)
            case .compact720:
                return Self.fittedDimensions(nativeWidth: nativeWidth, nativeHeight: nativeHeight, maxWidth: 1280, maxHeight: 720)
            }
        }

        private static func fittedDimensions(nativeWidth: Int, nativeHeight: Int, maxWidth: Int, maxHeight: Int) -> (width: Int, height: Int) {
            guard nativeWidth > 0, nativeHeight > 0 else {
                return evenDimensions(width: maxWidth, height: maxHeight)
            }

            let widthScale = CGFloat(maxWidth) / CGFloat(nativeWidth)
            let heightScale = CGFloat(maxHeight) / CGFloat(nativeHeight)
            let scale = min(widthScale, heightScale, 1)
            let fittedWidth = Int((CGFloat(nativeWidth) * scale).rounded(.down))
            let fittedHeight = Int((CGFloat(nativeHeight) * scale).rounded(.down))
            return evenDimensions(width: fittedWidth, height: fittedHeight)
        }

        private static func evenDimensions(width: Int, height: Int) -> (width: Int, height: Int) {
            let normalizedWidth = max(2, width - (width % 2))
            let normalizedHeight = max(2, height - (height % 2))
            return (normalizedWidth, normalizedHeight)
        }
    }

    enum SessionControlStatus {
        case ready
        case pending
        case applied
        case rejected
    }

    struct SessionControlState {
        var selectedQuality: StreamQualityPreset = .balanced
        var activeQuality: StreamQualityPreset = .balanced
        var selectedResolution: StreamResolutionPreset = .native
        var activeResolution: StreamResolutionPreset = .native
        var activeConfiguration: StreamConfiguration?
        var pendingConfiguration: StreamConfiguration?
        var isRequestPending = false
        var status: SessionControlStatus = .ready
        var statusTitle = "Session controls ready"
        var statusMessage = "The live stream is using the current host profile."
        var isImmersiveModeEnabled = false
        var isFullscreen = false

        var activeSummary: String {
            let resolutionLabel = activeResolution.title
            let qualityLabel = activeQuality.title
            return "\(resolutionLabel) · \(qualityLabel)"
        }
    }

    @Published var state: State = .disconnected
    @Published var serverName: String?
    @Published var sessionControls = SessionControlState()
    @Published var showClipboardToast = false
    @Published var clipboardToastText = ""
    @Published var isAudioMuted = false {
        didSet {
            ClientCore.shared.isAudioMuted.value = isAudioMuted
        }
    }

    private var clipboardToastWorkItem: DispatchWorkItem?
    private var lastBonjourEndpoint: NWEndpoint?
    private var lastBonjourName: String?
    private var lastPIN: String?
    private var reconnectAttempts = 0
    private var isUserDisconnect = false
    private var reconnectWorkItem: DispatchWorkItem?
    private var streamConfigTimeoutWorkItem: DispatchWorkItem?
    private let maxReconnectAttempts = 3
    private let streamConfigRequestTimeout: TimeInterval = 10

    init() {
        ClientCore.shared.onSessionReady = { [weak self] in
            Task { @MainActor [weak self] in
                guard let self else { return }
                self.reconnectAttempts = 0
                self.state = .connected
                self.resetSessionControlsForConnectedSession()
            }
        }
        ClientCore.shared.onServerIdentity = { [weak self] serverName in
            Task { @MainActor [weak self] in
                self?.serverName = serverName
            }
        }
        ClientCore.shared.onDisconnected = { [weak self] in
            Task { @MainActor [weak self] in
                guard let self else { return }
                self.sessionControls.isImmersiveModeEnabled = false
                self.sessionControls.isFullscreen = false
                self.cancelStreamConfigRequestTimeout()

                if self.isUserDisconnect {
                    self.isUserDisconnect = false
                    self.state = .disconnected
                    self.serverName = nil
                    self.sessionControls = SessionControlState()
                    return
                }
                self.attemptReconnect()
            }
        }
        ClientCore.shared.onError = { [weak self] message in
            Task { @MainActor [weak self] in
                self?.state = .error(message)
            }
        }
        ClientCore.shared.onStreamConfigResponse = { [weak self] response in
            Task { @MainActor [weak self] in
                self?.handleStreamConfigResponse(response)
            }
        }
        ClientCore.shared.onStreamConfigRejected = { [weak self] rejection in
            Task { @MainActor [weak self] in
                self?.handleStreamConfigRejection(rejection)
            }
        }
        ClientCore.shared.onStreamConfigError = { [weak self] error in
            Task { @MainActor [weak self] in
                self?.handleStreamConfigError(error)
            }
        }
        ClientCore.shared.onClipboardSynced = { [weak self] text in
            Task { @MainActor [weak self] in
                self?.triggerClipboardToast(text: text)
            }
        }
    }

    func connectBonjour(endpoint: NWEndpoint, name: String) {
        cancelStreamConfigRequestTimeout()
        reconnectAttempts = 0
        isUserDisconnect = false
        state = .connecting
        serverName = name
        sessionControls = SessionControlState()
        lastBonjourEndpoint = endpoint
        lastBonjourName = name
        lastPIN = nil
        ClientCore.shared.start(endpoint: endpoint, name: name)
    }

    func connectPIN(pin: String) {
        cancelStreamConfigRequestTimeout()
        reconnectAttempts = 0
        isUserDisconnect = false
        state = .connecting
        sessionControls = SessionControlState()
        lastPIN = pin
        lastBonjourEndpoint = nil
        lastBonjourName = nil
        ClientCore.shared.startICE(pin: pin)
    }

    func disconnect() {
        reconnectWorkItem?.cancel()
        reconnectWorkItem = nil
        cancelStreamConfigRequestTimeout()
        isUserDisconnect = true
        reconnectAttempts = 0
        sessionControls.isImmersiveModeEnabled = false
        sessionControls.isFullscreen = false
        ClientCore.shared.stop()
        state = .disconnected
        serverName = nil
        sessionControls = SessionControlState()
    }

    func retry() {
        reconnectAttempts = 0
        performReconnect()
    }

    func requestStreamConfiguration(quality: StreamQualityPreset? = nil, resolution: StreamResolutionPreset? = nil) {
        if let quality {
            sessionControls.selectedQuality = quality
        }

        if let resolution {
            sessionControls.selectedResolution = resolution
        }

        guard !sessionControls.isRequestPending else { return }
        guard let config = buildRequestedConfiguration() else {
            sessionControls.status = .ready
            sessionControls.statusTitle = "Waiting for host display"
            sessionControls.statusMessage = "Stream controls become active after the session reports the host resolution."
            return
        }

        sessionControls.pendingConfiguration = config
        sessionControls.isRequestPending = true
        sessionControls.status = .pending
        sessionControls.statusTitle = "Requesting stream update"
        sessionControls.statusMessage = "Asking the host for \(sessionControls.selectedResolution.title) at \(sessionControls.selectedQuality.detail)."

        ClientCore.shared.requestStreamConfiguration(config)
        scheduleStreamConfigRequestTimeout()
    }

    func setImmersiveModeEnabled(_ enabled: Bool) {
        sessionControls.isImmersiveModeEnabled = enabled
        if !enabled {
            sessionControls.isFullscreen = false
        }
    }

    func updateFullscreenState(_ isFullscreen: Bool) {
        sessionControls.isFullscreen = isFullscreen
        if !isFullscreen {
            sessionControls.isImmersiveModeEnabled = false
        }
    }

    private func attemptReconnect() {
        guard reconnectAttempts < maxReconnectAttempts else {
            state = .error("Connection lost after \(maxReconnectAttempts) retries")
            return
        }

        guard lastBonjourEndpoint != nil || lastPIN != nil else {
            cancelStreamConfigRequestTimeout()
            state = .disconnected
            serverName = nil
            sessionControls = SessionControlState()
            return
        }

        let delay = pow(2.0, Double(reconnectAttempts))
        reconnectAttempts += 1
        state = .connecting

        let work = DispatchWorkItem { [weak self] in
            self?.performReconnect()
        }
        reconnectWorkItem = work
        DispatchQueue.main.asyncAfter(deadline: .now() + delay, execute: work)
    }

    private func performReconnect() {
        if let endpoint = lastBonjourEndpoint, let name = lastBonjourName {
            state = .connecting
            serverName = name
            cancelStreamConfigRequestTimeout()
            ClientCore.shared.start(endpoint: endpoint, name: name)
        } else if let pin = lastPIN {
            state = .connecting
            cancelStreamConfigRequestTimeout()
            ClientCore.shared.startICE(pin: pin)
        } else {
            state = .disconnected
        }
    }

    private func resetSessionControlsForConnectedSession() {
        let nativeWidth = ClientCore.shared.serverScreenWidth
        let nativeHeight = ClientCore.shared.serverScreenHeight
        let defaultConfiguration = StreamConfiguration(
            width: UInt32(max(2, nativeWidth)),
            height: UInt32(max(2, nativeHeight)),
            bitrate: UInt32(ERDConstants.defaultBitrate),
            framesPerSecond: UInt16(ERDConstants.defaultFPS)
        )

        sessionControls = SessionControlState(
            selectedQuality: .balanced,
            activeQuality: .balanced,
            selectedResolution: .native,
            activeResolution: .native,
            activeConfiguration: defaultConfiguration,
            pendingConfiguration: nil,
            isRequestPending: false,
            status: .ready,
            statusTitle: "Native stream live",
            statusMessage: "Use the floating controls to tune quality, resolution, or enter immersive mode.",
            isImmersiveModeEnabled: false,
            isFullscreen: false
        )
        cancelStreamConfigRequestTimeout()
    }

    private func buildRequestedConfiguration() -> StreamConfiguration? {
        let nativeWidth = ClientCore.shared.serverScreenWidth
        let nativeHeight = ClientCore.shared.serverScreenHeight

        guard nativeWidth > 0, nativeHeight > 0 else {
            return nil
        }

        let dimensions = sessionControls.selectedResolution.dimensions(for: nativeWidth, nativeHeight: nativeHeight)
        return StreamConfiguration(
            width: UInt32(dimensions.width),
            height: UInt32(dimensions.height),
            bitrate: UInt32(sessionControls.selectedQuality.bitrate),
            framesPerSecond: UInt16(sessionControls.selectedQuality.framesPerSecond)
        )
    }

    private func handleStreamConfigResponse(_ response: StreamConfigurationResponsePayload) {
        let activeConfiguration = response.activeConfiguration
        let nativeWidth = ClientCore.shared.serverScreenWidth
        let nativeHeight = ClientCore.shared.serverScreenHeight
        let matchedQuality = nearestQualityPreset(for: activeConfiguration)
        let matchedResolution = nearestResolutionPreset(for: activeConfiguration, nativeWidth: nativeWidth, nativeHeight: nativeHeight)

        sessionControls.activeConfiguration = activeConfiguration
        sessionControls.pendingConfiguration = nil
        sessionControls.isRequestPending = false
        cancelStreamConfigRequestTimeout()
        sessionControls.activeQuality = matchedQuality
        sessionControls.selectedQuality = matchedQuality
        sessionControls.activeResolution = matchedResolution
        sessionControls.selectedResolution = matchedResolution
        sessionControls.status = .applied
        sessionControls.statusTitle = "Stream profile applied"
        sessionControls.statusMessage = "Live at \(Int(activeConfiguration.width))×\(Int(activeConfiguration.height)) · \(Int(activeConfiguration.framesPerSecond)) fps · \(formatBitrate(Int(activeConfiguration.bitrate)))."
    }

    private func handleStreamConfigRejection(_ rejection: StreamConfigurationRejectPayload) {
        cancelStreamConfigRequestTimeout()
        sessionControls.pendingConfiguration = nil
        sessionControls.isRequestPending = false
        sessionControls.selectedQuality = sessionControls.activeQuality
        sessionControls.selectedResolution = sessionControls.activeResolution
        sessionControls.status = .rejected
        sessionControls.statusTitle = "Host kept current stream"
        sessionControls.statusMessage = rejection.message.isEmpty ? "The host rejected the requested stream profile." : rejection.message
    }

    private func handleStreamConfigError(_ error: StreamConfigurationErrorPayload) {
        cancelStreamConfigRequestTimeout()
        sessionControls.pendingConfiguration = nil
        sessionControls.isRequestPending = false
        sessionControls.selectedQuality = sessionControls.activeQuality
        sessionControls.selectedResolution = sessionControls.activeResolution
        sessionControls.status = .rejected
        sessionControls.statusTitle = "Stream update error"
        sessionControls.statusMessage = error.message.isEmpty ? "The host returned an error while applying the requested stream profile." : error.message
    }

    private func nearestQualityPreset(for configuration: StreamConfiguration) -> StreamQualityPreset {
        StreamQualityPreset.allCases.min { lhs, rhs in
            qualityDistance(from: lhs, to: configuration) < qualityDistance(from: rhs, to: configuration)
        } ?? .balanced
    }

    private func nearestResolutionPreset(for configuration: StreamConfiguration, nativeWidth: Int, nativeHeight: Int) -> StreamResolutionPreset {
        StreamResolutionPreset.allCases.min { lhs, rhs in
            resolutionDistance(from: lhs, to: configuration, nativeWidth: nativeWidth, nativeHeight: nativeHeight) < resolutionDistance(from: rhs, to: configuration, nativeWidth: nativeWidth, nativeHeight: nativeHeight)
        } ?? .native
    }

    private func qualityDistance(from preset: StreamQualityPreset, to configuration: StreamConfiguration) -> Int {
        abs(preset.bitrate - Int(configuration.bitrate)) + (abs(preset.framesPerSecond - Int(configuration.framesPerSecond)) * 100_000)
    }

    private func resolutionDistance(from preset: StreamResolutionPreset, to configuration: StreamConfiguration, nativeWidth: Int, nativeHeight: Int) -> Int {
        let dimensions = preset.dimensions(for: nativeWidth, nativeHeight: nativeHeight)
        return abs(dimensions.width - Int(configuration.width)) + abs(dimensions.height - Int(configuration.height))
    }

    private func formatBitrate(_ bitrate: Int) -> String {
        let mbps = Double(bitrate) / 1_000_000.0
        return String(format: "%.1f Mbps", mbps)
    }

    private func scheduleStreamConfigRequestTimeout() {
        streamConfigTimeoutWorkItem?.cancel()

        let workItem = DispatchWorkItem { [weak self] in
            guard let self, self.sessionControls.isRequestPending else { return }
            self.sessionControls.pendingConfiguration = nil
            self.sessionControls.isRequestPending = false
            self.sessionControls.selectedQuality = self.sessionControls.activeQuality
            self.sessionControls.selectedResolution = self.sessionControls.activeResolution
            self.sessionControls.status = .rejected
            self.sessionControls.statusTitle = "Stream update timed out"
            self.sessionControls.statusMessage = "The host did not respond in time. Please try again."
        }

        streamConfigTimeoutWorkItem = workItem
        DispatchQueue.main.asyncAfter(deadline: .now() + streamConfigRequestTimeout, execute: workItem)
    }

    private func cancelStreamConfigRequestTimeout() {
        streamConfigTimeoutWorkItem?.cancel()
        streamConfigTimeoutWorkItem = nil
    }

    private func triggerClipboardToast(text: String) {
        clipboardToastWorkItem?.cancel()
        
        let preview = text.count > 15 ? String(text.prefix(12)) + "..." : text
        clipboardToastText = "Clipboard Synced: \"\(preview)\""
        showClipboardToast = true
        
        let work = DispatchWorkItem { [weak self] in
            withAnimation(.easeInOut(duration: 0.22)) {
                self?.showClipboardToast = false
            }
        }
        clipboardToastWorkItem = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 2.5, execute: work)
    }
}
