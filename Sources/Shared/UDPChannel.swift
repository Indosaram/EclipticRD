import Foundation
import Network

public class UDPChannel {
    private var connection: NWConnection?
    private var listener: NWListener?
    private let queue = DispatchQueue(label: "eclipticrd.udp", qos: .userInteractive)
    private static let queueKey = DispatchSpecificKey<Bool>()
    private var sequenceCounter: UInt32 = 0
    private let seqLock = NSLock()

    public var onReceive: (@Sendable (Data, NWEndpoint?) -> Void)?
    public var onReady: (@Sendable () -> Void)?

    private let _isReady = AtomicBool(false)
    public var isReady: Bool { _isReady.value }

    public init() {
        queue.setSpecific(key: Self.queueKey, value: true)
    }

    deinit { stop() }

    // MARK: - Server side

    public func startListening(port: UInt16) throws {
        let params = NWParameters.udp
        params.allowLocalEndpointReuse = true
        listener = try NWListener(using: params, on: NWEndpoint.Port(rawValue: port)!)
        listener?.newConnectionHandler = { [weak self] conn in
            guard let self = self else { return }
            self.connection = conn
            conn.stateUpdateHandler = { [weak self] state in
                if case .ready = state {
                    ERDLog.network("[UDP] Listener connection ready")
                    self?._isReady.value = true
                    self?.onReady?()
                }
            }
            conn.start(queue: self.queue)
            self.receiveLoop(on: conn)
        }
        listener?.start(queue: queue)
    }

    // MARK: - Client side

    public func connect(host: String, port: UInt16) {
        let endpoint = NWEndpoint.hostPort(host: NWEndpoint.Host(host), port: NWEndpoint.Port(rawValue: port)!)
        connect(to: endpoint)
    }

    public func connect(to endpoint: NWEndpoint) {
        let params = NWParameters.udp
        params.allowLocalEndpointReuse = true
        let conn = NWConnection(to: endpoint, using: params)
        queue.async { [weak self] in
            guard let self = self else { return }
            self.connection = conn
            conn.stateUpdateHandler = { [weak self] state in
                switch state {
                case .ready:
                    ERDLog.network("[UDP] Connected to \(endpoint)")
                    self?._isReady.value = true
                    self?.onReady?()
                case .failed(let e): ERDLog.error("[UDP] Connection failed: \(e)")
                default: break
                }
            }
            conn.start(queue: self.queue)
            self.receiveLoop(on: conn)
        }
    }

    // MARK: - Send

    public func send(_ data: Data, to endpoint: NWEndpoint? = nil) {
        guard let conn = connection else {
            ERDLog.warning("[UDP] Send called while not connected — packet dropped")
            return
        }
        conn.send(content: data, completion: .contentProcessed { error in
            if let error = error { ERDLog.error("[UDP] Send error: \(error)") }
        })
    }

    public func sendPacket(type: PacketType, payload: Data, to endpoint: NWEndpoint? = nil) {
        seqLock.lock()
        sequenceCounter += 1
        let seq = sequenceCounter
        seqLock.unlock()
        let ts = UInt32(truncatingIfNeeded: UInt64(Date().timeIntervalSince1970 * 1000))
        let header = PacketHeader(type: type, sequence: seq, timestamp: ts)
        var packet = header.serialize()
        packet.append(payload)
        send(packet, to: endpoint)
    }

    /// Send a small datagram to trigger the other side's listener to register us
    public func sendPing() {
        // Send 1-byte ping to trigger connection handler on the listener side
        send(Data([0xFF]))
    }

    // MARK: - Stop

    public func stop() {
        let work = {
            self.connection?.cancel()
            self.connection = nil
            self.listener?.cancel()
            self.listener = nil
            self.sequenceCounter = 0
            self._isReady.value = false
        }

        if DispatchQueue.getSpecific(key: Self.queueKey) == true {
            work()
        } else {
            queue.sync { work() }
        }
    }

    // MARK: - Internal

    private func receiveLoop(on conn: NWConnection) {
        conn.receiveMessage { [weak self] content, _, isComplete, error in
            guard let self = self else { return }
            if let data = content, data.count > 1 { // Ignore 1-byte pings
                self.onReceive?(data, conn.currentPath?.remoteEndpoint)
            }
            // Always re-arm for more data
            if conn.state == .ready {
                self.receiveLoop(on: conn)
            }
        }
    }
}
