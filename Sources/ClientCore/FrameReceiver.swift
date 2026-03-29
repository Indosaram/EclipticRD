import Foundation

public class FrameReceiver {
    private struct FrameAssembly {
        let header: FrameHeaderPayload
        var chunks: [UInt16: Data]
        let startTime: Date

        var isComplete: Bool { chunks.count == Int(header.totalChunks) }

        func assemble() -> Data? {
            guard isComplete else { return nil }
            var result = Data(capacity: Int(header.totalSize))
            for i in 0..<header.totalChunks {
                guard let chunk = chunks[i] else { return nil }
                result.append(chunk)
            }
            return result
        }
    }

    private var frames: [UInt32: FrameAssembly] = [:]
    private let assemblyQueue = DispatchQueue(label: "eclipticrd.frame-assembly", qos: .userInteractive)
    private var cleanupTimer: DispatchSourceTimer?
    private let timeout: Double = ERDConstants.frameAssemblyTimeout

    public var onFrameReady: ((Data, FrameHeaderPayload) -> Void)?
    public var onCursorUpdate: ((CursorUpdate) -> Void)?

    public init() {
        startCleanupTimer()
    }

    deinit {
        // Use async (not sync) to avoid deadlock if deallocated on assemblyQueue.
        // Timer cancel is safe to call asynchronously.
        let timer = cleanupTimer
        assemblyQueue.async {
            timer?.cancel()
        }
    }

    public func stop() {
        assemblyQueue.sync {
            self.cleanupTimer?.cancel()
            self.cleanupTimer = nil
            self.frames.removeAll()
        }
    }

    public func handlePacket(_ data: Data) {
        guard data.count >= ERDConstants.packetHeaderSize else { return }
        guard let header = PacketHeader.deserialize(from: data) else { return }
        let payload = data.subdata(in: ERDConstants.packetHeaderSize..<data.count)

        assemblyQueue.async { [weak self] in
            switch header.type {
            case .frameHeader:
                self?.handleFrameHeader(payload)
            case .frameChunk:
                self?.handleFrameChunk(payload)
            case .cursorUpdate:
                if let cursor = CursorUpdate.deserialize(from: payload) {
                    self?.onCursorUpdate?(cursor)
                }
            default: break
            }
        }
    }

    private func handleFrameHeader(_ data: Data) {
        guard let fh = FrameHeaderPayload.deserialize(from: data) else {
            ERDLog.error("[FrameReceiver] Failed to deserialize frame header from \(data.count) bytes")
            return
        }
        frames[fh.frameId] = FrameAssembly(header: fh, chunks: [:], startTime: Date())
    }

    private func handleFrameChunk(_ data: Data) {
        guard let chunk = FrameChunkPayload.deserialize(from: data) else {
            ERDLog.error("[FrameReceiver] Failed to deserialize chunk from \(data.count) bytes")
            return
        }
        guard frames[chunk.frameId] != nil else {
            ERDLog.debug("[FrameReceiver] Orphan chunk: frame#\(chunk.frameId) idx=\(chunk.chunkIndex) (no header yet)")
            return
        }

        frames[chunk.frameId]!.chunks[chunk.chunkIndex] = chunk.chunkData
        let assembly = frames[chunk.frameId]!

        if assembly.isComplete {
            let completed = frames.removeValue(forKey: chunk.frameId)!
            if let frameData = completed.assemble() {
                ERDLog.video("[FrameReceiver] ✅ Frame #\(chunk.frameId) assembled: \(frameData.count) bytes")
                onFrameReady?(frameData, completed.header)
            }
        }
    }

    private func startCleanupTimer() {
        let timer = DispatchSource.makeTimerSource(queue: assemblyQueue)
        timer.schedule(deadline: .now() + 1.0, repeating: 1.0)
        timer.setEventHandler { [weak self] in
            guard let self = self else { return }
            let now = Date()
            self.frames = self.frames.filter { now.timeIntervalSince($0.value.startTime) < self.timeout }
        }
        timer.resume()
        cleanupTimer = timer
    }
}
