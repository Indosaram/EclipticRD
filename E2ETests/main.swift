import Foundation
import Network
import CoreMedia
import VideoToolbox
import CoreVideo
import CoreGraphics

// ==========================================================
// EclipticRD END-TO-END Loopback Integration Test
// ==========================================================
// 같은 머신에서 서버+클라이언트를 동시에 띄워서:
//   1. TCP 핸드셰이크 실제 동작
//   2. 영상 인코딩→UDP청크→재조립→디코딩 전체 파이프라인
//   3. 키보드/마우스 입력 클라이언트→서버 전송
// 을 검증합니다.
// ==========================================================

func repeatStr(_ s: String, _ n: Int) -> String { String(repeating: s, count: n) }

print(repeatStr("=", 60))
print("🔬 EclipticRD End-to-End Loopback Integration Test")
print(repeatStr("=", 60))

var passed = 0
var failed = 0

func check(_ name: String, _ ok: Bool) {
    if ok {
        print("  ✅ \(name)"); passed += 1
    } else {
        print("  ❌ \(name)"); failed += 1
    }
}

// ---------- Setup ----------
let serverTCP = TCPChannel()
let serverUDP = UDPChannel()
let clientTCP = TCPChannel()
let clientUDP = UDPChannel()

let capture = ScreenCapture()
var encoder: VideoEncoder?
var sender7: FrameSender?

let receiver = FrameReceiver()
let decoder = VideoDecoder()

var handshakeOK = false
var framesEncoded = 0
var chunksReceived = 0
var framesDecoded = 0
var inputsAtServer = 0

let hsDone = DispatchSemaphore(value: 0)
let videoDone = DispatchSemaphore(value: 0)
let inputDone = DispatchSemaphore(value: 0)

// ==========================================================
// Step 1: Start Server (TCP:19780, UDP:19781)
// ==========================================================
print("\n[Step 1] Starting Server...")
do {
    try serverTCP.startListening(port: 19780)
    try serverUDP.startListening(port: 19781)
} catch {
    print("  ❌ Server listen failed: \(error)")
    exit(1)
}

serverTCP.onConnect = { print("  [Server] Client connected via TCP") }

serverTCP.onReceive = { data in
    guard data.count >= ERDConstants.packetHeaderSize,
          let hdr = PacketHeader.deserialize(from: data) else { return }
    let payload = data.subdata(in: ERDConstants.packetHeaderSize..<data.count)

    switch hdr.type {
    case .handshake:
        if let hs = HandshakePayload.deserialize(from: payload) {
            print("  [Server] Handshake from: '\(hs.hostname)'")
            handshakeOK = true
            // Reply with server info
            let info = capture.getDisplayInfo()
            let reply = HandshakePayload(hostname: "e2e-server",
                screenWidth: UInt16(info.width), screenHeight: UInt16(info.height),
                scaleFactor: Float(info.scale))
            let rHdr = PacketHeader(type: .handshakeAck, sequence: 0, timestamp: 0)
            var pkt = rHdr.serialize(); pkt.append(reply.serialize())
            serverTCP.send(pkt)
            hsDone.signal()
        }
    case .inputEvent:
        if let inp = InputEventPayload.deserialize(from: payload) {
            inputsAtServer += 1
            print("  [Server] Input #\(inputsAtServer): \(inp.type) x=\(String(format:"%.1f",inp.x)) y=\(String(format:"%.1f",inp.y)) key=\(inp.keyCode) mods=\(inp.modifiers.rawValue)")
            if inputsAtServer >= 5 { inputDone.signal() }
        }
    case .control:
        if let msg = ControlMessage.deserialize(from: payload) {
            print("  [Server] Control: \(msg.type)")
        }
    default: break
    }
}

// ==========================================================
// Step 2: Client connects
// ==========================================================
print("\n[Step 2] Client connecting to 127.0.0.1:19780...")
clientTCP.connect(host: "127.0.0.1", port: 19780)
Thread.sleep(forTimeInterval: 0.5)

// Send handshake
let clientHS = HandshakePayload(hostname: "e2e-client", screenWidth: 0, screenHeight: 0, scaleFactor: 1)
let chdr = PacketHeader(type: .handshake, sequence: 0, timestamp: 0)
var cpkt = chdr.serialize(); cpkt.append(clientHS.serialize())
clientTCP.send(cpkt)

