use std::ops::{BitOr, BitOrAssign};

use crate::{
    codec::{push_f32, push_u16, Decoder},
    CodecError, WireCodec,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum InputEventType {
    MouseMove = 0,
    LeftMouseDown = 1,
    LeftMouseUp = 2,
    RightMouseDown = 3,
    RightMouseUp = 4,
    ScrollWheel = 5,
    KeyDown = 6,
    KeyUp = 7,
    FlagsChanged = 8,
    LeftMouseDragged = 9,
    RightMouseDragged = 10,
    MiddleMouseDown = 11,
    MiddleMouseUp = 12,
    Reset = 13,
    RelativeMove = 14,
    GamepadAxis = 15,
    GamepadButtonDown = 16,
    GamepadButtonUp = 17,
    PenMove = 18,
    PenDown = 19,
    PenUp = 20,
}

impl InputEventType {
    pub const ALL: [Self; 21] = [
        Self::MouseMove,
        Self::LeftMouseDown,
        Self::LeftMouseUp,
        Self::RightMouseDown,
        Self::RightMouseUp,
        Self::ScrollWheel,
        Self::KeyDown,
        Self::KeyUp,
        Self::FlagsChanged,
        Self::LeftMouseDragged,
        Self::RightMouseDragged,
        Self::MiddleMouseDown,
        Self::MiddleMouseUp,
        Self::Reset,
        Self::RelativeMove,
        Self::GamepadAxis,
        Self::GamepadButtonDown,
        Self::GamepadButtonUp,
        Self::PenMove,
        Self::PenDown,
        Self::PenUp,
    ];
}

impl TryFrom<u8> for InputEventType {
    type Error = CodecError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::MouseMove),
            1 => Ok(Self::LeftMouseDown),
            2 => Ok(Self::LeftMouseUp),
            3 => Ok(Self::RightMouseDown),
            4 => Ok(Self::RightMouseUp),
            5 => Ok(Self::ScrollWheel),
            6 => Ok(Self::KeyDown),
            7 => Ok(Self::KeyUp),
            8 => Ok(Self::FlagsChanged),
            9 => Ok(Self::LeftMouseDragged),
            10 => Ok(Self::RightMouseDragged),
            11 => Ok(Self::MiddleMouseDown),
            12 => Ok(Self::MiddleMouseUp),
            13 => Ok(Self::Reset),
            14 => Ok(Self::RelativeMove),
            15 => Ok(Self::GamepadAxis),
            16 => Ok(Self::GamepadButtonDown),
            17 => Ok(Self::GamepadButtonUp),
            18 => Ok(Self::PenMove),
            19 => Ok(Self::PenDown),
            20 => Ok(Self::PenUp),
            unknown => Err(CodecError::UnknownInputEventType(unknown)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers(u16);

impl Modifiers {
    pub const SHIFT: Self = Self(1 << 0);
    pub const CONTROL: Self = Self(1 << 1);
    pub const OPTION: Self = Self(1 << 2);
    pub const COMMAND: Self = Self(1 << 3);
    pub const CAPS_LOCK: Self = Self(1 << 4);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn from_bits_retain(bits: u16) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u16 {
        self.0
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for Modifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for Modifiers {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputEvent {
    pub event_type: InputEventType,
    pub x: f32,
    pub y: f32,
    pub key_code: u16,
    pub modifiers: Modifiers,
    pub scroll_dx: f32,
    pub scroll_dy: f32,
}

impl InputEvent {
    pub const SIZE: usize = 21;

    pub fn pen_move(x: f32, y: f32, pressure: f32, tilt_x: f32, tilt_y: f32) -> Self {
        Self {
            event_type: InputEventType::PenMove,
            x,
            y,
            key_code: (tilt_y.clamp(-90.0, 90.0) as i16) as u16,
            modifiers: Modifiers::empty(),
            scroll_dx: pressure.clamp(0.0, 1.0),
            scroll_dy: tilt_x.clamp(-90.0, 90.0),
        }
    }

    pub fn pen_down(x: f32, y: f32, pressure: f32, tilt_x: f32, tilt_y: f32) -> Self {
        Self {
            event_type: InputEventType::PenDown,
            x,
            y,
            key_code: (tilt_y.clamp(-90.0, 90.0) as i16) as u16,
            modifiers: Modifiers::empty(),
            scroll_dx: pressure.clamp(0.0, 1.0),
            scroll_dy: tilt_x.clamp(-90.0, 90.0),
        }
    }

    pub fn pen_up(x: f32, y: f32) -> Self {
        Self {
            event_type: InputEventType::PenUp,
            x,
            y,
            key_code: 0,
            modifiers: Modifiers::empty(),
            scroll_dx: 0.0,
            scroll_dy: 0.0,
        }
    }

    pub fn gamepad_axis(gamepad_id: u8, axis_id: u8, x: f32, y: f32) -> Self {
        let code = ((gamepad_id as u16) << 8) | (axis_id as u16);
        Self {
            event_type: InputEventType::GamepadAxis,
            x,
            y,
            key_code: code,
            modifiers: Modifiers::empty(),
            scroll_dx: 0.0,
            scroll_dy: 0.0,
        }
    }

    pub fn gamepad_button(gamepad_id: u8, button_id: u8, pressed: bool) -> Self {
        let code = ((gamepad_id as u16) << 8) | (button_id as u16);
        let event_type = if pressed {
            InputEventType::GamepadButtonDown
        } else {
            InputEventType::GamepadButtonUp
        };
        Self {
            event_type,
            x: 0.0,
            y: 0.0,
            key_code: code,
            modifiers: Modifiers::empty(),
            scroll_dx: 0.0,
            scroll_dy: 0.0,
        }
    }
}

impl WireCodec for InputEvent {
    fn encode(&self) -> Result<Vec<u8>, CodecError> {
        let mut output = Vec::with_capacity(Self::SIZE);
        output.push(self.event_type as u8);
        push_f32(&mut output, self.x);
        push_f32(&mut output, self.y);
        push_u16(&mut output, self.key_code);
        push_u16(&mut output, self.modifiers.bits());
        push_f32(&mut output, self.scroll_dx);
        push_f32(&mut output, self.scroll_dy);
        Ok(output)
    }

    fn decode(input: &[u8]) -> Result<Self, CodecError> {
        let mut decoder = Decoder::new(input);
        let event_type = InputEventType::try_from(decoder.u8("input event type")?)?;
        let x = decoder.f32("input x")?;
        let y = decoder.f32("input y")?;
        let key_code = decoder.u16("input key code")?;
        let modifiers = Modifiers::from_bits_retain(decoder.u16("input modifiers")?);
        let scroll_dx = decoder.f32("input scroll dx")?;
        let scroll_dy = decoder.f32("input scroll dy")?;
        decoder.finish("input event")?;
        Ok(Self {
            event_type,
            x,
            y,
            key_code,
            modifiers,
            scroll_dx,
            scroll_dy,
        })
    }
}

/// Normalizes bottom-left-origin client coordinates for the host wire format.
pub fn normalize_client_coordinates(
    local_x: f32,
    local_y: f32,
    view_width: f32,
    view_height: f32,
) -> (f32, f32) {
    let x = (local_x / view_width).clamp(0.0, 1.0);
    let y = 1.0 - (local_y / view_height).clamp(0.0, 1.0);
    (x, y)
}

/// Maps normalized wire coordinates into host logical pixels.
pub fn map_to_host_pixels(
    normalized_x: f32,
    normalized_y: f32,
    host_width: f32,
    host_height: f32,
) -> (f32, f32) {
    (normalized_x * host_width, normalized_y * host_height)
}
