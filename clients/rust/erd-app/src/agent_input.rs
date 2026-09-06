use std::{
    collections::HashSet,
    fmt,
    io::Cursor,
    time::{Duration, Instant},
};

use base64::Engine;
use erd_proto::{InputEvent, InputEventType, Modifiers};
use image::{codecs::jpeg::JpegEncoder, codecs::png::PngEncoder, ColorType, ImageEncoder};
use serde::{Deserialize, Serialize};

use crate::input::InputKey;

fn default_click_count() -> u32 {
    1
}

fn default_drag_steps() -> u32 {
    10
}

fn default_drag_duration() -> u64 {
    200
}

fn default_key_hold_ms() -> u64 {
    50
}

fn default_type_delay_ms() -> u64 {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenInfo {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub connected_host: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ScreenshotFormat {
    #[default]
    Png,
    Jpeg,
}

pub fn nv12_to_rgb(width: u32, height: u32, nv12_buf: &[u8]) -> Option<Vec<u8>> {
    let w = width as usize;
    let h = height as usize;
    let expected_len = w * h * 3 / 2;
    if nv12_buf.len() < expected_len || w == 0 || h == 0 {
        return None;
    }

    let mut rgb = Vec::with_capacity(w * h * 3);
    let uv_plane_start = w * h;

    for y in 0..h {
        let y_row_start = y * w;
        let uv_row_start = uv_plane_start + (y / 2) * w;
        for x in 0..w {
            let y_val = nv12_buf[y_row_start + x] as f32;
            let uv_offset = uv_row_start + (x / 2) * 2;
            let u_val = nv12_buf[uv_offset] as f32 - 128.0;
            let v_val = nv12_buf[uv_offset + 1] as f32 - 128.0;

            let r = (y_val + 1.5748 * v_val).clamp(0.0, 255.0) as u8;
            let g = (y_val - 0.1873 * u_val - 0.4681 * v_val).clamp(0.0, 255.0) as u8;
            let b = (y_val + 1.8556 * u_val).clamp(0.0, 255.0) as u8;

            rgb.push(r);
            rgb.push(g);
            rgb.push(b);
        }
    }

    Some(rgb)
}

pub fn encode_nv12_screenshot(
    width: u32,
    height: u32,
    nv12_buf: &[u8],
    format: ScreenshotFormat,
) -> Result<String, String> {
    let rgb = nv12_to_rgb(width, height, nv12_buf)
        .ok_or_else(|| "invalid nv12 buffer size or zero dimensions".to_string())?;

    let mut output = Vec::new();
    let mut cursor = Cursor::new(&mut output);

    match format {
        ScreenshotFormat::Png => {
            let encoder = PngEncoder::new(&mut cursor);
            encoder
                .write_image(&rgb, width, height, ColorType::Rgb8.into())
                .map_err(|e| format!("failed to encode png: {e}"))?;
        }
        ScreenshotFormat::Jpeg => {
            let mut encoder = JpegEncoder::new_with_quality(&mut cursor, 80);
            encoder
                .encode(&rgb, width, height, ColorType::Rgb8.into())
                .map_err(|e| format!("failed to encode jpeg: {e}"))?;
        }
    }

    Ok(base64::prelude::BASE64_STANDARD.encode(&output))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MouseButton {
    #[default]
    Left,
    Right,
    Middle,
}

impl MouseButton {
    pub fn to_down_event_type(self) -> InputEventType {
        match self {
            Self::Left => InputEventType::LeftMouseDown,
            Self::Right => InputEventType::RightMouseDown,
            Self::Middle => InputEventType::MiddleMouseDown,
        }
    }

    pub fn to_up_event_type(self) -> InputEventType {
        match self {
            Self::Left => InputEventType::LeftMouseUp,
            Self::Right => InputEventType::RightMouseUp,
            Self::Middle => InputEventType::MiddleMouseUp,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AgentAction {
    MouseMove {
        x: f32,
        y: f32,
        #[serde(default)]
        normalized: bool,
    },
    MouseDown {
        #[serde(default)]
        button: MouseButton,
    },
    MouseUp {
        #[serde(default)]
        button: MouseButton,
    },
    Click {
        x: f32,
        y: f32,
        #[serde(default)]
        button: MouseButton,
        #[serde(default = "default_click_count")]
        count: u32,
        #[serde(default)]
        normalized: bool,
    },
    Drag {
        start_x: f32,
        start_y: f32,
        end_x: f32,
        end_y: f32,
        #[serde(default)]
        button: MouseButton,
        #[serde(default = "default_drag_steps")]
        steps: u32,
        #[serde(default = "default_drag_duration")]
        duration_ms: u64,
        #[serde(default)]
        normalized: bool,
    },
    Scroll {
        dx: f32,
        dy: f32,
        #[serde(default)]
        x: Option<f32>,
        #[serde(default)]
        y: Option<f32>,
        #[serde(default)]
        normalized: bool,
    },
    KeyDown {
        key: String,
    },
    KeyUp {
        key: String,
    },
    KeyPress {
        key: String,
        #[serde(default = "default_key_hold_ms")]
        hold_ms: u64,
    },
    Hotkey {
        keys: Vec<String>,
    },
    TypeText {
        text: String,
        #[serde(default = "default_type_delay_ms")]
        delay_ms: u64,
        #[serde(default)]
        paste_mode: bool,
    },
    ReleaseAll,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentInputError {
    UnknownKey(String),
    EmptyHotkey,
    InvalidCoordinates { x: f32, y: f32 },
}

impl fmt::Display for AgentInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownKey(k) => write!(f, "unknown key name: '{k}'"),
            Self::EmptyHotkey => write!(f, "hotkey must contain at least one key"),
            Self::InvalidCoordinates { x, y } => {
                write!(f, "invalid non-finite coordinates ({x}, {y})")
            }
        }
    }
}

impl std::error::Error for AgentInputError {}

pub fn parse_key_name(name: &str) -> Result<(InputKey, Modifiers), AgentInputError> {
    let trimmed = name.trim();
    let lower = trimmed.to_ascii_lowercase();

    match lower.as_str() {
        "shift" | "leftshift" | "rightshift" => {
            return Ok((InputKey::WindowsVirtualKey(0x10), Modifiers::SHIFT));
        }
        "ctrl" | "control" | "leftctrl" | "rightctrl" => {
            return Ok((InputKey::WindowsVirtualKey(0x11), Modifiers::CONTROL));
        }
        "alt" | "option" | "leftalt" | "rightalt" => {
            return Ok((InputKey::WindowsVirtualKey(0x12), Modifiers::OPTION));
        }
        "super" | "win" | "windows" | "cmd" | "command" | "meta" => {
            return Ok((InputKey::WindowsVirtualKey(0x5B), Modifiers::COMMAND));
        }
        "capslock" => {
            return Ok((InputKey::WindowsVirtualKey(0x14), Modifiers::CAPS_LOCK));
        }
        _ => {}
    }

    let vk = match lower.as_str() {
        "enter" | "return" => 0x0D,
        "esc" | "escape" => 0x1B,
        "tab" => 0x09,
        "space" | " " => 0x20,
        "backspace" => 0x08,
        "delete" | "del" => 0x2E,
        "insert" | "ins" => 0x2D,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" | "pgup" => 0x21,
        "pagedown" | "pgdn" => 0x22,
        "arrowup" | "up" => 0x26,
        "arrowdown" | "down" => 0x28,
        "arrowleft" | "left" => 0x25,
        "arrowright" | "right" => 0x27,
        "f1" => 0x70,
        "f2" => 0x71,
        "f3" => 0x72,
        "f4" => 0x73,
        "f5" => 0x74,
        "f6" => 0x75,
        "f7" => 0x76,
        "f8" => 0x77,
        "f9" => 0x78,
        "f10" => 0x79,
        "f11" => 0x7A,
        "f12" => 0x7B,
        "-" | "minus" => 0xBD,
        "=" | "equal" => 0xBB,
        "[" | "bracketleft" => 0xDB,
        "]" | "bracketright" => 0xDD,
        "\\" | "backslash" => 0xDC,
        ";" | "semicolon" => 0xBA,
        "'" | "quote" => 0xDE,
        "," | "comma" => 0xBC,
        "." | "period" => 0xBE,
        "/" | "slash" => 0xBF,
        "`" | "grave" => 0xC0,
        _ => {
            if trimmed.len() == 1 {
                let ch = trimmed.chars().next().unwrap();
                if ch.is_ascii_alphabetic() {
                    let upper = ch.to_ascii_uppercase() as u16;
                    let is_upper = ch.is_ascii_uppercase();
                    let mods = if is_upper {
                        Modifiers::SHIFT
                    } else {
                        Modifiers::empty()
                    };
                    return Ok((InputKey::WindowsVirtualKey(upper), mods));
                } else if ch.is_ascii_digit() {
                    return Ok((InputKey::WindowsVirtualKey(ch as u16), Modifiers::empty()));
                }
            }
            return Err(AgentInputError::UnknownKey(name.to_string()));
        }
    };

    Ok((InputKey::WindowsVirtualKey(vk), Modifiers::empty()))
}

pub fn parse_hotkey_string(hotkey: &str) -> Result<(InputKey, Modifiers), AgentInputError> {
    let parts: Vec<&str> = hotkey
        .split('+')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return Err(AgentInputError::EmptyHotkey);
    }

    let mut combined_modifiers = Modifiers::empty();
    let mut target_key = None;

    for (i, part) in parts.iter().enumerate() {
        let is_last = i == parts.len() - 1;
        let (key, mods) = parse_key_name(part)?;
        if is_last {
            target_key = Some(key);
            combined_modifiers |= mods;
        } else if mods == Modifiers::empty() {
            combined_modifiers |= match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => Modifiers::CONTROL,
                "alt" | "opt" | "option" => Modifiers::OPTION,
                "shift" => Modifiers::SHIFT,
                "super" | "win" | "cmd" | "command" => Modifiers::COMMAND,
                _ => return Err(AgentInputError::UnknownKey(part.to_string())),
            };
        } else {
            combined_modifiers |= mods;
        }
    }

    let key = target_key.ok_or(AgentInputError::EmptyHotkey)?;
    Ok((key, combined_modifiers))
}

pub fn normalize_agent_coordinates(
    x: f32,
    y: f32,
    normalized: bool,
    host_width: f32,
    host_height: f32,
) -> Result<(f32, f32), AgentInputError> {
    if !x.is_finite() || !y.is_finite() {
        return Err(AgentInputError::InvalidCoordinates { x, y });
    }

    if normalized {
        let norm_x = x.clamp(0.0, 1.0);
        let norm_y = (1.0 - y).clamp(0.0, 1.0);
        Ok((norm_x, norm_y))
    } else {
        if host_width <= 0.0 || host_height <= 0.0 {
            return Ok((0.0, 0.0));
        }
        let norm_x = (x / host_width).clamp(0.0, 1.0);
        let norm_y = (1.0 - (y / host_height)).clamp(0.0, 1.0);
        Ok((norm_x, norm_y))
    }
}

pub fn synthesize_ascii_char(ch: char) -> Option<(InputKey, Modifiers)> {
    if ch.is_ascii_alphabetic() {
        let vk = ch.to_ascii_uppercase() as u16;
        let mods = if ch.is_ascii_uppercase() {
            Modifiers::SHIFT
        } else {
            Modifiers::empty()
        };
        Some((InputKey::WindowsVirtualKey(vk), mods))
    } else if ch.is_ascii_digit() {
        Some((InputKey::WindowsVirtualKey(ch as u16), Modifiers::empty()))
    } else {
        match ch {
            ' ' => Some((InputKey::WindowsVirtualKey(0x20), Modifiers::empty())),
            '\n' | '\r' => Some((InputKey::WindowsVirtualKey(0x0D), Modifiers::empty())),
            '\t' => Some((InputKey::WindowsVirtualKey(0x09), Modifiers::empty())),
            '-' => Some((InputKey::WindowsVirtualKey(0xBD), Modifiers::empty())),
            '_' => Some((InputKey::WindowsVirtualKey(0xBD), Modifiers::SHIFT)),
            '=' => Some((InputKey::WindowsVirtualKey(0xBB), Modifiers::empty())),
            '+' => Some((InputKey::WindowsVirtualKey(0xBB), Modifiers::SHIFT)),
            '[' => Some((InputKey::WindowsVirtualKey(0xDB), Modifiers::empty())),
            '{' => Some((InputKey::WindowsVirtualKey(0xDB), Modifiers::SHIFT)),
            ']' => Some((InputKey::WindowsVirtualKey(0xDD), Modifiers::empty())),
            '}' => Some((InputKey::WindowsVirtualKey(0xDD), Modifiers::SHIFT)),
            '\\' => Some((InputKey::WindowsVirtualKey(0xDC), Modifiers::empty())),
            '|' => Some((InputKey::WindowsVirtualKey(0xDC), Modifiers::SHIFT)),
            ';' => Some((InputKey::WindowsVirtualKey(0xBA), Modifiers::empty())),
            ':' => Some((InputKey::WindowsVirtualKey(0xBA), Modifiers::SHIFT)),
            '\'' => Some((InputKey::WindowsVirtualKey(0xDE), Modifiers::empty())),
            '"' => Some((InputKey::WindowsVirtualKey(0xDE), Modifiers::SHIFT)),
            ',' => Some((InputKey::WindowsVirtualKey(0xBC), Modifiers::empty())),
            '<' => Some((InputKey::WindowsVirtualKey(0xBC), Modifiers::SHIFT)),
            '.' => Some((InputKey::WindowsVirtualKey(0xBE), Modifiers::empty())),
            '>' => Some((InputKey::WindowsVirtualKey(0xBE), Modifiers::SHIFT)),
            '/' => Some((InputKey::WindowsVirtualKey(0xBF), Modifiers::empty())),
            '?' => Some((InputKey::WindowsVirtualKey(0xBF), Modifiers::SHIFT)),
            '`' => Some((InputKey::WindowsVirtualKey(0xC0), Modifiers::empty())),
            '~' => Some((InputKey::WindowsVirtualKey(0xC0), Modifiers::SHIFT)),
            '!' => Some((InputKey::WindowsVirtualKey(0x31), Modifiers::SHIFT)),
            '@' => Some((InputKey::WindowsVirtualKey(0x32), Modifiers::SHIFT)),
            '#' => Some((InputKey::WindowsVirtualKey(0x33), Modifiers::SHIFT)),
            '$' => Some((InputKey::WindowsVirtualKey(0x34), Modifiers::SHIFT)),
            '%' => Some((InputKey::WindowsVirtualKey(0x35), Modifiers::SHIFT)),
            '^' => Some((InputKey::WindowsVirtualKey(0x36), Modifiers::SHIFT)),
            '&' => Some((InputKey::WindowsVirtualKey(0x37), Modifiers::SHIFT)),
            '*' => Some((InputKey::WindowsVirtualKey(0x38), Modifiers::SHIFT)),
            '(' => Some((InputKey::WindowsVirtualKey(0x39), Modifiers::SHIFT)),
            ')' => Some((InputKey::WindowsVirtualKey(0x30), Modifiers::SHIFT)),
            _ => None,
        }
    }
}

pub struct InputStateTracker {
    active_buttons: HashSet<MouseButton>,
    active_keys: HashSet<u16>,
    active_modifiers: Modifiers,
    last_action_at: Instant,
    hold_timeout: Duration,
}

impl Default for InputStateTracker {
    fn default() -> Self {
        Self {
            active_buttons: HashSet::new(),
            active_keys: HashSet::new(),
            active_modifiers: Modifiers::empty(),
            last_action_at: Instant::now(),
            hold_timeout: Duration::from_millis(5000),
        }
    }
}

impl InputStateTracker {
    pub fn new(hold_timeout: Duration) -> Self {
        Self {
            hold_timeout,
            ..Default::default()
        }
    }

    pub fn record_button_down(&mut self, button: MouseButton) {
        self.active_buttons.insert(button);
        self.last_action_at = Instant::now();
    }

    pub fn record_button_up(&mut self, button: MouseButton) {
        self.active_buttons.remove(&button);
        self.last_action_at = Instant::now();
    }

    pub fn record_key_down(&mut self, key_code: u16, modifiers: Modifiers) {
        self.active_keys.insert(key_code);
        self.active_modifiers |= modifiers;
        self.last_action_at = Instant::now();
    }

    pub fn record_key_up(&mut self, key_code: u16) {
        self.active_keys.remove(&key_code);
        self.last_action_at = Instant::now();
    }

    pub fn is_empty(&self) -> bool {
        self.active_buttons.is_empty()
            && self.active_keys.is_empty()
            && self.active_modifiers == Modifiers::empty()
    }

    pub fn clear(&mut self) {
        self.active_buttons.clear();
        self.active_keys.clear();
        self.active_modifiers = Modifiers::empty();
        self.last_action_at = Instant::now();
    }

    pub fn release_all(&mut self, current_x: f32, current_y: f32) -> Vec<InputEvent> {
        let mut events = Vec::new();

        for button in self.active_buttons.drain() {
            events.push(InputEvent {
                event_type: button.to_up_event_type(),
                x: current_x,
                y: current_y,
                key_code: 0,
                modifiers: self.active_modifiers,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
        }

        for key_code in self.active_keys.drain() {
            events.push(InputEvent {
                event_type: InputEventType::KeyUp,
                x: current_x,
                y: current_y,
                key_code,
                modifiers: self.active_modifiers,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
        }

        self.active_modifiers = Modifiers::empty();

        events.push(InputEvent {
            event_type: InputEventType::Reset,
            x: current_x,
            y: current_y,
            key_code: 0,
            modifiers: Modifiers::empty(),
            scroll_dx: 0.0,
            scroll_dy: 0.0,
        });

        self.last_action_at = Instant::now();
        events
    }

    pub fn check_timeout(&mut self, current_x: f32, current_y: f32) -> Option<Vec<InputEvent>> {
        if !self.is_empty() && self.last_action_at.elapsed() > self.hold_timeout {
            Some(self.release_all(current_x, current_y))
        } else {
            None
        }
    }
}

pub fn convert_agent_action_to_events(
    action: &AgentAction,
    tracker: &mut InputStateTracker,
    current_pos: &mut (f32, f32),
    host_width: f32,
    host_height: f32,
) -> Result<Vec<InputEvent>, AgentInputError> {
    let mut events = Vec::new();

    match action {
        AgentAction::MouseMove { x, y, normalized } => {
            let (wire_x, wire_y) =
                normalize_agent_coordinates(*x, *y, *normalized, host_width, host_height)?;
            *current_pos = (wire_x, wire_y);
            events.push(InputEvent {
                event_type: InputEventType::MouseMove,
                x: wire_x,
                y: wire_y,
                key_code: 0,
                modifiers: tracker.active_modifiers,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
        }
        AgentAction::MouseDown { button } => {
            tracker.record_button_down(*button);
            events.push(InputEvent {
                event_type: button.to_down_event_type(),
                x: current_pos.0,
                y: current_pos.1,
                key_code: 0,
                modifiers: tracker.active_modifiers,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
        }
        AgentAction::MouseUp { button } => {
            tracker.record_button_up(*button);
            events.push(InputEvent {
                event_type: button.to_up_event_type(),
                x: current_pos.0,
                y: current_pos.1,
                key_code: 0,
                modifiers: tracker.active_modifiers,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
        }
        AgentAction::Click {
            x,
            y,
            button,
            count,
            normalized,
        } => {
            let (wire_x, wire_y) =
                normalize_agent_coordinates(*x, *y, *normalized, host_width, host_height)?;
            *current_pos = (wire_x, wire_y);
            events.push(InputEvent {
                event_type: InputEventType::MouseMove,
                x: wire_x,
                y: wire_y,
                key_code: 0,
                modifiers: tracker.active_modifiers,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
            for _ in 0..*count {
                events.push(InputEvent {
                    event_type: button.to_down_event_type(),
                    x: wire_x,
                    y: wire_y,
                    key_code: 0,
                    modifiers: tracker.active_modifiers,
                    scroll_dx: 0.0,
                    scroll_dy: 0.0,
                });
                events.push(InputEvent {
                    event_type: button.to_up_event_type(),
                    x: wire_x,
                    y: wire_y,
                    key_code: 0,
                    modifiers: tracker.active_modifiers,
                    scroll_dx: 0.0,
                    scroll_dy: 0.0,
                });
            }
        }
        AgentAction::Drag {
            start_x,
            start_y,
            end_x,
            end_y,
            button,
            steps,
            normalized,
            ..
        } => {
            let (sx, sy) = normalize_agent_coordinates(
                *start_x,
                *start_y,
                *normalized,
                host_width,
                host_height,
            )?;
            let (ex, ey) =
                normalize_agent_coordinates(*end_x, *end_y, *normalized, host_width, host_height)?;

            *current_pos = (sx, sy);
            events.push(InputEvent {
                event_type: InputEventType::MouseMove,
                x: sx,
                y: sy,
                key_code: 0,
                modifiers: tracker.active_modifiers,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });

            tracker.record_button_down(*button);
            events.push(InputEvent {
                event_type: button.to_down_event_type(),
                x: sx,
                y: sy,
                key_code: 0,
                modifiers: tracker.active_modifiers,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });

            let step_count = (*steps).max(1);
            for i in 1..=step_count {
                let t = i as f32 / step_count as f32;
                let cur_x = sx + (ex - sx) * t;
                let cur_y = sy + (ey - sy) * t;
                *current_pos = (cur_x, cur_y);
                events.push(InputEvent {
                    event_type: InputEventType::LeftMouseDragged,
                    x: cur_x,
                    y: cur_y,
                    key_code: 0,
                    modifiers: tracker.active_modifiers,
                    scroll_dx: 0.0,
                    scroll_dy: 0.0,
                });
            }

            tracker.record_button_up(*button);
            events.push(InputEvent {
                event_type: button.to_up_event_type(),
                x: ex,
                y: ey,
                key_code: 0,
                modifiers: tracker.active_modifiers,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
        }
        AgentAction::Scroll {
            dx,
            dy,
            x,
            y,
            normalized,
        } => {
            if let (Some(px), Some(py)) = (x, y) {
                let (wx, wy) =
                    normalize_agent_coordinates(*px, *py, *normalized, host_width, host_height)?;
                *current_pos = (wx, wy);
            }
            events.push(InputEvent {
                event_type: InputEventType::ScrollWheel,
                x: current_pos.0,
                y: current_pos.1,
                key_code: 0,
                modifiers: tracker.active_modifiers,
                scroll_dx: *dx,
                scroll_dy: *dy,
            });
        }
        AgentAction::KeyDown { key } => {
            let (k, mods) = parse_key_name(key)?;
            let macos_code = crate::input::InputKeyMap::to_macos(k).unwrap_or(0);
            tracker.record_key_down(macos_code, mods);
            events.push(InputEvent {
                event_type: InputEventType::KeyDown,
                x: current_pos.0,
                y: current_pos.1,
                key_code: macos_code,
                modifiers: tracker.active_modifiers,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
        }
        AgentAction::KeyUp { key } => {
            let (k, _) = parse_key_name(key)?;
            let macos_code = crate::input::InputKeyMap::to_macos(k).unwrap_or(0);
            tracker.record_key_up(macos_code);
            events.push(InputEvent {
                event_type: InputEventType::KeyUp,
                x: current_pos.0,
                y: current_pos.1,
                key_code: macos_code,
                modifiers: tracker.active_modifiers,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
        }
        AgentAction::KeyPress { key, .. } => {
            let (k, mods) = parse_key_name(key)?;
            let macos_code = crate::input::InputKeyMap::to_macos(k).unwrap_or(0);
            events.push(InputEvent {
                event_type: InputEventType::KeyDown,
                x: current_pos.0,
                y: current_pos.1,
                key_code: macos_code,
                modifiers: mods,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
            events.push(InputEvent {
                event_type: InputEventType::KeyUp,
                x: current_pos.0,
                y: current_pos.1,
                key_code: macos_code,
                modifiers: mods,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
        }
        AgentAction::Hotkey { keys } => {
            let combined = keys.join("+");
            let (k, mods) = parse_hotkey_string(&combined)?;
            let macos_code = crate::input::InputKeyMap::to_macos(k).unwrap_or(0);
            events.push(InputEvent {
                event_type: InputEventType::KeyDown,
                x: current_pos.0,
                y: current_pos.1,
                key_code: macos_code,
                modifiers: mods,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
            events.push(InputEvent {
                event_type: InputEventType::KeyUp,
                x: current_pos.0,
                y: current_pos.1,
                key_code: macos_code,
                modifiers: mods,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            });
        }
        AgentAction::TypeText { text, .. } => {
            for ch in text.chars() {
                if let Some((k, mods)) = synthesize_ascii_char(ch) {
                    if let Some(macos_code) = crate::input::InputKeyMap::to_macos(k) {
                        events.push(InputEvent {
                            event_type: InputEventType::KeyDown,
                            x: current_pos.0,
                            y: current_pos.1,
                            key_code: macos_code,
                            modifiers: mods,
                            scroll_dx: 0.0,
                            scroll_dy: 0.0,
                        });
                        events.push(InputEvent {
                            event_type: InputEventType::KeyUp,
                            x: current_pos.0,
                            y: current_pos.1,
                            key_code: macos_code,
                            modifiers: mods,
                            scroll_dx: 0.0,
                            scroll_dy: 0.0,
                        });
                    }
                }
            }
        }
        AgentAction::ReleaseAll => {
            events.extend(tracker.release_all(current_pos.0, current_pos.1));
        }
    }

    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_key_names_and_modifiers() {
        let (key, mods) = parse_key_name("Enter").unwrap();
        assert_eq!(key, InputKey::WindowsVirtualKey(0x0D));
        assert_eq!(mods, Modifiers::empty());

        let (key, mods) = parse_key_name("Ctrl").unwrap();
        assert_eq!(key, InputKey::WindowsVirtualKey(0x11));
        assert_eq!(mods, Modifiers::CONTROL);

        let (key, mods) = parse_key_name("a").unwrap();
        assert_eq!(key, InputKey::WindowsVirtualKey(0x41));
        assert_eq!(mods, Modifiers::empty());

        let (key, mods) = parse_key_name("A").unwrap();
        assert_eq!(key, InputKey::WindowsVirtualKey(0x41));
        assert_eq!(mods, Modifiers::SHIFT);
    }

    #[test]
    fn parses_hotkey_combinations() {
        let (key, mods) = parse_hotkey_string("Ctrl+Shift+T").unwrap();
        assert_eq!(key, InputKey::WindowsVirtualKey(0x54));
        assert!(mods.contains(Modifiers::CONTROL));
        assert!(mods.contains(Modifiers::SHIFT));

        let (key, mods) = parse_hotkey_string("Super+Return").unwrap();
        assert_eq!(key, InputKey::WindowsVirtualKey(0x0D));
        assert!(mods.contains(Modifiers::COMMAND));

        assert!(matches!(
            parse_hotkey_string(""),
            Err(AgentInputError::EmptyHotkey)
        ));
        assert!(matches!(
            parse_hotkey_string("InvalidKey123"),
            Err(AgentInputError::UnknownKey(_))
        ));
    }

    #[test]
    fn normalizes_coordinates_with_y_flip() {
        let (x, y) = normalize_agent_coordinates(0.5, 0.25, true, 1920.0, 1080.0).unwrap();
        assert_eq!((x, y), (0.5, 0.75));

        let (x, y) = normalize_agent_coordinates(960.0, 270.0, false, 1920.0, 1080.0).unwrap();
        assert_eq!((x, y), (0.5, 0.75));

        assert!(normalize_agent_coordinates(f32::NAN, 0.0, true, 1920.0, 1080.0).is_err());
    }

    #[test]
    fn synthesizes_ascii_characters() {
        let (key, mods) = synthesize_ascii_char('H').unwrap();
        assert_eq!(key, InputKey::WindowsVirtualKey(0x48));
        assert_eq!(mods, Modifiers::SHIFT);

        let (key, mods) = synthesize_ascii_char('e').unwrap();
        assert_eq!(key, InputKey::WindowsVirtualKey(0x45));
        assert_eq!(mods, Modifiers::empty());

        let (key, mods) = synthesize_ascii_char('@').unwrap();
        assert_eq!(key, InputKey::WindowsVirtualKey(0x32));
        assert_eq!(mods, Modifiers::SHIFT);
    }

    #[test]
    fn tracker_records_and_releases_all() {
        let mut tracker = InputStateTracker::new(Duration::from_millis(50));
        assert!(tracker.is_empty());

        tracker.record_button_down(MouseButton::Left);
        tracker.record_key_down(0x41, Modifiers::CONTROL);
        assert!(!tracker.is_empty());

        let release_events = tracker.release_all(0.5, 0.5);
        assert!(tracker.is_empty());
        assert_eq!(release_events.len(), 3);
        assert!(release_events
            .iter()
            .any(|e| e.event_type == InputEventType::LeftMouseUp));
        assert!(release_events
            .iter()
            .any(|e| e.event_type == InputEventType::KeyUp));
        assert!(release_events
            .iter()
            .any(|e| e.event_type == InputEventType::Reset));
    }

    #[test]
    fn converts_actions_to_wire_input_events() {
        let mut tracker = InputStateTracker::default();
        let mut current_pos = (0.0, 0.0);

        let click = AgentAction::Click {
            x: 100.0,
            y: 200.0,
            button: MouseButton::Left,
            count: 1,
            normalized: false,
        };
        let events =
            convert_agent_action_to_events(&click, &mut tracker, &mut current_pos, 1000.0, 1000.0)
                .unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event_type, InputEventType::MouseMove);
        assert_eq!(events[1].event_type, InputEventType::LeftMouseDown);
        assert_eq!(events[2].event_type, InputEventType::LeftMouseUp);

        let hotkey = AgentAction::Hotkey {
            keys: vec!["Ctrl".into(), "c".into()],
        };
        let events =
            convert_agent_action_to_events(&hotkey, &mut tracker, &mut current_pos, 1000.0, 1000.0)
                .unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, InputEventType::KeyDown);
        assert_eq!(events[1].event_type, InputEventType::KeyUp);
        assert!(events[0].modifiers.contains(Modifiers::CONTROL));

        let type_text = AgentAction::TypeText {
            text: "ls".into(),
            delay_ms: 0,
            paste_mode: false,
        };
        let events = convert_agent_action_to_events(
            &type_text,
            &mut tracker,
            &mut current_pos,
            1000.0,
            1000.0,
        )
        .unwrap();
        assert_eq!(events.len(), 4);
    }

    #[test]
    fn encodes_nv12_screenshot_to_valid_png_and_jpeg() {
        let width = 64u32;
        let height = 64u32;
        let nv12_len = (width * height * 3 / 2) as usize;
        let nv12_buf = vec![128u8; nv12_len];

        let png_base64 =
            encode_nv12_screenshot(width, height, &nv12_buf, ScreenshotFormat::Png).unwrap();
        let png_bytes = base64::prelude::BASE64_STANDARD
            .decode(&png_base64)
            .unwrap();
        assert_eq!(&png_bytes[0..4], &[0x89, 0x50, 0x4E, 0x47]);

        let jpeg_base64 =
            encode_nv12_screenshot(width, height, &nv12_buf, ScreenshotFormat::Jpeg).unwrap();
        let jpeg_bytes = base64::prelude::BASE64_STANDARD
            .decode(&jpeg_base64)
            .unwrap();
        assert_eq!(&jpeg_bytes[0..2], &[0xFF, 0xD8]);
    }
}
