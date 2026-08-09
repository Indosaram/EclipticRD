import Foundation

public enum ControlMessageType: UInt8 {
    case requestKeyFrame = 0
    case startStream = 1
    case stopStream = 2
    case disconnect = 3
    case ping = 4
    case pong = 5
    case bitrateAdjust = 6
    case streamConfigRequest = 7
    case streamConfigResponse = 8
    case streamConfigReject = 9
    case streamConfigError = 10
    case clipboardSyncRequest = 11
    case clipboardSyncUpdate = 12
    case clipboardSyncError = 13
}

public struct ControlMessage {
    public let type: ControlMessageType
    public let payload: Data?

    public init(type: ControlMessageType, payload: Data? = nil) {
        self.type = type
        self.payload = payload
    }

    public func serialize() -> Data {
        var d = Data([type.rawValue])
        if let payload = payload {
            d.append(payload)
        }
        return d
    }

    public static func deserialize(from data: Data) -> ControlMessage? {
        guard data.count >= 1, let t = ControlMessageType(rawValue: data[0]) else { return nil }
        let payload: Data? = data.count > 1 ? data.subdata(in: 1..<data.count) : nil
        return ControlMessage(type: t, payload: payload)
    }
}

public struct BitrateAdjustPayload {
    public let targetBitrate: Int32

    public static let size = 4

    public init(targetBitrate: Int32) {
        self.targetBitrate = targetBitrate
    }

    public func serialize() -> Data {
        var d = Data(capacity: BitrateAdjustPayload.size)
        var br = targetBitrate.littleEndian
        d.append(Data(bytes: &br, count: 4))
        return d
    }

    public static func deserialize(from data: Data) -> BitrateAdjustPayload? {
        guard data.count >= size else { return nil }
        let br = data.withUnsafeBytes { $0.load(as: Int32.self).littleEndian }
        return BitrateAdjustPayload(targetBitrate: br)
    }
}

public struct CursorUpdate {
    public let x: Float
    public let y: Float
    public let cursorType: UInt8

    public static let size = 9

    public init(x: Float, y: Float, cursorType: UInt8 = 0) {
        self.x = x; self.y = y; self.cursorType = cursorType
    }

    public func serialize() -> Data {
        var d = Data(capacity: CursorUpdate.size)
        var fx = x.bitPattern.littleEndian; d.append(Data(bytes: &fx, count: 4))
        var fy = y.bitPattern.littleEndian; d.append(Data(bytes: &fy, count: 4))
        var ct = cursorType; d.append(Data(bytes: &ct, count: 1))
        return d
    }

    public static func deserialize(from data: Data) -> CursorUpdate? {
        guard data.count >= size else { return nil }
        let x = Float(bitPattern: data.subdata(in: 0..<4).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian })
        let y = Float(bitPattern: data.subdata(in: 4..<8).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian })
        return CursorUpdate(x: x, y: y, cursorType: data[8])
    }
}

public class AtomicBool {
    private let lock = NSLock()
    private var _value: Bool

    public init(_ value: Bool = false) { _value = value }

    public var value: Bool {
        get { lock.lock(); defer { lock.unlock() }; return _value }
        set { lock.lock(); defer { lock.unlock() }; _value = newValue }
    }
}
