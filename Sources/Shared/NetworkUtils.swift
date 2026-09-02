import Foundation

public enum NetworkUtils {
    /// Returns the local IPv4 address of a non-loopback network interface.
    /// Prefers en0 (WiFi) and en1 (Ethernet), but falls back to any active "en" interface
    /// to support USB-C adapters and other configurations.
    public static func getLocalIP() -> String? {
        var ifaddr: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&ifaddr) == 0, let firstAddr = ifaddr else { return nil }
        defer { freeifaddrs(ifaddr) }

        var preferred: String?
        var fallback: String?

        for ptr in sequence(first: firstAddr, next: { $0.pointee.ifa_next }) {
            guard let addr = ptr.pointee.ifa_addr else { continue }
            let sa = addr.pointee
            guard sa.sa_family == UInt8(AF_INET) else { continue }
            let name = String(cString: ptr.pointee.ifa_name)
            guard name.hasPrefix("en") else { continue }

            var hostname = [CChar](repeating: 0, count: Int(NI_MAXHOST))
            getnameinfo(addr, socklen_t(sa.sa_len),
                        &hostname, socklen_t(hostname.count), nil, 0, NI_NUMERICHOST)
            let ip = String(cString: hostname)

            if name == "en0" || name == "en1" {
                preferred = ip
                break
            } else if fallback == nil {
                fallback = ip
            }
        }
        return preferred ?? fallback
    }
}