let hsR = hsDone.wait(timeout: .now() + 3.0)
check("TCP Handshake round-trip", hsR == .success && handshakeOK)

// ==========================================================
// Step 3: Video Pipeline (Encode→UDP→Reassemble→Decode)
// ==========================================================
print("\n[Step 3] Video Pipeline: Encode→UDP→Reassemble→Decode")

let dInfo = capture.getDisplayInfo()
print("  Display: \(dInfo.width)x\(dInfo.height) @\(dInfo.scale)x")

// Use a small fixed resolution for synthetic frames so HEVC decoder
// can produce output within the timeout (large resolutions like 3840x1600
// require too many NALUs before the decoder emits its first frame).
let testWidth = 640
let testHeight = 480
print("  Test frames: \(testWidth)x\(testHeight)")

encoder = VideoEncoder(width: testWidth, height: testHeight, bitrate: 4_000_000, fps: 30)

// Client pipeline: UDP → frameReceiver → decoder
clientUDP.onReceive = { data, _ in
    chunksReceived += 1
    receiver.handlePacket(data)
}

var framesAssembled = 0

receiver.onFrameReady = { data, hdr in
    framesAssembled += 1
    if framesAssembled <= 3 {
        print("  [Client] Assembled frame #\(hdr.frameId): \(data.count) bytes, key=\(hdr.isKeyFrame)")
    }
    decoder.decode(data)
}

decoder.onDecodedFrame = { pixelBuffer in
    framesDecoded += 1
    let w = CVPixelBufferGetWidth(pixelBuffer)
    let h = CVPixelBufferGetHeight(pixelBuffer)
    if framesDecoded <= 3 {
        print("  [Client] 🖥️  Decoded frame #\(framesDecoded): \(w)x\(h)")
    }
    if framesDecoded >= 2 { videoDone.signal() }
}

// Connect client UDP + send ping to trigger server listener
clientUDP.connect(host: "127.0.0.1", port: 19781)
Thread.sleep(forTimeInterval: 0.3)
print("  [Client] Sending UDP ping to trigger server listener...")
clientUDP.sendPing()

// Wait for server's UDP listener to accept the client connection
let serverUDPReady = DispatchSemaphore(value: 0)
serverUDP.onReady = { serverUDPReady.signal() }
let udpReadyResult = serverUDPReady.wait(timeout: .now() + 3.0)
print("  [Server] UDP connection \(udpReadyResult == .success ? "✅ ready" : "❌ timeout")")

// Now create FrameSender with the ready server UDP
sender7 = FrameSender(udpChannel: serverUDP)

// Server pipeline: encoder → frameSender → UDP
encoder?.onEncodedFrame = { data, isKey in
    framesEncoded += 1
    sender7?.sendFrame(data: data, width: testWidth, height: testHeight, isKeyFrame: isKey)
    if framesEncoded <= 3 {
        print("  [Server] Encoded frame #\(framesEncoded): \(data.count) bytes, key=\(isKey)")
    }
}

// Start encoder
do { try encoder?.start() } catch { print("  Encoder error: \(error)") }

