import Foundation

public struct HandshakePayload {
    public let hostname: String
    public let screenWidth: UInt16
    public let screenHeight: UInt16
    public let scaleFactor: Float

    public init(hostname: String, screenWidth: UInt16, screenHeight: UInt16, scaleFactor: Float) {
        self.hostname = hostname
        self.screenWidth = screenWidth
        self.screenHeight = screenHeight
        self.scaleFactor = scaleFactor
    }

    public func serialize() -> Data {
        var data = Data()
        let nameData = hostname.data(using: .utf8) ?? Data()
        var nameLen = UInt16(nameData.count).littleEndian; data.append(Data(bytes: &nameLen, count: 2))
        data.append(nameData)
        var w = screenWidth.littleEndian;  data.append(Data(bytes: &w, count: 2))
        var h = screenHeight.littleEndian; data.append(Data(bytes: &h, count: 2))
        var s = scaleFactor.bitPattern.littleEndian; data.append(Data(bytes: &s, count: 4))
        return data
    }

    public static func deserialize(from data: Data) -> HandshakePayload? {
        guard data.count >= 2 else { return nil }
        var offset = 0
        let nameLen = Int(data.subdata(in: offset..<offset+2).withUnsafeBytes { $0.load(as: UInt16.self).littleEndian })
        offset += 2
        guard nameLen <= 1024, data.count >= offset + nameLen + 8 else { return nil }
        let hostname = String(data: data.subdata(in: offset..<offset+nameLen), encoding: .utf8) ?? ""
        offset += nameLen
        let w = data.subdata(in: offset..<offset+2).withUnsafeBytes { $0.load(as: UInt16.self).littleEndian }; offset += 2
        let h = data.subdata(in: offset..<offset+2).withUnsafeBytes { $0.load(as: UInt16.self).littleEndian }; offset += 2
        let sBits = data.subdata(in: offset..<offset+4).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian }
        let s = Float(bitPattern: sBits)
        return HandshakePayload(hostname: hostname, screenWidth: w, screenHeight: h, scaleFactor: s)
    }
}
