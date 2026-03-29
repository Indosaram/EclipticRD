import Foundation
import Network
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
    private var isStreaming = AtomicBool(false)
    // Protects inputReceiver, frameSender, videoEncoder from concurrent access
    // between tcpChannel.onReceive (TCP queue) and startStreaming/stopStreaming (async context).
    private let stateQueue = DispatchQueue(label: "eclipticrd.server.state")

    private init() {}

    public func start(pin: String? = nil) async {
        hostname = ProcessInfo.processInfo.hostName

        // Check permissions
        guard CGPreflightScreenCaptureAccess() else {
            ERDLog.warning("[Server] Checking Screen Recording permission...")
            ERDLog.warning("[Server]    Go to System Settings > Privacy & Security > Screen Recording")
            ERDLog.warning("[Server]    Enable this app and restart.")
            CGRequestScreenCaptureAccess()
            return
        }

        do {
            // 1. Start TCP listener for control/input
            try tcpChannel.startListening(port: ERDConstants.tcpPort)
            tcpChannel.advertiseService(name: hostname)
            ERDLog.info("[Server] Advertising as '\(hostname)' via Bonjour")

            // 2. Start UDP listener for video frames
            try udpChannel.startListening(port: ERDConstants.udpPort)
            ERDLog.info("[Server] Listening on UDP:\(ERDConstants.udpPort)")

            // 3. If PIN provided, exchange candidates via signaling
            if let pin = pin {
                await startSignaling(pin: pin)
            }

            // 4. Wait for client connection on TCP
            ERDLog.info("[Server] Waiting for client connection...")
            tcpChannel.onConnect = { [weak self] in
                ERDLog.info("[Server] Client connected!")
                Task { await self?.startStreaming() }
            }
            tcpChannel.onDisconnect = { [weak self] in
                ERDLog.info("[Server] Client disconnected")
                Task { await self?.stopStreaming() }
            }
            tcpChannel.onReceive = { [weak self] data in
                // Route input events
                guard let self = self else { return }
                guard data.count >= ERDConstants.packetHeaderSize else { return }
                guard let header = PacketHeader.deserialize(from: data) else { return }
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
                        }
                    }
                case .handshake:
                    if let hs = HandshakePayload.deserialize(from: payload) {
                        ERDLog.info("[Server] Received handshake from client: \(hs.hostname)")
                    }
                default: break
                }
            }
        } catch {
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
        guard !isStreaming.value else { return }
        isStreaming.value = true

        let info = screenCapture.getDisplayInfo()

        stateQueue.sync {
            inputReceiver = InputReceiver(screenWidth: CGFloat(info.width), screenHeight: CGFloat(info.height))
            frameSender = FrameSender(udpChannel: udpChannel)
        }

        // Send handshake to client (logical resolution for coordinate mapping)
        let handshake = HandshakePayload(hostname: hostname, screenWidth: UInt16(info.width),
                                          screenHeight: UInt16(info.height), scaleFactor: Float(info.scale))
        let header = PacketHeader(type: .handshakeAck, sequence: 0, timestamp: 0)
        var packet = header.serialize()
        packet.append(handshake.serialize())
        tcpChannel.send(packet)

        // Setup pipeline: capture → encode → send (physical resolution for encoding)
        let encoder = VideoEncoder(width: info.pixelWidth, height: info.pixelHeight)
        encoder.onEncodedFrame = { [weak self] data, isKeyFrame in
            guard let self = self else { return }
            let sender = self.stateQueue.sync { self.frameSender }
            sender?.sendFrame(data: data, width: info.pixelWidth, height: info.pixelHeight, isKeyFrame: isKeyFrame)
        }

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

        do {
            try encoder.start()
            try await screenCapture.start(fps: ERDConstants.defaultFPS)
            ERDLog.info("[Server] Streaming started: \(info.width)x\(info.height)")
        } catch {
            ERDLog.error("[Server] Failed to start streaming: \(error)")
        }
    }

    private func stopStreaming() async {
        guard isStreaming.value else { return }
        isStreaming.value = false
        do { try await screenCapture.stop() } catch { ERDLog.error("[Server] Error stopping capture: \(error)") }

        stateQueue.sync {
            videoEncoder?.stop()
            videoEncoder = nil
            frameSender = nil
            inputReceiver = nil
        }
        ERDLog.info("[Server] Streaming stopped")
    }

    public func stop() async {
        await stopStreaming()
        tcpChannel.stop()
        udpChannel.stop()
    }
}
