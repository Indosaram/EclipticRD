import Foundation
import ScreenCaptureKit
import CoreMedia
import CoreGraphics
import AppKit

public enum ScreenCaptureError: Error, Hashable {
    case noPermission
    case noDisplay
    case captureStartFailed
}

/// Capture seam so session orchestration (ServerCore) can be driven by a
/// synthetic frame source in tests without ScreenCaptureKit or TCC prompts.
public protocol FrameSource: AnyObject {
    var onFrame: ((CMSampleBuffer) -> Void)? { get set }
    var onCursorPosition: ((CGPoint) -> Void)? { get set }
    var onAudio: ((CMSampleBuffer) -> Void)? { get set }
    func getDisplayInfo() -> (width: Int, height: Int, pixelWidth: Int, pixelHeight: Int, scale: CGFloat)
    func start(fps: Int) async throws
    func updateConfiguration(width: Int, height: Int, fps: Int) async throws
    func stop() async throws
}

public class ScreenCapture: NSObject, SCStreamOutput, SCStreamDelegate {
    private var stream: SCStream?
    private let captureQueue = DispatchQueue(label: "eclipticrd.capture", qos: .userInteractive)
    private var display: SCDisplay?

    public var onFrame: ((CMSampleBuffer) -> Void)?
    public var onCursorPosition: ((CGPoint) -> Void)?
    public var onAudio: ((CMSampleBuffer) -> Void)?

    public private(set) var width: Int = 0
    public private(set) var height: Int = 0
    public private(set) var scaleFactor: CGFloat = 1.0

    public override init() { super.init() }

    public func getDisplayInfo() -> (width: Int, height: Int, pixelWidth: Int, pixelHeight: Int, scale: CGFloat) {
        let displayID = CGMainDisplayID()
        let w = CGDisplayPixelsWide(displayID)
        let h = CGDisplayPixelsHigh(displayID)
        let mode = CGDisplayCopyDisplayMode(displayID)
        let scale = CGFloat(mode?.pixelWidth ?? w) / CGFloat(w)
        let pw = Int(CGFloat(w) * scale)
        let ph = Int(CGFloat(h) * scale)
        return (w, h, pw, ph, scale)
    }

    public func start(fps: Int = ERDConstants.defaultFPS) async throws {
        // CGRequestScreenCaptureAccess() triggers the system prompt on first call.
        // On subsequent calls it returns the cached result without re-prompting.
        if !CGPreflightScreenCaptureAccess() {
            let granted = CGRequestScreenCaptureAccess()
            if !granted {
                // Open System Settings → Privacy → Screen Recording so the user
                // can manually toggle the permission (required for ad-hoc signed builds).
                if let url = URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture") {
                    await MainActor.run { NSWorkspace.shared.open(url) }
                }
                throw ScreenCaptureError.noPermission
            }
        }

        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
        guard let display = content.displays.first else { throw ScreenCaptureError.noDisplay }

        self.display = display
        let info = getDisplayInfo()
        self.width = info.width
        self.height = info.height
        self.scaleFactor = info.scale

        let filter = SCContentFilter(display: display, excludingApplications: [], exceptingWindows: [])
        let config = SCStreamConfiguration()
        config.width = info.pixelWidth
        config.height = info.pixelHeight
        config.minimumFrameInterval = CMTime(value: 1, timescale: CMTimeScale(fps))
        config.queueDepth = 5
        config.pixelFormat = kCVPixelFormatType_32BGRA
        config.showsCursor = true
        let hostAudioEnabled = UserDefaults.standard.object(forKey: "hostAudioEnabled") as? Bool ?? true
        config.capturesAudio = hostAudioEnabled

        let stream = SCStream(filter: filter, configuration: config, delegate: self)
        try stream.addStreamOutput(self, type: .screen, sampleHandlerQueue: captureQueue)
        if hostAudioEnabled {
            try stream.addStreamOutput(self, type: .audio, sampleHandlerQueue: captureQueue)
        }
        try await stream.startCapture()
        self.stream = stream
        ERDLog.video("[Capture] Started: \(width)x\(height) @\(fps)fps scale=\(scaleFactor)")
    }

    public func updateConfiguration(width: Int, height: Int, fps: Int) async throws {
        guard let stream = self.stream else {
            throw ScreenCaptureError.captureStartFailed
        }
        let config = SCStreamConfiguration()
        config.width = width
        config.height = height
        config.minimumFrameInterval = CMTime(value: 1, timescale: CMTimeScale(fps))
        config.queueDepth = 3
        config.pixelFormat = kCVPixelFormatType_32BGRA
        config.showsCursor = true
        let hostAudioEnabled = UserDefaults.standard.object(forKey: "hostAudioEnabled") as? Bool ?? true
        config.capturesAudio = hostAudioEnabled

        try await stream.updateConfiguration(config)
        self.width = width
        self.height = height
        ERDLog.video("[Capture] Configuration updated: \(width)x\(height) @\(fps)fps")
    }

    public func stop() async throws {
        try await stream?.stopCapture()
        stream = nil
        ERDLog.video("[Capture] Stopped")
    }

    // MARK: - SCStreamOutput
    public func stream(_ stream: SCStream, didOutputSampleBuffer sampleBuffer: CMSampleBuffer, of type: SCStreamOutputType) {
        if type == .screen {
            onFrame?(sampleBuffer)

            // Try to extract cursor position from sample buffer attachments (private API).
            // Falls back to NSEvent.mouseLocation if the attachment is unavailable.
            if let cursor = CMGetAttachment(sampleBuffer, key: "com.apple.screencapture.cursor.position" as CFString, attachmentModeOut: nil) as? [String: Any],
               let x = cursor["x"] as? CGFloat, let y = cursor["y"] as? CGFloat {
                onCursorPosition?(CGPoint(x: x, y: y))
            } else {
                // Fallback: NSEvent.mouseLocation (global, bottom-left origin).
                // CGDisplayBounds is thread-safe; NSScreen.main is MainActor-bound
                // and must not be touched from the capture queue.
                let mouseLocation = NSEvent.mouseLocation
                let bounds = CGDisplayBounds(CGMainDisplayID())
                let relativeX = mouseLocation.x - bounds.origin.x
                let relativeY = bounds.height - (mouseLocation.y - bounds.origin.y)
                onCursorPosition?(CGPoint(x: relativeX, y: relativeY))
            }
        } else if type == .audio {
            onAudio?(sampleBuffer)
        }
    }

    // MARK: - SCStreamDelegate
    public func stream(_ stream: SCStream, didStopWithError error: Error) {
        ERDLog.error("[Capture] Stream stopped with error: \(error)")
    }
}

extension ScreenCapture: FrameSource {}