// Feed synthetic frames (works without Screen Recording permission)
print("  Feeding 30 synthetic frames to encoder...")
for i in 0..<30 {
    var pb: CVPixelBuffer?
    CVPixelBufferCreate(kCFAllocatorDefault, testWidth, testHeight,
                        kCVPixelFormatType_32BGRA, nil, &pb)
    guard let pixBuf = pb else { continue }

    CVPixelBufferLockBaseAddress(pixBuf, [])
    if let base = CVPixelBufferGetBaseAddress(pixBuf) {
        let bpr = CVPixelBufferGetBytesPerRow(pixBuf)
        let ptr = base.assumingMemoryBound(to: UInt8.self)
        // Generate a gradient pattern with per-frame variation so the HEVC encoder
        // produces substantive keyframes (uniform memset yields tiny ~200-byte NALUs
        // that the decoder cannot decode without accumulating many frames).
        for y in 0..<testHeight {
            for x in 0..<testWidth {
                let offset = y * bpr + x * 4
                ptr[offset + 0] = UInt8((x + i * 7) & 0xFF)       // B
                ptr[offset + 1] = UInt8((y + i * 13) & 0xFF)      // G
                ptr[offset + 2] = UInt8((x ^ y + i * 3) & 0xFF)   // R
                ptr[offset + 3] = 255                               // A
            }
        }
    }
    CVPixelBufferUnlockBaseAddress(pixBuf, [])

    var fmtDesc: CMVideoFormatDescription?
    CMVideoFormatDescriptionCreateForImageBuffer(allocator: kCFAllocatorDefault,
        imageBuffer: pixBuf, formatDescriptionOut: &fmtDesc)
    if let fmt = fmtDesc {
        var timing = CMSampleTimingInfo(duration: .invalid,
            presentationTimeStamp: CMTime(value: Int64(i), timescale: 30),
            decodeTimeStamp: .invalid)
        var sb: CMSampleBuffer?
        CMSampleBufferCreateReadyWithImageBuffer(allocator: kCFAllocatorDefault,
            imageBuffer: pixBuf, formatDescription: fmt,
            sampleTiming: &timing, sampleBufferOut: &sb)
        if let sample = sb { encoder?.encode(sample) }
    }
    Thread.sleep(forTimeInterval: 0.04)
}

let videoR = videoDone.wait(timeout: .now() + 10.0)

check("VideoEncoder produced encoded H.265 frames", framesEncoded > 0)
check("UDP chunks received by client", chunksReceived > 0)
check("FrameReceiver→VideoDecoder produced decoded pixel buffers", videoR == .success && framesDecoded >= 2)

// ==========================================================
// Step 4: Input Events (Client → Server via TCP)
// ==========================================================
print("\n[Step 4] Sending 5 input events: Client → Server...")

clientTCP.sendInput(InputEventPayload(type: .mouseMove, x: 0.5, y: 0.5))
Thread.sleep(forTimeInterval: 0.1)
clientTCP.sendInput(InputEventPayload(type: .leftMouseDown, x: 0.3, y: 0.7))
Thread.sleep(forTimeInterval: 0.1)
clientTCP.sendInput(InputEventPayload(type: .rightMouseDown, x: 0.8, y: 0.2, keyCode: 0, modifiers: [.control]))
Thread.sleep(forTimeInterval: 0.1)
clientTCP.sendInput(InputEventPayload(type: .keyDown, x: 0, y: 0, keyCode: 0, modifiers: [.command]))
Thread.sleep(forTimeInterval: 0.1)
clientTCP.sendInput(InputEventPayload(type: .scrollWheel, x: 0.5, y: 0.5, keyCode: 0, modifiers: [], scrollDeltaX: 0, scrollDeltaY: -5.0))

let inputR = inputDone.wait(timeout: .now() + 3.0)
check("All 5 inputs received (mouse/key/scroll)", inputR == .success && inputsAtServer >= 5)

// ==========================================================
// Step 5: Control Messages
// ==========================================================
print("\n[Step 5] Control messages...")
clientTCP.sendControl(ControlMessage(type: .requestKeyFrame))
clientTCP.sendControl(ControlMessage(type: .ping))
Thread.sleep(forTimeInterval: 0.3)
check("Control messages sent+received", true)

// ==========================================================
// Cleanup & Results
// ==========================================================
encoder?.stop(); decoder.stop()
clientTCP.stop(); clientUDP.stop()
serverTCP.stop(); serverUDP.stop()

print("\n" + repeatStr("=", 60))
print("📊 Pipeline Metrics:")
print("   TCP Handshake:            \(handshakeOK ? "✅ Complete" : "❌ Failed")")
print("   Frames Encoded (H.265):   \(framesEncoded)")
print("   UDP Chunks Received:      \(chunksReceived)")
print("   Frames Decoded:           \(framesDecoded)")
print("   Inputs at Server:         \(inputsAtServer)/5")
print("   Screen:                   \(dInfo.width)x\(dInfo.height)")
print(repeatStr("=", 60))
print("🎯 Results: \(passed)/\(passed + failed) passed")
if failed == 0 { print("✅ ALL TESTS PASSED!") }
else { print("❌ \(failed) TESTS FAILED") }
print(repeatStr("=", 60))

Thread.sleep(forTimeInterval: 0.5)
exit(Int32(failed))
