import Foundation

public enum InputEventType: UInt8 {
    case mouseMove = 0
    case leftMouseDown = 1
    case leftMouseUp = 2
    case rightMouseDown = 3
    case rightMouseUp = 4
    case scrollWheel = 5
    case keyDown = 6
    case keyUp = 7
    case flagsChanged = 8
    case leftMouseDragged = 9
    case rightMouseDragged = 10
}

public struct ModifierFlags: OptionSet {
    public let rawValue: UInt16
    public init(rawValue: UInt16) { self.rawValue = rawValue }

    public static let shift   = ModifierFlags(rawValue: 1 << 0)
    public static let control = ModifierFlags(rawValue: 1 << 1)
    public static let option  = ModifierFlags(rawValue: 1 << 2)
    public static let command = ModifierFlags(rawValue: 1 << 3)
    public static let capsLock = ModifierFlags(rawValue: 1 << 4)
}

public struct InputEventPayload {
    public let type: InputEventType
    public let x: Float
    public let y: Float
    public let keyCode: UInt16
    public let modifiers: ModifierFlags
    public let scrollDeltaX: Float
    public let scrollDeltaY: Float

    public static let size = 21

    public init(type: InputEventType, x: Float, y: Float, keyCode: UInt16 = 0,
                modifiers: ModifierFlags = [], scrollDeltaX: Float = 0, scrollDeltaY: Float = 0) {
        self.type = type; self.x = x; self.y = y; self.keyCode = keyCode
        self.modifiers = modifiers; self.scrollDeltaX = scrollDeltaX; self.scrollDeltaY = scrollDeltaY
    }

    public func serialize() -> Data {
        var d = Data(capacity: InputEventPayload.size)
        var t = type.rawValue; d.append(Data(bytes: &t, count: 1))
        var fx = x.bitPattern.littleEndian; d.append(Data(bytes: &fx, count: 4))
        var fy = y.bitPattern.littleEndian; d.append(Data(bytes: &fy, count: 4))
        var kc = keyCode.littleEndian; d.append(Data(bytes: &kc, count: 2))
        var mf = modifiers.rawValue.littleEndian; d.append(Data(bytes: &mf, count: 2))
        var sdx = scrollDeltaX.bitPattern.littleEndian; d.append(Data(bytes: &sdx, count: 4))
        var sdy = scrollDeltaY.bitPattern.littleEndian; d.append(Data(bytes: &sdy, count: 4))
        return d
    }

    public static func deserialize(from data: Data) -> InputEventPayload? {
        guard data.count >= size else { return nil }
        guard let t = InputEventType(rawValue: data[0]) else { return nil }
        let x = Float(bitPattern: data.subdata(in: 1..<5).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian })
        let y = Float(bitPattern: data.subdata(in: 5..<9).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian })
        let kc = data.subdata(in: 9..<11).withUnsafeBytes { $0.load(as: UInt16.self).littleEndian }
        let mf = ModifierFlags(rawValue: data.subdata(in: 11..<13).withUnsafeBytes { $0.load(as: UInt16.self).littleEndian })
        let sdx = Float(bitPattern: data.subdata(in: 13..<17).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian })
        let sdy = Float(bitPattern: data.subdata(in: 17..<21).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian })
        return InputEventPayload(type: t, x: x, y: y, keyCode: kc, modifiers: mf, scrollDeltaX: sdx, scrollDeltaY: sdy)
    }
}
