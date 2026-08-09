import Foundation
import Network

public class BonjourBrowser {
    public struct DiscoveredHost: Identifiable, Hashable, Equatable {
        public let id: String
        public let name: String
        public let endpoint: NWEndpoint

        public init(id: String, name: String, endpoint: NWEndpoint) {
            self.id = id; self.name = name; self.endpoint = endpoint
        }

        public var endpointDescription: String {
            switch endpoint {
            case .service(_, let type, let domain, _):
                return "\(type) (\(domain))"
            case .hostPort(let host, let port):
                return "\(host):\(port)"
            default:
                return "\(endpoint)"
            }
        }

        public func hash(into hasher: inout Hasher) { hasher.combine(id) }
        public static func == (lhs: DiscoveredHost, rhs: DiscoveredHost) -> Bool { lhs.id == rhs.id }
    }

    private var browser: NWBrowser?
    private let queue = DispatchQueue(label: "eclipticrd.bonjour")
    private var hosts: [DiscoveredHost] = []

    public var onHostsChanged: (@Sendable ([DiscoveredHost]) -> Void)?

    public init() {}

    public func startBrowsing() {
        let params = NWParameters()
        params.includePeerToPeer = true
        let descriptor = NWBrowser.Descriptor.bonjour(type: ERDConstants.bonjourServiceType, domain: ERDConstants.bonjourDomain)
        browser = NWBrowser(for: descriptor, using: params)

        browser?.browseResultsChangedHandler = { [weak self] results, _ in
            guard let self = self else { return }
            self.hosts = results.compactMap { result in
                if case .service(let name, _, _, _) = result.endpoint {
                    return DiscoveredHost(id: name, name: name, endpoint: result.endpoint)
                }
                return nil
            }
            ERDLog.network("[Bonjour] Found \(self.hosts.count) hosts")
            self.onHostsChanged?(self.hosts)
        }

        browser?.stateUpdateHandler = { state in
            if case .failed(let err) = state { ERDLog.error("[Bonjour] Browser failed: \(err)") }
        }

        browser?.start(queue: queue)
        ERDLog.network("[Bonjour] Browsing for services...")
    }

    public func stopBrowsing() {
        // Synchronize on queue to avoid data race with browseResultsChangedHandler
        queue.sync {
            browser?.cancel()
            browser = nil
            hosts = []
        }
    }
}
