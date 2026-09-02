import Foundation
import Security

// Pairing identity, Keychain storage, and bootstrap PIN state.
//
// First contact: the client offers TLS-PSK identity "erd-b1.<salt>", whose
// key derives from the host's 8-digit bootstrap PIN. The host approves the
// device with an explicit prompt before any input or screen access is
// granted, and repeated PIN failures lock the bootstrap path globally.
// On approval the host generates a 256-bit pairing key and hands it to the
// client over the encrypted channel; both sides persist it in the Keychain.
// Later sessions use identity "erd-p1.<uuid>" with that key, which cannot
// be brute-forced offline the way the PIN can.
//
// Residual risk: an attacker who records the bootstrap handshake and cracks
// the PIN offline recovers that one session and the pairing key. Consent,
// lockout, and PIN expiry mitigate this; SPAKE2+ is the follow-up once a
// vetted Swift PAKE implementation is available.

/// A persisted trust relationship with one remote device.
public struct PairingRecord: Codable, Equatable, Identifiable, Sendable {
    public let id: String
    public var name: String
    public let key: Data
    public let addedAt: Date

    public init(id: String = UUID().uuidString, name: String, key: Data, addedAt: Date = Date()) {
        self.id = id
        self.name = name
        self.key = key
        self.addedAt = addedAt
    }

    public var pskIdentity: String { "erd-p1.\(id)" }
}

/// File-backed storage for pairing records.
///
/// Plaintext by design (user decision, 2026-09-01): the pairing key only
/// protects the network path — anyone with this user's disk access already
/// controls the machine, so the OS keychain adds friction (locked keychains
/// break headless/test sessions) without raising the real bar. File is 0600
/// under the user's Application Support directory.
public enum PairingStore {
    private static let fileLock = NSLock()

    private static var storeURL: URL {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("EclipticRD", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir.appendingPathComponent("pairing-keys.json")
    }

    public static func loadAll() -> [PairingRecord] {
        fileLock.lock()
        defer { fileLock.unlock() }
        return loadAllUnlocked()
    }

    public static func load(id: String) -> PairingRecord? {
        loadAll().first { $0.id == id }
    }

    @discardableResult
    public static func save(_ record: PairingRecord) -> Bool {
        fileLock.lock()
        defer { fileLock.unlock() }
        // NSLock is not reentrant: call the unlocked variants, never the
        // public locked wrappers, while holding fileLock.
        var records = loadAllUnlocked().filter { $0.id != record.id }
        records.append(record)
        return writeUnlocked(records)
    }

    @discardableResult
    public static func delete(id: String) -> Bool {
        fileLock.lock()
        defer { fileLock.unlock() }
        var records = loadAllUnlocked()
        records.removeAll { $0.id == id }
        return writeUnlocked(records)
    }

    /// Caller must hold fileLock.
    private static func loadAllUnlocked() -> [PairingRecord] {
        guard let data = try? Data(contentsOf: storeURL),
              let records = try? JSONDecoder().decode([PairingRecord].self, from: data)
        else { return [] }
        return records
    }

    /// Caller must hold fileLock. Atomic 0600 write.
    private static func writeUnlocked(_ records: [PairingRecord]) -> Bool {
        guard let data = try? JSONEncoder().encode(records) else { return false }
        do {
            try data.write(to: storeURL, options: .atomic)
            try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: storeURL.path)
            return true
        } catch {
            ERDLog.error("[PairingStore] Failed to persist pairing: \(error)")
            return false
        }
    }
}

/// One TLS pre-shared key entry offered on a channel.
public struct ERDPSK: Equatable, Sendable {
    public let identity: String
    public let key: Data

    public init(identity: String, key: Data) {
        self.identity = identity
        self.key = key
    }
}

/// Host-side pairing authority: pending bootstrap PIN state, failure lockout,
/// and the set of PSKs the TCP listener should accept.
public final class PairingManager: @unchecked Sendable {
    public static let shared = PairingManager()

    private struct PendingPairing {
        let pin: String
        let expiresAt: Date
    }

