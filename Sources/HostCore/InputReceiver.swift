import Foundation
import CoreGraphics
import ApplicationServices

public class InputReceiver {
    private var screenWidth: CGFloat
    private var screenHeight: CGFloat
    private let rateLock = NSLock()
    private var eventTokens: Double = Double(ERDConstants.inputBurstCapacity)
    private var lastRefillTime = Date()

    public init(screenWidth: CGFloat, screenHeight: CGFloat) {
        self.screenWidth = screenWidth
        self.screenHeight = screenHeight
    }

    public func updateScreenSize(width: CGFloat, height: CGFloat) {
        self.screenWidth = width
        self.screenHeight = height
    }

    public func handleInputEvent(_ data: Data) {
        guard AXIsProcessTrusted() else { return }
        guard let event = InputEventPayload.deserialize(from: data) else { return }
        guard allowInputEvent() else { return }
        guard !event.x.isNaN, !event.y.isNaN else { return }

        let x = CGFloat(min(max(event.x, 0), 1)) * screenWidth
        let y = CGFloat(min(max(event.y, 0), 1)) * screenHeight
        let point = CGPoint(x: x, y: y)

        let source = CGEventSource(stateID: .hidSystemState)

        switch event.type {
        case .mouseMove:
            post(.mouseMoved, at: point, button: .left, source: source)
        case .leftMouseDragged:
            post(.leftMouseDragged, at: point, button: .left, source: source)
        case .rightMouseDragged:
            post(.rightMouseDragged, at: point, button: .right, source: source)
        case .leftMouseDown:
            post(.leftMouseDown, at: point, button: .left, source: source)
        case .leftMouseUp:
            post(.leftMouseUp, at: point, button: .left, source: source)
        case .rightMouseDown:
            post(.rightMouseDown, at: point, button: .right, source: source)
        case .rightMouseUp:
            post(.rightMouseUp, at: point, button: .right, source: source)
        case .scrollWheel:
            if let cgEvent = CGEvent(scrollWheelEvent2Source: source, units: .pixel, wheelCount: 2,
                                     wheel1: Int32(event.scrollDeltaY), wheel2: Int32(event.scrollDeltaX), wheel3: 0) {
                cgEvent.post(tap: .cghidEventTap)
            }
        case .keyDown:
            postKey(code: event.keyCode, down: true, modifiers: event.modifiers, source: source)
        case .keyUp:
            postKey(code: event.keyCode, down: false, modifiers: event.modifiers, source: source)
        case .flagsChanged:
            if let cgEvent = CGEvent(keyboardEventSource: source, virtualKey: event.keyCode, keyDown: true) {
                var flags = CGEventFlags()
                if event.modifiers.contains(.shift)   { flags.insert(.maskShift) }
                if event.modifiers.contains(.control) { flags.insert(.maskControl) }
                if event.modifiers.contains(.option)  { flags.insert(.maskAlternate) }
                if event.modifiers.contains(.command) { flags.insert(.maskCommand) }
                if event.modifiers.contains(.capsLock) { flags.insert(.maskAlphaShift) }
                cgEvent.flags = flags
                cgEvent.post(tap: .cghidEventTap)
            }
        }
    }

    /// Token bucket: sustained 200 events/s with a short burst allowance,
    /// so a hostile peer cannot flood the host UI with synthetic input.
    private func allowInputEvent() -> Bool {
        rateLock.lock(); defer { rateLock.unlock() }
        let now = Date()
        eventTokens = min(Double(ERDConstants.inputBurstCapacity),
                          eventTokens + now.timeIntervalSince(lastRefillTime) * Double(ERDConstants.maxInputEventsPerSecond))
        lastRefillTime = now
        guard eventTokens >= 1 else { return false }
        eventTokens -= 1
        return true
    }

    private func post(_ type: CGEventType, at point: CGPoint, button: CGMouseButton, source: CGEventSource?) {
        if let cgEvent = CGEvent(mouseEventSource: source, mouseType: type, mouseCursorPosition: point, mouseButton: button) {
            cgEvent.post(tap: .cghidEventTap)
        }
    }

    private func postKey(code: UInt16, down: Bool, modifiers: ModifierFlags, source: CGEventSource?) {
        if let cgEvent = CGEvent(keyboardEventSource: source, virtualKey: CGKeyCode(code), keyDown: down) {
            var flags = CGEventFlags()
            if modifiers.contains(.shift)   { flags.insert(.maskShift) }
            if modifiers.contains(.control) { flags.insert(.maskControl) }
            if modifiers.contains(.option)  { flags.insert(.maskAlternate) }
            if modifiers.contains(.command) { flags.insert(.maskCommand) }
            cgEvent.flags = flags
            cgEvent.post(tap: .cghidEventTap)
        }
    }
}
