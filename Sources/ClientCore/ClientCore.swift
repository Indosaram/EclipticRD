import Foundation
import Network
import MetalKit
import AppKit

public class ClientCore {
    public static let shared = ClientCore()

    private let tcpChannel = TCPChannel()
    private let udpChannel = UDPChannel()
    private let frameReceiver = FrameReceiver()
    private let decoder = VideoDecoder()
    private let audioPlayer = ClientAudioPlayer()
    public let isAudioMuted = AtomicBool(false)
    private var renderer: MetalRenderer?
    private var inputSender: InputSender?
    // Protects inputSender from concurrent access between TCP callbacks and UI thread.
    private let stateQueue = DispatchQueue(label: "eclipticrd.client.state")

    private var abrTimer: DispatchSourceTimer?
    private let abrQueue = DispatchQueue(label: "eclipticrd.client.abr")
    private var currentBitrate: Int = ERDConstants.defaultBitrate
    private var lowLossStartTime: Date?

    private var clipboardMonitor: ClipboardMonitor?
    private var peerCapabilities: HandshakeCapabilities = []
    private var nextStreamConfigRequestID: UInt32 = 1
    public var onStreamConfigResponse: (@Sendable (StreamConfigurationResponsePayload) -> Void)?
    public var onStreamConfigRejected: (@Sendable (StreamConfigurationRejectPayload) -> Void)?
    public var onStreamConfigError: (@Sendable (StreamConfigurationErrorPayload) -> Void)?

    public var onSessionReady: (@Sendable () -> Void)?
    public var onServerIdentity: (@Sendable (String) -> Void)?
    public var onDisconnected: (@Sendable () -> Void)?
    public var onError: (@Sendable (String) -> Void)?
    public var onClipboardSynced: (@Sendable (String) -> Void)?

    public var serverHost: String?
    private var udpTargetHost: String?
    private var _serverScreenWidth: Int = 0
    private var _serverScreenHeight: Int = 0
    public var serverScreenWidth: Int {
        get { stateQueue.sync { _serverScreenWidth } }
        set { stateQueue.sync { _serverScreenWidth = newValue } }
    }
    public var serverScreenHeight: Int {
        get { stateQueue.sync { _serverScreenHeight } }
        set { stateQueue.sync { _serverScreenHeight = newValue } }
    }

    private init() {
        // Wire pipeline: UDP → FrameReceiver → VideoDecoder → MetalRenderer
        udpChannel.onReceive = { [weak self] data, endpoint in
            guard let self = self else { return }
            guard data.count >= ERDConstants.packetHeaderSize else { return }
            
            // Bypass frame assembly queue for zero-jitter, real-time audio playback
            if data[2] == PacketType.audioFrame.rawValue {
                if !self.isAudioMuted.value {
                    let payload = data.subdata(in: ERDConstants.packetHeaderSize..<data.count)
                    self.audioPlayer.play(data: payload)
                }
            } else {
                self.frameReceiver.handlePacket(data)
            }
        }

        frameReceiver.onFrameReady = { [weak self] data, header in
            self?.serverScreenWidth = Int(header.width)
            self?.serverScreenHeight = Int(header.height)
            self?.decoder.decode(data)
        }

        frameReceiver.onCursorUpdate = { [weak self] cursor in
            self?.renderer?.updateCursor(x: cursor.x, y: cursor.y)
        }

        decoder.onDecodedFrame = { [weak self] pixelBuffer in
            self?.renderer?.displayFrame(pixelBuffer)
        }
    }

    // MARK: - Connection Methods

    /// Connect via direct IP (LAN manual)
    public func start(host: String?) {
        guard let host = host else { return }
        self.serverHost = host
        ERDLog.info("[Client] Connecting to \(host)")

        tcpChannel.connect(host: host, port: ERDConstants.tcpPort)
        setupTCPHandlers(host: host)
    }

