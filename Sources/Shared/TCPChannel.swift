import Foundation
import Network

public class TCPChannel {
    private var connection: NWConnection?
    private var listener: NWListener?
    private let queue = DispatchQueue(label: "eclipticrd.tcp", qos: .userInteractive)
    private static let queueKey = DispatchSpecificKey<Bool>()
    private var receiveBuffer = Data()

    public var onConnect: (@Sendable () -> Void)?
    public var onDisconnect: (@Sendable () -> Void)?
    public var onReceive: (@Sendable (Data) -> Void)?

    public var remoteHostIP: String? {
        guard let endpoint = connection?.currentPath?.remoteEndpoint else { return nil }
        if case .hostPort(let host, _) = endpoint { return "\(host)" }
        return nil
    }

    public init() {
        queue.setSpecific(key: Self.queueKey, value: true)
    }

    deinit { stop() }

    // MARK: - Server side

    public func startListening(port: UInt16) throws {
        let params = NWParameters.tcp
        listener = try NWListener(using: params, on: NWEndpoint.Port(rawValue: port)!)
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

    // MARK: - Client side

    public func connect(host: String, port: UInt16) {
        let endpoint = NWEndpoint.hostPort(host: NWEndpoint.Host(host), port: NWEndpoint.Port(rawValue: port)!)
        connect(to: endpoint)
    }

    public func connect(to endpoint: NWEndpoint) {
        let conn = NWConnection(to: endpoint, using: .tcp)
        queue.async { [weak self] in
            self?.setupConnection(conn)
        }
    }

    public func connectICE(endpoints: [NWEndpoint]) {
        // Try all endpoints simultaneously, keep the first one that connects.
        // NOTE: stateUpdateHandler already runs on `queue`, so no queue.sync needed.
        for endpoint in endpoints {
            let conn = NWConnection(to: endpoint, using: .tcp)
            conn.stateUpdateHandler = { [weak self] state in
                guard let self = self else { return }
                switch state {
                case .ready:
                    if self.connection == nil {
                        self.connection = conn
                        ERDLog.network("[Client] ICE Won by \(endpoint)")
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
    }

    // MARK: - Send

    public func send(_ data: Data) {
        // Length-prefixed framing: 4-byte little-endian length + payload
        var len = UInt32(data.count).littleEndian
        var frame = Data(bytes: &len, count: 4)
        frame.append(data)
        connection?.send(content: frame, completion: .contentProcessed { error in
            if let error = error { ERDLog.error("[TCP] Send error: \(error)") }
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
        // Synchronize on the queue to avoid data races with receiveBuffer and connection state.
        // Avoid deadlock if already on the queue by checking the queue-specific key.
        let work = {
            self.connection?.cancel()
            self.connection = nil
            self.listener?.cancel()
            self.listener = nil
            self.receiveBuffer = Data()
        }

        if DispatchQueue.getSpecific(key: Self.queueKey) == true {
            work()
        } else {
            queue.sync { work() }
        }
    }

    // MARK: - Internal

    private func setupConnection(_ conn: NWConnection) {
        connection = conn
        conn.stateUpdateHandler = { [weak self] state in
            switch state {
            case .ready:
                ERDLog.network("[TCP] Connected to \(conn.endpoint)")
                self?.startReceiving()
                self?.onConnect?()
            case .failed(let err):
                ERDLog.error("[TCP] Connection failed: \(err)")
                self?.onDisconnect?()
            case .cancelled:
                self?.onDisconnect?()
            default: break
            }
        }
        conn.start(queue: queue)
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
                return
            }
            if !isComplete {
                self.startReceiving()
            } else {
                self.onDisconnect?()
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
            guard receiveBuffer.count >= 4 + length else { break }
            let payload = receiveBuffer.subdata(in: 4..<(4 + length))
            receiveBuffer.removeSubrange(0..<(4 + length))
            onReceive?(payload)
        }
    }

    private func currentTimestamp() -> UInt32 {
        UInt32(truncatingIfNeeded: UInt64(Date().timeIntervalSince1970 * 1000))
    }
}
