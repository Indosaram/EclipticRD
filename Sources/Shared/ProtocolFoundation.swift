import Foundation

public struct StreamConfiguration {
    public let width: UInt32
    public let height: UInt32
    public let bitrate: UInt32
    public let framesPerSecond: UInt16

    public static let size = 14

    public init(width: UInt32, height: UInt32, bitrate: UInt32, framesPerSecond: UInt16) {
        self.width = width
        self.height = height
        self.bitrate = bitrate
        self.framesPerSecond = framesPerSecond
    }

    public func serialize() -> Data {
        var data = Data(capacity: Self.size)
        var w = width.littleEndian; data.append(Data(bytes: &w, count: 4))
        var h = height.littleEndian; data.append(Data(bytes: &h, count: 4))
        var b = bitrate.littleEndian; data.append(Data(bytes: &b, count: 4))
        var fps = framesPerSecond.littleEndian; data.append(Data(bytes: &fps, count: 2))
        return data
    }

    public static func deserialize(from data: Data) -> StreamConfiguration? {
        guard data.count >= size else { return nil }
        let width = data.subdata(in: 0..<4).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian }
        let height = data.subdata(in: 4..<8).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian }
        let bitrate = data.subdata(in: 8..<12).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian }
        let fps = data.subdata(in: 12..<14).withUnsafeBytes { $0.load(as: UInt16.self).littleEndian }
        return StreamConfiguration(width: width, height: height, bitrate: bitrate, framesPerSecond: fps)
    }
}

public enum StreamConfigurationErrorCode: UInt8 {
    case invalidRequest = 0
    case unsupportedDimensions = 1
    case unsupportedBitrate = 2
    case unsupportedFPS = 3
    case rejectedByPeer = 4
}

public struct StreamConfigurationRequestPayload {
    public let requestID: UInt32
    public let desiredConfiguration: StreamConfiguration

    public static let size = 4 + StreamConfiguration.size

    public init(requestID: UInt32, desiredConfiguration: StreamConfiguration) {
        self.requestID = requestID
        self.desiredConfiguration = desiredConfiguration
    }

    public func serialize() -> Data {
        var data = Data(capacity: Self.size)
        var id = requestID.littleEndian; data.append(Data(bytes: &id, count: 4))
        data.append(desiredConfiguration.serialize())
        return data
    }

    public static func deserialize(from data: Data) -> StreamConfigurationRequestPayload? {
        guard data.count >= size else { return nil }
        let requestID = data.subdata(in: 0..<4).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian }
        guard let desiredConfiguration = StreamConfiguration.deserialize(from: data.subdata(in: 4..<data.count)) else { return nil }
        return StreamConfigurationRequestPayload(requestID: requestID, desiredConfiguration: desiredConfiguration)
    }
}

public struct StreamConfigurationResponsePayload {
    public let requestID: UInt32
    public let activeConfiguration: StreamConfiguration

    public static let size = 4 + StreamConfiguration.size

    public init(requestID: UInt32, activeConfiguration: StreamConfiguration) {
        self.requestID = requestID
        self.activeConfiguration = activeConfiguration
    }

    public func serialize() -> Data {
        var data = Data(capacity: Self.size)
        var id = requestID.littleEndian; data.append(Data(bytes: &id, count: 4))
        data.append(activeConfiguration.serialize())
        return data
    }

    public static func deserialize(from data: Data) -> StreamConfigurationResponsePayload? {
        guard data.count >= size else { return nil }
        let requestID = data.subdata(in: 0..<4).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian }
        guard let activeConfiguration = StreamConfiguration.deserialize(from: data.subdata(in: 4..<data.count)) else { return nil }
        return StreamConfigurationResponsePayload(requestID: requestID, activeConfiguration: activeConfiguration)
    }
}

public struct StreamConfigurationRejectPayload {
    public let requestID: UInt32
    public let reason: StreamConfigurationErrorCode
    public let message: String

    public init(requestID: UInt32, reason: StreamConfigurationErrorCode, message: String) {
        self.requestID = requestID
        self.reason = reason
        self.message = message
    }

