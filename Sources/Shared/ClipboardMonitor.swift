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
    private let pollQueue = DispatchQueue(label: "eclipticrd.clipboard.poll")
    private let lock = NSLock()

    /// The changeCount after our last write, used to suppress echo
    private var lastWrittenChangeCount: Int = -1
    /// Hash of the last text we sent or applied, for content-level dedup
    private var lastTextHash: Int = 0
    /// Whether monitoring is active
    private var isRunning = false
    /// Last observed changeCount
    private var lastChangeCount: Int = 0

    private let onLocalChange: (String) -> Void

    /// - Parameter onLocalChange: Called when the local pasteboard text changes
    ///   (excluding changes applied via `applyRemoteText`).
    public init(onLocalChange: @escaping (String) -> Void) {
        self.onLocalChange = onLocalChange
    }

    public func start() {
        lock.lock()
        guard !isRunning else { lock.unlock(); return }
        isRunning = true
        lastChangeCount = pasteboard.changeCount
        lastWrittenChangeCount = -1
        lastTextHash = 0
        lock.unlock()

        let timer = DispatchSource.makeTimerSource(queue: pollQueue)
        timer.schedule(deadline: .now() + 0.5, repeating: 0.5)
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

        pasteboard.clearContents()
        pasteboard.setString(text, forType: .string)

        lock.lock()
        lastWrittenChangeCount = pasteboard.changeCount
        lastChangeCount = pasteboard.changeCount
        lock.unlock()
    }

    private func poll() {
        let currentCount = pasteboard.changeCount

        lock.lock()
        let running = isRunning
        let prevCount = lastChangeCount
        let writtenCount = lastWrittenChangeCount
        let prevHash = lastTextHash
        lock.unlock()

        guard running, currentCount != prevCount else { return }

        // Suppress echo: if this changeCount matches what we just wrote, skip
        if currentCount == writtenCount {
            lock.lock()
            lastChangeCount = currentCount
            lock.unlock()
            return
        }

        lock.lock()
        lastChangeCount = currentCount
        lock.unlock()

        guard let text = pasteboard.string(forType: .string), !text.isEmpty else { return }

        // Content-level dedup: don't send if identical to last sent/applied text
        let hash = text.hashValue
        guard hash != prevHash else { return }

        // Check size limit
        guard let textData = text.data(using: .utf8),
              textData.count <= ERDConstants.maxClipboardTextBytes else { return }

        lock.lock()
        lastTextHash = hash
        lock.unlock()

        onLocalChange(text)
    }
}
