import XCTest

// Crypto + wire-protocol unit tests: HKDF (RFC 5869), AES-GCM datagram
// cipher with replay window, bounds-checked parsing, handshake v3, pairing.

final class HKDFTests: XCTestCase {
    // RFC 5869 Appendix A, Test Case 1 (SHA-256)
    func testRFC5869Case1() {
        let ikm = Data(repeating: 0x0b, count: 22)
        let salt = Data([0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c])
        let info = Data([0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9])
        let okm = HKDF.derive(ikm: ikm, salt: salt, info: info, length: 42)
        let expected = Data([
            0x3c, 0xb2, 0x5f, 0x25, 0xfa, 0xac, 0xd5, 0x7a, 0x90, 0x43, 0x4f, 0x64, 0xd0, 0x36, 0x2f, 0x2a,
            0x2d, 0x2d, 0x0a, 0x90, 0xcf, 0x1a, 0x5a, 0x4c, 0x5d, 0xb0, 0x2d, 0x56, 0xec, 0xc4, 0xc5, 0xbf,
            0x34, 0x00, 0x72, 0x08, 0xd5, 0xb8, 0x87, 0x18, 0x58, 0x65])
        XCTAssertEqual(okm, expected)
    }

    // RFC 5869 Appendix A, Test Case 3 (SHA-256, empty salt/info)
    func testRFC5869Case3() {
        let ikm = Data(repeating: 0x0b, count: 22)
        let okm = HKDF.derive(ikm: ikm, salt: Data(), info: Data(), length: 42)
        let expected = Data([
            0x8d, 0xa4, 0xe7, 0x75, 0xa5, 0x63, 0xc1, 0x8f, 0x71, 0x5f, 0x80, 0x2a, 0x06, 0x3c, 0x5a, 0x31,
            0xb8, 0xa1, 0x1f, 0x5c, 0x5e, 0xe1, 0x87, 0x9e, 0xc3, 0x45, 0x4e, 0x5f, 0x3c, 0x73, 0x8d, 0x2d,
            0x9d, 0x20, 0x13, 0x95, 0xfa, 0xa4, 0xb6, 0x1a, 0x96, 0xc8])
        XCTAssertEqual(okm, expected)
    }
}

final class DatagramCipherTests: XCTestCase {
    private func makeMatchingPair() -> (sender: DatagramCipher, receiver: DatagramCipher) {
        let master = ERDCrypto.randomKey()
        let salt = ERDCrypto.randomKey().prefix(16)
        let sender = DatagramCipher.udpCipher(masterKey: master, sessionSalt: salt, clientToHost: true)
        let receiver = DatagramCipher.udpCipher(masterKey: master, sessionSalt: salt, clientToHost: true)
        return (sender, receiver)
    }

    private let aad = Data("erd-packet-header".utf8)

    func testSealOpenRoundtrip() {
        let (sender, receiver) = makeMatchingPair()
        let plaintext = Data((0..<1000).map { _ in UInt8.random(in: 0...255) })
        let datagram = sender.seal(plaintext, aad: aad)
        XCTAssertNotNil(datagram)
        XCTAssertEqual(datagram!.count, plaintext.count + DatagramCipher.nonceSize + DatagramCipher.tagSize)
        XCTAssertEqual(receiver.open(datagram!, aad: aad), plaintext)
    }

    func testTamperedDatagramRejected() {
        let (sender, receiver) = makeMatchingPair()
        var datagram = sender.seal(Data("payload".utf8), aad: aad)!
        datagram[datagram.count - 1] ^= 0xFF
        XCTAssertNil(receiver.open(datagram, aad: aad))
    }

    func testAADMismatchRejected() {
        let (sender, receiver) = makeMatchingPair()
        let datagram = sender.seal(Data("payload".utf8), aad: aad)
        XCTAssertNil(receiver.open(datagram!, aad: Data("different".utf8)))
    }

    func testDuplicateDatagramRejected() {
        let (sender, receiver) = makeMatchingPair()
        let datagram = sender.seal(Data("once".utf8), aad: aad)!
        XCTAssertNotNil(receiver.open(datagram, aad: aad))
        XCTAssertNil(receiver.open(datagram, aad: aad))
    }

    func testStaleDatagramOutsideWindowRejected() {
        let (sender, receiver) = makeMatchingPair()
        var datagrams: [Data] = []
        for _ in 0...Int(DatagramCipher.windowSize) + 8 {
            datagrams.append(sender.seal(Data("stale".utf8), aad: aad)!)
        }
        // Receiver advances to the newest packet first
        XCTAssertNotNil(receiver.open(datagrams.last!, aad: aad))
        // The oldest packet is now far outside the replay window
        XCTAssertNil(receiver.open(datagrams.first!, aad: aad))
    }

    func testDirectionKeysAreIndependent() {
        let master = ERDCrypto.randomKey()
        let salt = ERDCrypto.randomKey().prefix(16)
        let clientSend = DatagramCipher.udpCipher(masterKey: master, sessionSalt: salt, clientToHost: true)
        let wrongReceiver = DatagramCipher.udpCipher(masterKey: master, sessionSalt: salt, clientToHost: false)
        let datagram = clientSend.seal(Data("cross".utf8), aad: aad)
        XCTAssertNil(wrongReceiver.open(datagram!, aad: aad))
    }
}

