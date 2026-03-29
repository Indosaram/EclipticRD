import Foundation
import os

public enum ERDLog {
    private static let subsystem = "com.indo.EclipticRD"

    private static let generalLog = OSLog(subsystem: subsystem, category: "general")
    private static let networkLog = OSLog(subsystem: subsystem, category: "network")
    private static let videoLog   = OSLog(subsystem: subsystem, category: "video")

    public static func info(_ message: String) {
        os_log(.info, log: generalLog, "%{public}@", message)
    }

    public static func error(_ message: String) {
        os_log(.error, log: generalLog, "%{public}@", message)
    }

    public static func debug(_ message: String) {
        os_log(.debug, log: generalLog, "%{public}@", message)
    }

    public static func warning(_ message: String) {
        os_log(.default, log: generalLog, "%{public}@", message)
    }

    public static func network(_ message: String) {
        os_log(.info, log: networkLog, "%{public}@", message)
    }

    public static func video(_ message: String) {
        os_log(.info, log: videoLog, "%{public}@", message)
    }
}
