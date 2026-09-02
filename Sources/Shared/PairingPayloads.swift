import Foundation

/// Client -> host: request device pairing. Sent over the PIN-protected
/// bootstrap TLS channel before any screen or input capability is granted.
public struct PairingRequestPayload {
    public let hostname: String

    public init(hostname: String) {
        self.hostname = hostname
    }

    public func serialize() -> Data {
        let nameData = Data(hostname.utf8).prefix(Int(UInt16.max))
        var d = Data(capacity: 2 + nameData.count)
        var nameLen = UInt16(nameData.count).littleEndian
        d.append(Data(bytes: &nameLen, count: 2))
        d.append(nameData)
        return d
    }

    public static func deserialize(from data: Data) -> PairingRequestPayload? {
        guard data.count >= 2 else { return nil }
        let nameLen = Int(data.subdata(in: 0..<2).withUnsafeBytes { $0.load(as: UInt16.self).littleEndian })
        guard nameLen <= 1024, data.count >= 2 + nameLen else { return nil }
        let hostname = String(data: data.subdata(in: 2..<(2 + nameLen)), encoding: .utf8) ?? ""
        return PairingRequestPayload(hostname: hostname)
    }
}

/// Host -> client: the pairing grant. `key` is the 256-bit pairing key the
/// client must persist; `hostName` is the display name the client stores for
/// future connections.
public struct PairingGrantPayload {
    public let pairingID: String
    public let hostName: String
    public let key: Data

    public init(pairingID: String, hostName: String, key: Data) {
        self.pairingID = pairingID
        self.hostName = hostName
        self.key = key
    }

    public func serialize() -> Data {
        let idData = Data(pairingID.utf8).prefix(Int(UInt8.max))
        let nameData = Data(hostName.utf8).prefix(Int(UInt16.max))
        var d = Data(capacity: 2 + idData.count + 2 + nameData.count + 1 + key.count)
        var idLen = UInt8(idData.count); d.append(Data(bytes: &idLen, count: 1))
        d.append(idData)
        var nameLen = UInt16(nameData.count).littleEndian
        d.append(Data(bytes: &nameLen, count: 2))
        d.append(nameData)
        var keyLen = UInt8(key.count); d.append(Data(bytes: &keyLen, count: 1))
        d.append(key)
        return d
    }

    public static func deserialize(from data: Data) -> PairingGrantPayload? {
        guard data.count >= 1 else { return nil }
        let idLen = Int(data[0])
        var offset = 1
        guard data.count >= offset + idLen + 2 else { return nil }
        let pairingID = String(data: data.subdata(in: offset..<(offset + idLen)), encoding: .utf8) ?? ""
        offset += idLen
        let nameLen = Int(data.subdata(in: offset..<(offset + 2)).withUnsafeBytes { $0.load(as: UInt16.self).littleEndian })
        offset += 2
        guard data.count >= offset + nameLen + 1 else { return nil }
        let hostName = String(data: data.subdata(in: offset..<(offset + nameLen)), encoding: .utf8) ?? ""
        offset += nameLen
        let keyLen = Int(data[offset])
        offset += 1
        guard keyLen == 32, data.count >= offset + keyLen else { return nil }
        let key = data.subdata(in: offset..<(offset + keyLen))
        return PairingGrantPayload(pairingID: pairingID, hostName: hostName, key: key)
    }
}

public enum PairingRejectReason: UInt8 {
    case deniedByHost = 0
    case lockedOut = 1
    case pairingDisabled = 2
}

public struct PairingRejectPayload {
    public let reason: PairingRejectReason

    public init(reason: PairingRejectReason) {
        self.reason = reason
    }

    public func serialize() -> Data {
        var r = reason.rawValue
        return Data(bytes: &r, count: 1)
    }

    public static func deserialize(from data: Data) -> PairingRejectPayload? {
        guard data.count >= 1, let reason = PairingRejectReason(rawValue: data[0]) else { return nil }
        return PairingRejectPayload(reason: reason)
    }
}
