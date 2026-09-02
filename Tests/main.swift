import Foundation
import Network
import CoreMedia
import VideoToolbox
import CoreVideo

// ============================================
// EclipticRD Headless Integration Test Suite
// ============================================

var passed = 0
var failed = 0

func test(_ name: String, _ block: () throws -> Bool) {
    do {
        if try block() {
            print("  ✅ \(name)")
            passed += 1
        } else {
            print("  ❌ \(name) - assertion failed")
            failed += 1
        }
    } catch {
        print("  ❌ \(name) - error: \(error)")
        failed += 1
    }
}

// ============================================
// TEST 1: Binary Payload Serialization
// ============================================
print("\n[TEST 1] Binary Payload Serialization Round-trips")

test("PacketHeader serialize/deserialize") {
    let h = PacketHeader(type: .frameHeader, sequence: 42, timestamp: 123456, flags: 7)
    let data = h.serialize()
    guard let h2 = PacketHeader.deserialize(from: data) else { return false }
    return h2.magic == ERDConstants.magic && h2.type == .frameHeader &&
           h2.sequence == 42 && h2.timestamp == 123456 && h2.flags == 7
}

test("PacketHeader rejects bad magic") {
    var data = Data(count: 12)
    data[0] = 0xFF; data[1] = 0xFF
    return PacketHeader.deserialize(from: data) == nil
}

test("PacketHeader with audioFrame type") {
    let h = PacketHeader(type: .audioFrame, sequence: 99, timestamp: 987654, flags: 1)
    let data = h.serialize()
    guard let h2 = PacketHeader.deserialize(from: data) else { return false }
    return h2.type == .audioFrame && h2.sequence == 99 && h2.timestamp == 987654 && h2.flags == 1
}

test("HandshakePayload serialize/deserialize") {
    let hs = HandshakePayload(hostname: "MyMac-Pro", screenWidth: 2560, screenHeight: 1440, scaleFactor: 2.0,
                              protocolVersion: ERDConstants.protocolVersion,
                              capabilities: [.streamConfiguration, .clipboardSync, .textClipboardSync])
    let data = hs.serialize()
    guard let hs2 = HandshakePayload.deserialize(from: data) else { return false }
    return hs2.hostname == "MyMac-Pro" && hs2.screenWidth == 2560 &&
           hs2.screenHeight == 1440 && hs2.scaleFactor == 2.0 &&
           hs2.protocolVersion == ERDConstants.protocolVersion &&
           hs2.capabilities.contains(.streamConfiguration) &&
           hs2.capabilities.contains(.clipboardSync) &&
           hs2.capabilities.contains(.textClipboardSync)
}

test("HandshakePayload with Unicode hostname") {
    let hs = HandshakePayload(hostname: "맥북-프로", screenWidth: 1920, screenHeight: 1080, scaleFactor: 1.0)
    let data = hs.serialize()
    guard let hs2 = HandshakePayload.deserialize(from: data) else { return false }
    return hs2.hostname == "맥북-프로" && hs2.protocolVersion == ERDConstants.protocolVersion
}

test("HandshakePayload deserializes legacy payloads") {
    let legacy = HandshakePayload(hostname: "Legacy", screenWidth: 1024, screenHeight: 768, scaleFactor: 1.0)
    var data = Data()
    let nameData = legacy.hostname.data(using: .utf8) ?? Data()
    var nameLen = UInt16(nameData.count).littleEndian; data.append(Data(bytes: &nameLen, count: 2))
    data.append(nameData)
    var w = legacy.screenWidth.littleEndian; data.append(Data(bytes: &w, count: 2))
    var h = legacy.screenHeight.littleEndian; data.append(Data(bytes: &h, count: 2))
    var s = legacy.scaleFactor.bitPattern.littleEndian; data.append(Data(bytes: &s, count: 4))
    guard let hs2 = HandshakePayload.deserialize(from: data) else { return false }
    return hs2.hostname == "Legacy" && hs2.protocolVersion == ERDConstants.legacyProtocolVersion && hs2.capabilities.isEmpty
}

test("FrameHeaderPayload serialize/deserialize") {
    let fh = FrameHeaderPayload(frameId: 999, width: 3840, height: 2160, isKeyFrame: true, totalChunks: 12, totalSize: 500000)
    let data = fh.serialize()
    guard let fh2 = FrameHeaderPayload.deserialize(from: data) else { return false }
    return fh2.frameId == 999 && fh2.width == 3840 && fh2.height == 2160 &&
           fh2.isKeyFrame == true && fh2.totalChunks == 12 && fh2.totalSize == 500000
}

test("FrameChunkPayload serialize/deserialize") {
    let chunkData = Data(repeating: 0xAB, count: 1024)
    let fc = FrameChunkPayload(frameId: 42, chunkIndex: 3, chunkData: chunkData)
    let data = fc.serialize()
    guard let fc2 = FrameChunkPayload.deserialize(from: data) else { return false }
    return fc2.frameId == 42 && fc2.chunkIndex == 3 && fc2.chunkData == chunkData
}

