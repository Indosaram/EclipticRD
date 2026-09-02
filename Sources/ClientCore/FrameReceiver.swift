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

    private struct LossEntry {
        let timestamp: Date
        let lost: Int
        let total: Int
    }

    private var frames: [UInt32: FrameAssembly] = [:]
    // Chunks that arrived before their frame header under UDP reordering
    private var orphanChunks: [UInt32: [UInt16: Data]] = [:]
    private let assemblyQueue = DispatchQueue(label: "eclipticrd.frame-assembly", qos: .userInteractive)
    private var cleanupTimer: DispatchSourceTimer?
    private let timeout: Double = ERDConstants.frameAssemblyTimeout

    private var expectedFrameId: UInt32 = 0
    private var hasReceivedFirstFrame = false
    private var lossEntries: [LossEntry] = []
    private let lossLock = NSLock()

    private var completedFrameTimestamps: [Date] = []
    public private(set) var totalFramesReceived: Int = 0

    public var onFrameReady: ((Data, FrameHeaderPayload) -> Void)?
    public var onCursorUpdate: ((CursorUpdate) -> Void)?

    /// Computed property: FPS based on frames completed in the last 1 second
    public var recentFPS: Double {
        assemblyQueue.sync {
            let cutoff = Date().addingTimeInterval(-1.0)
            let recentCount = completedFrameTimestamps.filter { $0 > cutoff }.count
            return Double(recentCount)
        }
    }

    public var frameLossRatio: Double {
        lossLock.lock()
        defer { lossLock.unlock() }
        let cutoff = Date().addingTimeInterval(-5.0)
        let recent = lossEntries.filter { $0.timestamp > cutoff }
        let totalLost = recent.reduce(0) { $0 + $1.lost }
        let totalFrames = recent.reduce(0) { $0 + $1.total }
        guard totalFrames > 0 else { return 0.0 }
        return Double(totalLost) / Double(totalFrames)
    }

    public init() {
        startCleanupTimer()
    }

    deinit {
        let timer = cleanupTimer
        assemblyQueue.async {
            timer?.cancel()
        }
    }

    public func stop() {
        assemblyQueue.sync {
            self.frames.removeAll()
            self.orphanChunks.removeAll()
            self.completedFrameTimestamps.removeAll()
            self.totalFramesReceived = 0
            self.expectedFrameId = 0
        }
        lossLock.lock()
        lossEntries.removeAll()
        hasReceivedFirstFrame = false
        lossLock.unlock()
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

        if fh.isKeyFrame {
            let keysToRemove = frames.keys.filter { $0 < fh.frameId }
            for key in keysToRemove {
                if let assembly = frames[key], !assembly.isComplete {
                    frames.removeValue(forKey: key)
                }
            }
        }

        trackFrameLoss(receivedFrameId: fh.frameId)
        frames[fh.frameId] = FrameAssembly(header: fh, chunks: [:], startTime: Date())

        if let pending = orphanChunks.removeValue(forKey: fh.frameId) {
            for (index, data) in pending where index < fh.totalChunks {
                frames[fh.frameId]!.chunks[index] = data
            }
        }

        // A fully-buffered orphan frame completes the moment its header lands.
        if frames[fh.frameId]!.isComplete {
            let completed = frames.removeValue(forKey: fh.frameId)!
            if let frameData = completed.assemble() {
                totalFramesReceived += 1
                completedFrameTimestamps.append(Date())
                let cutoff = Date().addingTimeInterval(-2.0)
                completedFrameTimestamps.removeAll { $0 < cutoff }
                onFrameReady?(frameData, completed.header)
            }
        }
    }

    private func handleFrameChunk(_ data: Data) {
        guard let chunk = FrameChunkPayload.deserialize(from: data) else {
            ERDLog.error("[FrameReceiver] Failed to deserialize chunk from \(data.count) bytes")
            return
        }

        if frames[chunk.frameId] != nil {
            let assembly = frames[chunk.frameId]!
            guard chunk.chunkIndex < assembly.header.totalChunks else { return }

            frames[chunk.frameId]!.chunks[chunk.chunkIndex] = chunk.chunkData
            let updated = frames[chunk.frameId]!

            if updated.isComplete {
                let completed = frames.removeValue(forKey: chunk.frameId)!
                if let frameData = completed.assemble() {
                    ERDLog.video("[FrameReceiver] ✅ Frame #\(chunk.frameId) assembled: \(frameData.count) bytes")
                    totalFramesReceived += 1
                    completedFrameTimestamps.append(Date())
                    let cutoff = Date().addingTimeInterval(-2.0)
                    completedFrameTimestamps.removeAll { $0 < cutoff }
                    onFrameReady?(frameData, completed.header)
                }
            }
            return
        }

        // No header yet: hold the chunk for reassembly when the header lands.
        guard orphanChunks.count < 16 || orphanChunks[chunk.frameId] != nil else { return }
        var orphans = orphanChunks[chunk.frameId] ?? [:]
        guard orphans.count < Int(ERDConstants.maxChunksPerFrame) else {
            orphanChunks.removeValue(forKey: chunk.frameId)
            return
        }
        orphans[chunk.chunkIndex] = chunk.chunkData
        orphanChunks[chunk.frameId] = orphans
    }

    private func trackFrameLoss(receivedFrameId: UInt32) {
        lossLock.lock()
        defer { lossLock.unlock() }

        if !hasReceivedFirstFrame {
            hasReceivedFirstFrame = true
            expectedFrameId = receivedFrameId + 1
            lossEntries.append(LossEntry(timestamp: Date(), lost: 0, total: 1))
            return
        }

        let gap = receivedFrameId > expectedFrameId ? Int(receivedFrameId - expectedFrameId) : 0
        lossEntries.append(LossEntry(timestamp: Date(), lost: gap, total: 1 + gap))
        expectedFrameId = max(expectedFrameId, receivedFrameId &+ 1)

        let cutoff = Date().addingTimeInterval(-10.0)
        lossEntries.removeAll { $0.timestamp < cutoff }
    }

    private func startCleanupTimer() {
        let timer = DispatchSource.makeTimerSource(queue: assemblyQueue)
        timer.schedule(deadline: .now() + 1.0, repeating: 1.0)
        timer.setEventHandler { [weak self] in
            guard let self = self else { return }
            let now = Date()
            self.frames = self.frames.filter { now.timeIntervalSince($0.value.startTime) < self.timeout }
            self.orphanChunks.removeAll()
        }
        timer.resume()
        cleanupTimer = timer
    }
}
