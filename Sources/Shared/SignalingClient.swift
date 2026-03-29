import Foundation

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

public class SignalingClient {
    private let pin: String
    private let role: String
    private var webSocketTask: URLSessionWebSocketTask?
    private var isSearching = false
    private var pendingContinuation: CheckedContinuation<SessionCandidate, Error>?
    private var activeTask: URLSessionDataTask?
    private let lock = NSLock()

    public init(pin: String, role: String) {
        self.pin = pin
        self.role = role
    }

    deinit { stop() }

    public func exchangeCandidate(_ local: SessionCandidate) async throws -> SessionCandidate {
        // Post our candidate to ntfy.sh/<pin>
        let topicURL = URL(string: "https://ntfy.sh/eclipticrd-\(pin)")!

        let encoder = JSONEncoder()
        let payload = try encoder.encode(local)

        var postRequest = URLRequest(url: topicURL)
        postRequest.httpMethod = "POST"
        postRequest.httpBody = payload
        postRequest.setValue("application/json", forHTTPHeaderField: "Content-Type")

        ERDLog.network("[Signaling] Posting candidate to ntfy.sh/eclipticrd-\(pin)")
        let _ = try await URLSession.shared.data(for: postRequest)

        // Subscribe via SSE (Server-Sent Events) and wait for peer's candidate
        return try await withCheckedThrowingContinuation { continuation in
            lock.lock()
            isSearching = true
            pendingContinuation = continuation
            lock.unlock()

            var sseURL = URLComponents(url: topicURL.appendingPathComponent("json"), resolvingAgainstBaseURL: false)!
            sseURL.queryItems = [URLQueryItem(name: "poll", value: "1"), URLQueryItem(name: "since", value: "30s")]

            var request = URLRequest(url: sseURL.url!)
            request.timeoutInterval = 30

            let task = URLSession.shared.dataTask(with: request) { [weak self] data, _, error in
                guard let self = self else { return }

                self.lock.lock()
                guard self.isSearching else {
                    // Already cancelled via stop() — continuation was already resumed there
                    self.lock.unlock()
                    return
                }
                self.isSearching = false
                self.pendingContinuation = nil
                self.lock.unlock()

                if let error = error {
                    continuation.resume(throwing: error)
                    return
                }
                guard let data = data else {
                    continuation.resume(throwing: NSError(domain: "Signaling", code: 1, userInfo: [NSLocalizedDescriptionKey: "Signaling timeout waiting for peer"]))
                    return
                }

                // Parse JSON lines from ntfy.sh response
                let lines = String(data: data, encoding: .utf8)?.split(separator: "\n") ?? []
                let decoder = JSONDecoder()

                for line in lines.reversed() {
                    // Each line is a ntfy message JSON containing a "message" field
                    if let lineData = line.data(using: .utf8),
                       let ntfyMessage = try? JSONSerialization.jsonObject(with: lineData) as? [String: Any],
                       let messageStr = ntfyMessage["message"] as? String,
                       let messageData = messageStr.data(using: .utf8),
                       let candidate = try? decoder.decode(SessionCandidate.self, from: messageData),
                       candidate.role != self.role {
                        continuation.resume(returning: candidate)
                        return
                    }
                }

                continuation.resume(throwing: NSError(domain: "Signaling", code: 2, userInfo: [NSLocalizedDescriptionKey: "Signaling timeout waiting for peer"]))
            }
            activeTask = task
            task.resume()
        }
    }

    public func stop() {
        lock.lock()
        isSearching = false
        activeTask?.cancel()
        activeTask = nil
        let continuation = pendingContinuation
        pendingContinuation = nil
        lock.unlock()

        // Resume outside lock to avoid potential deadlock
        continuation?.resume(throwing: CancellationError())

        webSocketTask?.cancel(with: .goingAway, reason: nil)
        webSocketTask = nil
    }
}