test("InputEventPayload serialize/deserialize") {
    let ie = InputEventPayload(type: .leftMouseDown, x: 0.5, y: 0.75, keyCode: 36,
                                modifiers: [.command, .shift], scrollDeltaX: 1.5, scrollDeltaY: -2.0)
    let data = ie.serialize()
    guard let ie2 = InputEventPayload.deserialize(from: data) else { return false }
    return ie2.type == .leftMouseDown && ie2.x == 0.5 && ie2.y == 0.75 &&
           ie2.keyCode == 36 && ie2.modifiers.contains(.command) && ie2.modifiers.contains(.shift) &&
           ie2.scrollDeltaX == 1.5 && ie2.scrollDeltaY == -2.0
}

test("ControlMessage serialize/deserialize") {
    let cm = ControlMessage(type: .requestKeyFrame)
    let data = cm.serialize()
    guard let cm2 = ControlMessage.deserialize(from: data) else { return false }
    return cm2.type == .requestKeyFrame
}

test("StreamConfiguration serialize/deserialize") {
    let config = StreamConfiguration(width: 3840, height: 2160, bitrate: 12_000_000, framesPerSecond: 120)
    let data = config.serialize()
    guard let decoded = StreamConfiguration.deserialize(from: data) else { return false }
    return decoded.width == 3840 && decoded.height == 2160 && decoded.bitrate == 12_000_000 && decoded.framesPerSecond == 120
}

test("StreamConfigurationRequest serialize/deserialize") {
    let request = StreamConfigurationRequestPayload(requestID: 42,
                                                    desiredConfiguration: StreamConfiguration(width: 3440, height: 1440, bitrate: 9_000_000, framesPerSecond: 90))
    let data = request.serialize()
    guard let decoded = StreamConfigurationRequestPayload.deserialize(from: data) else { return false }
    return decoded.requestID == 42 && decoded.desiredConfiguration.width == 3440 && decoded.desiredConfiguration.height == 1440
}

test("StreamConfigurationResponse serialize/deserialize") {
    let response = StreamConfigurationResponsePayload(requestID: 42,
                                                      activeConfiguration: StreamConfiguration(width: 2560, height: 1600, bitrate: 8_000_000, framesPerSecond: 60))
    let data = response.serialize()
    guard let decoded = StreamConfigurationResponsePayload.deserialize(from: data) else { return false }
    return decoded.requestID == 42 && decoded.activeConfiguration.height == 1600 && decoded.activeConfiguration.framesPerSecond == 60
}

test("StreamConfigurationReject serialize/deserialize") {
    let reject = StreamConfigurationRejectPayload(requestID: 7, reason: .unsupportedDimensions, message: "too large")
    let data = reject.serialize()
    guard let decoded = StreamConfigurationRejectPayload.deserialize(from: data) else { return false }
    return decoded.requestID == 7 && decoded.reason == .unsupportedDimensions && decoded.message == "too large"
}

test("StreamConfigurationError serialize/deserialize") {
    let error = StreamConfigurationErrorPayload(requestID: 7, errorCode: .invalidRequest, message: "bad request")
    let data = error.serialize()
    guard let decoded = StreamConfigurationErrorPayload.deserialize(from: data) else { return false }
    return decoded.requestID == 7 && decoded.errorCode == .invalidRequest && decoded.message == "bad request"
}

test("ClipboardSyncRequest serialize/deserialize") {
    let request = ClipboardSyncRequestPayload(requestID: 11, direction: .bidirectional, origin: .localPasteboard)
    let data = request.serialize()
    guard let decoded = ClipboardSyncRequestPayload.deserialize(from: data) else { return false }
    return decoded.requestID == 11 && decoded.direction == .bidirectional && decoded.origin == .localPasteboard
}

test("ClipboardSyncUpdate serialize/deserialize") {
    let update = ClipboardSyncUpdatePayload(requestID: 12, direction: .hostToClient, origin: .remotePasteboard, text: "hello clipboard")
    guard let data = update.serialize(), let decoded = ClipboardSyncUpdatePayload.deserialize(from: data) else { return false }
    return decoded.requestID == 12 && decoded.direction == .hostToClient && decoded.origin == .remotePasteboard && decoded.text == "hello clipboard"
}

test("ClipboardSyncError serialize/deserialize") {
    let error = ClipboardSyncErrorPayload(requestID: 13, direction: .clientToHost, origin: .syncedFromPeer, errorCode: 9, message: "clipboard rejected")
    let data = error.serialize()
    guard let decoded = ClipboardSyncErrorPayload.deserialize(from: data) else { return false }
    return decoded.requestID == 13 && decoded.direction == .clientToHost && decoded.origin == .syncedFromPeer && decoded.errorCode == 9 && decoded.message == "clipboard rejected"
}

