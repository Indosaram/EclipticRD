import Foundation

public struct FrameHeaderPayload {
    public let frameId: UInt32
    public let width: UInt16
    public let height: UInt16
    public let isKeyFrame: Bool
    public let totalChunks: UInt16
    public let totalSize: UInt32

    public static let size = 16

    public init(frameId: UInt32, width: UInt16, height: UInt16, isKeyFrame: Bool, totalChunks: UInt16, totalSize: UInt32) {
        self.frameId = frameId; self.width = width; self.height = height
        self.isKeyFrame = isKeyFrame; self.totalChunks = totalChunks; self.totalSize = totalSize
    }

    public func serialize() -> Data {
        var d = Data(capacity: FrameHeaderPayload.size)
        var fid = frameId.littleEndian; d.append(Data(bytes: &fid, count: 4))
        var w = width.littleEndian;     d.append(Data(bytes: &w, count: 2))
        var h = height.littleEndian;    d.append(Data(bytes: &h, count: 2))
        var kf: UInt8 = isKeyFrame ? 1 : 0; d.append(Data(bytes: &kf, count: 1))
        var tc = totalChunks.littleEndian; d.append(Data(bytes: &tc, count: 2))
        // 1 byte padding
        var pad: UInt8 = 0; d.append(Data(bytes: &pad, count: 1))
        var ts = totalSize.littleEndian; d.append(Data(bytes: &ts, count: 4))
        return d
    }

    public static func deserialize(from data: Data) -> FrameHeaderPayload? {
        guard data.count >= size else { return nil }
        let fid = data.subdata(in: 0..<4).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian }
        let w = data.subdata(in: 4..<6).withUnsafeBytes { $0.load(as: UInt16.self).littleEndian }
        let h = data.subdata(in: 6..<8).withUnsafeBytes { $0.load(as: UInt16.self).littleEndian }
        let kf = data[8] != 0
        let tc = data.subdata(in: 9..<11).withUnsafeBytes { $0.load(as: UInt16.self).littleEndian }
        let ts = data.subdata(in: 12..<16).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian }
        return FrameHeaderPayload(frameId: fid, width: w, height: h, isKeyFrame: kf, totalChunks: tc, totalSize: ts)
    }
}

public struct FrameChunkPayload {
    public let frameId: UInt32
    public let chunkIndex: UInt16
    public let chunkData: Data

    public static let headerSize = 6

    public init(frameId: UInt32, chunkIndex: UInt16, chunkData: Data) {
        self.frameId = frameId; self.chunkIndex = chunkIndex; self.chunkData = chunkData
    }

    public func serialize() -> Data {
        var d = Data(capacity: FrameChunkPayload.headerSize + chunkData.count)
        var fid = frameId.littleEndian; d.append(Data(bytes: &fid, count: 4))
        var ci = chunkIndex.littleEndian; d.append(Data(bytes: &ci, count: 2))
        d.append(chunkData)
        return d
    }

    public static func deserialize(from data: Data) -> FrameChunkPayload? {
        guard data.count >= headerSize else { return nil }
        let fid = data.subdata(in: 0..<4).withUnsafeBytes { $0.load(as: UInt32.self).littleEndian }
        let ci = data.subdata(in: 4..<6).withUnsafeBytes { $0.load(as: UInt16.self).littleEndian }
        let chunk = data.subdata(in: headerSize..<data.count)
        return FrameChunkPayload(frameId: fid, chunkIndex: ci, chunkData: chunk)
    }
}
