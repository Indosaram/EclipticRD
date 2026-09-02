use std::sync::Mutex;
use std::time::Instant;

use erd_proto::{map_to_host_pixels, InputEvent, InputEventType, Modifiers};
use thiserror::Error;

const MAX_EVENTS_PER_SECOND: f64 = 200.0;
const BURST_CAPACITY: f64 = 400.0;

#[derive(Debug, Error)]
pub enum InputError {
    #[error("Accessibility access is not granted")]
    PermissionDenied,
    #[error("invalid input coordinates")]
    InvalidCoordinates,
    #[error("CoreGraphics could not create the event")]
    EventCreation,
    #[error("input rate limit exceeded")]
    RateLimited,
    #[error("input injection is available only on macOS")]
    Unsupported,
}

struct RateLimit {
    tokens: f64,
    last_refill: Instant,
}

pub struct InputInjector {
    host_width: f32,
    host_height: f32,
    rate_limit: Mutex<RateLimit>,
}

impl InputInjector {
    pub fn new(host_width: f32, host_height: f32) -> Self {
        Self {
            host_width,
            host_height,
            rate_limit: Mutex::new(RateLimit {
                tokens: BURST_CAPACITY,
                last_refill: Instant::now(),
            }),
        }
    }

    pub fn map_coordinates(&self, event: &InputEvent) -> Result<(f32, f32), InputError> {
        if !event.x.is_finite() || !event.y.is_finite() {
            return Err(InputError::InvalidCoordinates);
        }
        Ok(map_to_host_pixels(
            event.x.clamp(0.0, 1.0),
            event.y.clamp(0.0, 1.0),
            self.host_width,
            self.host_height,
        ))
    }

    fn allow_event(&self) -> bool {
        let mut state = self.rate_limit.lock().expect("input rate limiter poisoned");
        let now = Instant::now();
        state.tokens = (state.tokens
            + now.duration_since(state.last_refill).as_secs_f64() * MAX_EVENTS_PER_SECOND)
            .min(BURST_CAPACITY);
        state.last_refill = now;
        if state.tokens < 1.0 {
            return false;
        }
        state.tokens -= 1.0;
        true
    }