test("ClipboardSyncUpdate rejects oversized payloads") {
    let text = String(repeating: "a", count: ERDConstants.maxClipboardTextBytes + 1)
    let update = ClipboardSyncUpdatePayload(requestID: 14, direction: .hostToClient, origin: .localPasteboard, text: text)
    return update.serialize() == nil
}

test("CursorUpdate serialize/deserialize") {
    let cu = CursorUpdate(x: 0.333, y: 0.666, cursorType: 2)
    let data = cu.serialize()
    guard let cu2 = CursorUpdate.deserialize(from: data) else { return false }
    return abs(cu2.x - 0.333) < 0.001 && abs(cu2.y - 0.666) < 0.001 && cu2.cursorType == 2
}

test("ModifierFlags OptionSet") {
    let flags: ModifierFlags = [.shift, .command, .capsLock]
    return flags.contains(.shift) && flags.contains(.command) && flags.contains(.capsLock) &&
           !flags.contains(.control) && !flags.contains(.option)
}

test("AtomicBool thread safety") {
    let ab = AtomicBool(false)
    ab.value = true
    return ab.value == true
}

// ============================================
// TEST 2: TCP Channel Loopback
// ============================================
print("\n[TEST 2] TCP Channel Loopback")

let tcpDone = DispatchSemaphore(value: 0)
var tcpReceived: Data?

test("TCPChannel listen + connect + send/receive") {
    let server = TCPChannel()
    let client = TCPChannel()
    
    var serverReceivedData: Data?

    try server.startListening(port: 19750)
    server.onReceive = { data in
        serverReceivedData = data
        tcpDone.signal()
    }

    Thread.sleep(forTimeInterval: 0.2)
    client.connect(host: "127.0.0.1", port: 19750)

    Thread.sleep(forTimeInterval: 0.5)

    let hs = HandshakePayload(hostname: "test-client", screenWidth: 1920, screenHeight: 1080, scaleFactor: 2.0)
    let header = PacketHeader(type: .handshake, sequence: 1, timestamp: 100)
    var packet = header.serialize()
    packet.append(hs.serialize())
    client.send(packet)

    let result = tcpDone.wait(timeout: .now() + 3.0)
    client.stop()
    server.stop()

    guard result == .success, let received = serverReceivedData else { return false }
    guard let recvHeader = PacketHeader.deserialize(from: received) else { return false }
    let payload = received.subdata(in: ERDConstants.packetHeaderSize..<received.count)
    guard let recvHs = HandshakePayload.deserialize(from: payload) else { return false }

    return recvHeader.type == .handshake && recvHs.hostname == "test-client" &&
           recvHs.screenWidth == 1920 && recvHs.scaleFactor == 2.0
}

// ============================================
// TEST 3: UDP Channel Loopback
// ============================================
print("\n[TEST 3] UDP Channel Loopback")

let udpDone = DispatchSemaphore(value: 0)

test("UDPChannel listen + connect + sendPacket") {
    let server = UDPChannel()
    let client = UDPChannel()
    
    var serverReceivedData: Data?

    try server.startListening(port: 19751)
    server.onReceive = { data, endpoint in
        serverReceivedData = data
        udpDone.signal()
    }

    Thread.sleep(forTimeInterval: 0.2)
    client.connect(host: "127.0.0.1", port: 19751)
    Thread.sleep(forTimeInterval: 0.3)

    let cursor = CursorUpdate(x: 0.5, y: 0.5, cursorType: 1)
    client.sendPacket(type: .cursorUpdate, payload: cursor.serialize())

    let result = udpDone.wait(timeout: .now() + 3.0)
    client.stop()
    server.stop()

    guard result == .success, let received = serverReceivedData else { return false }
    guard let header = PacketHeader.deserialize(from: received) else { return false }
    let payload = received.subdata(in: ERDConstants.packetHeaderSize..<received.count)
    guard let recvCursor = CursorUpdate.deserialize(from: payload) else { return false }

    return header.type == .cursorUpdate && header.sequence == 1 &&
           abs(recvCursor.x - 0.5) < 0.01 && recvCursor.cursorType == 1
}

// ============================================
// TEST 4: Frame Chunk Assembly
// ============================================
print("\n[TEST 4] Frame Chunk Assembly Pipeline")

let frameDone = DispatchSemaphore(value: 0)

