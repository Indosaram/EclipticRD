import Foundation
import CryptoKit
import Security
import CommonCrypto

// Cryptographic core of the v3 wire protocol.
//
// TCP runs TLS 1.3 with external PSKs: a bootstrap PSK derived from an
// 8-digit PIN plus a per-connection salt, or a 256-bit random pairing key
// persisted in the Keychain after first approval (see ERDIdentity).
// UDP datagrams are sealed with AES-256-GCM under direction-separated keys
// derived from the pairing key and a per-session salt.
//
// HKDF is implemented locally because CryptoKit's HKDF API requires macOS 14
// while EclipticRD targets macOS 13.

/// RFC 5869 HKDF-SHA256 (Extract/Expand); test vectors live in the XCTest target.
enum HKDF {
    static func extract(salt: Data, ikm: Data) -> Data {
        let key = SymmetricKey(data: salt.isEmpty ? Data(repeating: 0, count: 32) : salt)
        return Data(HMAC<SHA256>.authenticationCode(for: ikm, using: key))
    }

    static func expand(prk: Data, info: Data, length: Int) -> Data {
        precondition(length >= 0 && length <= 255 * 32, "HKDF-Expand length out of range")
        var okm = Data()
        var t = Data()
        var counter: UInt8 = 1
        while okm.count < length {
            var input = t
            input.append(info)
            input.append(counter)
            t = Data(HMAC<SHA256>.authenticationCode(for: input, using: SymmetricKey(data: prk)))
            okm.append(t)
            counter &+= 1
        }
        return okm.prefix(length)
    }

    static func derive(ikm: Data, salt: Data, info: Data, length: Int = 32) -> Data {
        expand(prk: extract(salt: salt, ikm: ikm), info: info, length: length)
    }
}

/// AES-256-GCM cipher for UDP datagrams with sliding-window replay protection.
///
/// Datagram wire format: `[12-byte nonce][ciphertext][16-byte tag]`
/// Nonce layout: `[4-byte session prefix][8-byte big-endian packet counter]`.
/// Prefix and traffic key are direction-separated, so nonce reuse across the
/// two directions is impossible even under counter resets.
public final class DatagramCipher: @unchecked Sendable {
    public static let nonceSize = 12
    public static let tagSize = 16
    public static let windowSize: UInt64 = 4096

    private let key: SymmetricKey
    private let noncePrefix: [UInt8] // 4 bytes, derived per direction per session
    private let lock = NSLock()
    private var sendCounter: UInt64 = 0
    private var highestReceived: UInt64 = 0
    private var receivedBlocks: [UInt64: UInt64] = [:] // block index -> 64-bit seen mask

    init(key: Data, noncePrefix: Data) {
        precondition(key.count == 32, "DatagramCipher expects a 256-bit key")
        precondition(noncePrefix.count == 4, "DatagramCipher expects a 4-byte nonce prefix")
        self.key = SymmetricKey(data: key)
        self.noncePrefix = [UInt8](noncePrefix)
    }

    /// Derive the per-direction UDP traffic cipher from the pairing master key
    /// and the session salt exchanged inside the authenticated handshake.
    public static func udpCipher(masterKey: Data, sessionSalt: Data, clientToHost: Bool) -> DatagramCipher {
        let ikm = HKDF.derive(ikm: masterKey, salt: sessionSalt, info: Data("erd/udp-ikm/v3".utf8))
        let info = Data(clientToHost ? "erd/udp-c2h/v3".utf8 : "erd/udp-h2c/v3".utf8)
        let key = HKDF.derive(ikm: ikm, salt: sessionSalt, info: info, length: 32)
        let prefix = HKDF.derive(ikm: ikm, salt: sessionSalt, info: info + Data("/nonce".utf8), length: 4)
        return DatagramCipher(key: key, noncePrefix: prefix)
    }

    /// Encrypt `plaintext`; returns nonce||ciphertext||tag, or nil on internal failure.
    public func seal(_ plaintext: Data, aad: Data = Data()) -> Data? {
        lock.lock()
        sendCounter &+= 1
        let counter = sendCounter
        lock.unlock()

        var nonceBytes = noncePrefix
        var bigCounter = counter.bigEndian
        withUnsafeBytes(of: &bigCounter) { nonceBytes.append(contentsOf: $0) }
        guard let nonce = try? AES.GCM.Nonce(data: Data(nonceBytes)),
              let box = try? AES.GCM.seal(plaintext, using: key, nonce: nonce, authenticating: aad)
        else { return nil }
        return Data(nonceBytes) + box.ciphertext + box.tag
    }

