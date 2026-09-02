//! Wayland-safe Linux input injection through `/dev/uinput`.
//!
//! The kernel sees the generated keyboard and absolute pointer as ordinary
//! input devices, so injection works under Hyprland without compositor-specific
//! virtual-input protocols.
//!
//! ## Permissions (Arch/Omarchy)
//!
//! Create a dedicated `uinput` group and grant it the device with udev:
//!
//! ```text
//! # /etc/udev/rules.d/70-eclipticrd-uinput.rules
//! KERNEL=="uinput", GROUP="uinput", MODE="0660", OPTIONS+="static_node=uinput"
//! ```
//!
//! Then run `sudo groupadd -f uinput`, add the host user with
//! `sudo usermod -aG uinput $USER`, load `uinput`, reload udev rules, and log
//! out/in. The process must never run setuid or as root merely for injection.

use std::io;

use erd_proto::{InputEvent as WireInputEvent, InputEventType, Modifiers};
use evdev::{
    uinput::VirtualDevice, AbsInfo, AbsoluteAxisCode, AbsoluteAxisEvent, AttributeSet, EventType,
    InputEvent, KeyCode, RelativeAxisCode, UinputAbsSetup,
};

const ABSOLUTE_AXIS_MAX: i32 = 65_535;

/// Target output position and dimensions in compositor/global logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub desktop_width: u32,
    pub desktop_height: u32,
}

impl OutputGeometry {
    pub fn single_output(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
            desktop_width: width,
            desktop_height: height,
        }
    }
}

/// Pure coordinate mapping used by the uinput backend.
pub fn map_normalized_to_output(
    normalized_x: f32,
    normalized_y: f32,
    geometry: OutputGeometry,
) -> (u32, u32) {
    let x = normalized_x.clamp(0.0, 1.0) * geometry.width.saturating_sub(1) as f32;
    let y = normalized_y.clamp(0.0, 1.0) * geometry.height.saturating_sub(1) as f32;
    (
        (i64::from(geometry.x) + x.round() as i64).max(0) as u32,
        (i64::from(geometry.y) + y.round() as i64).max(0) as u32,
    )
}

fn scale_to_uinput(value: u32, extent: u32) -> i32 {
    if extent <= 1 {
        return 0;
    }
    ((u64::from(value.min(extent - 1)) * ABSOLUTE_AXIS_MAX as u64) / u64::from(extent - 1)) as i32
}