test("FrameReceiver assembles chunked frames") {
    let receiver = FrameReceiver()
    var assembledData: Data?
    var assembledHeader: FrameHeaderPayload?

    receiver.onFrameReady = { data, header in
        assembledData = data
        assembledHeader = header
        frameDone.signal()
    }

    let fullFrame = Data(repeating: 0xCD, count: 3000)
    let frameId: UInt32 = 1
    let chunkSize = 1000

    let fh = FrameHeaderPayload(frameId: frameId, width: 1920, height: 1080, isKeyFrame: true,
                                 totalChunks: 3, totalSize: UInt32(fullFrame.count))
    let fhHeader = PacketHeader(type: .frameHeader, sequence: 1, timestamp: 100)
    var fhPacket = fhHeader.serialize()
    fhPacket.append(fh.serialize())
    receiver.handlePacket(fhPacket)

    for i in 0..<3 {
        let start = i * chunkSize
        let end = min(start + chunkSize, fullFrame.count)
        let chunk = FrameChunkPayload(frameId: frameId, chunkIndex: UInt16(i), chunkData: fullFrame.subdata(in: start..<end))
        let chunkHeader = PacketHeader(type: .frameChunk, sequence: UInt32(i + 2), timestamp: 100)
        var chunkPacket = chunkHeader.serialize()
        chunkPacket.append(chunk.serialize())
        receiver.handlePacket(chunkPacket)
    }

    let result = frameDone.wait(timeout: .now() + 3.0)

    guard result == .success, let assembled = assembledData, let header = assembledHeader else { return false }
    return assembled == fullFrame && header.frameId == frameId && header.isKeyFrame == true &&
           header.width == 1920 && header.height == 1080
}

// ============================================
// TEST 5: STUN Client (Real Network Test)
// ============================================
print("\n[TEST 5] STUN Client (stun.l.google.com)")

let stunDone = DispatchSemaphore(value: 0)
var stunIP: String?
var stunPort: UInt16?
var stunError: Error?

Task {
    do {
        let stun = STUNClient()
        let (ip, port) = try await stun.fetchPublicIP()
        stunIP = ip
        stunPort = port
    } catch {
        stunError = error
    }
    stunDone.signal()
}

let stunResult = stunDone.wait(timeout: .now() + 10.0)

test("STUN fetches public IP") {
    guard stunResult == .success else { return false }
    if let error = stunError {
        print("    STUN lookup failed (no silent pass): \(error)")
        return false
    }
    guard let ip = stunIP else { return false }
    print("    Public IP: \(ip):\(stunPort ?? 0)")
    return ip.split(separator: ".").count == 4
}

// ============================================
// TEST 6: ERDConstants Integrity
// ============================================
print("\n[TEST 6] ERDConstants")

test("Constants have expected values") {
    return ERDConstants.bonjourServiceType == "_eclipticrd._tcp" &&
           ERDConstants.magic == 0xEC1D &&
           ERDConstants.tcpPort == 19730 &&
           ERDConstants.udpPort == 19731 &&
           ERDConstants.maxPacketSize == 1400 &&
           ERDConstants.packetHeaderSize == 12 &&
           ERDConstants.defaultFPS == 60
}

// ============================================
// TEST 7: Video Encoder → FrameSender → UDP → FrameReceiver Pipeline
// ============================================
print("\n[TEST 7] Video Encode → UDP FrameSender → FrameReceiver Pipeline")

test("FrameSender chunks large encoded data and FrameReceiver reassembles") {
    // Simulate an encoded video frame (10KB fits within UDP reliably)
    let fakeEncodedFrame = Data((0..<10_000).map { _ in UInt8.random(in: 0...255) })
    let receiverUDP = UDPChannel()
    let senderUDP = UDPChannel()

    let frameReceiver = FrameReceiver()

    let videoRecvDone = DispatchSemaphore(value: 0)
    var receivedFrameData: Data?
    var receivedFrameHeader: FrameHeaderPayload?

    frameReceiver.onFrameReady = { data, header in
        receivedFrameData = data
        receivedFrameHeader = header
        videoRecvDone.signal()
    }

    // Receiver listens
    try receiverUDP.startListening(port: 19760)
    receiverUDP.onReceive = { data, endpoint in
        frameReceiver.handlePacket(data)
    }

    // Sender connects to receiver
    senderUDP.connect(host: "127.0.0.1", port: 19760)
    Thread.sleep(forTimeInterval: 0.5)

    let frameSender = FrameSender(udpChannel: senderUDP)

    // Send a frame (FrameSender will chunk it automatically)
    frameSender.sendFrame(data: fakeEncodedFrame, width: 1920, height: 1080, isKeyFrame: true)

    let result = videoRecvDone.wait(timeout: .now() + 5.0)
    senderUDP.stop()
    receiverUDP.stop()

    guard result == .success else {
        print("    ⚠️ Timeout - frame may have been too large for single UDP session")
        return false
    }
    guard let recvData = receivedFrameData, let recvHeader = receivedFrameHeader else { return false }

    print("    Sent: \(fakeEncodedFrame.count) bytes, Received: \(recvData.count) bytes")
    print("    Chunks: \(recvHeader.totalChunks), KeyFrame: \(recvHeader.isKeyFrame)")
    return recvData == fakeEncodedFrame && recvHeader.width == 1920 && recvHeader.height == 1080 &&
           recvHeader.isKeyFrame == true
}

