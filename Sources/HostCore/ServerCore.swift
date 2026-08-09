import Foundation
import Network
import CoreMedia
import CoreGraphics

public class ServerCore {
    public static let shared = ServerCore()

    private let tcpChannel = TCPChannel()
    private let udpChannel = UDPChannel()
    private var frameSender: FrameSender?
    private var inputReceiver: InputReceiver?
    private let screenCapture = ScreenCapture()
    private var videoEncoder: VideoEncoder?
    private var hostname: String = ""
    public private(set) var isRunning = false
    private var isStreaming = AtomicBool(false)
    // Protects inputReceiver, frameSender, videoEncoder from concurrent access
    // between tcpChannel.onReceive (TCP queue) and startStreaming/stopStreaming (async context).
    private let stateQueue = DispatchQueue(label: "eclipticrd.server.state")

    private var heartbeatTimer: DispatchSourceTimer?
    private let heartbeatQueue = DispatchQueue(label: "eclipticrd.server.heartbeat")
    private var lastPongTime: Date = Date()
    private let pongLock = NSLock()

    private var clipboardMonitor: ClipboardMonitor?
    private var peerCapabilities: HandshakeCapabilities = []
    private var activeStreamConfig: StreamConfiguration?

    public var onRunningChanged: (@Sendable (Bool) -> Void)?

    private init() {}

    public func start(pin: String? = nil) async {
        hostname = ProcessInfo.processInfo.hostName

        do {
            // 1. Start TCP listener for control/input
            try tcpChannel.startListening(port: ERDConstants.tcpPort)
            isRunning = true
            onRunningChanged?(true)

            // 2. Start UDP listener for video frames
            try udpChannel.startListening(port: ERDConstants.udpPort)
            ERDLog.info("[Server] Listening on UDP:\(ERDConstants.udpPort)")

            // 3. Set up handlers BEFORE advertising/signaling so incoming connections are handled
            tcpChannel.onConnect = { [weak self] in
                ERDLog.info("[Server] Client connected!")
                Task { await self?.startStreaming() }
            }
            tcpChannel.onDisconnect = { [weak self] in
                ERDLog.info("[Server] Client disconnected")
                Task { await self?.stopStreaming() }
            }
            tcpChannel.onReceive = { [weak self] data in
                guard let self = self else { return }
                guard data.count >= ERDConstants.packetHeaderSize else { return }
                guard let header = PacketHeader.deserialize(from: data) else {
                    return
                }
                let payload = data.subdata(in: ERDConstants.packetHeaderSize..<data.count)

                switch header.type {
                case .inputEvent:
                    let receiver = self.stateQueue.sync { self.inputReceiver }
                    receiver?.handleInputEvent(payload)
                case .control:
                    if let msg = ControlMessage.deserialize(from: payload) {
                        switch msg.type {
                        case .requestKeyFrame:
                            let encoder = self.stateQueue.sync { self.videoEncoder }
                            encoder?.forceKeyFrame()
                        case .startStream: Task { await self.startStreaming() }
                        case .stopStream: Task { await self.stopStreaming() }
                        case .disconnect: Task { await self.stop() }
                        case .ping: break
                        case .streamConfigRequest:
                            if let data = msg.payload,
                               let req = StreamConfigurationRequestPayload.deserialize(from: data) {
                                Task { await self.handleStreamConfigRequest(req) }
                            }
                        case .streamConfigResponse, .streamConfigReject, .streamConfigError:
                            break // Server doesn't process these
                        case .clipboardSyncRequest:
                            break // Clipboard start/stop handled at session level
                        case .clipboardSyncUpdate:
                            if let data = msg.payload,
                               let update = ClipboardSyncUpdatePayload.deserialize(from: data) {
                                self.handleClipboardUpdate(update)
                            }
                        case .clipboardSyncError:
                            if let data = msg.payload,
                               let err = ClipboardSyncErrorPayload.deserialize(from: data) {
                                ERDLog.warning("[Server] Clipboard sync error from client: \(err.message)")
                            }
                        case .pong:
                            self.pongLock.lock()
                            self.lastPongTime = Date()
                            self.pongLock.unlock()
                        case .bitrateAdjust:
                            if let data = msg.payload,
                               let adj = BitrateAdjustPayload.deserialize(from: data) {
                                let newBitrate = Int(adj.targetBitrate)
                                ERDLog.info("[Server] Bitrate adjust request: \(newBitrate)")
                                let encoder = self.stateQueue.sync { self.videoEncoder }
                                encoder?.updateBitrate(newBitrate)
                            }
                        }
                    }
                case .handshake:
                    if let hs = HandshakePayload.deserialize(from: payload) {
                        ERDLog.info("[Server] Received handshake from client: \(hs.hostname) caps=\(hs.capabilities.rawValue)")
                        self.stateQueue.sync { self.peerCapabilities = hs.capabilities }
                        if self.isStreaming.value {
                            self.startClipboardSync()
                        }
                    }
                default: break
                }
            }

            // 4. Advertise via Bonjour (handlers are ready for incoming connections)
            tcpChannel.advertiseService(name: hostname)
            ERDLog.info("[Server] Advertising as '\(hostname)' via Bonjour")

            // 5. If PIN provided, exchange candidates via signaling
            if let pin = pin {
                await startSignaling(pin: pin)
            }

            ERDLog.info("[Server] Waiting for client connection...")
        } catch {
            isRunning = false
            onRunningChanged?(false)
            ERDLog.error("[Server] Fatal error: \(error)")
        }
    }