/// Maps the v3/macOS virtual-key convention to Linux evdev keys.
///
/// The wire key code intentionally remains the existing macOS key code so all
/// hosts interoperate with the current clients.
pub fn macos_keycode_to_evdev(key_code: u16) -> Option<KeyCode> {
    Some(match key_code {
        0 => KeyCode::KEY_A,
        1 => KeyCode::KEY_S,
        2 => KeyCode::KEY_D,
        3 => KeyCode::KEY_F,
        4 => KeyCode::KEY_H,
        5 => KeyCode::KEY_G,
        6 => KeyCode::KEY_Z,
        7 => KeyCode::KEY_X,
        8 => KeyCode::KEY_C,
        9 => KeyCode::KEY_V,
        11 => KeyCode::KEY_B,
        12 => KeyCode::KEY_Q,
        13 => KeyCode::KEY_W,
        14 => KeyCode::KEY_E,
        15 => KeyCode::KEY_R,
        16 => KeyCode::KEY_Y,
        17 => KeyCode::KEY_T,
        18 => KeyCode::KEY_1,
        19 => KeyCode::KEY_2,
        20 => KeyCode::KEY_3,
        21 => KeyCode::KEY_4,
        22 => KeyCode::KEY_6,
        23 => KeyCode::KEY_5,
        24 => KeyCode::KEY_EQUAL,
        25 => KeyCode::KEY_9,
        26 => KeyCode::KEY_7,
        27 => KeyCode::KEY_MINUS,
        28 => KeyCode::KEY_8,
        29 => KeyCode::KEY_0,
        30 => KeyCode::KEY_RIGHTBRACE,
        31 => KeyCode::KEY_O,
        32 => KeyCode::KEY_U,
        33 => KeyCode::KEY_LEFTBRACE,
        34 => KeyCode::KEY_I,
        35 => KeyCode::KEY_P,
        36 => KeyCode::KEY_ENTER,
        37 => KeyCode::KEY_L,
        38 => KeyCode::KEY_J,
        39 => KeyCode::KEY_APOSTROPHE,
        40 => KeyCode::KEY_K,
        41 => KeyCode::KEY_SEMICOLON,
        42 => KeyCode::KEY_BACKSLASH,
        43 => KeyCode::KEY_COMMA,
        44 => KeyCode::KEY_SLASH,
        45 => KeyCode::KEY_N,
        46 => KeyCode::KEY_M,
        47 => KeyCode::KEY_DOT,
        48 => KeyCode::KEY_TAB,
        49 => KeyCode::KEY_SPACE,
        50 => KeyCode::KEY_GRAVE,
        51 => KeyCode::KEY_BACKSPACE,
        53 => KeyCode::KEY_ESC,
        54 => KeyCode::KEY_RIGHTMETA,
        55 => KeyCode::KEY_LEFTMETA,
        56 => KeyCode::KEY_LEFTSHIFT,
        57 => KeyCode::KEY_CAPSLOCK,
        58 => KeyCode::KEY_LEFTALT,
        59 => KeyCode::KEY_LEFTCTRL,
        60 => KeyCode::KEY_RIGHTSHIFT,
        61 => KeyCode::KEY_RIGHTALT,
        62 => KeyCode::KEY_RIGHTCTRL,
        63 => KeyCode::KEY_FN,
        64 => KeyCode::KEY_F17,
        65 => KeyCode::KEY_KPDOT,
        67 => KeyCode::KEY_KPASTERISK,
        69 => KeyCode::KEY_KPPLUS,
        71 => KeyCode::KEY_NUMLOCK,
        75 => KeyCode::KEY_KPSLASH,
        76 => KeyCode::KEY_KPENTER,
        78 => KeyCode::KEY_KPMINUS,
        79 => KeyCode::KEY_F18,
        80 => KeyCode::KEY_F19,
        81 => KeyCode::KEY_KPEQUAL,
        82 => KeyCode::KEY_KP0,
        83 => KeyCode::KEY_KP1,
        84 => KeyCode::KEY_KP2,
        85 => KeyCode::KEY_KP3,
        86 => KeyCode::KEY_KP4,
        87 => KeyCode::KEY_KP5,
        88 => KeyCode::KEY_KP6,
        89 => KeyCode::KEY_KP7,
        91 => KeyCode::KEY_KP8,
        92 => KeyCode::KEY_KP9,
        96 => KeyCode::KEY_F5,
        97 => KeyCode::KEY_F6,
        98 => KeyCode::KEY_F7,
        99 => KeyCode::KEY_F3,
        100 => KeyCode::KEY_F8,
        101 => KeyCode::KEY_F9,
        103 => KeyCode::KEY_F11,
        105 => KeyCode::KEY_F13,
        106 => KeyCode::KEY_F16,
        107 => KeyCode::KEY_F14,
        109 => KeyCode::KEY_F10,
        111 => KeyCode::KEY_F12,
        113 => KeyCode::KEY_F15,
        114 => KeyCode::KEY_INSERT,
        115 => KeyCode::KEY_HOME,
        116 => KeyCode::KEY_PAGEUP,
        117 => KeyCode::KEY_DELETE,
        118 => KeyCode::KEY_F4,
        119 => KeyCode::KEY_END,
        120 => KeyCode::KEY_F2,
        121 => KeyCode::KEY_PAGEDOWN,
        122 => KeyCode::KEY_F1,
        123 => KeyCode::KEY_LEFT,
        124 => KeyCode::KEY_RIGHT,
        125 => KeyCode::KEY_DOWN,
        126 => KeyCode::KEY_UP,
        _ => return None,
    })
}

fn modifier_key(modifier: Modifiers) -> Option<KeyCode> {
    if modifier == Modifiers::SHIFT {
        Some(KeyCode::KEY_LEFTSHIFT)
    } else if modifier == Modifiers::CONTROL {
        Some(KeyCode::KEY_LEFTCTRL)
    } else if modifier == Modifiers::OPTION {
        Some(KeyCode::KEY_LEFTALT)
    } else if modifier == Modifiers::COMMAND {
        Some(KeyCode::KEY_LEFTMETA)
    } else if modifier == Modifiers::CAPS_LOCK {
        Some(KeyCode::KEY_CAPSLOCK)
    } else {
        None
    }
}

