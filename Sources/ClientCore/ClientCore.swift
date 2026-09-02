import Foundation
import Network
import MetalKit
import AppKit
import Security

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
    private var activePairing: PairingRecord?
    private var sessionPairingKey: Data?
    private var sessionSalt = Data()
    private var audioFragments: [UInt32: (chunks: [UInt16: Data], total: UInt16)] = [:]
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
            
            // Bypass frame assembly queue for zero-jitter, real-time audio playback.
            // Magic check first: spoofed datagrams must not reach any handler.
            guard data[0] == UInt8(truncatingIfNeeded: ERDConstants.magic),
                  data[1] == UInt8(ERDConstants.magic >> 8) else { return }
            if data[2] == PacketType.audioFrame.rawValue {
                if !self.isAudioMuted.value {
                    let payload = data.subdata(in: ERDConstants.packetHeaderSize..<data.count)
                    self.handleAudioPayload(payload)
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

    /// Connect via direct IP (LAN manual). Supply a previously granted
    /// pairing record, or a bootstrap PIN for first-time pairing; connecting
    /// with neither is refused.
    public func start(host: String?, pairing: PairingRecord?, bootstrapPIN: String?) {
        guard let host = host else { return }
        self.serverHost = host
        guard setTransportSecurity(pairing: pairing, bootstrapPIN: bootstrapPIN) else { return }
        ERDLog.info("[Client] Connecting to \(host)")

        tcpChannel.connect(host: host, port: ERDConstants.tcpPort)
        setupTCPHandlers(host: host)
    }

    public func start(host: String?) {
        start(host: host, pairing: PairingManager.shared.pairedDevices().first, bootstrapPIN: nil)
    }

    /// Connect via Bonjour endpoint
    public func start(endpoint: NWEndpoint, name: String, pairing: PairingRecord?, bootstrapPIN: String?) {
        ERDLog.info("[Client] Connecting to Bonjour Endpoint: \(name)")
        guard setTransportSecurity(pairing: pairing, bootstrapPIN: bootstrapPIN) else { return }
        tcpChannel.connect(to: endpoint)
        setupTCPHandlers(host: nil)
    }

    public func start(endpoint: NWEndpoint, name: String) {
        start(endpoint: endpoint, name: name, pairing: PairingManager.shared.pairedDevices().first, bootstrapPIN: nil)
    }

    @discardableResult
    private func setTransportSecurity(pairing: PairingRecord?, bootstrapPIN: String?) -> Bool {
        if let pairing {
            tcpChannel.security = .psk([PairingManager.shared.clientPSK(for: pairing)])
            activePairing = pairing
            return true
        }
        if let bootstrapPIN {
            tcpChannel.security = .psk([PairingManager.shared.clientPSK(pin: bootstrapPIN)])
            activePairing = nil
            return true
        }
        onError?("No paired device and no bootstrap PIN")
        return false
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

                // Install handlers before dialing: the TLS channel can become
                // ready before connectICE returns.
                tcpChannel.security = .psk([PairingManager.shared.clientPSK(pin: pin)])
                setupTCPHandlers(host: nil)
                tcpChannel.connectICE(endpoints: endpoints)

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
            audioFragments.removeAll()
            sessionPairingKey = nil
            sessionSalt = Data()
            activePairing = nil
            renderer = nil
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

            ERDLog.info("[Client] TLS channel established")
            if let pairing = self.activePairing {
                self.sendHandshake(with: pairing)
            } else {
                // Bootstrap channel: request pairing before any capability flows
                let request = PairingRequestPayload(hostname: ProcessInfo.processInfo.hostName)
                let header = PacketHeader(type: .pairingRequest, sequence: 0, timestamp: 0)
                var packet = header.serialize()
                packet.append(request.serialize())
                self.tcpChannel.send(packet)
            }
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

            if header.type == .pairingGrant {
                if let grant = PairingGrantPayload.deserialize(from: payload) {
                    let record = PairingRecord(id: grant.pairingID, name: grant.hostName, key: grant.key)
                    PairingManager.shared.adoptPairing(record)
                    self.activePairing = record
                    ERDLog.info("[Client] Paired with host \(grant.hostName)")
                    self.sendHandshake(with: record)
                }
            } else if header.type == .pairingReject {
                if let reject = PairingRejectPayload.deserialize(from: payload) {
                    ERDLog.warning("[Client] Pairing rejected: \(reject.reason)")
                    self.onError?("Pairing rejected")
                    self.stop()
                }
            } else if header.type == .handshakeAck {
                if let hs = HandshakePayload.deserialize(from: payload) {
                    guard hs.protocolVersion == ERDConstants.protocolVersion else {
                        ERDLog.warning("[Client] Host speaks protocol v\(hs.protocolVersion), expected v\(ERDConstants.protocolVersion)")
                        self.onError?("Protocol version mismatch")
                        self.stop()
                        return
                    }
                    ERDLog.info("[Client] Server: \(hs.hostname) \(hs.screenWidth)x\(hs.screenHeight) caps=\(hs.capabilities.rawValue)")
                    self.onServerIdentity?(hs.hostname)
                    self.serverScreenWidth = Int(hs.screenWidth)
                    self.serverScreenHeight = Int(hs.screenHeight)
                    self.stateQueue.sync { self.peerCapabilities = hs.capabilities }

                    let (key, salt) = self.stateQueue.sync { (self.sessionPairingKey, self.sessionSalt) }
                    if let key, salt.count == 16 {
                        self.udpChannel.sendCipher = DatagramCipher.udpCipher(masterKey: key, sessionSalt: salt, clientToHost: true)
                        self.udpChannel.receiveCipher = DatagramCipher.udpCipher(masterKey: key, sessionSalt: salt, clientToHost: false)
                    }

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

    // MARK: - Session Security

    private func sendHandshake(with pairing: PairingRecord) {
        var salt = Data(count: 16)
        _ = salt.withUnsafeMutableBytes { SecRandomCopyBytes(kSecRandomDefault, 16, $0.baseAddress!) }
        stateQueue.sync {
            self.sessionSalt = salt
            self.sessionPairingKey = pairing.key
        }

        let hs = HandshakePayload(hostname: ProcessInfo.processInfo.hostName,
                                   screenWidth: 0, screenHeight: 0, scaleFactor: 1.0,
                                   capabilities: [.streamConfiguration, .textClipboardSync],
                                   pairingID: pairing.id,
                                   sessionSalt: salt)
        let header = PacketHeader(type: .handshake, sequence: 0, timestamp: 0)
        var packet = header.serialize()
        packet.append(hs.serialize())
        tcpChannel.send(packet)
        ERDLog.info("[Client] Handshake sent (paired as \(pairing.name))")
    }

    private func handleAudioPayload(_ payload: Data) {
        guard payload.count >= 8 else {
            audioPlayer.play(data: payload)
            return
        }
        let frameId = payload.subdata(in: 0..<4).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian }
        let index = payload.subdata(in: 4..<6).withUnsafeBytes { $0.load(as: UInt16.self).littleEndian }
        let total = payload.subdata(in: 6..<8).withUnsafeBytes { $0.load(as: UInt16.self).littleEndian }
        let chunk = payload.subdata(in: 8..<payload.count)

        guard total >= 1, index < total else { return }
        if total == 1 {
            audioPlayer.play(data: chunk)
            return
        }

        stateQueue.sync {
            if self.audioFragments.count > 64 { self.audioFragments.removeAll() }
            var entry = self.audioFragments[frameId] ?? (chunks: [:], total: total)
            entry.chunks[index] = chunk
            self.audioFragments[frameId] = entry

            if entry.chunks.count == Int(total) {
                self.audioFragments.removeValue(forKey: frameId)
                var assembled = Data()
                for i in 0..<total {
                    assembled.append(entry.chunks[i] ?? Data())
                }
                self.audioPlayer.play(data: assembled)
            }
        }
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