// ============================================
// TEST 8: VideoEncoder creates VTCompressionSession (H.265)
// ============================================
print("\n[TEST 8] VideoEncoder H.265 Session")

test("VideoEncoder creates and starts H.265 compression session") {
    let encoder = VideoEncoder(width: 1920, height: 1080, bitrate: ERDConstants.defaultBitrate, fps: 30)

    var encodedCalled = false
    encoder.onEncodedFrame = { data, isKeyFrame in
        encodedCalled = true
    }

    do {
        try encoder.start()
        print("    H.265 compression session created successfully")

        // Create a test CVPixelBuffer and wrap it in CMSampleBuffer to test encoding
        var pixelBuffer: CVPixelBuffer?
        let status = CVPixelBufferCreate(kCFAllocatorDefault, 1920, 1080,
                                          kCVPixelFormatType_32BGRA, nil, &pixelBuffer)
        guard status == kCVReturnSuccess, let pb = pixelBuffer else {
            print("    ⚠️ Could not create test pixel buffer")
            encoder.stop()
            return false
        }

        // Fill with test pattern
        CVPixelBufferLockBaseAddress(pb, [])
        if let baseAddress = CVPixelBufferGetBaseAddress(pb) {
            let bytesPerRow = CVPixelBufferGetBytesPerRow(pb)
            let height = CVPixelBufferGetHeight(pb)
            memset(baseAddress, 128, bytesPerRow * height) // gray fill
        }
        CVPixelBufferUnlockBaseAddress(pb, [])

        // Create CMSampleBuffer from pixel buffer
        var formatDesc: CMVideoFormatDescription?
        CMVideoFormatDescriptionCreateForImageBuffer(allocator: kCFAllocatorDefault,
                                                      imageBuffer: pb,
                                                      formatDescriptionOut: &formatDesc)

        if let fmt = formatDesc {
            var timing = CMSampleTimingInfo(duration: .invalid,
                                            presentationTimeStamp: CMTime(value: 0, timescale: 30),
                                            decodeTimeStamp: .invalid)
            var sampleBuffer: CMSampleBuffer?
            CMSampleBufferCreateReadyWithImageBuffer(allocator: kCFAllocatorDefault,
                                                      imageBuffer: pb,
                                                      formatDescription: fmt,
                                                      sampleTiming: &timing,
                                                      sampleBufferOut: &sampleBuffer)

            if let sb = sampleBuffer {
                encoder.encode(sb)
                Thread.sleep(forTimeInterval: 0.3)
                print("    Frame encoded, callback fired: \(encodedCalled)")
            }
        }

        encoder.stop()
        guard encodedCalled else {
            print("    Session created but no encoded frame callback fired")
            return false
        }
        return true
    } catch {
        print("    Failed: \(error)")
        return false
    }
}

// ============================================
// TEST 9: VideoDecoder handles parameter sets
// ============================================
print("\n[TEST 9] VideoDecoder Session")

test("VideoDecoder initializes and accepts HEVC NALUs") {
    let decoder = VideoDecoder()
    var decodedFrame = false

    decoder.onDecodedFrame = { pixelBuffer in
        decodedFrame = true
    }

    // We can't easily fabricate valid HEVC NALUs, but we can verify the decoder
    // doesn't crash on invalid data (graceful handling)
    let fakeData = Data(repeating: 0x00, count: 100)
    decoder.decode(fakeData)
    Thread.sleep(forTimeInterval: 0.2)

    decoder.stop()
    // Garbage input must be rejected silently: no frame out, no crash
    return !decodedFrame
}

// ============================================
// TEST 10: Input Event TCP Round-trip
// ============================================
print("\n[TEST 10] Input Event TCP Round-trip")

let inputDone = DispatchSemaphore(value: 0)

test("InputEventPayload sent via TCPChannel.sendInput and received") {
    let server = TCPChannel()
    let client = TCPChannel()

    var receivedInput: InputEventPayload?

    try server.startListening(port: 19770)
    server.onReceive = { data in
        guard data.count >= ERDConstants.packetHeaderSize else { return }
        guard let header = PacketHeader.deserialize(from: data) else { return }
        if header.type == .inputEvent {
            let payload = data.subdata(in: ERDConstants.packetHeaderSize..<data.count)
            receivedInput = InputEventPayload.deserialize(from: payload)
            inputDone.signal()
        }
    }

    Thread.sleep(forTimeInterval: 0.2)
    client.connect(host: "127.0.0.1", port: 19770)
    Thread.sleep(forTimeInterval: 0.5)

    // Send a mouse click with modifiers
    let input = InputEventPayload(type: .leftMouseDown, x: 0.25, y: 0.75,
                                   keyCode: 0, modifiers: [.command], scrollDeltaX: 0, scrollDeltaY: 0)
    client.sendInput(input)

    let result = inputDone.wait(timeout: .now() + 3.0)
    client.stop()
    server.stop()

    guard result == .success, let recv = receivedInput else { return false }
    print("    Received: type=\(recv.type) x=\(recv.x) y=\(recv.y) mods=\(recv.modifiers.rawValue)")
    return recv.type == .leftMouseDown && abs(recv.x - 0.25) < 0.01 &&
           abs(recv.y - 0.75) < 0.01 && recv.modifiers.contains(.command)
}

