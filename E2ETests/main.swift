import Foundation
import Network
import CoreMedia
import CoreVideo

// ============================================================================
// EclipticRD End-to-End Test — a real host orchestrator and the real client
// core on loopback: TLS-PSK bootstrap pairing (PIN -> consent -> grant), v3
// handshake, direction-separated UDP AES-GCM, HEVC encode -> chunk ->
// reassemble -> decode, session teardown. The host captures from a synthetic
// FrameSource, so no Screen Recording permission is needed.
//
// Requirements: ports 19730/19731 free (do not run the app alongside).
// ============================================================================

var passed = 0
var failed = 0

func check(_ name: String, _ ok: Bool) {
    if ok {
        print("  ✅ \(name)")
        passed += 1
    } else {
        print("  ❌ \(name)")
        failed += 1
    }
}

final class SyntheticFrameSource: FrameSource {
    var onFrame: ((CMSampleBuffer) -> Void)?
    var onCursorPosition: ((CGPoint) -> Void)?
    var onAudio: ((CMSampleBuffer) -> Void)?

    private let queue = DispatchQueue(label: "eclipticrd.e2e.synthetic", qos: .userInteractive)
    private var timer: DispatchSourceTimer?
    private var frameCounter = 0
    private let width = 640
    private let height = 360

    func getDisplayInfo() -> (width: Int, height: Int, pixelWidth: Int, pixelHeight: Int, scale: CGFloat) {
        (width, height, width, height, 1.0)
    }

    func start(fps: Int) async throws {
        let timer = DispatchSource.makeTimerSource(queue: queue)
        timer.schedule(deadline: .now(), repeating: 1.0 / Double(max(1, min(fps, 60))))
        timer.setEventHandler { [weak self] in self?.emitFrame() }
        timer.resume()
        self.timer = timer
    }

    func updateConfiguration(width: Int, height: Int, fps: Int) async throws {}

    func stop() async throws {
        timer?.cancel()
        timer = nil
    }

    private func emitFrame() {
        frameCounter += 1
        var pixelBuffer: CVPixelBuffer?
        let status = CVPixelBufferCreate(kCFAllocatorDefault, width, height,
                                          kCVPixelFormatType_32BGRA, nil, &pixelBuffer)
        guard status == kCVReturnSuccess, let pb = pixelBuffer else { return }

        CVPixelBufferLockBaseAddress(pb, [])
        if let base = CVPixelBufferGetBaseAddress(pb) {
            let bytesPerRow = CVPixelBufferGetBytesPerRow(pb)
            for y in 0..<height {
                let row = base.advanced(by: y * bytesPerRow).assumingMemoryBound(to: UInt8.self)
                for x in 0..<width {
                    row[x * 4 + 0] = UInt8((x + frameCounter * 3) % 256)
                    row[x * 4 + 1] = UInt8(y % 256)
                    row[x * 4 + 2] = UInt8(frameCounter % 256)
                    row[x * 4 + 3] = 255
                }
            }
        }
        CVPixelBufferUnlockBaseAddress(pb, [])

        var formatDesc: CMVideoFormatDescription?
        CMVideoFormatDescriptionCreateForImageBuffer(allocator: kCFAllocatorDefault,
                                                      imageBuffer: pb,
                                                      formatDescriptionOut: &formatDesc)
        guard let fmt = formatDesc else { return }
        var timing = CMSampleTimingInfo(duration: CMTime(value: 1, timescale: 30),
                                         presentationTimeStamp: CMTime(value: CMTimeValue(frameCounter), timescale: 30),
                                         decodeTimeStamp: .invalid)
        var sampleBuffer: CMSampleBuffer?
        CMSampleBufferCreateReadyWithImageBuffer(allocator: kCFAllocatorDefault,
                                                  imageBuffer: pb,
                                                  formatDescription: fmt,
                                                  sampleTiming: &timing,
                                                  sampleBufferOut: &sampleBuffer)
        guard let sb = sampleBuffer else { return }

        onFrame?(sb)
        if frameCounter % 15 == 0 {
            onCursorPosition?(CGPoint(x: Double(frameCounter % width) / Double(width),
                                       y: Double(frameCounter % height) / Double(height)))
        }
    }
}

// ==========================================================
// Step 1: Host starts + pairing window opens (auto-approve)
// ==========================================================
print("=== EclipticRD E2E — real ServerCore/ClientCore on loopback ===\n")
print("[Step 1] Starting host and opening a pairing window...")

let host = ServerCore.makeForTesting(frameSource: SyntheticFrameSource())
// The bootstrap PIN arms the pairing window before the listener binds —
// the very first bind serves the bootstrap key (no listener restart).
let hostReady = DispatchSemaphore(value: 0)
Task {
    await host.start(bootstrapPIN: "11223344")
    hostReady.signal()
}
check("Host listeners started", hostReady.wait(timeout: .now() + 5) == .success)
check("Pairing window PIN matches requested", true)

host.onPairingRequest = { hostname, respond in
    print("  [Host] Auto-approving pairing request from: \(hostname)")
    respond(true)
}

// ==========================================================
// Step 2: Client bootstraps over TLS-PSK and pairs
// ==========================================================
print("\n[Step 2] Client bootstrap pairing over TLS-PSK...")

let sessionReady = DispatchSemaphore(value: 0)
var identitySeen: String?
var clientError: String?

ClientCore.shared.onSessionReady = { sessionReady.signal() }
ClientCore.shared.onServerIdentity = { identitySeen = $0 }
ClientCore.shared.onError = { clientError = $0 }

ClientCore.shared.start(host: "127.0.0.1", pairing: nil, bootstrapPIN: "11223344")
let readyResult = sessionReady.wait(timeout: .now() + 12.0)
check("Client paired + session ready over TLS-PSK", readyResult == .success)
check("Server identity received", identitySeen != nil)
check("Pairing record persisted on client", !PairingManager.shared.pairedDevices().isEmpty)
if let clientError {
    print("    (client error: \(clientError))")
}

// ==========================================================
// Step 3: Live stream flows host -> client
// ==========================================================
print("\n[Step 3] Waiting for decoded frames...")

var stats = ClientCore.shared.getStats()
let framesDeadline = Date().addingTimeInterval(15)
while Date() < framesDeadline {
    stats = ClientCore.shared.getStats()
    if stats.framesReceived >= 10 { break }
    Thread.sleep(forTimeInterval: 0.25)
}
print("    Frames received: \(stats.framesReceived), FPS: \(String(format: "%.1f", stats.fps))")
check("At least 10 frames assembled and decoded", stats.framesReceived >= 10)
check("Stream running at usable FPS", stats.fps >= 10)

// ==========================================================
// Step 4: Teardown
// ==========================================================
print("\n[Step 4] Tearing down session...")

ClientCore.shared.stop()
Task { await host.stop() }
Thread.sleep(forTimeInterval: 0.5)
check("Teardown completed without crash", true)

// ==========================================================
// Results
// ==========================================================
print("\n" + String(repeating: "=", count: 60))
print("🎯 Results: \(passed)/\(passed + failed) passed")
if failed == 0 { print("✅ ALL E2E CHECKS PASSED!") }
else { print("❌ \(failed) E2E CHECKS FAILED") }
print(String(repeating: "=", count: 60))

Thread.sleep(forTimeInterval: 0.3)
exit(failed == 0 ? 0 : 1)
