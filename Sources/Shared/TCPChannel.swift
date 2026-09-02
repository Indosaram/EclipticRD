import Foundation
import Network
import Security

/// Transport security selection for a TCP channel.
///
/// `.psk` upgrades the channel to TLS 1.3 with external pre-shared keys.
/// The host offers one PSK per paired device plus the pending bootstrap key
/// (see PairingManager.hostPSKs); the client offers exactly one. Identity
/// strings on the wire pick the key, so no certificates are involved.
public enum ChannelSecurity {
    case none
    case psk([ERDPSK])

    var tlsParameters: NWParameters? {
        guard case let .psk(psks) = self, !psks.isEmpty else { return nil }
        return Self.tlsOptions(psks)
    }

    static func tlsOptions(_ psks: [ERDPSK]) -> NWParameters {
        let options = NWProtocolTLS.Options()
        let sec = options.securityProtocolOptions
        for psk in psks {
            sec_protocol_options_add_pre_shared_key(sec,
                                                    Self.dispatchData(psk.key) as dispatch_data_t,
                                                    Self.dispatchData(Data(psk.identity.utf8)) as dispatch_data_t)
        }
        sec_protocol_options_set_min_tls_protocol_version(sec, .TLSv12)
        sec_protocol_options_set_max_tls_protocol_version(sec, .TLSv13)
        // PSK-only TLS exchanges no certificate; the transcript MAC binds
        // both sides to the shared key, which is the authentication. Reading
        // back the negotiated PSK identity (to cross-check the app-layer
        // pairing id) is not possible: Security exposes
        // sec_protocol_metadata_access_pre_shared_keys with a C-only
        // dispatch_data_t that Swift cannot introspect.
        sec_protocol_options_set_verify_block(sec, { _, _, complete in complete(true) }, DispatchQueue.global(qos: .userInitiated))
        return NWParameters(tls: options)
    }

    private static func dispatchData(_ data: Data) -> DispatchData {
        // DispatchData(bytes:) copies the buffer (default destructor), so the
        // result outlives the Data storage.
        data.withUnsafeBytes { DispatchData(bytes: $0) }
    }
}

public class TCPChannel {
    private var connection: NWConnection?
    private var listener: NWListener?
    private let queue = DispatchQueue(label: "eclipticrd.tcp", qos: .userInteractive)
    private static let queueKey = DispatchSpecificKey<Bool>()
    private var receiveBuffer = Data()
    private var timeoutWorkItem: DispatchWorkItem?
    private var didNotifyDisconnect = false
    public var security: ChannelSecurity
    private var listenPort: UInt16?

    public var onConnect: (@Sendable () -> Void)?
    public var onDisconnect: (@Sendable () -> Void)?
    public var onReceive: (@Sendable (Data) -> Void)?
    public var onConnectionFailed: (@Sendable () -> Void)?
    public private(set) var lastFailureDescription: String?
    public private(set) var connectionStateDescription: String?

    public var remoteHostIP: String? {
        guard let endpoint = connection?.currentPath?.remoteEndpoint else { return nil }
        if case .hostPort(let host, _) = endpoint { return "\(host)" }
        return nil
    }

    public init(security: ChannelSecurity = .none) {
        self.security = security
        queue.setSpecific(key: Self.queueKey, value: true)
    }

    deinit { stop() }

    private var connectionParameters: NWParameters {
        let params = security.tlsParameters ?? NWParameters.tcp
        // Pairing swaps PSKs by restarting the listener; without endpoint
        // reuse the immediate rebind loses the race with the old socket.
        params.allowLocalEndpointReuse = true
        return params
    }

    // MARK: - Server side

    public func startListening(port: UInt16) throws {
        listenPort = port
        listener = try NWListener(using: connectionParameters, on: NWEndpoint.Port(rawValue: port)!)
        listener?.newConnectionHandler = { [weak self] conn in
            self?.setupConnection(conn)
        }
        listener?.stateUpdateHandler = { state in
            ERDLog.network("[TCP] Listener state: \(state)")
        }
        listener?.start(queue: queue)
    }