final class PacketBoundsTests: XCTestCase {
    func testTruncatedPacketHeaderRejected() {
        let header = PacketHeader(type: .frameHeader, sequence: 1, timestamp: 2)
        let data = header.serialize()
        XCTAssertNil(PacketHeader.deserialize(from: data.prefix(ERDConstants.packetHeaderSize - 1)))
    }

    func testUnknownPacketTypeRejected() {
        var data = PacketHeader(type: .ping, sequence: 1, timestamp: 2).serialize()
        data[2] = 0xEE
        XCTAssertNil(PacketHeader.deserialize(from: data))
    }

    func testOversizedFrameTotalSizeRejected() {
        let header = FrameHeaderPayload(frameId: 1, width: 16, height: 16, isKeyFrame: true,
                                        totalChunks: 2, totalSize: UInt32(ERDConstants.maxFrameBytes + 1))
        XCTAssertNil(FrameHeaderPayload.deserialize(from: header.serialize()))
    }

    func testOversizedChunkCountRejected() {
        let header = FrameHeaderPayload(frameId: 1, width: 16, height: 16, isKeyFrame: true,
                                        totalChunks: ERDConstants.maxChunksPerFrame + 1, totalSize: 1024)
        XCTAssertNil(FrameHeaderPayload.deserialize(from: header.serialize()))
    }

    func testHandshakeV3RoundtripCarriesIdentityAndSalt() {
        let salt = ERDCrypto.randomKey().prefix(16)
        let hs = HandshakePayload(hostname: "host-a", screenWidth: 2560, screenHeight: 1440,
                                  scaleFactor: 2.0, capabilities: [.streamConfiguration],
                                  pairingID: "ABCD-1234", sessionSalt: salt)
        let decoded = HandshakePayload.deserialize(from: hs.serialize())
        XCTAssertNotNil(decoded)
        XCTAssertEqual(decoded?.pairingID, "ABCD-1234")
        XCTAssertEqual(decoded?.sessionSalt, salt)
        XCTAssertEqual(decoded?.protocolVersion, ERDConstants.protocolVersion)
    }

    func testHandshakeWithoutV3TailIsAccepted() {
        let hs = HandshakePayload(hostname: "legacy-shape", screenWidth: 1920, screenHeight: 1080, scaleFactor: 1.0)
        let decoded = HandshakePayload.deserialize(from: hs.serialize())
        XCTAssertNotNil(decoded)
        XCTAssertEqual(decoded?.pairingID, "")
        XCTAssertEqual(decoded?.sessionSalt, Data())
    }
}

final class PairingTests: XCTestCase {
    func testRandomPINFormat() {
        for _ in 0..<20 {
            let pin = ERDCrypto.randomPIN()
            XCTAssertEqual(pin.count, ERDConstants.pinLength)
            XCTAssertTrue(pin.allSatisfy { $0.isNumber })
        }
    }

    func testBootstrapPSKIsDeterministicPerPIN() {
        XCTAssertEqual(ERDCrypto.bootstrapPSK(pin: "12345678"), ERDCrypto.bootstrapPSK(pin: "12345678"))
        XCTAssertNotEqual(ERDCrypto.bootstrapPSK(pin: "12345678"), ERDCrypto.bootstrapPSK(pin: "87654321"))
    }

    func testPairingPayloadRoundtrips() {
        let request = PairingRequestPayload(hostname: "client-mac")
        XCTAssertEqual(PairingRequestPayload.deserialize(from: request.serialize())?.hostname, "client-mac")

        let grant = PairingGrantPayload(pairingID: "UUID-1", hostName: "host-mac", key: ERDCrypto.randomKey())
        let decodedGrant = PairingGrantPayload.deserialize(from: grant.serialize())
        XCTAssertEqual(decodedGrant?.pairingID, "UUID-1")
        XCTAssertEqual(decodedGrant?.hostName, "host-mac")
        XCTAssertEqual(decodedGrant?.key, grant.key)

        let reject = PairingRejectPayload(reason: .deniedByHost)
        XCTAssertEqual(PairingRejectPayload.deserialize(from: reject.serialize())?.reason, .deniedByHost)
    }

    func testGrantWithWrongKeyLengthRejected() {
        let grant = PairingGrantPayload(pairingID: "UUID-2", hostName: "host", key: Data(repeating: 1, count: 31))
        XCTAssertNil(PairingGrantPayload.deserialize(from: grant.serialize()))
    }

    func testPairingStoreRoundtrip() {
        let record = PairingRecord(name: "test-device-\(UUID().uuidString)", key: ERDCrypto.randomKey())
        XCTAssertTrue(PairingStore.save(record))
        XCTAssertEqual(PairingStore.load(id: record.id), record)
        XCTAssertTrue(PairingStore.delete(id: record.id))
        XCTAssertNil(PairingStore.load(id: record.id))
    }

    func testBootstrapLockoutAfterRepeatedFailures() {
        PairingManager.shared.beginPairing()
        var lockedOut = false
        for _ in 0..<ERDConstants.maxPairingAttempts {
            lockedOut = PairingManager.shared.recordBootstrapFailure()
        }
        XCTAssertTrue(lockedOut)
        XCTAssertFalse(PairingManager.shared.hostPSKs().contains { $0.identity == PairingManager.bootstrapIdentity })
        // A fresh pairing window clears the lockout
        PairingManager.shared.beginPairing()
        XCTAssertTrue(PairingManager.shared.hostPSKs().contains { $0.identity == PairingManager.bootstrapIdentity })
        PairingManager.shared.cancelPairing()
    }
}
