import Foundation

public struct HandshakeCapabilities: OptionSet {
    public let rawValue: UInt64

    public init(rawValue: UInt64) {
        self.rawValue = rawValue
    }

    public static let streamConfiguration = HandshakeCapabilities(rawValue: 1 << 0)
    public static let clipboardSync = HandshakeCapabilities(rawValue: 1 << 1)
    public static let textClipboardSync = HandshakeCapabilities(rawValue: 1 << 2)
}

public struct HandshakePayload {
    public let hostname: String
    public let screenWidth: UInt16
    public let screenHeight: UInt16
    public let scaleFactor: Float
    public let protocolVersion: UInt8
    public let capabilities: HandshakeCapabilities
    public let pairingID: String
    public let sessionSalt: Data

    public init(hostname: String, screenWidth: UInt16, screenHeight: UInt16, scaleFactor: Float, protocolVersion: UInt8 = ERDConstants.protocolVersion, capabilities: HandshakeCapabilities = [], pairingID: String = "", sessionSalt: Data = Data()) {
        self.hostname = hostname
        self.screenWidth = screenWidth
        self.screenHeight = screenHeight
        self.scaleFactor = scaleFactor
        self.protocolVersion = protocolVersion
        self.capabilities = capabilities
        self.pairingID = pairingID
        self.sessionSalt = sessionSalt
    }

    public func serialize() -> Data {
        var data = Data()
        let nameData = hostname.data(using: .utf8) ?? Data()
        var nameLen = UInt16(nameData.count).littleEndian; data.append(Data(bytes: &nameLen, count: 2))
        data.append(nameData)
        var w = screenWidth.littleEndian; data.append(Data(bytes: &w, count: 2))
        var h = screenHeight.littleEndian; data.append(Data(bytes: &h, count: 2))
        var s = scaleFactor.bitPattern.littleEndian; data.append(Data(bytes: &s, count: 4))
        var version = protocolVersion; data.append(Data(bytes: &version, count: 1))
        var flags = capabilities.rawValue.littleEndian; data.append(Data(bytes: &flags, count: 8))
        let idData = Data(pairingID.utf8).prefix(256)
        var idLen = UInt16(idData.count).littleEndian; data.append(Data(bytes: &idLen, count: 2))
        data.append(idData)
        data.append(sessionSalt.prefix(16))
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
        offset += 4

        var protocolVersion = ERDConstants.legacyProtocolVersion
        var capabilities: HandshakeCapabilities = []
        var pairingID = ""
        var sessionSalt = Data()
        if data.count >= offset + 9 {
            protocolVersion = data[offset]
            let flags = data.subdata(in: (offset + 1)..<(offset + 9)).withUnsafeBytes { $0.load(as: UInt64.self).littleEndian }
            capabilities = HandshakeCapabilities(rawValue: flags)
            offset += 9
            if data.count >= offset + 2 {
                let idLen = Int(data.subdata(in: offset..<(offset + 2)).withUnsafeBytes { $0.load(as: UInt16.self).littleEndian })
                offset += 2
                guard idLen <= 256, data.count >= offset + idLen else { return nil }
                pairingID = String(data: data.subdata(in: offset..<(offset + idLen)), encoding: .utf8) ?? ""
                offset += idLen
                if data.count >= offset + 16 {
                    sessionSalt = data.subdata(in: offset..<(offset + 16))
                }
            }
        }

        return HandshakePayload(hostname: hostname, screenWidth: w, screenHeight: h, scaleFactor: s, protocolVersion: protocolVersion, capabilities: capabilities, pairingID: pairingID, sessionSalt: sessionSalt)
    }
}