    private func startSignaling(pin: String) async {
        do {
            let stun = STUNClient()
            let (publicIP, publicPort) = try await stun.fetchPublicIP()
            ERDLog.info("[Server] STUN Public IP: \(publicIP):\(publicPort)")

            let localIP = NetworkUtils.getLocalIP() ?? "127.0.0.1"
            let signaling = SignalingClient(pin: pin, role: "server")
            let ourCandidate = SessionCandidate(role: "server", localIP: localIP, localPort: ERDConstants.tcpPort,
                                                  publicIP: publicIP, publicPort: publicPort)

            ERDLog.info("[Server] Exchanging candidates via PIN: \(pin)")
            let _ = try await signaling.exchangeCandidate(ourCandidate)
            signaling.stop()
            ERDLog.info("[Server] Signaling complete, waiting for client TCP connection...")
        } catch {
            ERDLog.error("[Server] Signaling failed: \(error)")
        }
    }

    private func startStreaming() async {
        guard !isStreaming.value else {
            return
        }

        guard CGPreflightScreenCaptureAccess() else {
            ERDLog.warning("[Server] Screen Recording permission not granted")
            ERDLog.warning("[Server]    Go to System Settings > Privacy & Security > Screen Recording")
            return
        }

        isStreaming.value = true

        let info = screenCapture.getDisplayInfo()
        let initialConfiguration = StreamConfiguration(
            width: UInt32(info.pixelWidth),
            height: UInt32(info.pixelHeight),
            bitrate: UInt32(ERDConstants.defaultBitrate),
            framesPerSecond: UInt16(ERDConstants.defaultFPS)
        )

        stateQueue.sync {
            inputReceiver = InputReceiver(screenWidth: CGFloat(info.width), screenHeight: CGFloat(info.height))
            frameSender = FrameSender(udpChannel: udpChannel)
            activeStreamConfig = initialConfiguration
        }

        // Send handshake to client (logical resolution for coordinate mapping)
        let handshake = HandshakePayload(hostname: hostname, screenWidth: UInt16(info.width),
                                          screenHeight: UInt16(info.height), scaleFactor: Float(info.scale),
                                          capabilities: [.streamConfiguration, .textClipboardSync])
        let header = PacketHeader(type: .handshakeAck, sequence: 0, timestamp: 0)
        var packet = header.serialize()
        packet.append(handshake.serialize())
        tcpChannel.send(packet)

        let preferredFPS = UserDefaults.standard.integer(forKey: "preferredFPS")
        let fps = preferredFPS > 0 ? preferredFPS : ERDConstants.defaultFPS

        // Setup pipeline: capture → encode → send (physical resolution for encoding)
        let encoder = makeVideoEncoder(
            width: info.pixelWidth,
            height: info.pixelHeight,
            bitrate: ERDConstants.defaultBitrate,
            fps: fps
        )

        stateQueue.sync { videoEncoder = encoder }

        screenCapture.onFrame = { [weak self] sampleBuffer in
            guard let self = self else { return }
            let encoder = self.stateQueue.sync { self.videoEncoder }
            encoder?.encode(sampleBuffer)
        }
        screenCapture.onCursorPosition = { [weak self] point in
            guard let self = self else { return }
            let nx = Float(point.x) / Float(info.width)
            let ny = Float(point.y) / Float(info.height)
            let sender = self.stateQueue.sync { self.frameSender }
            sender?.sendCursorUpdate(x: nx, y: ny)
        }
        screenCapture.onAudio = { [weak self] sampleBuffer in
            self?.handleAudioFrame(sampleBuffer)
        }

        do {
            try encoder.start()
            try await screenCapture.start(fps: fps)
            startHeartbeat()
            startClipboardSync()
            ERDLog.info("[Server] Streaming started: \(info.width)x\(info.height)")
        } catch {
            isStreaming.value = false
            stopHeartbeat()
            do { try await screenCapture.stop() } catch { }
            stateQueue.sync {
                videoEncoder?.stop()
                videoEncoder = nil
                frameSender = nil
                inputReceiver = nil
                activeStreamConfig = nil
            }
            ERDLog.error("[Server] Failed to start streaming: \(error)")
        }
    }