fn modifier_events(previous: Modifiers, current: Modifiers) -> Vec<InputEvent> {
    [
        Modifiers::SHIFT,
        Modifiers::CONTROL,
        Modifiers::OPTION,
        Modifiers::COMMAND,
        Modifiers::CAPS_LOCK,
    ]
    .into_iter()
    .filter_map(|modifier| {
        let was_down = previous.contains(modifier);
        let is_down = current.contains(modifier);
        (was_down != is_down).then(|| {
            InputEvent::new(
                EventType::KEY.0,
                modifier_key(modifier).expect("known modifier").code(),
                i32::from(is_down),
            )
        })
    })
    .collect()
}

pub struct LinuxInputInjector {
    pointer: VirtualDevice,
    keyboard: VirtualDevice,
    geometry: OutputGeometry,
    modifiers: Modifiers,
}

impl LinuxInputInjector {
    pub fn new(geometry: OutputGeometry) -> io::Result<Self> {
        if geometry.width == 0
            || geometry.height == 0
            || geometry.desktop_width == 0
            || geometry.desktop_height == 0
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "output and desktop dimensions must be non-zero",
            ));
        }

        let pointer_keys =
            AttributeSet::from_iter([KeyCode::BTN_LEFT, KeyCode::BTN_RIGHT, KeyCode::BTN_MIDDLE]);
        let relative_axes = AttributeSet::from_iter([
            RelativeAxisCode::REL_WHEEL,
            RelativeAxisCode::REL_HWHEEL,
            RelativeAxisCode::REL_WHEEL_HI_RES,
            RelativeAxisCode::REL_HWHEEL_HI_RES,
        ]);
        let abs_info = AbsInfo::new(0, 0, ABSOLUTE_AXIS_MAX, 0, 0, 1);
        let abs_x = UinputAbsSetup::new(AbsoluteAxisCode::ABS_X, abs_info);
        let abs_y = UinputAbsSetup::new(AbsoluteAxisCode::ABS_Y, abs_info);
        let pointer = VirtualDevice::builder()?
            .name("EclipticRD Virtual Pointer")
            .with_keys(&pointer_keys)?
            .with_relative_axes(&relative_axes)?
            .with_absolute_axis(&abs_x)?
            .with_absolute_axis(&abs_y)?
            .build()?;

        let mut keyboard_keys = AttributeSet::<KeyCode>::new();
        for code in 0..=126 {
            if let Some(key) = macos_keycode_to_evdev(code) {
                keyboard_keys.insert(key);
            }
        }
        for key in [
            KeyCode::KEY_LEFTSHIFT,
            KeyCode::KEY_LEFTCTRL,
            KeyCode::KEY_LEFTALT,
            KeyCode::KEY_LEFTMETA,
            KeyCode::KEY_CAPSLOCK,
        ] {
            keyboard_keys.insert(key);
        }
        let keyboard = VirtualDevice::builder()?
            .name("EclipticRD Virtual Keyboard")
            .with_keys(&keyboard_keys)?
            .build()?;

        Ok(Self {
            pointer,
            keyboard,
            geometry,
            modifiers: Modifiers::empty(),
        })
    }

    pub fn update_geometry(&mut self, geometry: OutputGeometry) {
        self.geometry = geometry;
    }

    pub fn inject(&mut self, event: &WireInputEvent) -> io::Result<()> {
        if !event.x.is_finite()
            || !event.y.is_finite()
            || !event.scroll_dx.is_finite()
            || !event.scroll_dy.is_finite()
        {
            return Ok(());
        }

        match event.event_type {
            InputEventType::MouseMove
            | InputEventType::LeftMouseDragged
            | InputEventType::RightMouseDragged
            | InputEventType::LeftMouseDown
            | InputEventType::LeftMouseUp
            | InputEventType::RightMouseDown
            | InputEventType::RightMouseUp => {
                let (pixel_x, pixel_y) = map_normalized_to_output(event.x, event.y, self.geometry);
                let mut events = vec![
                    *AbsoluteAxisEvent::new(
                        AbsoluteAxisCode::ABS_X,
                        scale_to_uinput(pixel_x, self.geometry.desktop_width),
                    ),
                    *AbsoluteAxisEvent::new(
                        AbsoluteAxisCode::ABS_Y,
                        scale_to_uinput(pixel_y, self.geometry.desktop_height),
                    ),
                ];
                let button = match event.event_type {
                    InputEventType::LeftMouseDown => Some((KeyCode::BTN_LEFT, 1)),
                    InputEventType::LeftMouseUp => Some((KeyCode::BTN_LEFT, 0)),
                    InputEventType::RightMouseDown => Some((KeyCode::BTN_RIGHT, 1)),
                    InputEventType::RightMouseUp => Some((KeyCode::BTN_RIGHT, 0)),
                    _ => None,
                };
                if let Some((key, value)) = button {
                    events.push(InputEvent::new(EventType::KEY.0, key.code(), value));
                }
                self.pointer.emit(&events)
            }
            InputEventType::ScrollWheel => {
                let vertical = scroll_units(event.scroll_dy);
                let horizontal = scroll_units(event.scroll_dx);
                let mut events = Vec::with_capacity(4);
                if vertical != 0 {
                    events.push(InputEvent::new(
                        EventType::RELATIVE.0,
                        RelativeAxisCode::REL_WHEEL.0,
                        vertical.signum(),
                    ));
                    events.push(InputEvent::new(
                        EventType::RELATIVE.0,
                        RelativeAxisCode::REL_WHEEL_HI_RES.0,
                        vertical,
                    ));
                }
                if horizontal != 0 {
                    events.push(InputEvent::new(
                        EventType::RELATIVE.0,
                        RelativeAxisCode::REL_HWHEEL.0,
                        horizontal.signum(),
                    ));
                    events.push(InputEvent::new(
                        EventType::RELATIVE.0,
                        RelativeAxisCode::REL_HWHEEL_HI_RES.0,
                        horizontal,
                    ));
                }
                if events.is_empty() {
                    Ok(())
                } else {
                    self.pointer.emit(&events)
                }
            }
            InputEventType::KeyDown | InputEventType::KeyUp => {
                let modifier_changes = modifier_events(self.modifiers, event.modifiers);
                if !modifier_changes.is_empty() {
                    self.keyboard.emit(&modifier_changes)?;
                }
                self.modifiers = event.modifiers;
                if let Some(key) = macos_keycode_to_evdev(event.key_code) {
                    self.keyboard.emit(&[InputEvent::new(
                        EventType::KEY.0,
                        key.code(),
                        i32::from(event.event_type == InputEventType::KeyDown),
                    )])?;
                }
                Ok(())
            }
            InputEventType::FlagsChanged => {
                let events = modifier_events(self.modifiers, event.modifiers);
                self.modifiers = event.modifiers;
                if events.is_empty() {
                    Ok(())
                } else {
                    self.keyboard.emit(&events)
                }
            }
        }
    }
}