    private let lock = NSLock()
    private var pending: PendingPairing?
    private var failureCount = 0
    private var failureWindowStart = Date.distantPast
    private var bootstrapLockedUntil = Date.distantPast

    // MARK: - Bootstrap PIN lifecycle (host)

    /// Start accepting bootstrap connections; returns the PIN to display.
    /// An explicit numeric PIN of the configured length (internet mode, where
    /// both sides type the same code) overrides the generated one.
    @discardableResult
    public func beginPairing(explicitPIN: String? = nil) -> String {
        var pin = ERDCrypto.randomPIN()
        if let explicitPIN, explicitPIN.count == ERDConstants.pinLength,
           explicitPIN.allSatisfy({ $0.isNumber }) {
            pin = explicitPIN
        }
        let pending = PendingPairing(pin: pin,
                                     expiresAt: Date().addingTimeInterval(ERDConstants.pairingPINExpiry))
        lock.lock()
        self.pending = pending
        failureCount = 0
        bootstrapLockedUntil = .distantPast
        lock.unlock()
        return pin
    }

    public func cancelPairing() {
        lock.lock()
        pending = nil
        lock.unlock()
    }

    public var isPairingActive: Bool {
        lock.withLock { pending != nil && pending!.expiresAt > Date() }
    }

    /// All PSKs the host listener should accept: every paired device plus the
    /// pending bootstrap entry while pairing is active and not locked out.
    public func hostPSKs() -> [ERDPSK] {
        lock.lock(); defer { lock.unlock() }
        var psks = PairingStore.loadAll().map { ERDPSK(identity: $0.pskIdentity, key: ERDCrypto.pairingPSK($0.key)) }
        if let pending, pending.expiresAt > Date(), Date() >= bootstrapLockedUntil {
            psks.append(ERDPSK(identity: Self.bootstrapIdentity,
                               key: ERDCrypto.bootstrapPSK(pin: pending.pin)))
        }
        return psks
    }

    /// Resolve one offered identity to its PSK (used by the TLS verify hook).
    public func resolvePSK(identity: String) -> ERDPSK? {
        hostPSKs().first { $0.identity == identity }
    }

    /// Record a failed bootstrap authentication. Returns true when bootstrap
    /// is now locked out.
    @discardableResult
    public func recordBootstrapFailure() -> Bool {
        lock.lock(); defer { lock.unlock() }
        let now = Date()
        if now.timeIntervalSince(failureWindowStart) > ERDConstants.pairingAttemptWindow {
            failureCount = 0
            failureWindowStart = now
        }
        failureCount += 1
        if failureCount >= ERDConstants.maxPairingAttempts {
            bootstrapLockedUntil = now.addingTimeInterval(ERDConstants.pairingLockout)
            pending = nil
            return true
        }
        return false
    }

    // MARK: - Grant / adopt (both sides)

    /// Host: mint + persist a pairing record for an approved bootstrap peer.
    public func grantPairing(hostname: String) -> PairingRecord {
        let record = PairingRecord(name: hostname, key: ERDCrypto.randomKey())
        PairingStore.save(record)
        return record
    }

    /// Client: persist a pairing record received from a host.
    public func adoptPairing(_ record: PairingRecord) {
        PairingStore.save(record)
    }

    public func pairedDevices() -> [PairingRecord] {
        PairingStore.loadAll().sorted { $0.addedAt > $1.addedAt }
    }

    public func revokePairing(id: String) {
        PairingStore.delete(id: id)
    }

    // MARK: - Client side

    public static let bootstrapIdentity = "erd-b1"

    /// The client connects either with a previously granted pairing key or,
    /// while pairing for the first time, with the bootstrap PSK for `pin`.
    public func clientPSK(pin: String) -> ERDPSK {
        ERDPSK(identity: Self.bootstrapIdentity, key: ERDCrypto.bootstrapPSK(pin: pin))
    }

    public func clientPSK(for record: PairingRecord) -> ERDPSK {
        ERDPSK(identity: record.pskIdentity, key: ERDCrypto.pairingPSK(record.key))
    }
}

private extension NSLock {
    func withLock<T>(_ body: () -> T) -> T {
        lock()
        defer { unlock() }
        return body()
    }
}
