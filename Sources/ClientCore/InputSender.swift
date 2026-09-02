import AppKit

public class InputSender {
    private let tcpChannel: TCPChannel
    private var captureView: NSView?
    private var localMonitor: Any?

    public init(tcpChannel: TCPChannel) {
        self.tcpChannel = tcpChannel
    }

    deinit { stopCapturing() }

    public func startCapturing(in view: NSView) {
        self.captureView = view

        let eventMask: NSEvent.EventTypeMask = [
            .mouseMoved, .leftMouseDown, .leftMouseUp, .rightMouseDown, .rightMouseUp,
            .leftMouseDragged, .rightMouseDragged, .scrollWheel,
            .keyDown, .keyUp, .flagsChanged
        ]

        localMonitor = NSEvent.addLocalMonitorForEvents(matching: eventMask) { [weak self] event in
            self?.handleEvent(event)
            return event
        }
    }

    public func stopCapturing() {
        if let m = localMonitor {
            // NSEvent monitor removal must happen on the main thread; stop()
            // can be triggered from TCP callback queues during disconnect.
            if Thread.isMainThread {
                NSEvent.removeMonitor(m)
            } else {
                DispatchQueue.main.async { NSEvent.removeMonitor(m) }
            }
            localMonitor = nil
        }
        captureView = nil
    }

    private func handleEvent(_ event: NSEvent) {
        guard let view = captureView else { return }

        // Events landing on session overlay controls (buttons, pickers, text
        // fields) belong to this Mac, not the remote host. Keyboard events
        // carry the cursor position but must not be filtered by it.
        let isPointerEvent: Bool = {
            switch event.type {
            case .mouseMoved, .leftMouseDown, .leftMouseUp, .rightMouseDown, .rightMouseUp,
                 .leftMouseDragged, .rightMouseDragged, .otherMouseDown, .otherMouseUp,
                 .otherMouseDragged, .scrollWheel:
                return true
            default:
                return false
            }
        }()
        if isPointerEvent,
           let contentView = view.window?.contentView,
           let hit = contentView.hitTest(event.locationInWindow),
           hit !== view, !hit.isDescendant(of: view) {
            return
        }

        let location = view.convert(event.locationInWindow, from: nil)
        let bounds = view.bounds
        guard bounds.width > 0, bounds.height > 0 else { return }

        let normX = Float(max(0, min(1, location.x / bounds.width)))
        let normY = Float(max(0, min(1, 1.0 - (location.y / bounds.height)))) // Flip Y

        var inputType: InputEventType
        var keyCode: UInt16 = 0
        var scrollDX: Float = 0
        var scrollDY: Float = 0

        switch event.type {
        case .mouseMoved:       inputType = .mouseMove
        case .leftMouseDown:    inputType = .leftMouseDown
        case .leftMouseUp:      inputType = .leftMouseUp
        case .rightMouseDown:   inputType = .rightMouseDown
        case .rightMouseUp:     inputType = .rightMouseUp
        case .leftMouseDragged: inputType = .leftMouseDragged
        case .rightMouseDragged: inputType = .rightMouseDragged
        case .scrollWheel:
            inputType = .scrollWheel
            scrollDX = Float(event.scrollingDeltaX)
            scrollDY = Float(event.scrollingDeltaY)
        case .keyDown:
            inputType = .keyDown
            keyCode = event.keyCode
        case .keyUp:
            inputType = .keyUp
            keyCode = event.keyCode
        case .flagsChanged:
            inputType = .flagsChanged
            keyCode = event.keyCode
        default: return
        }

        // Map NSEvent modifier flags
        var modifiers = ModifierFlags()
        if event.modifierFlags.contains(.shift)   { modifiers.insert(.shift) }
        if event.modifierFlags.contains(.control) { modifiers.insert(.control) }
        if event.modifierFlags.contains(.option)  { modifiers.insert(.option) }
        if event.modifierFlags.contains(.command) { modifiers.insert(.command) }
        if event.modifierFlags.contains(.capsLock) { modifiers.insert(.capsLock) }

        let payload = InputEventPayload(type: inputType, x: normX, y: normY, keyCode: keyCode,
                                         modifiers: modifiers, scrollDeltaX: scrollDX, scrollDeltaY: scrollDY)
        tcpChannel.sendInput(payload)
    }
}