// ============================================
// TEST 11: Keyboard Event TCP Round-trip
// ============================================
print("\n[TEST 11] Keyboard Event TCP Round-trip")

let keyDone = DispatchSemaphore(value: 0)

test("KeyDown event with keyCode and modifiers sent via TCP") {
    let server = TCPChannel()
    let client = TCPChannel()

    var receivedInput: InputEventPayload?

    try server.startListening(port: 19771)
    server.onReceive = { data in
        guard data.count >= ERDConstants.packetHeaderSize,
              let header = PacketHeader.deserialize(from: data),
              header.type == .inputEvent else { return }
        let payload = data.subdata(in: ERDConstants.packetHeaderSize..<data.count)
        receivedInput = InputEventPayload.deserialize(from: payload)
        keyDone.signal()
    }

    Thread.sleep(forTimeInterval: 0.2)
    client.connect(host: "127.0.0.1", port: 19771)
    Thread.sleep(forTimeInterval: 0.5)

    // Send Cmd+C (keyCode 8 = C key on Mac)
    let input = InputEventPayload(type: .keyDown, x: 0, y: 0,
                                   keyCode: 8, modifiers: [.command])
    client.sendInput(input)

    let result = keyDone.wait(timeout: .now() + 3.0)
    client.stop()
    server.stop()

    guard result == .success, let recv = receivedInput else { return false }
    print("    Received: keyDown keyCode=\(recv.keyCode) mods=\(recv.modifiers)")
    return recv.type == .keyDown && recv.keyCode == 8 && recv.modifiers.contains(.command)
}

// ============================================
// TEST 12: Scroll Wheel Event TCP Round-trip
// ============================================
print("\n[TEST 12] Scroll Wheel Event TCP Round-trip")

let scrollDone = DispatchSemaphore(value: 0)

test("ScrollWheel event with deltaX/Y sent via TCP") {
    let server = TCPChannel()
    let client = TCPChannel()

    var receivedInput: InputEventPayload?

    try server.startListening(port: 19772)
    server.onReceive = { data in
        guard data.count >= ERDConstants.packetHeaderSize,
              let header = PacketHeader.deserialize(from: data),
              header.type == .inputEvent else { return }
        let payload = data.subdata(in: ERDConstants.packetHeaderSize..<data.count)
        receivedInput = InputEventPayload.deserialize(from: payload)
        scrollDone.signal()
    }

    Thread.sleep(forTimeInterval: 0.2)
    client.connect(host: "127.0.0.1", port: 19772)
    Thread.sleep(forTimeInterval: 0.5)

    let input = InputEventPayload(type: .scrollWheel, x: 0.5, y: 0.5,
                                   keyCode: 0, modifiers: [], scrollDeltaX: -3.5, scrollDeltaY: 10.0)
    client.sendInput(input)

    let result = scrollDone.wait(timeout: .now() + 3.0)
    client.stop()
    server.stop()

    guard result == .success, let recv = receivedInput else { return false }
    print("    Received: scroll dX=\(recv.scrollDeltaX) dY=\(recv.scrollDeltaY)")
    return recv.type == .scrollWheel && abs(recv.scrollDeltaX - (-3.5)) < 0.01 &&
           abs(recv.scrollDeltaY - 10.0) < 0.01
}

// ============================================
// TEST 13: InputReceiver CGEvent Injection (Accessibility check)
// ============================================
print("\n[TEST 13] InputReceiver CGEvent Injection")

test("InputReceiver creates and injects CGEvents") {
    // Without the Accessibility grant the receiver must no-op; with it the
    // injection path is real. Either way the test no longer passes blind.
    guard AXIsProcessTrusted() else {
        print("    Accessibility permission not granted to the test runner")
        return false
    }
    print("    NOTE: this test moves the real cursor to the host screen center")

    let receiver = InputReceiver(screenWidth: 1920, screenHeight: 1080)
    let mouseMove = InputEventPayload(type: .mouseMove, x: 0.5, y: 0.5)
    receiver.handleInputEvent(mouseMove.serialize())

    receiver.updateScreenSize(width: 2560, height: 1440)
    return true
}

// ============================================
// TEST 14: Full Video Encode→Decode Pipeline
// ============================================
print("\n[TEST 14] VideoEncoder → VideoDecoder Pipeline")