    /// Connect via Bonjour endpoint
    public func start(endpoint: NWEndpoint, name: String) {
        ERDLog.info("[Client] Connecting to Bonjour Endpoint: \(name)")
        tcpChannel.connect(to: endpoint)
        setupTCPHandlers(host: nil)
    }

    /// Connect via PIN (Internet / NAT traversal)
    public func startICE(pin: String) {
        Task {
            do {
                // 1. Get our public IP via STUN
                let stun = STUNClient()
                let (publicIP, publicPort) = try await stun.fetchPublicIP()
                ERDLog.info("[ClientCore] STUN Public IP: \(publicIP):\(publicPort)")

                // 2. Get local IP
                let localIP = NetworkUtils.getLocalIP() ?? "127.0.0.1"
                let localPort = ERDConstants.tcpPort

                // 3. Exchange candidates via ntfy.sh
                let signaling = SignalingClient(pin: pin, role: "client")
                let ourCandidate = SessionCandidate(role: "client", localIP: localIP, localPort: localPort,
                                                     publicIP: publicIP, publicPort: publicPort)

                ERDLog.info("[ClientCore] Waiting for server candidate via ntfy.sh...")
                let serverCandidate = try await signaling.exchangeCandidate(ourCandidate)
                ERDLog.info("[ClientCore] Retrieved Server Candidate: \(serverCandidate.publicIP):\(serverCandidate.publicPort)")
                signaling.stop()

                // 4. Try ICE: connect to both local and public IP
                var endpoints: [NWEndpoint] = []

                // Local IP endpoint
                if let port = NWEndpoint.Port(rawValue: serverCandidate.localPort) {
                    endpoints.append(.hostPort(host: NWEndpoint.Host(serverCandidate.localIP), port: port))
                }
                // Public IP endpoint
                if let port = NWEndpoint.Port(rawValue: serverCandidate.publicPort) {
                    endpoints.append(.hostPort(host: NWEndpoint.Host(serverCandidate.publicIP), port: port))
                }

                tcpChannel.connectICE(endpoints: endpoints)
                // Pass nil so the onConnect handler resolves host from
                // the actual winning TCP connection (local or public).
                setupTCPHandlers(host: nil)

            } catch {
                ERDLog.error("[ClientCore] ICE connection failed: \(error)")
                self.onError?("ICE connection failed: \(error.localizedDescription)")
            }
        }
    }

    public func stop() {
        stopClipboardSync()
        stopABR()
        stateQueue.sync {
            inputSender?.stopCapturing()
            inputSender = nil
        }
        frameReceiver.stop()
        decoder.stop()
        audioPlayer.stop()
        serverHost = nil
        udpTargetHost = nil
        serverScreenWidth = 0
        serverScreenHeight = 0
        tcpChannel.stop()
        udpChannel.stop()
        ERDLog.info("[Client] Disconnected from server")
    }

    // MARK: - Internal