    public func advertiseService(name: String) {
        listener?.service = NWListener.Service(name: name, type: ERDConstants.bonjourServiceType, domain: ERDConstants.bonjourDomain)
    }

    /// Swap the offered PSKs (e.g. after a pairing grant) and restart the
    /// listener once the old one has fully released its port.
    public func updateSecurity(_ newSecurity: ChannelSecurity) {
        queue.async { [weak self] in
            guard let self = self else { return }
            self.security = newSecurity
            guard let port = self.listenPort, let old = self.listener else { return }
            self.listener = nil
            old.stateUpdateHandler = { [weak self] state in
                if case .cancelled = state {
                    old.stateUpdateHandler = nil
                    self?.rebindListener(port: port, attempts: 3)
                }
            }
            old.cancel()
        }
    }

    /// Rebind with bounded retries — on slower machines the cancelled
    /// listener's port can linger briefly past the .cancelled callback.
    private func rebindListener(port: UInt16, attempts: Int) {
        do {
            try startListening(port: port)
        } catch {
            guard attempts > 1 else {
                ERDLog.error("[TCP] Listener rebind failed permanently: \(error)")
                return
            }
            ERDLog.warning("[TCP] Listener rebind failed (\(attempts - 1) retries left), retrying...")
            queue.asyncAfter(deadline: .now() + 0.3) { [weak self] in
                self?.rebindListener(port: port, attempts: attempts - 1)
            }
        }
    }

    // MARK: - Client side

    public func connect(host: String, port: UInt16) {
        let endpoint = NWEndpoint.hostPort(host: NWEndpoint.Host(host), port: NWEndpoint.Port(rawValue: port)!)
        connect(to: endpoint)
    }

    public func connect(to endpoint: NWEndpoint) {
        let conn = NWConnection(to: endpoint, using: connectionParameters)
        queue.async { [weak self] in
            self?.setupConnection(conn)
        }
        setTimeout(ERDConstants.connectionTimeout)
    }

    public func connectICE(endpoints: [NWEndpoint]) {
        for endpoint in endpoints {
            let conn = NWConnection(to: endpoint, using: connectionParameters)
            conn.stateUpdateHandler = { [weak self] state in
                guard let self = self else { return }
                switch state {
                case .ready:
                    if self.connection == nil {
                        self.connection = conn
                        self.didNotifyDisconnect = false
                        self.cancelTimeout()
                        ERDLog.network("[Client] ICE Won by \(endpoint)")
                        conn.stateUpdateHandler = { [weak self] state in
                            switch state {
                            case .ready:
                                break
                            case .failed(let err):
                                ERDLog.error("[TCP] ICE connection failed: \(err)")
                                self?.notifyDisconnect()
                            case .cancelled:
                                self?.notifyDisconnect()
                            default:
                                break
                            }
                        }
                        self.startReceiving()
                        self.onConnect?()
                    } else {
                        conn.cancel()
                    }
                case .failed, .cancelled:
                    break
                default: break
                }
            }
            conn.start(queue: queue)
        }
        setTimeout(ERDConstants.connectionTimeout)
    }

    public func setTimeout(_ seconds: Double) {
        queue.async { [weak self] in
            self?.cancelTimeout()
            let work = DispatchWorkItem { [weak self] in
                guard let self = self else { return }
                ERDLog.error("[TCP] Connection timeout after \(seconds)s")
                self.connection?.cancel()
                self.connection = nil
                self.notifyDisconnect()
            }
            self?.timeoutWorkItem = work
            self?.queue.asyncAfter(deadline: .now() + seconds, execute: work)
        }
    }

    // MARK: - Send

    public func send(_ data: Data) {
        // Length-prefixed framing: 4-byte little-endian length + payload
        var len = UInt32(data.count).littleEndian
        var frame = Data(bytes: &len, count: 4)
        frame.append(data)
        connection?.send(content: frame, completion: .contentProcessed { error in
            if let error = error {
                ERDLog.error("[TCP] Send error: \(error)")
            }
        })
    }

