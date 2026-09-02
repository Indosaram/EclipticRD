import Foundation
import AudioToolbox

public class ClientAudioPlayer {
    private var queue: AudioQueueRef?
    private let queueAccessQueue = DispatchQueue(label: "eclipticrd.audioplayer")
    private var isRunning = false
    private let format: AudioStreamBasicDescription

    public init() {
        // Configure ASBD for ScreenCaptureKit standard: 48kHz Stereo Float32 Interleaved
        var asbd = AudioStreamBasicDescription()
        asbd.mSampleRate = 48000.0
        asbd.mFormatID = kAudioFormatLinearPCM
        asbd.mFormatFlags = kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked
        asbd.mBytesPerPacket = 8 // 4 bytes (Float32) * 2 channels
        asbd.mFramesPerPacket = 1
        asbd.mBytesPerFrame = 8
        asbd.mChannelsPerFrame = 2
        asbd.mBitsPerChannel = 32
        self.format = asbd
    }

    deinit {
        stop()
    }

    public func start() {
        queueAccessQueue.sync {
            guard !isRunning else { return }

            var tempQueue: AudioQueueRef?
            var fmt = format
            // Create a low-latency playback output queue; buffers are freed
            // in the completion callback, so every enqueued buffer is released
            // exactly once after playback.
            let status = AudioQueueNewOutput(&fmt, { _, inQueue, inBuffer in
                AudioQueueFreeBuffer(inQueue, inBuffer)
            }, nil, nil, nil, 0, &tempQueue)

            guard status == noErr, let q = tempQueue else {
                ERDLog.error("[AudioPlayer] Failed to create AudioQueue: \(status)")
                return
            }

            self.queue = q
            AudioQueueStart(q, nil)
            self.isRunning = true
            ERDLog.info("[AudioPlayer] Started AudioQueue playback")
        }
    }

    public func stop() {
        queueAccessQueue.sync {
            guard isRunning, let q = queue else { return }
            AudioQueueStop(q, true)
            AudioQueueDispose(q, true)
            self.queue = nil
            self.isRunning = false
            ERDLog.info("[AudioPlayer] Stopped AudioQueue playback")
        }
    }

    public func play(data: Data) {
        queueAccessQueue.async { [weak self] in
            guard let self = self, self.isRunning, let q = self.queue else { return }

            // Garbage or spoofed datagrams must not reach the audio allocator
            guard !data.isEmpty, data.count <= 65_536 else { return }

            var buffer: AudioQueueBufferRef?
            let status = AudioQueueAllocateBuffer(q, UInt32(data.count), &buffer)

            guard status == noErr, let buf = buffer else { return }

            // Copy audio payload into native buffer
            data.withUnsafeBytes { rawBuffer in
                if let baseAddress = rawBuffer.baseAddress {
                    memcpy(buf.pointee.mAudioData, baseAddress, data.count)
                }
            }
            buf.pointee.mAudioDataByteSize = UInt32(data.count)

            // Enqueue buffer for immediate zero-latency audio playback
            AudioQueueEnqueueBuffer(q, buf, 0, nil)
        }
    }
}