    public func serialize() -> Data {
        var data = Data()
        var id = requestID.littleEndian; data.append(Data(bytes: &id, count: 4))
        var code = reason.rawValue; data.append(Data(bytes: &code, count: 1))
        let messageData = message.data(using: .utf8) ?? Data()
        var length = UInt16(min(messageData.count, Int(UInt16.max))).littleEndian
        data.append(Data(bytes: &length, count: 2))
        data.append(messageData.prefix(Int(length)))
        return data
    }

    public static func deserialize(from data: Data) -> StreamConfigurationRejectPayload? {
        guard data.count >= 7 else { return nil }
        let requestID = data.subdata(in: 0..<4).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian }
        guard let reason = StreamConfigurationErrorCode(rawValue: data[4]) else { return nil }
        let length = Int(data.subdata(in: 5..<7).withUnsafeBytes { $0.load(as: UInt16.self).littleEndian })
        guard data.count >= 7 + length else { return nil }
        let message = String(data: data.subdata(in: 7..<(7 + length)), encoding: .utf8) ?? ""
        return StreamConfigurationRejectPayload(requestID: requestID, reason: reason, message: message)
    }
}

public struct StreamConfigurationErrorPayload {
    public let requestID: UInt32
    public let errorCode: StreamConfigurationErrorCode
    public let message: String

    public init(requestID: UInt32, errorCode: StreamConfigurationErrorCode, message: String) {
        self.requestID = requestID
        self.errorCode = errorCode
        self.message = message
    }

    public func serialize() -> Data {
        var data = Data()
        var id = requestID.littleEndian; data.append(Data(bytes: &id, count: 4))
        var code = errorCode.rawValue; data.append(Data(bytes: &code, count: 1))
        let messageData = message.data(using: .utf8) ?? Data()
        var length = UInt16(min(messageData.count, Int(UInt16.max))).littleEndian
        data.append(Data(bytes: &length, count: 2))
        data.append(messageData.prefix(Int(length)))
        return data
    }

    public static func deserialize(from data: Data) -> StreamConfigurationErrorPayload? {
        guard data.count >= 7 else { return nil }
        let requestID = data.subdata(in: 0..<4).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian }
        guard let errorCode = StreamConfigurationErrorCode(rawValue: data[4]) else { return nil }
        let length = Int(data.subdata(in: 5..<7).withUnsafeBytes { $0.load(as: UInt16.self).littleEndian })
        guard data.count >= 7 + length else { return nil }
        let message = String(data: data.subdata(in: 7..<(7 + length)), encoding: .utf8) ?? ""
        return StreamConfigurationErrorPayload(requestID: requestID, errorCode: errorCode, message: message)
    }
}

public enum ClipboardSyncDirection: UInt8 {
    case hostToClient = 0
    case clientToHost = 1
    case bidirectional = 2
}

public enum ClipboardSyncOrigin: UInt8 {
    case localPasteboard = 0
    case remotePasteboard = 1
    case syncedFromPeer = 2
}

public struct ClipboardSyncRequestPayload {
    public let requestID: UInt32
    public let direction: ClipboardSyncDirection
    public let origin: ClipboardSyncOrigin

    public static let size = 6

    public init(requestID: UInt32, direction: ClipboardSyncDirection, origin: ClipboardSyncOrigin) {
        self.requestID = requestID
        self.direction = direction
        self.origin = origin
    }

    public func serialize() -> Data {
        var data = Data(capacity: Self.size)
        var id = requestID.littleEndian; data.append(Data(bytes: &id, count: 4))
        var dir = direction.rawValue; data.append(Data(bytes: &dir, count: 1))
        var org = origin.rawValue; data.append(Data(bytes: &org, count: 1))
        return data
    }

