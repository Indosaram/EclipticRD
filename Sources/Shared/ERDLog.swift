import Foundation
import os

public enum ERDLog {
    private static let subsystem = "com.indo.EclipticRD"

    private static let generalLog = OSLog(subsystem: subsystem, category: "general")
    private static let networkLog = OSLog(subsystem: subsystem, category: "network")
    private static let videoLog   = OSLog(subsystem: subsystem, category: "video")

    // Set ERD_CONSOLE_LOG=1 to mirror log lines to stdout (E2E runner uses this).
    private static func console(_ message: String) {
        if ProcessInfo.processInfo.environment["ERD_CONSOLE_LOG"] == "1" {
            print(message)
        }
    }

    public static func info(_ message: String) {
        console(message)
        os_log(.info, log: generalLog, "%{private}@", message)
    }

    public static func error(_ message: String) {
        console(message)
        os_log(.error, log: generalLog, "%{public}@", message)
    }

    public static func debug(_ message: String) {
        console(message)
        os_log(.debug, log: generalLog, "%{private}@", message)
    }

    public static func warning(_ message: String) {
        console(message)
        os_log(.default, log: generalLog, "%{public}@", message)
    }

    public static func network(_ message: String) {
        console(message)
        os_log(.info, log: networkLog, "%{private}@", message)
    }

    public static func video(_ message: String) {
        console(message)
        os_log(.info, log: videoLog, "%{public}@", message)
    }
}