    public func sendControl(_ msg: ControlMessage) {
        let header = PacketHeader(type: .control, sequence: 0, timestamp: currentTimestamp())
        var packet = header.serialize()
        packet.append(msg.serialize())
        send(packet)
    }

    public func sendInput(_ input: InputEventPayload) {
        let header = PacketHeader(type: .inputEvent, sequence: 0, timestamp: currentTimestamp())
        var packet = header.serialize()
        packet.append(input.serialize())
        send(packet)
    }

    // MARK: - Stop

    public func stop() {
        let work = {
            self.cancelTimeout()
            self.connection?.cancel()
            self.connection = nil
            self.listener?.cancel()
            self.listener = nil
            self.receiveBuffer = Data()
            self.notifyDisconnect()
        }

        if DispatchQueue.getSpecific(key: Self.queueKey) == true {
            work()
        } else {
            queue.sync { work() }
        }
    }

    public func disconnectConnection() {
        let work = {
            self.cancelTimeout()
            self.connection?.cancel()
            self.connection = nil
            self.receiveBuffer = Data()
            self.notifyDisconnect()
        }

        if DispatchQueue.getSpecific(key: Self.queueKey) == true {
            work()
        } else {
            queue.sync { work() }
        }
    }

    // MARK: - Internal

    private func cancelTimeout() {
        timeoutWorkItem?.cancel()
        timeoutWorkItem = nil
    }

    private func setupConnection(_ conn: NWConnection) {
        connection = conn
        didNotifyDisconnect = false
        var becameReady = false
        conn.stateUpdateHandler = { [weak self] state in
            self?.connectionStateDescription = "\(state)"
            switch state {
            case .ready:
                becameReady = true
                self?.cancelTimeout()
                ERDLog.network("[TCP] Connected to \(conn.endpoint)")
                self?.startReceiving()
                self?.onConnect?()
            case .failed(let err):
                ERDLog.error("[TCP] Connection failed: \(err)")
                if let self {
                    queue.async { self.lastFailureDescription = "\(err)" }
                    // A connection that never became ready failed the TLS
                    // handshake — feed the bootstrap lockout counter.
                    if !becameReady { self.onConnectionFailed?() }
                }
                self?.notifyDisconnect()
            case .cancelled:
                self?.notifyDisconnect()
            default: break
            }
        }
        conn.start(queue: queue)
    }

    private func notifyDisconnect() {
        guard !didNotifyDisconnect else { return }
        didNotifyDisconnect = true
        onDisconnect?()
    }

    private func startReceiving() {
        connection?.receive(minimumIncompleteLength: 1, maximumLength: 262144) { [weak self] content, _, isComplete, error in
            guard let self = self else { return }
            if let data = content {
                self.receiveBuffer.append(data)
                self.processBuffer()
            }
            if let error = error {
                ERDLog.error("[TCP] Receive error: \(error)")
                self.notifyDisconnect()
                return
            }
            if !isComplete {
                self.startReceiving()
            } else {
                self.notifyDisconnect()
            }
        }
    }

    private func processBuffer() {
        while receiveBuffer.count >= 4 {
            let length = Int(receiveBuffer.prefix(4).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian })
            guard length > 0, length <= 16_777_216 else {
                // Invalid length — drop buffer to recover
                ERDLog.error("[TCP] Invalid frame length: \(length), dropping buffer")
                receiveBuffer.removeAll()
                break
            }
            guard receiveBuffer.count >= 4 + length else {
                break
            }
            let payload = receiveBuffer.subdata(in: 4..<(4 + length))
            receiveBuffer.removeSubrange(0..<(4 + length))
            onReceive?(payload)
        }
    }

    private func currentTimestamp() -> UInt32 {
        UInt32(truncatingIfNeeded: UInt64(Date().timeIntervalSince1970 * 1000))
    }
}