    private func setupTCPHandlers(host: String?) {
        tcpChannel.onConnect = { [weak self] in
            guard let self = self else { return }

            // Resolve server IP for UDP
            let resolvedHost = host ?? self.tcpChannel.remoteHostIP ?? "127.0.0.1"
            self.serverHost = resolvedHost
            self.udpTargetHost = resolvedHost

            if host == nil {
                ERDLog.info("[Client] Resolved server IP: \(resolvedHost)")
            }

            ERDLog.info("[Client] TCP connected, sending handshake...")
            let hs = HandshakePayload(hostname: ProcessInfo.processInfo.hostName,
                                       screenWidth: 0, screenHeight: 0, scaleFactor: 1.0,
                                       capabilities: [.streamConfiguration, .textClipboardSync])
            let header = PacketHeader(type: .handshake, sequence: 0, timestamp: 0)
            var packet = header.serialize()
            packet.append(hs.serialize())
            self.tcpChannel.send(packet)
        }

        tcpChannel.onReceive = { [weak self] data in
            guard let self = self else { return }
            guard data.count >= ERDConstants.packetHeaderSize else {
                return
            }
            guard let header = PacketHeader.deserialize(from: data) else {
                return
            }
            let payload = data.subdata(in: ERDConstants.packetHeaderSize..<data.count)

            if header.type == .handshakeAck {
                if let hs = HandshakePayload.deserialize(from: payload) {
                    ERDLog.info("[Client] Server: \(hs.hostname) \(hs.screenWidth)x\(hs.screenHeight) caps=\(hs.capabilities.rawValue)")
                    self.onServerIdentity?(hs.hostname)
                    self.serverScreenWidth = Int(hs.screenWidth)
                    self.serverScreenHeight = Int(hs.screenHeight)
                    self.stateQueue.sync { self.peerCapabilities = hs.capabilities }

                    let udpHost = self.udpTargetHost ?? self.serverHost ?? "127.0.0.1"
                    self.udpChannel.onReady = { [weak self] in
                        self?.udpChannel.sendPing()
                    }
                    self.udpChannel.connect(host: udpHost, port: ERDConstants.udpPort)
                    ERDLog.info("[Client] Connecting UDP to \(udpHost):\(ERDConstants.udpPort)")
                    self.startABR()
                    self.startClipboardSync()
                    self.audioPlayer.start()
                    self.onSessionReady?()
                }
            } else if header.type == .control {
                if let msg = ControlMessage.deserialize(from: payload) {
                    switch msg.type {
                    case .ping:
                        self.tcpChannel.sendControl(ControlMessage(type: .pong))
                    case .streamConfigResponse:
                        if let data = msg.payload,
                           let resp = StreamConfigurationResponsePayload.deserialize(from: data) {
                            ERDLog.info("[Client] Stream config accepted: \(resp.activeConfiguration.width)x\(resp.activeConfiguration.height)")
                            self.onStreamConfigResponse?(resp)
                        }
                    case .streamConfigReject:
                        if let data = msg.payload,
                           let reject = StreamConfigurationRejectPayload.deserialize(from: data) {
                            ERDLog.warning("[Client] Stream config rejected: \(reject.message)")
                            self.onStreamConfigRejected?(reject)
                        }
                    case .streamConfigError:
                        if let data = msg.payload,
                           let err = StreamConfigurationErrorPayload.deserialize(from: data) {
                            ERDLog.error("[Client] Stream config error: \(err.message)")
                            self.onStreamConfigError?(err)
                        }
                    case .clipboardSyncUpdate:
                        if let data = msg.payload,
                           let update = ClipboardSyncUpdatePayload.deserialize(from: data) {
                            self.handleClipboardUpdate(update)
                        }
                    case .clipboardSyncError:
                        if let data = msg.payload,
                           let err = ClipboardSyncErrorPayload.deserialize(from: data) {
                            ERDLog.warning("[Client] Clipboard sync error: \(err.message)")
                        }
                    default: break
                    }
                }
            }
        }

        tcpChannel.onDisconnect = { [weak self] in
            ERDLog.info("[Client] Disconnected from server")
            self?.onDisconnected?()
            self?.stop()
        }
    }

    public func setupRenderer(mtkView: MTKView) {
        renderer = MetalRenderer(mtkView: mtkView)
    }

    public func startInput(in view: NSView) {
        let sender = InputSender(tcpChannel: tcpChannel)
        sender.startCapturing(in: view)
        stateQueue.sync { inputSender = sender }
    }

    // MARK: - Stream Configuration

    public func requestStreamConfiguration(_ config: StreamConfiguration) {
        let requestID = stateQueue.sync { () -> UInt32 in
            let id = nextStreamConfigRequestID
            nextStreamConfigRequestID += 1
            return id
        }
        let request = StreamConfigurationRequestPayload(requestID: requestID, desiredConfiguration: config)
        let msg = ControlMessage(type: .streamConfigRequest, payload: request.serialize())
        tcpChannel.sendControl(msg)
        ERDLog.info("[Client] Requesting stream config: \(config.width)x\(config.height) @\(config.framesPerSecond)fps")
    }

