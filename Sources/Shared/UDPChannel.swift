import Foundation
import Network

public class UDPChannel {
    private var connection: NWConnection?
    private var listener: NWListener?
    private let queue = DispatchQueue(label: "eclipticrd.udp", qos: .userInteractive)
    private static let queueKey = DispatchSpecificKey<Bool>()
    private var sequenceCounter: UInt32 = 0
    private let seqLock = NSLock()
    private let cipherLock = NSLock()
    private var _sendCipher: DatagramCipher?
    private var _receiveCipher: DatagramCipher?
    private var isReceiving = false

    public var onReceive: (@Sendable (Data, NWEndpoint?) -> Void)?
    public var onReady: (@Sendable () -> Void)?

    private let _isReady = AtomicBool(false)
    public var isReady: Bool { _isReady.value }

    /// Direction-separated traffic ciphers: datagrams we emit are sealed with
    /// `sendCipher`, incoming ones opened with `receiveCipher`. Distinct keys
    /// keep the AES-GCM nonce spaces independent per direction.
    public var sendCipher: DatagramCipher? {
        get { cipherLock.lock(); defer { cipherLock.unlock() }; return _sendCipher }
        set { cipherLock.lock(); _sendCipher = newValue; cipherLock.unlock() }
    }

    public var receiveCipher: DatagramCipher? {
        get { cipherLock.lock(); defer { cipherLock.unlock() }; return _receiveCipher }
        set { cipherLock.lock(); _receiveCipher = newValue; cipherLock.unlock() }
    }

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
                self?.reconcileReceiving(conn, state: state)
            }
            conn.start(queue: self.queue)
            self.armReceiving(on: conn)
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
                self?.reconcileReceiving(conn, state: state)
            }
            conn.start(queue: self.queue)
            self.armReceiving(on: conn)
        }
    }

    // MARK: - Send

    public func send(_ data: Data, to endpoint: NWEndpoint? = nil) {
        guard let conn = connection else { return }
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
        if let cipher = sendCipher {
            guard let sealed = cipher.seal(payload, aad: packet) else {
                ERDLog.error("[UDP] Failed to seal \(type) packet")
                return
            }
            packet.append(sealed)
        } else {
            packet.append(payload)
        }
        send(packet, to: endpoint)
    }

    /// Send a small datagram to trigger the other side's listener to register us
    public func sendPing() {
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
            self.isReceiving = false
            self._isReady.value = false
        }

        if DispatchQueue.getSpecific(key: Self.queueKey) == true {
            work()
        } else {
            queue.sync { work() }
        }
    }

    // MARK: - Internal

    private func reconcileReceiving(_ conn: NWConnection, state: NWConnection.State) {
        guard case .ready = state else { return }
        armReceiving(on: conn)
    }

    private func armReceiving(on conn: NWConnection) {
        let work = {
            guard self.connection === conn, !self.isReceiving else { return }
            self.isReceiving = true
            self.receiveLoop(on: conn)
        }
        if DispatchQueue.getSpecific(key: Self.queueKey) == true {
            work()
        } else {
            queue.async(execute: work)
        }
    }

    private func receiveLoop(on conn: NWConnection) {
        conn.receiveMessage { [weak self] content, _, _, error in
            guard let self = self else { return }
            if let data = content, data.count > 1 {
                if let packet = self.decodeDatagram(data) {
                    self.onReceive?(packet, conn.currentPath?.remoteEndpoint)
                }
            }
            if let error = error {
                ERDLog.error("[UDP] Receive error: \(error)")
            }
            // receiveMessage reports isComplete == true after datagrams on
            // listener-created UDP connections; only a dead connection may
            // stop the loop.
            let finish = {
                self.isReceiving = false
                switch conn.state {
                case .failed, .cancelled:
                    return
                default:
                    self.armReceiving(on: conn)
                }
            }
            if DispatchQueue.getSpecific(key: Self.queueKey) == true {
                finish()
            } else {
                self.queue.async(execute: finish)
            }
        }
    }

    private func decodeDatagram(_ data: Data) -> Data? {
        guard let cipher = receiveCipher else { return data }
        guard data.count > ERDConstants.packetHeaderSize,
              PacketHeader.deserialize(from: data.prefix(ERDConstants.packetHeaderSize)) != nil
        else { return nil }
        let header = data.prefix(ERDConstants.packetHeaderSize)
        guard let plaintext = cipher.open(data.subdata(in: ERDConstants.packetHeaderSize..<data.count),
                                          aad: Data(header))
        else { return nil }
        return header + plaintext
    }
}