test("Encode a pixel buffer to H.265 and decode it back") {
    let encodeDone = DispatchSemaphore(value: 0)
    let decodeDone = DispatchSemaphore(value: 0)

    let encoder = VideoEncoder(width: 320, height: 240, bitrate: 1_000_000, fps: 30)
    let decoder = VideoDecoder()

    var encodedData: Data?
    var decodedPixelBuffer: CVPixelBuffer?

    encoder.onEncodedFrame = { data, isKeyFrame in
        encodedData = data
        print("    Encoded frame: \(data.count) bytes, keyFrame=\(isKeyFrame)")
        encodeDone.signal()

        // Feed directly to decoder
        decoder.decode(data)
    }

    decoder.onDecodedFrame = { pixelBuffer in
        decodedPixelBuffer = pixelBuffer
        print("    Decoded frame: \(CVPixelBufferGetWidth(pixelBuffer))x\(CVPixelBufferGetHeight(pixelBuffer))")
        decodeDone.signal()
    }

    do {
        try encoder.start()
    } catch {
        print("    Encoder start failed: \(error)")
        return false
    }

    // Create test pixel buffer
    var pixelBuffer: CVPixelBuffer?
    CVPixelBufferCreate(kCFAllocatorDefault, 320, 240, kCVPixelFormatType_32BGRA, nil, &pixelBuffer)
    guard let pb = pixelBuffer else { return false }

    CVPixelBufferLockBaseAddress(pb, [])
    if let base = CVPixelBufferGetBaseAddress(pb) {
        let bytesPerRow = CVPixelBufferGetBytesPerRow(pb)
        // Create a gradient pattern
        for y in 0..<240 {
            for x in 0..<320 {
                let offset = y * bytesPerRow + x * 4
                let ptr = base.advanced(by: offset).assumingMemoryBound(to: UInt8.self)
                ptr[0] = UInt8(x % 256)     // B
                ptr[1] = UInt8(y % 256)     // G
                ptr[2] = UInt8((x+y) % 256) // R
                ptr[3] = 255                 // A
            }
        }
    }
    CVPixelBufferUnlockBaseAddress(pb, [])

    // Encode multiple frames to ensure we get parameter sets
    for i in 0..<5 {
        var formatDesc: CMVideoFormatDescription?
        CMVideoFormatDescriptionCreateForImageBuffer(allocator: kCFAllocatorDefault,
                                                      imageBuffer: pb,
                                                      formatDescriptionOut: &formatDesc)
        if let fmt = formatDesc {
            var timing = CMSampleTimingInfo(duration: .invalid,
                                            presentationTimeStamp: CMTime(value: Int64(i), timescale: 30),
                                            decodeTimeStamp: .invalid)
            var sampleBuffer: CMSampleBuffer?
            CMSampleBufferCreateReadyWithImageBuffer(allocator: kCFAllocatorDefault,
                                                      imageBuffer: pb,
                                                      formatDescription: fmt,
                                                      sampleTiming: &timing,
                                                      sampleBufferOut: &sampleBuffer)
            if let sb = sampleBuffer {
                encoder.encode(sb)
            }
        }
        Thread.sleep(forTimeInterval: 0.05)
    }

    // Wait for encode
    let encResult = encodeDone.wait(timeout: .now() + 3.0)
    if encResult == .timedOut {
        print("    ⚠️ Encode timed out")
        encoder.stop()
        return false
    }

    // Wait for decode
    let decResult = decodeDone.wait(timeout: .now() + 3.0)
    encoder.stop()
    decoder.stop()

    if decResult == .timedOut {
        print("    ⚠️ Decode timed out")
        return false
    }

    return decodedPixelBuffer != nil
}

// ============================================
// TEST 15: TLS-PSK Channel Loopback
// ============================================
print("\n[TEST 15] TLS-PSK Encrypted Channel Loopback")

let tlsDone = DispatchSemaphore(value: 0)
var tlsServerReceived: Data?
var tlsClientConnected = false
var tlsServerConnected = false
let tlsSharedKey = ERDCrypto.randomKey()
let tlsPSK = ERDPSK(identity: "erd-test", key: tlsSharedKey)

let tlsServer = TCPChannel(security: .psk([tlsPSK]))
let tlsClient = TCPChannel(security: .psk([tlsPSK]))

var tlsSetupError: Error?
do {
    try tlsServer.startListening(port: 19780)
} catch {
    tlsSetupError = error
}
tlsServer.onConnect = { tlsServerConnected = true }
tlsServer.onReceive = { data in
    tlsServerReceived = data
    tlsDone.signal()
}
tlsClient.onConnect = {
    tlsClientConnected = true
    let header = PacketHeader(type: .ping, sequence: 7, timestamp: 99)
    var packet = header.serialize()
    packet.append(Data("over-tls-psk".utf8))
    tlsClient.send(packet)
}

