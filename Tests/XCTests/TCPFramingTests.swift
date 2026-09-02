import XCTest
import Network

// TCPChannel length-prefix framing under adversarial segment timing: frames
// coalesced into one segment slice into discrete deliveries; a frame arriving
// in fragments delivers exactly once, only when fully buffered.
final class TCPFramingTests: XCTestCase {
    private final class Receiver {
        let done = DispatchSemaphore(value: 0)
        private let lock = NSLock()
        private var _payloads: [Data] = []
        var payloads: [Data] {
            lock.lock(); defer { lock.unlock() }
            return _payloads
        }
        func submit(_ data: Data) {
            lock.lock()
            _payloads.append(data)
            let count = _payloads.count
            lock.unlock()
            done.signal()
            _ = count
        }
        func waitAndDrain(expected: Int, timeout: TimeInterval) -> Bool {
            for _ in 0..<expected {
                if done.wait(timeout: .now() + timeout) == .timedOut { return false }
            }
            return true
        }
    }

    private static func framed(_ packet: Data) -> Data {
        var wire = Data()
        var len = UInt32(packet.count).littleEndian
        wire.append(Data(bytes: &len, count: 4))
        wire.append(packet)
        return wire
    }

    private func rawConnection(port: UInt16) -> NWConnection {
        let conn = NWConnection(host: "127.0.0.1",
                                port: NWEndpoint.Port(rawValue: port)!,
                                using: .tcp)
        let ready = DispatchSemaphore(value: 0)
        conn.stateUpdateHandler = { state in
            if case .ready = state { ready.signal() }
        }
        conn.start(queue: DispatchQueue(label: "eclipticrd.test.raw"))
        XCTAssertEqual(ready.wait(timeout: .now() + 3), .success)
        Thread.sleep(forTimeInterval: 0.1)
        return conn
    }

    func testCoalescedFramesDeliveredDiscretely() throws {
        let port: UInt16 = 19790
        let server = TCPChannel()
        let receiver = Receiver()
        try server.startListening(port: port)
        server.onReceive = { receiver.submit($0) }
        Thread.sleep(forTimeInterval: 0.2)

        let raw = rawConnection(port: port)
        let packets = (1...3).map { i -> Data in
            PacketHeader(type: .ping, sequence: UInt32(i), timestamp: 0).serialize()
                + Data("payload-\(i)".utf8)
        }
        var wire = Data()
        packets.forEach { wire.append(Self.framed($0)) }
        raw.send(content: wire, completion: .contentProcessed { _ in })

        XCTAssertTrue(receiver.waitAndDrain(expected: 3, timeout: 3), "coalesced frames were not all delivered")
        XCTAssertEqual(receiver.payloads, packets)
        raw.cancel()
        server.stop()
    }

    func testFragmentedFrameAssembledExactlyOnce() throws {
        let port: UInt16 = 19791
        let server = TCPChannel()
        let receiver = Receiver()
        try server.startListening(port: port)
        server.onReceive = { receiver.submit($0) }
        Thread.sleep(forTimeInterval: 0.2)

        let raw = rawConnection(port: port)
        let packet = PacketHeader(type: .ping, sequence: 42, timestamp: 7).serialize()
            + Data(repeating: 0xAB, count: 5000)
        let wire = Self.framed(packet)

        // Three writes with real gaps force multiple partial server reads.
        raw.send(content: wire[0..<2], completion: .contentProcessed { _ in })
        Thread.sleep(forTimeInterval: 0.15)
        raw.send(content: wire[2..<3000], completion: .contentProcessed { _ in })
        Thread.sleep(forTimeInterval: 0.15)
        raw.send(content: wire[3000...], completion: .contentProcessed { _ in })

        XCTAssertTrue(receiver.waitAndDrain(expected: 1, timeout: 3), "fragmented frame was never assembled")
        Thread.sleep(forTimeInterval: 0.3) // window for any spurious extra delivery
        XCTAssertEqual(receiver.payloads.count, 1)
        XCTAssertEqual(receiver.payloads.first, packet)
        raw.cancel()
        server.stop()
    }
}