    private func stopStreaming() async {
        guard isStreaming.value else { return }
        isStreaming.value = false
        stopClipboardSync()
        stopHeartbeat()
        do { try await screenCapture.stop() } catch { ERDLog.error("[Server] Error stopping capture: \(error)") }

        stateQueue.sync {
            videoEncoder?.stop()
            videoEncoder = nil
            frameSender = nil
            inputReceiver = nil
            activeStreamConfig = nil
        }
        ERDLog.info("[Server] Streaming stopped")
    }

    public func stop() async {
        await stopStreaming()
        tcpChannel.stop()
        udpChannel.stop()
        isRunning = false
        onRunningChanged?(false)
    }

    // MARK: - Stream Configuration

    private func handleStreamConfigRequest(_ request: StreamConfigurationRequestPayload) async {
        let desired = request.desiredConfiguration

        // Validate requested dimensions
        let info = screenCapture.getDisplayInfo()
        let maxW = UInt32(info.pixelWidth)
        let maxH = UInt32(info.pixelHeight)

        if desired.width > maxW || desired.height > maxH || desired.width == 0 || desired.height == 0 {
            let reject = StreamConfigurationRejectPayload(
                requestID: request.requestID,
                reason: .unsupportedDimensions,
                message: "Dimensions must be 1..\(maxW)x\(maxH)")
            let msg = ControlMessage(type: .streamConfigReject, payload: reject.serialize())
            tcpChannel.sendControl(msg)
            return
        }

        if desired.framesPerSecond == 0 || desired.framesPerSecond > 120 {
            let reject = StreamConfigurationRejectPayload(
                requestID: request.requestID,
                reason: .unsupportedFPS,
                message: "FPS must be 1..120")
            let msg = ControlMessage(type: .streamConfigReject, payload: reject.serialize())
            tcpChannel.sendControl(msg)
            return
        }

        let clampedBitrate = max(UInt32(ERDConstants.minBitrate), min(desired.bitrate, UInt32(ERDConstants.maxBitrate)))

        let currentConfiguration = stateQueue.sync { self.activeStreamConfig }
        let captureDimensionsChanged = currentConfiguration.map {
            Int($0.width) != Int(desired.width) || Int($0.height) != Int(desired.height)
        } ?? true

        if captureDimensionsChanged {
            let encoder = makeVideoEncoder(
                width: Int(desired.width),
                height: Int(desired.height),
                bitrate: Int(clampedBitrate),
                fps: Int(desired.framesPerSecond)
            )

            do {
                try encoder.start()
            } catch {
                encoder.stop()
                let errPayload = StreamConfigurationErrorPayload(
                    requestID: request.requestID,
                    errorCode: .invalidRequest,
                    message: "Failed to restart encoder: \(error.localizedDescription)")
                let msg = ControlMessage(type: .streamConfigError, payload: errPayload.serialize())
                tcpChannel.sendControl(msg)
                return
            }

            let oldEncoder = stateQueue.sync { () -> VideoEncoder? in
                let encoder = self.videoEncoder
                self.videoEncoder = nil
                return encoder
            }

            do {
                try await screenCapture.updateConfiguration(
                    width: Int(desired.width),
                    height: Int(desired.height),
                    fps: Int(desired.framesPerSecond)
                )
            } catch {
                encoder.stop()
                stateQueue.sync { self.videoEncoder = oldEncoder }
                let errPayload = StreamConfigurationErrorPayload(
                    requestID: request.requestID,
                    errorCode: .invalidRequest,
                    message: "Failed to update capture: \(error.localizedDescription)")
                let msg = ControlMessage(type: .streamConfigError, payload: errPayload.serialize())
                tcpChannel.sendControl(msg)
                return
            }

            stateQueue.sync { self.videoEncoder = encoder }
            oldEncoder?.stop()
        } else {
            do {
                try await screenCapture.updateConfiguration(
                    width: Int(desired.width),
                    height: Int(desired.height),
                    fps: Int(desired.framesPerSecond)
                )
            } catch {
                let errPayload = StreamConfigurationErrorPayload(
                    requestID: request.requestID,
                    errorCode: .invalidRequest,
                    message: "Failed to update capture: \(error.localizedDescription)")
                let msg = ControlMessage(type: .streamConfigError, payload: errPayload.serialize())
                tcpChannel.sendControl(msg)
                return
            }

            let encoder = stateQueue.sync { self.videoEncoder }
            encoder?.updateBitrate(Int(clampedBitrate))
            encoder?.updateFPS(Int(desired.framesPerSecond))
        }

        let applied = StreamConfiguration(
            width: desired.width,
            height: desired.height,
            bitrate: clampedBitrate,
            framesPerSecond: desired.framesPerSecond)
        stateQueue.sync { activeStreamConfig = applied }

        let response = StreamConfigurationResponsePayload(requestID: request.requestID, activeConfiguration: applied)
        let msg = ControlMessage(type: .streamConfigResponse, payload: response.serialize())
        tcpChannel.sendControl(msg)
        ERDLog.info("[Server] Stream config applied: \(applied.width)x\(applied.height) @\(applied.framesPerSecond)fps \(applied.bitrate)bps")
    }

