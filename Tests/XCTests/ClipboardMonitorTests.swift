import XCTest
import AppKit

// Exercises ClipboardMonitor against the real system pasteboard: remote
// writes must not echo back, local changes must fire, and concealed /
// transient entries must never leave the machine. The original pasteboard
// string is restored after each test.
final class ClipboardMonitorTests: XCTestCase {
    private var savedString: String?

    override func setUp() {
        super.setUp()
        savedString = NSPasteboard.general.string(forType: .string)
    }

    override func tearDown() {
        let pb = NSPasteboard.general
        pb.clearContents()
        if let savedString {
            pb.setString(savedString, forType: .string)
        }
        super.tearDown()
    }

    private final class Spy {
        private let lock = NSLock()
        private var _texts: [String] = []
        var texts: [String] {
            lock.lock(); defer { lock.unlock() }
            return _texts
        }
        func record(_ text: String) {
            lock.lock()
            _texts.append(text)
            lock.unlock()
        }
    }

    private func waitFor(_ condition: @autoclosure () -> Bool, timeout: TimeInterval) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if condition() { return true }
            Thread.sleep(forTimeInterval: 0.05)
        }
        return condition()
    }

    private func writeToPasteboard(_ text: String, concealed: Bool = false) {
        let pb = NSPasteboard.general
        let concealedType = NSPasteboard.PasteboardType("org.nspasteboard.ConcealedType")
        if concealed {
            // Atomic write: declare the marker type in the same change, the
            // way real password managers do — no unconcealed window.
            pb.declareTypes([.string, concealedType], owner: nil)
            pb.setString(text, forType: .string)
        } else {
            pb.clearContents()
            pb.setString(text, forType: .string)
        }
    }

    func testRemoteWriteDoesNotEchoBack() {
        // Isolate from leftovers of other tests: a monitor started mid-session
        // correctly reports pasteboard texts it has never seen — the property
        // under test is that the JUST-APPLIED remote text is not re-emitted
        // as a local change.
        writeToPasteboard("e2e-baseline-\(UUID().uuidString)")
        let spy = Spy()
        let monitor = ClipboardMonitor(onLocalChange: { spy.record($0) }, pollInterval: 0.1)
        monitor.start()
        _ = waitFor(false, timeout: 0.6) // first poll cycle settles

        let remoteText = "e2e-remote-\(UUID().uuidString)"
        monitor.applyRemoteText(remoteText)
        // Surface any echo of the applied text within two poll cycles.
        _ = waitFor(!spy.texts.isEmpty, timeout: 1.2)
        XCTAssertFalse(spy.texts.contains(remoteText), "remote write echoed back: \(spy.texts)")
        monitor.stop() // stop before tearDown restores the old pasteboard
    }

    func testLocalChangeEmittedOnce() {
        let spy = Spy()
        let monitor = ClipboardMonitor(onLocalChange: { spy.record($0) }, pollInterval: 0.1)
        monitor.start()
        Thread.sleep(forTimeInterval: 0.6)

        let marker = "e2e-local-\(UUID().uuidString)"
        writeToPasteboard(marker)
        let emitted = waitFor(spy.texts.contains(marker), timeout: 3.0)
        XCTAssertTrue(emitted, "local pasteboard change was not detected")
        monitor.stop() // stop before tearDown restores the old pasteboard
    }

    func testConcealedPasteboardSuppressed() throws {
        let spy = Spy()
        let monitor = ClipboardMonitor(onLocalChange: { spy.record($0) }, pollInterval: 0.1)
        monitor.start()
        _ = waitFor(false, timeout: 0.6)

        let secret = "e2e-secret-\(UUID().uuidString)"
        let concealedMarker = "org.nspasteboard.ConcealedType"
        writeToPasteboard(secret, concealed: true)

        // Watch for the marker being stripped while the text survives: a
        // clipboard-manager rewrite makes the environment unable to test
        // suppression honestly.
        var externallyRewritten = false
        let deadline = Date().addingTimeInterval(2.0)
        while Date() < deadline {
            if spy.texts.contains(secret) { break }
            let types = NSPasteboard.general.types?.map(\.rawValue) ?? []
            if !types.contains(concealedMarker),
               NSPasteboard.general.string(forType: .string) == secret {
                externallyRewritten = true
                break
            }
            RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        }
        monitor.stop()

        if externallyRewritten {
            throw XCTSkip("A clipboard manager rewrote the concealed entry plain; suppression untestable here")
        }
        XCTAssertFalse(spy.texts.contains(secret), "concealed pasteboard entry was transmitted: \(spy.texts)")
    }
}