fn scroll_units(delta: f32) -> i32 {
    if delta == 0.0 {
        0
    } else {
        let rounded = delta.round() as i32;
        if rounded == 0 {
            delta.signum() as i32
        } else {
            rounded
        }
    }
}

impl Drop for LinuxInputInjector {
    fn drop(&mut self) {
        let releases = modifier_events(self.modifiers, Modifiers::empty());
        if !releases.is_empty() {
            let _ = self.keyboard.emit(&releases);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_normalized_coordinates_into_target_output() {
        let geometry = OutputGeometry {
            x: 1920,
            y: 100,
            width: 2560,
            height: 1440,
            desktop_width: 4480,
            desktop_height: 1540,
        };
        assert_eq!(map_normalized_to_output(0.0, 0.0, geometry), (1920, 100));
        assert_eq!(map_normalized_to_output(1.0, 1.0, geometry), (4479, 1539));
        assert_eq!(map_normalized_to_output(-2.0, 2.0, geometry), (1920, 1539));
    }

    #[test]
    fn maps_v3_keycodes_and_modifiers() {
        assert_eq!(macos_keycode_to_evdev(0), Some(KeyCode::KEY_A));
        assert_eq!(macos_keycode_to_evdev(36), Some(KeyCode::KEY_ENTER));
        assert_eq!(macos_keycode_to_evdev(123), Some(KeyCode::KEY_LEFT));
        assert_eq!(macos_keycode_to_evdev(u16::MAX), None);

        let flags = Modifiers::SHIFT | Modifiers::COMMAND | Modifiers::CAPS_LOCK;
        let events = modifier_events(Modifiers::empty(), flags);
        assert_eq!(events.len(), 3);
        assert!(events.iter().all(|event| event.value() == 1));
        let releases = modifier_events(flags, Modifiers::empty());
        assert!(releases.iter().all(|event| event.value() == 0));
    }
}
