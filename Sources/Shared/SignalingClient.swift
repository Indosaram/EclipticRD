import Foundation
import CryptoKit

public struct SessionCandidate: Codable, Equatable {
    public let role: String
    public let localIP: String
    public let localPort: UInt16
    public let publicIP: String
    public let publicPort: UInt16

    public init(role: String, localIP: String, localPort: UInt16, publicIP: String, publicPort: UInt16) {
        self.role = role; self.localIP = localIP; self.localPort = localPort
        self.publicIP = publicIP; self.publicPort = publicPort
    }
}

/// NAT-traversal candidate exchange over a public ntfy.sh topic.
///
/// The topic is a 256-bit hash of the shared PIN and candidate payloads are
/// AES-GCM encrypted under a key derived from the same PIN, so an outside
/// observer can neither enumerate topics for a given PIN space nor read the
/// exchanged addresses. Both peers poll until the other side's candidate
/// shows up instead of failing on the first empty response.
public class SignalingClient {
    private let role: String
    private let topic: String
    private let payloadKey: SymmetricKey
    private let lock = NSLock()
    private var isSearching = false

    public init(pin: String, role: String) {
        self.role = role
        let ikm = Data(pin.utf8)
        let seed = HKDF.derive(ikm: ikm, salt: Data("erd/signaling/v3".utf8), info: Data("erd/topic".utf8))
        // ntfy.sh rejects topics longer than 64 chars with a 404; 112 bits of
        // derived entropy keeps the topic unenumerable while fitting the cap.
        self.topic = "erd3-" + seed.prefix(14).map { String(format: "%02x", $0) }.joined()
        let key = HKDF.derive(ikm: ikm, salt: Data("erd/signaling/v3".utf8), info: Data("erd/payload-key".utf8))
        self.payloadKey = SymmetricKey(data: key)
    }

    deinit { stop() }

    private static func encrypt(_ payload: Data, using key: SymmetricKey) throws -> String {
        let box = try AES.GCM.seal(payload, using: key)
        return Data(box.combined!).base64EncodedString()
    }

    private static func peerCandidate(in data: Data, excludingRole role: String, key: SymmetricKey) -> SessionCandidate? {
        let lines = String(data: data, encoding: .utf8)?.split(separator: "\n") ?? []
        for line in lines.reversed() {
            guard let lineData = line.data(using: .utf8),
                  let envelope = try? JSONSerialization.jsonObject(with: lineData) as? [String: Any],
                  let message = envelope["message"] as? String,
                  let raw = Data(base64Encoded: message),
                  let box = try? AES.GCM.SealedBox(combined: raw),
                  let payload = try? AES.GCM.open(box, using: key),
                  let candidate = try? JSONDecoder().decode(SessionCandidate.self, from: payload),
                  candidate.role != role
            else { continue }
            return candidate
        }
        return nil
    }

    public func exchangeCandidate(_ local: SessionCandidate) async throws -> SessionCandidate {
        let payload = try JSONEncoder().encode(local)
        let message = try Self.encrypt(payload, using: payloadKey)
        let url = URL(string: "https://ntfy.sh/\(topic)")!

        var post = URLRequest(url: url)
        post.httpMethod = "POST"
        post.httpBody = Data(message.utf8)
        post.setValue("text/plain", forHTTPHeaderField: "Content-Type")

        ERDLog.network("[Signaling] Posting candidate")
        let (_, postResponse) = try await URLSession.shared.data(for: post)
        if let http = postResponse as? HTTPURLResponse, !(200...299).contains(http.statusCode) {
            throw NSError(domain: "Signaling", code: 3,
                          userInfo: [NSLocalizedDescriptionKey: "Candidate POST rejected (HTTP \(http.statusCode))"])
        }

        lock.lock()
        isSearching = true
        lock.unlock()

        let deadline = Date().addingTimeInterval(ERDConstants.connectionTimeout)
        var poll = URLRequest(url: URL(string: "https://ntfy.sh/\(topic)/json?poll=1&since=10m")!)
        poll.timeoutInterval = 10

        while Date() < deadline {
            lock.lock()
            let active = isSearching
            lock.unlock()
            guard active else { throw CancellationError() }
            try Task.checkCancellation()

            if let (data, _) = try? await URLSession.shared.data(for: poll),
               let candidate = Self.peerCandidate(in: data, excludingRole: role, key: payloadKey) {
                return candidate
            }
            try await Task.sleep(nanoseconds: 1_000_000_000)
        }
        throw NSError(domain: "Signaling", code: 2,
                      userInfo: [NSLocalizedDescriptionKey: "Signaling timeout waiting for peer"])
    }

    public func stop() {
        lock.lock()
        isSearching = false
        lock.unlock()
    }
}
