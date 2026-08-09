import Foundation

public enum PacketType: UInt8 {
    case handshake = 0
    case handshakeAck = 1
    case frameHeader = 2
    case frameChunk = 3
    case cursorUpdate = 4
    case inputEvent = 5
    case control = 6
    case ping = 7
    case audioFrame = 8
}

public struct PacketHeader {
    public let magic: UInt16
    public let type: PacketType
    public let sequence: UInt32
    public let timestamp: UInt32
    public let flags: UInt8

    public init(type: PacketType, sequence: UInt32, timestamp: UInt32, flags: UInt8 = 0) {
        self.magic = ERDConstants.magic
        self.type = type
        self.sequence = sequence
        self.timestamp = timestamp
        self.flags = flags
    }

    public func serialize() -> Data {
        var data = Data(capacity: ERDConstants.packetHeaderSize)
        var m = magic.littleEndian; data.append(Data(bytes: &m, count: 2))
        var t = type.rawValue;      data.append(Data(bytes: &t, count: 1))
        var s = sequence.littleEndian; data.append(Data(bytes: &s, count: 4))
        var ts = timestamp.littleEndian; data.append(Data(bytes: &ts, count: 4))
        var f = flags;              data.append(Data(bytes: &f, count: 1))
        return data
    }

    public static func deserialize(from data: Data) -> PacketHeader? {
        guard data.count >= ERDConstants.packetHeaderSize else { return nil }
        let magic = data.subdata(in: 0..<2).withUnsafeBytes { $0.load(as: UInt16.self).littleEndian }
        guard magic == ERDConstants.magic else { return nil }
        guard let type = PacketType(rawValue: data[2]) else { return nil }
        let seq = data.subdata(in: 3..<7).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian }
        let ts = data.subdata(in: 7..<11).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian }
        let flags = data[11]
        return PacketHeader(type: type, sequence: seq, timestamp: ts, flags: flags)
    }
}
