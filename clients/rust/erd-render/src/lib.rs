//! Ecliptic Remote Desktop presentation primitives.

mod audio;

pub use audio::*;

/// Cursor overlay state consumed by the platform renderer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CursorOverlay {
    pub x: f32,
    pub y: f32,
    pub cursor_type: u8,
}

impl Default for CursorOverlay {
    fn default() -> Self {
        Self {
            x: -1.0,
            y: -1.0,
            cursor_type: 0,
        }
    }
}

impl CursorOverlay {
    pub fn update(&mut self, x: f32, y: f32, cursor_type: u8) {
        self.x = x.clamp(0.0, 1.0);
        self.y = y.clamp(0.0, 1.0);
        self.cursor_type = cursor_type;
    }
}
