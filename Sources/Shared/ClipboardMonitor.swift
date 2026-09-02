import Foundation
import AppKit

/// Monitors the system pasteboard for text changes and provides loop prevention
/// for bidirectional clipboard sync between host and client.
///
/// Loop prevention strategy:
/// - Tracks `changeCount` to detect local pasteboard changes
/// - When applying remote text, records the resulting changeCount to suppress the echo
/// - Uses content hash dedup to avoid sending identical text back
public class ClipboardMonitor {
    private let pasteboard = NSPasteboard.general
    private var pollTimer: DispatchSourceTimer?
    private let pollQueue = DispatchQueue(label: "eclipticrd.clipboard.poll", qos: .utility)
    private let lock = NSLock()

    // Concealed/transient pasteboard entries (password managers, 2FA copies)
    // must never leave the machine.
    private static let suppressedPasteboardTypes = [
        "org.nspasteboard.ConcealedType",
        "org.nspasteboard.TransientType",
    ]

    /// The changeCount after our last write, used to suppress echo
    private var lastWrittenChangeCount: Int = -1
    /// Hash of the last text we sent or applied, for content-level dedup
    private var lastTextHash: Int = 0
    /// Whether monitoring is active
    private var isRunning = false
    /// Last observed changeCount
    private var lastChangeCount: Int = 0

    private let onLocalChange: (String) -> Void
    private let pollInterval: TimeInterval

    /// - pollInterval: shrink in tests (e.g. 0.1) for fast, deterministic
    ///   detection; production default is 0.5s.
    public init(onLocalChange: @escaping (String) -> Void, pollInterval: TimeInterval = 0.5) {
        self.onLocalChange = onLocalChange
        self.pollInterval = pollInterval
    }

    public func start() {
        lock.lock()
        guard !isRunning else { lock.unlock(); return }
        isRunning = true
        lastChangeCount = pasteboard.changeCount
        lastWrittenChangeCount = -1
        lastTextHash = 0
        lock.unlock()

        // Poll on a dedicated queue: dispatch sources on .main never fire in
        // test/CLI contexts whose main run loop is not fully driven, and this
        // keeps the poll path off the main thread entirely. Pasteboard reads
        // are safe off-main; the only write path (applyRemoteText) stays on
        // the main queue.
        let timer = DispatchSource.makeTimerSource(queue: pollQueue)
        timer.schedule(deadline: .now() + pollInterval, repeating: pollInterval)
        timer.setEventHandler { [weak self] in
            self?.poll()
        }
        timer.resume()
        pollTimer = timer
    }

    public func stop() {
        lock.lock()
        isRunning = false
        lock.unlock()
        pollTimer?.cancel()
        pollTimer = nil
    }

    /// Apply text received from the remote peer to the local pasteboard.
    /// The resulting changeCount is recorded so the next poll won't echo it back.
    public func applyRemoteText(_ text: String) {
        let hash = text.hashValue
        lock.lock()
        lastTextHash = hash
        lock.unlock()

        DispatchQueue.main.async { [weak self] in
            guard let self = self else { return }
            self.pasteboard.clearContents()
            self.pasteboard.setString(text, forType: .string)

            self.lock.lock()
            self.lastWrittenChangeCount = self.pasteboard.changeCount
            self.lastChangeCount = self.pasteboard.changeCount
            self.lock.unlock()
        }
    }

    private func poll() {
        let currentCount = pasteboard.changeCount
        ERDLog.debug("[Clipboard] poll count=\(currentCount)")

        lock.lock()
        let running = isRunning
        let prevCount = lastChangeCount
        let writtenCount = lastWrittenChangeCount
        let prevHash = lastTextHash
        lock.unlock()

        guard running, currentCount != prevCount else {
            ERDLog.debug("[Clipboard] poll skipped (count unchanged)")
            return
        }

        // Suppress echo: if this changeCount matches what we just wrote, skip
        if currentCount == writtenCount {
            lock.lock()
            lastChangeCount = currentCount
            lock.unlock()
            ERDLog.debug("[Clipboard] poll suppressed echo")
            return
        }

        lock.lock()
        lastChangeCount = currentCount
        lock.unlock()

        // Concealed/transient check BEFORE reading text: non-atomic writers
        // (declare-then-fill) briefly expose the string without the marker
        // data, and reading first would leak exactly what must stay hidden.
        if let types = pasteboard.types,
           types.contains(where: { Self.suppressedPasteboardTypes.contains($0.rawValue) }) {
            return
        }

        guard let text = pasteboard.string(forType: .string), !text.isEmpty else { return }

        // Content-level dedup: don't send if identical to last sent/applied text
        let hash = text.hashValue
        guard hash != prevHash else { return }

        ERDLog.debug("[Clipboard] emit (count=\(currentCount), written=\(writtenCount), prevHash=\(prevHash), hash=\(hash))")

        // Check size limit
        guard let textData = text.data(using: .utf8),
              textData.count <= ERDConstants.maxClipboardTextBytes else { return }

        lock.lock()
        lastTextHash = hash
        lock.unlock()

        onLocalChange(text)
    }
}