    /// Decrypt a datagram produced by `seal`. Returns nil for tampered,
    /// duplicated, or too-old packets. Thread-safe.
    public func open(_ datagram: Data, aad: Data = Data()) -> Data? {
        guard datagram.count > Self.nonceSize + Self.tagSize else { return nil }
        let nonceData = datagram.subdata(in: 0..<Self.nonceSize)
        let counter = nonceData.subdata(in: 4..<12).withUnsafeBytes { $0.load(as: UInt64.self).bigEndian }
        let ciphertext = datagram.subdata(in: Self.nonceSize..<(datagram.count - Self.tagSize))
        let tag = datagram.subdata(in: (datagram.count - Self.tagSize)..<datagram.count)

        lock.lock(); defer { lock.unlock() }

        // Replay window fast reject (overflow-safe: never add to a wire value).
        guard highestReceived <= counter || highestReceived - counter < Self.windowSize else { return nil }
        let block = counter / 64
        let mask: UInt64 = 1 << (counter % 64)
        if (receivedBlocks[block] ?? 0) & mask != 0 { return nil }

        guard let nonce = try? AES.GCM.Nonce(data: nonceData),
              let box = try? AES.GCM.SealedBox(nonce: nonce, ciphertext: ciphertext, tag: tag),
              let plaintext = try? AES.GCM.open(box, using: key, authenticating: aad)
        else { return nil }

        receivedBlocks[block, default: 0] |= mask
        if counter > highestReceived { highestReceived = counter }
        let cutoff = highestReceived >= Self.windowSize ? (highestReceived - Self.windowSize) / 64 : 0
        if receivedBlocks.count > 128 {
            receivedBlocks = receivedBlocks.filter { $0.key >= cutoff }
        }
        return plaintext
    }
}

/// Shared helpers for the identity material backing the TLS-PSK channels.
public enum ERDCrypto {
    /// TLS-PSK for a bootstrap (first-time pairing) connection. The host
    /// rotates the PIN per pairing session, so each session gets a fresh key
    /// without per-connection salt negotiation.
    ///
    /// The PIN is PBKDF2-stretched so an attacker who records the bootstrap
    /// handshake cannot brute-force it offline with cheap HMAC evaluations.
    public static func bootstrapPSK(pin: String) -> Data {
        let stretched = stretch(Data(pin.utf8),
                                salt: Data("erd/bootstrap/v3".utf8),
                                rounds: ERDConstants.bootstrapPINStretchRounds)
        return HKDF.derive(ikm: stretched,
                           salt: Data("erd/bootstrap/v3".utf8),
                           info: Data("erd/tls-psk".utf8))
    }

    /// PBKDF2-SHA256 (CommonCrypto), 32-byte output. Roughly 0.3s per call at
    /// the configured round count — the derivation runs once per pairing
    /// window, never per packet.
    private static func stretch(_ secret: Data, salt: Data, rounds: UInt32) -> Data {
        var derived = Data(repeating: 0, count: 32)
        let status = derived.withUnsafeMutableBytes { derivedBytes in
            secret.withUnsafeBytes { secretBytes in
                salt.withUnsafeBytes { saltBytes in
                    CCKeyDerivationPBKDF(
                        CCPBKDFAlgorithm(kCCPBKDF2),
                        secretBytes.baseAddress?.assumingMemoryBound(to: Int8.self), secret.count,
                        saltBytes.baseAddress?.assumingMemoryBound(to: UInt8.self), salt.count,
                        CCPseudoRandomAlgorithm(kCCPRFHmacAlgSHA256), rounds,
                        derivedBytes.baseAddress?.assumingMemoryBound(to: UInt8.self), 32)
                }
            }
        }
        precondition(status == kCCSuccess, "PBKDF2 failed: \(status)")
        return derived
    }

    /// PSK for a paired device: the raw 256-bit Keychain pairing key.
    public static func pairingPSK(_ key: Data) -> Data { key }

    /// Uniform 32-byte random key material.
    public static func randomKey() -> Data {
        var key = Data(count: 32)
        let status = key.withUnsafeMutableBytes {
            SecRandomCopyBytes(kSecRandomDefault, 32, $0.baseAddress!)
        }
        precondition(status == errSecSuccess, "SecRandomCopyBytes failed: \(status)")
        return key
    }

    /// 8-digit zero-padded bootstrap PIN from the system RNG.
    public static func randomPIN(digits: Int = ERDConstants.pinLength) -> String {
        var value: UInt64 = 0
        let bound = UInt64(1) << (6 * digits) // keep modulo bias negligible
        repeat {
            let status = withUnsafeMutableBytes(of: &value) {
                SecRandomCopyBytes(kSecRandomDefault, 8, $0.baseAddress!)
            }
            precondition(status == errSecSuccess, "SecRandomCopyBytes failed: \(status)")
        } while value >= UInt64.max - (UInt64.max % bound)
        let modulus = UInt64(pow(10.0, Double(digits)))
        let number = String(value % modulus)
        return String(repeating: "0", count: max(0, digits - number.count)) + number
    }
}