    #[cfg(target_os = "macos")]
    pub fn inject(&self, event: &InputEvent) -> Result<(), InputError> {
        use core_graphics::event::{
            CGEvent, CGEventTapLocation, CGEventType, CGMouseButton, ScrollEventUnit,
        };
        use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
        use core_graphics::geometry::CGPoint;

        if !accessibility_is_trusted() {
            return Err(InputError::PermissionDenied);
        }
        if !self.allow_event() {
            return Err(InputError::RateLimited);
        }
        let (x, y) = self.map_coordinates(event)?;
        let point = CGPoint::new(x as f64, y as f64);
        let flags = cg_flags(event.modifiers);
        let source = || {
            CGEventSource::new(CGEventSourceStateID::HIDSystemState)
                .map_err(|_| InputError::EventCreation)
        };

        let cg_event = match event.event_type {
            InputEventType::MouseMove => CGEvent::new_mouse_event(
                source()?,
                CGEventType::MouseMoved,
                point,
                CGMouseButton::Left,
            ),
            InputEventType::LeftMouseDragged => CGEvent::new_mouse_event(
                source()?,
                CGEventType::LeftMouseDragged,
                point,
                CGMouseButton::Left,
            ),
            InputEventType::RightMouseDragged => CGEvent::new_mouse_event(
                source()?,
                CGEventType::RightMouseDragged,
                point,
                CGMouseButton::Right,
            ),
            InputEventType::LeftMouseDown => CGEvent::new_mouse_event(
                source()?,
                CGEventType::LeftMouseDown,
                point,
                CGMouseButton::Left,
            ),
            InputEventType::LeftMouseUp => CGEvent::new_mouse_event(
                source()?,
                CGEventType::LeftMouseUp,
                point,
                CGMouseButton::Left,
            ),
            InputEventType::RightMouseDown => CGEvent::new_mouse_event(
                source()?,
                CGEventType::RightMouseDown,
                point,
                CGMouseButton::Right,
            ),
            InputEventType::RightMouseUp => CGEvent::new_mouse_event(
                source()?,
                CGEventType::RightMouseUp,
                point,
                CGMouseButton::Right,
            ),
            InputEventType::ScrollWheel => CGEvent::new_scroll_event(
                source()?,
                ScrollEventUnit::PIXEL,
                2,
                event.scroll_dy.round() as i32,
                event.scroll_dx.round() as i32,
                0,
            ),
            InputEventType::KeyDown => CGEvent::new_keyboard_event(source()?, event.key_code, true),
            InputEventType::KeyUp => CGEvent::new_keyboard_event(source()?, event.key_code, false),
            InputEventType::FlagsChanged => {
                CGEvent::new_keyboard_event(source()?, event.key_code, true)
            }
        }
        .map_err(|_| InputError::EventCreation)?;
        cg_event.set_flags(flags);
        cg_event.post(CGEventTapLocation::HID);
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    pub fn inject(&self, _event: &InputEvent) -> Result<(), InputError> {
        Err(InputError::Unsupported)
    }
}

#[cfg(target_os = "macos")]
fn cg_flags(modifiers: Modifiers) -> core_graphics::event::CGEventFlags {
    use core_graphics::event::CGEventFlags;
    let mut flags = CGEventFlags::empty();
    if modifiers.contains(Modifiers::SHIFT) {
        flags.insert(CGEventFlags::CGEventFlagShift);
    }
    if modifiers.contains(Modifiers::CONTROL) {
        flags.insert(CGEventFlags::CGEventFlagControl);
    }
    if modifiers.contains(Modifiers::OPTION) {
        flags.insert(CGEventFlags::CGEventFlagAlternate);
    }
    if modifiers.contains(Modifiers::COMMAND) {
        flags.insert(CGEventFlags::CGEventFlagCommand);
    }
    if modifiers.contains(Modifiers::CAPS_LOCK) {
        flags.insert(CGEventFlags::CGEventFlagAlphaShift);
    }
    flags
}

#[cfg(target_os = "macos")]
pub fn accessibility_is_trusted() -> bool {
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }
    unsafe { AXIsProcessTrusted() != 0 }
}

#[cfg(not(target_os = "macos"))]
pub fn accessibility_is_trusted() -> bool {
    false
}

#[cfg(target_os = "macos")]
pub fn request_accessibility() -> bool {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        static kAXTrustedCheckOptionPrompt: core_foundation::string::CFStringRef;
        fn AXIsProcessTrustedWithOptions(
            options: core_foundation::dictionary::CFDictionaryRef,
        ) -> u8;
    }

    unsafe {
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let options = CFDictionary::from_CFType_pairs(&[(key, CFBoolean::true_value())]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) != 0
    }
}

#[cfg(not(target_os = "macos"))]
pub fn request_accessibility() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_coordinates_map_to_absolute_host_pixels() {
        let injector = InputInjector::new(1920.0, 1080.0);
        let event = InputEvent {
            event_type: InputEventType::MouseMove,
            x: 0.25,
            y: 0.75,
            key_code: 0,
            modifiers: Modifiers::empty(),
            scroll_dx: 0.0,
            scroll_dy: 0.0,
        };
        assert_eq!(injector.map_coordinates(&event).unwrap(), (480.0, 810.0));
    }

    #[test]
    fn coordinates_are_clamped_and_nan_is_rejected() {
        let injector = InputInjector::new(100.0, 50.0);
        let mut event = InputEvent {
            event_type: InputEventType::MouseMove,
            x: -1.0,
            y: 2.0,
            key_code: 0,
            modifiers: Modifiers::empty(),
            scroll_dx: 0.0,
            scroll_dy: 0.0,
        };
        assert_eq!(injector.map_coordinates(&event).unwrap(), (0.0, 50.0));
        event.x = f32::NAN;
        assert!(matches!(
            injector.map_coordinates(&event),
            Err(InputError::InvalidCoordinates)
        ));
    }
}
