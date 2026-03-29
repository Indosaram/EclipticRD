import Foundation

public enum ControlMessageType: UInt8 {
    case requestKeyFrame = 0
    case startStream = 1
    case stopStream = 2
    case disconnect = 3
    case ping = 4
}

public struct ControlMessage {
    public let type: ControlMessageType

    public init(type: ControlMessageType) {
        self.type = type
    }

    public func serialize() -> Data {
        return Data([type.rawValue])
    }

    public static func deserialize(from data: Data) -> ControlMessage? {
        guard data.count >= 1, let t = ControlMessageType(rawValue: data[0]) else { return nil }
        return ControlMessage(type: t)
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