    // MARK: - Clipboard Sync

    private func startClipboardSync() {
        let peerCaps = stateQueue.sync { self.peerCapabilities }
        guard peerCaps.contains(.textClipboardSync) else {
            ERDLog.info("[Client] Server does not support clipboard sync, skipping")
            return
        }

        let monitor = ClipboardMonitor { [weak self] text in
            guard let self = self else { return }
            let update = ClipboardSyncUpdatePayload(
                requestID: 0,
                direction: .clientToHost,
                origin: .localPasteboard,
                text: text)
            if let data = update.serialize() {
                let msg = ControlMessage(type: .clipboardSyncUpdate, payload: data)
                self.tcpChannel.sendControl(msg)
            }
        }
        stateQueue.sync { self.clipboardMonitor = monitor }
        monitor.start()
        ERDLog.info("[Client] Clipboard sync started")
    }

    private func stopClipboardSync() {
        let monitor = stateQueue.sync { self.clipboardMonitor }
        monitor?.stop()
        stateQueue.sync { self.clipboardMonitor = nil }
    }

    private func handleClipboardUpdate(_ update: ClipboardSyncUpdatePayload) {
        let monitor = stateQueue.sync { self.clipboardMonitor }
        monitor?.applyRemoteText(update.text)
        ERDLog.info("[Client] Clipboard received from server (\(update.text.count) chars)")
        onClipboardSynced?(update.text)
    }

    // MARK: - Adaptive Bitrate

    private func startABR() {
        stopABR()
        currentBitrate = ERDConstants.defaultBitrate
        lowLossStartTime = nil

        let timer = DispatchSource.makeTimerSource(queue: abrQueue)
        timer.schedule(deadline: .now() + 3.0, repeating: 3.0)
        timer.setEventHandler { [weak self] in
            self?.evaluateBitrate()
        }
        timer.resume()
        abrTimer = timer
    }

    private func stopABR() {
        abrTimer?.cancel()
        abrTimer = nil
    }

    private func evaluateBitrate() {
        let lossRatio = frameReceiver.frameLossRatio
        var newBitrate = currentBitrate

        if lossRatio > 0.10 {
            lowLossStartTime = nil
            newBitrate = max(ERDConstants.minBitrate, Int(Double(currentBitrate) * 0.75))
            ERDLog.info("[Client] ABR: loss \(String(format: "%.1f", lossRatio * 100))%% → reduce to \(newBitrate)")
        } else if lossRatio < 0.02 {
            if let start = lowLossStartTime {
                if Date().timeIntervalSince(start) >= 10.0 {
                    newBitrate = min(ERDConstants.maxBitrate, Int(Double(currentBitrate) * 1.10))
                    lowLossStartTime = Date()
                    ERDLog.info("[Client] ABR: stable → increase to \(newBitrate)")
                }
            } else {
                lowLossStartTime = Date()
            }
        } else {
            lowLossStartTime = nil
        }

        if newBitrate != currentBitrate {
            currentBitrate = newBitrate
            let payload = BitrateAdjustPayload(targetBitrate: Int32(newBitrate))
            let msg = ControlMessage(type: .bitrateAdjust, payload: payload.serialize())
            tcpChannel.sendControl(msg)
        }
    }

    public struct StreamStats {
        public let fps: Double
        public let bitrate: Int
        public let frameLoss: Double
        public let framesReceived: Int
    }

    public func getStats() -> StreamStats {
        StreamStats(
            fps: frameReceiver.recentFPS,
            bitrate: currentBitrate,
            frameLoss: frameReceiver.frameLossRatio,
            framesReceived: frameReceiver.totalFramesReceived
        )
    }
}
