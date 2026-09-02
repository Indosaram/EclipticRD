import Foundation
import VideoToolbox
import CoreMedia

public enum VideoEncoderError: Error {
    case sessionCreationFailed
    case encodingFailed
}

public class VideoEncoder {
    private var session: VTCompressionSession?
    private let encoderQueue = DispatchQueue(label: "eclipticrd.encoder", qos: .userInteractive)
    private static let queueKey = DispatchSpecificKey<Bool>()
    private var frameCount: Int64 = 0
    private var retainedSelf: Unmanaged<VideoEncoder>?

    private let width: Int
    private let height: Int
    private let bitrate: Int
    private let fps: Int
    private var pendingForceKeyFrame = AtomicBool(false)

    public var onEncodedFrame: ((Data, Bool) -> Void)?

    public init(width: Int, height: Int, bitrate: Int = ERDConstants.defaultBitrate, fps: Int = ERDConstants.defaultFPS) {
        self.width = width; self.height = height; self.bitrate = bitrate; self.fps = fps
        encoderQueue.setSpecific(key: Self.queueKey, value: true)
    }

    deinit { stop() }

    public func start() throws {
        retainedSelf = Unmanaged.passRetained(self)

        let err = VTCompressionSessionCreate(
            allocator: kCFAllocatorDefault,
            width: Int32(width), height: Int32(height),
            codecType: kCMVideoCodecType_HEVC,
            encoderSpecification: nil, imageBufferAttributes: nil, compressedDataAllocator: nil,
            outputCallback: encoderCallback,
            refcon: retainedSelf!.toOpaque(),
            compressionSessionOut: &session
        )
        guard err == noErr, let session = session else {
            retainedSelf?.release()
            retainedSelf = nil
            throw VideoEncoderError.sessionCreationFailed
        }

        VTSessionSetProperty(session, key: kVTCompressionPropertyKey_RealTime, value: kCFBooleanTrue)
        VTSessionSetProperty(session, key: kVTCompressionPropertyKey_AllowFrameReordering, value: kCFBooleanFalse)
        VTSessionSetProperty(session, key: kVTCompressionPropertyKey_AverageBitRate, value: bitrate as CFTypeRef)
        VTSessionSetProperty(session, key: kVTCompressionPropertyKey_MaxKeyFrameInterval, value: ERDConstants.keyFrameInterval as CFTypeRef)
        VTSessionSetProperty(session, key: kVTCompressionPropertyKey_ExpectedFrameRate, value: fps as CFTypeRef)
        VTSessionSetProperty(session, key: kVTCompressionPropertyKey_ProfileLevel, value: kVTProfileLevel_HEVC_Main_AutoLevel)

        VTCompressionSessionPrepareToEncodeFrames(session)
        ERDLog.video("[Encoder] Started: H.265 \(width)x\(height) @\(bitrate/1_000_000)Mbps")
    }

    public func stop() {
        let work = {
            if let session = self.session {
                VTCompressionSessionInvalidate(session)
                self.session = nil
            }
            self.retainedSelf?.release()
            self.retainedSelf = nil
            self.frameCount = 0
            ERDLog.video("[Encoder] Stopped")
        }

        if DispatchQueue.getSpecific(key: Self.queueKey) == true {
            work()
        } else {
            encoderQueue.sync { work() }
        }
    }

    public func encode(_ sampleBuffer: CMSampleBuffer) {
        encoderQueue.async { [weak self] in
            guard let self = self else { return }
            guard let session = self.session else { return }
            guard let imageBuffer = CMSampleBufferGetImageBuffer(sampleBuffer) else {
                return
            }
            let pts = CMTime(value: self.frameCount, timescale: CMTimeScale(self.fps))
            self.frameCount += 1

            var frameProps: CFDictionary? = nil
            if self.pendingForceKeyFrame.value {
                self.pendingForceKeyFrame.value = false
                frameProps = [kVTEncodeFrameOptionKey_ForceKeyFrame: true] as CFDictionary
            }

            VTCompressionSessionEncodeFrame(session, imageBuffer: imageBuffer, presentationTimeStamp: pts,
                                            duration: .invalid, frameProperties: frameProps, sourceFrameRefcon: nil, infoFlagsOut: nil)
        }
    }

    public func forceKeyFrame() {
        pendingForceKeyFrame.value = true
    }

    public func updateBitrate(_ newBitrate: Int) {
        encoderQueue.async { [weak self] in
            guard let self = self, let session = self.session else { return }
            VTSessionSetProperty(session, key: kVTCompressionPropertyKey_AverageBitRate, value: newBitrate as CFTypeRef)
        }
    }

    public func updateFPS(_ newFPS: Int) {
        encoderQueue.async { [weak self] in
            guard let self = self, let session = self.session else { return }
            VTSessionSetProperty(session, key: kVTCompressionPropertyKey_ExpectedFrameRate, value: newFPS as CFTypeRef)
        }
    }

    fileprivate func handleEncodedFrame(_ sampleBuffer: CMSampleBuffer) {
        guard let dataBuffer = CMSampleBufferGetDataBuffer(sampleBuffer) else { return }

        let isKeyFrame: Bool = {
            guard let attachments = CMSampleBufferGetSampleAttachmentsArray(sampleBuffer, createIfNecessary: false) as? [[CFString: Any]],
                  let first = attachments.first else { return true }
            return !(first[kCMSampleAttachmentKey_NotSync] as? Bool ?? false)
        }()

        var length = 0
        var dataPointer: UnsafeMutablePointer<Int8>?
        CMBlockBufferGetDataPointer(dataBuffer, atOffset: 0, lengthAtOffsetOut: nil, totalLengthOut: &length, dataPointerOut: &dataPointer)

        guard let pointer = dataPointer, length > 0 else { return }

        var frameData = Data()

        // For keyframes, prepend VPS/SPS/PPS from the format description
        // so the decoder can create its decoding session.
        if isKeyFrame, let formatDesc = CMSampleBufferGetFormatDescription(sampleBuffer) {
            var paramCount: Int = 0
            CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(formatDesc, parameterSetIndex: 0,
                parameterSetPointerOut: nil, parameterSetSizeOut: nil, parameterSetCountOut: &paramCount, nalUnitHeaderLengthOut: nil)

            for i in 0..<paramCount {
                var paramPtr: UnsafePointer<UInt8>?
                var paramSize: Int = 0
                let status = CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(formatDesc, parameterSetIndex: i,
                    parameterSetPointerOut: &paramPtr, parameterSetSizeOut: &paramSize, parameterSetCountOut: nil, nalUnitHeaderLengthOut: nil)
                if status == noErr, let ptr = paramPtr, paramSize > 0 {
                    // Write 4-byte AVCC length prefix + parameter set data
                    var len = UInt32(paramSize).bigEndian
                    frameData.append(Data(bytes: &len, count: 4))
                    frameData.append(Data(bytes: ptr, count: paramSize))
                }
            }
        }

        frameData.append(Data(bytes: pointer, count: length))
        onEncodedFrame?(frameData, isKeyFrame)
    }
}

private func encoderCallback(outputCallbackRefCon: UnsafeMutableRawPointer?,
                              sourceFrameRefCon: UnsafeMutableRawPointer?,
                              status: OSStatus,
                              infoFlags: VTEncodeInfoFlags,
                              sampleBuffer: CMSampleBuffer?) {
    guard status == noErr, let sampleBuffer = sampleBuffer, let refcon = outputCallbackRefCon else { return }
    let encoder = Unmanaged<VideoEncoder>.fromOpaque(refcon).takeUnretainedValue()
    encoder.handleEncodedFrame(sampleBuffer)
}
