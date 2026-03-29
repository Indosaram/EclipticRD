import Foundation

public class FrameSender {
    private let udpChannel: UDPChannel
    private var frameIdCounter: UInt32 = 0
    private let counterLock = NSLock()

    public init(udpChannel: UDPChannel) {
        self.udpChannel = udpChannel
    }

    public func sendFrame(data: Data, width: Int, height: Int, isKeyFrame: Bool) {
        counterLock.lock()
        frameIdCounter += 1
        let frameId = frameIdCounter
        counterLock.unlock()

        let maxPayload = ERDConstants.maxPayloadSize - FrameChunkPayload.headerSize
        let totalChunks = UInt16((data.count + maxPayload - 1) / maxPayload)

        guard totalChunks <= ERDConstants.maxChunksPerFrame else {
            ERDLog.error("[FrameSender] Frame too large: \(data.count) bytes, \(totalChunks) chunks")
            return
        }

        // 1. Send frame header via UDP
        let header = FrameHeaderPayload(
            frameId: frameId, width: UInt16(width), height: UInt16(height),
            isKeyFrame: isKeyFrame, totalChunks: totalChunks, totalSize: UInt32(data.count)
        )
        udpChannel.sendPacket(type: .frameHeader, payload: header.serialize())

        // 2. Send frame chunks via UDP
        var offset = 0
        var chunkIndex: UInt16 = 0
        while offset < data.count {
            let end = min(offset + maxPayload, data.count)
            let chunkData = data.subdata(in: offset..<end)
            let chunk = FrameChunkPayload(frameId: frameId, chunkIndex: chunkIndex, chunkData: chunkData)
            udpChannel.sendPacket(type: .frameChunk, payload: chunk.serialize())
            offset = end
            chunkIndex += 1
        }
    }

    public func sendCursorUpdate(x: Float, y: Float, cursorType: UInt8 = 0) {
        let cursor = CursorUpdate(x: x, y: y, cursorType: cursorType)
        udpChannel.sendPacket(type: .cursorUpdate, payload: cursor.serialize())
    }
}