    private func makeVideoEncoder(width: Int, height: Int, bitrate: Int, fps: Int) -> VideoEncoder {
        let encoder = VideoEncoder(width: width, height: height, bitrate: bitrate, fps: fps)
        encoder.onEncodedFrame = { [weak self] data, isKeyFrame in
            guard let self = self else { return }
            let sender = self.stateQueue.sync { self.frameSender }
            let activeConfiguration = self.stateQueue.sync { self.activeStreamConfig }
            let width = Int(activeConfiguration?.width ?? UInt32(width))
            let height = Int(activeConfiguration?.height ?? UInt32(height))
            sender?.sendFrame(data: data, width: width, height: height, isKeyFrame: isKeyFrame)
        }
        return encoder
    }

    // MARK: - Clipboard Sync

    private func startClipboardSync() {
        let existingMonitor = stateQueue.sync { self.clipboardMonitor }
        guard existingMonitor == nil else {
            return
        }

        let peerCaps = stateQueue.sync { self.peerCapabilities }
        guard peerCaps.contains(.textClipboardSync) else {
            ERDLog.info("[Server] Peer does not support clipboard sync, skipping")
            return
        }

        let monitor = ClipboardMonitor { [weak self] text in
            guard let self = self else { return }
            let update = ClipboardSyncUpdatePayload(
                requestID: 0,
                direction: .hostToClient,
                origin: .localPasteboard,
                text: text)
            if let data = update.serialize() {
                let msg = ControlMessage(type: .clipboardSyncUpdate, payload: data)
                self.tcpChannel.sendControl(msg)
            }
        }
        stateQueue.sync { self.clipboardMonitor = monitor }
        monitor.start()
        ERDLog.info("[Server] Clipboard sync started")
    }

    private func stopClipboardSync() {
        let monitor = stateQueue.sync { self.clipboardMonitor }
        monitor?.stop()
        stateQueue.sync { self.clipboardMonitor = nil }
    }

    private func handleClipboardUpdate(_ update: ClipboardSyncUpdatePayload) {
        let monitor = stateQueue.sync { self.clipboardMonitor }
        monitor?.applyRemoteText(update.text)
        ERDLog.info("[Server] Clipboard received from client (\(update.text.count) chars)")
    }

    // MARK: - Heartbeat

    private func startHeartbeat() {
        stopHeartbeat()
        pongLock.lock()
        lastPongTime = Date()
        pongLock.unlock()

        let timer = DispatchSource.makeTimerSource(queue: heartbeatQueue)
        timer.schedule(deadline: .now() + ERDConstants.heartbeatInterval,
                       repeating: ERDConstants.heartbeatInterval)
        timer.setEventHandler { [weak self] in
            guard let self = self, self.isStreaming.value else { return }

            self.tcpChannel.sendControl(ControlMessage(type: .ping))

            self.pongLock.lock()
            let elapsed = Date().timeIntervalSince(self.lastPongTime)
            self.pongLock.unlock()

            if elapsed > ERDConstants.heartbeatInterval * 3 {
                ERDLog.warning("[Server] Client heartbeat timeout (\(elapsed)s)")
                Task { await self.stopStreaming() }
                self.tcpChannel.disconnectConnection()
            }
        }
        timer.resume()
        heartbeatTimer = timer
        ERDLog.info("[Server] Heartbeat started")
    }

    private func stopHeartbeat() {
        heartbeatTimer?.cancel()
        heartbeatTimer = nil
    }

    private func handleAudioFrame(_ sampleBuffer: CMSampleBuffer) {
        guard isStreaming.value else { return }
        guard let blockBuffer = CMSampleBufferGetDataBuffer(sampleBuffer) else { return }

        var length = 0
        var dataPointer: UnsafeMutablePointer<Int8>?
        let status = CMBlockBufferGetDataPointer(blockBuffer, atOffset: 0, lengthAtOffsetOut: nil, totalLengthOut: &length, dataPointerOut: &dataPointer)

        guard status == noErr, let pointer = dataPointer, length > 0 else { return }

        let audioData = Data(bytes: pointer, count: length)

        // Send audioFrame packet over UDP for ultra-low latency system audio
        udpChannel.sendPacket(type: .audioFrame, payload: audioData)
    }
}
