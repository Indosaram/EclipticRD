import Foundation
import VideoToolbox
import CoreMedia

public class VideoDecoder {
    private var session: VTDecompressionSession?
    private var formatDescription: CMVideoFormatDescription?
    private let decoderQueue = DispatchQueue(label: "eclipticrd.decoder", qos: .userInteractive)
    private var retainedSelf: Unmanaged<VideoDecoder>?

    private var sps: Data?
    private var pps: Data?
    private var vps: Data?

    public var onDecodedFrame: ((CVPixelBuffer) -> Void)?

    public init() {}

    // NOTE: Do not call stop() from deinit — decoderQueue.sync would deadlock
    // if the last reference is released on decoderQueue itself.
    // Callers must call stop() explicitly before releasing.

    public func decode(_ data: Data) {
        decoderQueue.async { [weak self] in
            guard let self = self else { return }
            self.processNALUs(data)
        }
    }

    public func stop() {
        decoderQueue.sync {
            if let session = self.session {
                VTDecompressionSessionInvalidate(session)
                self.session = nil
            }
            self.retainedSelf?.release()
            self.retainedSelf = nil
            self.formatDescription = nil
            self.sps = nil; self.pps = nil; self.vps = nil
        }
    }

    private func processNALUs(_ data: Data) {
        // HEVC NALUs use 4-byte length prefix (AVCC format from VideoToolbox output)
        var offset = 0
        while offset + 4 <= data.count {
            let naluLength = data.subdata(in: offset..<offset+4).withUnsafeBytes { $0.load(as: UInt32.self).bigEndian }
            offset += 4
            guard naluLength > 0, offset + Int(naluLength) <= data.count else { break }

            let nalu = data.subdata(in: offset..<offset+Int(naluLength))
            let naluType = (nalu[0] >> 1) & 0x3F // HEVC NAL unit type

            switch naluType {
            case 32: // VPS
                vps = nalu
            case 33: // SPS
                sps = nalu
            case 34: // PPS
                pps = nalu
                createDecoderIfNeeded()
            default:
                // IDR (19, 20) or non-IDR (0, 1) - decode it
                if naluType <= 31 || naluType == 19 || naluType == 20 {
                    decodeNALU(nalu)
                }
            }
            offset += Int(naluLength)
        }
    }

    private func createDecoderIfNeeded() {
        guard let vps = vps, let sps = sps, let pps = pps else { return }

        // Nest withUnsafeBytes calls to keep pointers valid for the entire scope.
        var newFormat: CMVideoFormatDescription?
        vps.withUnsafeBytes { vpsBuffer in
            sps.withUnsafeBytes { spsBuffer in
                pps.withUnsafeBytes { ppsBuffer in
                    let pointers: [UnsafePointer<UInt8>] = [
                        vpsBuffer.baseAddress!.assumingMemoryBound(to: UInt8.self),
                        spsBuffer.baseAddress!.assumingMemoryBound(to: UInt8.self),
                        ppsBuffer.baseAddress!.assumingMemoryBound(to: UInt8.self)
                    ]
                    let sizes = [vps.count, sps.count, pps.count]

                    CMVideoFormatDescriptionCreateFromHEVCParameterSets(
                        allocator: kCFAllocatorDefault,
                        parameterSetCount: 3,
                        parameterSetPointers: pointers,
                        parameterSetSizes: sizes,
                        nalUnitHeaderLength: 4,
                        extensions: nil,
                        formatDescriptionOut: &newFormat
                    )
                }
            }
        }

        guard let format = newFormat else { return }

        if session != nil {
            VTDecompressionSessionInvalidate(session!)
            session = nil
        }
        retainedSelf?.release()
        retainedSelf = Unmanaged.passRetained(self)
        formatDescription = format

        let pixelBufferAttributes: [String: Any] = [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
            kCVPixelBufferMetalCompatibilityKey as String: true
        ]

        var callbackRecord = VTDecompressionOutputCallbackRecord(
            decompressionOutputCallback: decoderCallback,
            decompressionOutputRefCon: retainedSelf!.toOpaque()
        )

        VTDecompressionSessionCreate(
            allocator: kCFAllocatorDefault,
            formatDescription: format,
            decoderSpecification: nil,
            imageBufferAttributes: pixelBufferAttributes as CFDictionary,
            outputCallback: &callbackRecord,
            decompressionSessionOut: &session
        )
    }

    private func decodeNALU(_ nalu: Data) {
        guard let session = session, let format = formatDescription else { return }

        // Wrap NALU with 4-byte length prefix
        var length = UInt32(nalu.count).bigEndian
        var blockData = Data(bytes: &length, count: 4)
        blockData.append(nalu)

        var blockBuffer: CMBlockBuffer?
        let totalLength = blockData.count

        blockData.withUnsafeBytes { rawBuffer in
            guard let baseAddress = rawBuffer.baseAddress else { return }
            CMBlockBufferCreateWithMemoryBlock(
                allocator: kCFAllocatorDefault,
                memoryBlock: nil, blockLength: totalLength,
                blockAllocator: nil, customBlockSource: nil,
                offsetToData: 0, dataLength: totalLength,
                flags: 0, blockBufferOut: &blockBuffer)

            if let block = blockBuffer {
                CMBlockBufferReplaceDataBytes(with: baseAddress, blockBuffer: block, offsetIntoDestination: 0, dataLength: totalLength)
            }
        }

        guard let block = blockBuffer else { return }

        var sampleBuffer: CMSampleBuffer?
        var sizeArray = [totalLength]
        CMSampleBufferCreateReady(
            allocator: kCFAllocatorDefault,
            dataBuffer: block, formatDescription: format,
            sampleCount: 1, sampleTimingEntryCount: 0, sampleTimingArray: nil,
            sampleSizeEntryCount: 1, sampleSizeArray: &sizeArray,
            sampleBufferOut: &sampleBuffer)

        guard let buffer = sampleBuffer else { return }

        var flagsOut = VTDecodeInfoFlags()
        VTDecompressionSessionDecodeFrame(session, sampleBuffer: buffer, flags: [], frameRefcon: nil, infoFlagsOut: &flagsOut)
    }

    fileprivate func frameDecoded(_ imageBuffer: CVImageBuffer) {
        onDecodedFrame?(imageBuffer)
    }
}

private func decoderCallback(decompressionOutputRefCon: UnsafeMutableRawPointer?,
                              sourceFrameRefCon: UnsafeMutableRawPointer?,
                              status: OSStatus,
                              infoFlags: VTDecodeInfoFlags,
                              imageBuffer: CVImageBuffer?,
                              presentationTimeStamp: CMTime,
                              presentationDuration: CMTime) {
    guard status == noErr, let buffer = imageBuffer, let refcon = decompressionOutputRefCon else { return }
    let decoder = Unmanaged<VideoDecoder>.fromOpaque(refcon).takeUnretainedValue()
    decoder.frameDecoded(buffer)
}