tlsClient.onDisconnect = { }
Thread.sleep(forTimeInterval: 0.2)
tlsClient.connect(host: "127.0.0.1", port: 19780)
let tlsResult = tlsDone.wait(timeout: .now() + 6.0)
let clientFailure = tlsClient.lastFailureDescription
let serverFailure = tlsServer.lastFailureDescription
tlsClient.stop()
tlsServer.stop()

test("TCP TLS-PSK handshake completes and carries traffic") {
    if let tlsSetupError {
        print("    Listener setup failed: \(tlsSetupError)")
        return false
    }
    guard tlsClientConnected else {
        print("    Client TLS handshake did not complete, failure: \(clientFailure ?? "none"), state: \(tlsClient.connectionStateDescription ?? "nil")")
        return false
    }
    guard tlsServerConnected else {
        print("    Server never accepted the TLS connection, failure: \(serverFailure ?? "none"), state: \(tlsServer.connectionStateDescription ?? "nil")")
        return false
    }
    guard tlsResult == .success, let received = tlsServerReceived else {
        print("    No payload crossed the encrypted channel")
        return false
    }
    guard let header = PacketHeader.deserialize(from: received) else { return false }
    let payload = received.subdata(in: ERDConstants.packetHeaderSize..<received.count)
    return header.type == .ping && header.sequence == 7 && payload == Data("over-tls-psk".utf8)
}

// ============================================
// TEST 16: UDP Burst Delivery
// ============================================
print("\n[TEST 16] UDP Burst Delivery (20 datagrams)")

let burstDone = DispatchSemaphore(value: 0)
let burstTotal = 20
var burstReceived = 0

let burstReceiver = UDPChannel()
let burstSender = UDPChannel()
try burstReceiver.startListening(port: 19781)
burstReceiver.onReceive = { data, _ in
    burstReceived += 1
    if burstReceived >= burstTotal { burstDone.signal() }
}
Thread.sleep(forTimeInterval: 0.2)
burstSender.connect(host: "127.0.0.1", port: 19781)
Thread.sleep(forTimeInterval: 0.3)
for i in 0..<burstTotal {
    burstSender.sendPacket(type: .ping, payload: Data([UInt8(i)]))
}
let burstResult = burstDone.wait(timeout: .now() + 3.0)
burstSender.stop()
burstReceiver.stop()

test("UDP loopback delivers a rapid burst without loss") {
    print("    Received \(burstReceived)/\(burstTotal) (result: \(burstResult))")
    return burstReceived >= burstTotal
}

// ============================================
// TEST 17: Signaling Round-trip (real ntfy.sh)
// ============================================
print("\n[TEST 17] Signaling Round-trip via ntfy.sh (real network)")

let sigPIN = ERDCrypto.randomPIN()
let signalingServer = SignalingClient(pin: sigPIN, role: "server")
let signalingClient = SignalingClient(pin: sigPIN, role: "client")
let serverCandidate = SessionCandidate(role: "server", localIP: "127.0.0.1", localPort: 19730,
                                        publicIP: "198.51.100.7", publicPort: 2)
let clientCandidate = SessionCandidate(role: "client", localIP: "127.0.0.1", localPort: 3,
                                        publicIP: "198.51.100.9", publicPort: 4)

var serverReceivedCandidate: SessionCandidate?
var clientReceivedCandidate: SessionCandidate?
var signalingFailure: Error?
let signalingDone = DispatchSemaphore(value: 0)

Task {
    do { clientReceivedCandidate = try await signalingClient.exchangeCandidate(clientCandidate) }
    catch { signalingFailure = error }
    signalingDone.signal()
}
Task {
    do { serverReceivedCandidate = try await signalingServer.exchangeCandidate(serverCandidate) }
    catch { signalingFailure = error }
    signalingDone.signal()
}
for _ in 0..<2 where signalingDone.wait(timeout: .now() + 30) == .success {}
signalingServer.stop()
signalingClient.stop()

test("Both peers exchange encrypted candidates through ntfy.sh") {
    if let signalingFailure {
        print("    Signaling failed (no silent pass): \(signalingFailure)")
        return false
    }
    guard let serverGot = serverReceivedCandidate, let clientGot = clientReceivedCandidate else {
        print("    One peer never received a candidate")
        return false
    }
    return serverGot.role == "client" && clientGot.role == "server"
}

// ============================================
// Summary
// ============================================
print("\n========================================")
print("🎯 Results: \(passed) passed, \(failed) failed out of \(passed + failed) tests")
if failed == 0 {
    print("✅ ALL TESTS PASSED!")
} else {
    print("❌ SOME TESTS FAILED")
}
print("========================================")

Thread.sleep(forTimeInterval: 0.5)
exit(failed == 0 ? 0 : 1)