    public static func deserialize(from data: Data) -> ClipboardSyncRequestPayload? {
        guard data.count >= size else { return nil }
        let requestID = data.subdata(in: 0..<4).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian }
        guard let direction = ClipboardSyncDirection(rawValue: data[4]), let origin = ClipboardSyncOrigin(rawValue: data[5]) else { return nil }
        return ClipboardSyncRequestPayload(requestID: requestID, direction: direction, origin: origin)
    }
}

public struct ClipboardSyncUpdatePayload {
    public let requestID: UInt32
    public let direction: ClipboardSyncDirection
    public let origin: ClipboardSyncOrigin
    public let text: String

    public init(requestID: UInt32, direction: ClipboardSyncDirection, origin: ClipboardSyncOrigin, text: String) {
        self.requestID = requestID
        self.direction = direction
        self.origin = origin
        self.text = text
    }

    public func serialize() -> Data? {
        let textData = text.data(using: .utf8) ?? Data()
        guard textData.count <= ERDConstants.maxClipboardTextBytes else { return nil }
        var data = Data()
        var id = requestID.littleEndian; data.append(Data(bytes: &id, count: 4))
        var dir = direction.rawValue; data.append(Data(bytes: &dir, count: 1))
        var org = origin.rawValue; data.append(Data(bytes: &org, count: 1))
        var length = UInt16(textData.count).littleEndian; data.append(Data(bytes: &length, count: 2))
        data.append(textData)
        return data
    }

    public static func deserialize(from data: Data) -> ClipboardSyncUpdatePayload? {
        guard data.count >= 8 else { return nil }
        let requestID = data.subdata(in: 0..<4).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian }
        guard let direction = ClipboardSyncDirection(rawValue: data[4]), let origin = ClipboardSyncOrigin(rawValue: data[5]) else { return nil }
        let length = Int(data.subdata(in: 6..<8).withUnsafeBytes { $0.load(as: UInt16.self).littleEndian })
        guard length <= ERDConstants.maxClipboardTextBytes, data.count >= 8 + length else { return nil }
        let text = String(data: data.subdata(in: 8..<(8 + length)), encoding: .utf8) ?? ""
        return ClipboardSyncUpdatePayload(requestID: requestID, direction: direction, origin: origin, text: text)
    }
}

public struct ClipboardSyncErrorPayload {
    public let requestID: UInt32
    public let direction: ClipboardSyncDirection
    public let origin: ClipboardSyncOrigin
    public let errorCode: UInt8
    public let message: String

    public init(requestID: UInt32, direction: ClipboardSyncDirection, origin: ClipboardSyncOrigin, errorCode: UInt8, message: String) {
        self.requestID = requestID
        self.direction = direction
        self.origin = origin
        self.errorCode = errorCode
        self.message = message
    }

    public func serialize() -> Data {
        var data = Data()
        var id = requestID.littleEndian; data.append(Data(bytes: &id, count: 4))
        var dir = direction.rawValue; data.append(Data(bytes: &dir, count: 1))
        var org = origin.rawValue; data.append(Data(bytes: &org, count: 1))
        var code = errorCode; data.append(Data(bytes: &code, count: 1))
        let messageData = message.data(using: .utf8) ?? Data()
        var length = UInt16(min(messageData.count, Int(UInt16.max))).littleEndian
        data.append(Data(bytes: &length, count: 2))
        data.append(messageData.prefix(Int(length)))
        return data
    }

    public static func deserialize(from data: Data) -> ClipboardSyncErrorPayload? {
        guard data.count >= 9 else { return nil }
        let requestID = data.subdata(in: 0..<4).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian }
        guard let direction = ClipboardSyncDirection(rawValue: data[4]), let origin = ClipboardSyncOrigin(rawValue: data[5]) else { return nil }
        let errorCode = data[6]
        let length = Int(data.subdata(in: 7..<9).withUnsafeBytes { $0.load(as: UInt16.self).littleEndian })
        guard data.count >= 9 + length else { return nil }
        let message = String(data: data.subdata(in: 9..<(9 + length)), encoding: .utf8) ?? ""
        return ClipboardSyncErrorPayload(requestID: requestID, direction: direction, origin: origin, errorCode: errorCode, message: message)
    }
}
