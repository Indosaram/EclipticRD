import Foundation
import Network

public class STUNClient {
    private let serverHost: String
    private let serverPort: UInt16

    public init(serverHost: String = "stun.l.google.com", serverPort: UInt16 = 19302) {
        self.serverHost = serverHost
        self.serverPort = serverPort
    }

    public func fetchPublicIP() async throws -> (String, UInt16) {
        return try await withCheckedThrowingContinuation { continuation in
            let queue = DispatchQueue(label: "eclipticrd.stun")
            let endpoint = NWEndpoint.hostPort(host: NWEndpoint.Host(serverHost), port: NWEndpoint.Port(rawValue: serverPort)!)
            let params = NWParameters.udp
            if let ip = params.defaultProtocolStack.internetProtocol as? NWProtocolIP.Options {
                ip.version = .v4
            }
            let conn = NWConnection(to: endpoint, using: params)
            var completed = false

            func finish(result: Result<(String, UInt16), Error>) {
                guard !completed else { return }
                completed = true
                conn.cancel()
                switch result {
                case .success(let v): continuation.resume(returning: v)
                case .failure(let e): continuation.resume(throwing: e)
                }
            }

            conn.stateUpdateHandler = { [self] state in
                switch state {
                case .ready:
                    var request = Data(count: 20)
                    request[0] = 0x00; request[1] = 0x01
                    request[4] = 0x21; request[5] = 0x12; request[6] = 0xA4; request[7] = 0x42
                    for i in 8..<20 { request[i] = UInt8.random(in: 0...255) }

                    conn.send(content: request, completion: .contentProcessed { _ in })

                    conn.receiveMessage { data, _, _, error in
                        if let error = error {
                            finish(result: .failure(error))
                            return
                        }
                        guard let data = data, data.count >= 20 else {
                            finish(result: .failure(NSError(domain: "STUN", code: 1, userInfo: [NSLocalizedDescriptionKey: "Empty STUN response"])))
                            return
                        }
                        if let (ip, port) = Self.parseMappedAddress(from: data) {
                            finish(result: .success((ip, port)))
                        } else {
                            finish(result: .failure(NSError(domain: "STUN", code: 2, userInfo: [NSLocalizedDescriptionKey: "MAPPED-ADDRESS not found in STUN response"])))
                        }
                    }
                case .failed(let error):
                    finish(result: .failure(error))
                default:
                    break
                }
            }

            conn.start(queue: queue)

            queue.asyncAfter(deadline: .now() + 5.0) {
                finish(result: .failure(NSError(domain: "STUN", code: 3, userInfo: [NSLocalizedDescriptionKey: "STUN timeout"])))
            }
        }
    }

    private static func parseMappedAddress(from data: Data) -> (String, UInt16)? {
        let magicCookie: UInt32 = 0x2112A442
        var offset = 20 // Skip header

        while offset + 4 <= data.count {
            let attrType = UInt16(data[offset]) << 8 | UInt16(data[offset+1])
            let attrLen = Int(UInt16(data[offset+2]) << 8 | UInt16(data[offset+3]))
            offset += 4

            guard offset + attrLen <= data.count else { break }

            if attrType == 0x0020 { // XOR-MAPPED-ADDRESS
                guard attrLen >= 8 else { break }
                let family = data[offset + 1]
                let xPort = (UInt16(data[offset+2]) << 8 | UInt16(data[offset+3])) ^ UInt16(magicCookie >> 16)

                if family == 0x01 { // IPv4
                    let xAddr = UInt32(data[offset+4]) << 24 | UInt32(data[offset+5]) << 16 |
                                UInt32(data[offset+6]) << 8  | UInt32(data[offset+7])
                    let addr = xAddr ^ magicCookie
                    let ip = "\(addr >> 24 & 0xFF).\(addr >> 16 & 0xFF).\(addr >> 8 & 0xFF).\(addr & 0xFF)"
                    return (ip, xPort)
                }
            } else if attrType == 0x0001 { // MAPPED-ADDRESS (fallback)
                guard attrLen >= 8 else { break }
                let family = data[offset + 1]
                let port = UInt16(data[offset+2]) << 8 | UInt16(data[offset+3])

                if family == 0x01 {
                    let ip = "\(data[offset+4]).\(data[offset+5]).\(data[offset+6]).\(data[offset+7])"
                    return (ip, port)
                }
            }

            // Attributes are padded to 4-byte boundaries
            offset += (attrLen + 3) & ~3
        }
        return nil
    }
}
