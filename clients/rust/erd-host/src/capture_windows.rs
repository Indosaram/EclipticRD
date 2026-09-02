//! DXGI Desktop Duplication capture for the Windows host.
//!
//! [`WindowsCapture::acquire_next_frame`] waits for the next desktop update,
//! asks DXGI for dirty/move metadata, copies the GPU texture through a staging
//! texture, and returns a tightly packed top-down BGRA buffer. DXGI access-loss
//! recovery (display mode changes, lock/unlock, driver reset) is handled by the
//! underlying manager on the next call.
//!
//! CI proves this module builds. Runtime QA still requires a real Windows 10/11
//! interactive desktop with a Desktop Duplication-capable graphics driver.

use std::time::Duration;

use dxgi_capture_rs::{CaptureError as DxgiCaptureError, DXGIManager};
use thiserror::Error;

/// Pixel-space rectangle in top-left-origin desktop coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirtyRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// A DXGI move operation. Moves precede dirty rectangles when applying damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveRect {
    pub source_x: u32,
    pub source_y: u32,
    pub destination: DirtyRect,
}

/// Tightly packed top-down BGRA8 pixels plus Desktop Duplication metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub bgra: Vec<u8>,
    pub dirty_regions: Vec<DirtyRect>,
    pub move_regions: Vec<MoveRect>,
    pub pointer_position: Option<(i32, i32)>,
    pub pointer_visible: bool,
    pub accumulated_frames: u32,
    pub presentation_counter: i64,
}

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("DXGI Desktop Duplication initialization failed: {0}")]
    Initialization(String),
    #[error("DXGI capture timed out")]
    Timeout,
    #[error("DXGI desktop access was denied, usually because protected content is visible")]
    AccessDenied,
    #[error("DXGI desktop duplication was lost; retry to rebuild it")]
    AccessLost,
    #[error("DXGI desktop duplication could not be refreshed")]
    RefreshFailure,
    #[error("DXGI frame capture failed: {0}")]
    Capture(String),
    #[error("captured frame dimensions or buffer length overflowed")]
    InvalidFrame,
}

/// Blocking capture source for one Windows display.
pub struct WindowsCapture {
    manager: DXGIManager,
    display_index: usize,
}

impl WindowsCapture {
    pub fn new(display_index: usize, timeout: Duration) -> Result<Self, CaptureError> {
        let mut manager = DXGIManager::new(duration_ms(timeout))
            .map_err(|error| CaptureError::Initialization(error.to_string()))?;
        manager.set_capture_source_index(display_index);
        Ok(Self {
            manager,
            display_index,
        })
    }

    pub fn display_index(&self) -> usize {
        self.display_index
    }

    pub fn geometry(&self) -> (u32, u32) {
        let (width, height) = self.manager.geometry();
        (saturating_u32(width), saturating_u32(height))
    }

    pub fn set_timeout(&mut self, timeout: Duration) {
        self.manager.set_timeout_ms(duration_ms(timeout));
    }

    pub fn select_display(&mut self, display_index: usize) {
        self.manager.set_capture_source_index(display_index);
        self.display_index = display_index;
    }

    /// Acquire one update. `CaptureError::Timeout` means no desktop update was
    /// available before the requested deadline and is not a fatal condition.
    pub fn acquire_next_frame(&mut self, timeout: Duration) -> Result<CapturedFrame, CaptureError> {
        self.set_timeout(timeout);
        let (bgra, (width, height), metadata) = self
            .manager
            .capture_frame_components_with_metadata()
            .map_err(map_capture_error)?;

        let expected = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(CaptureError::InvalidFrame)?;
        if bgra.len() != expected {
            return Err(CaptureError::InvalidFrame);
        }

        let width = u32::try_from(width).map_err(|_| CaptureError::InvalidFrame)?;
        let height = u32::try_from(height).map_err(|_| CaptureError::InvalidFrame)?;
        let dirty_regions = metadata
            .dirty_rects
            .into_iter()
            .filter_map(|(left, top, right, bottom)| rect_from_edges(left, top, right, bottom))
            .collect();
        let move_regions = metadata
            .move_rects
            .into_iter()
            .filter_map(|region| {
                let destination = rect_from_edges(
                    region.destination_rect.0,
                    region.destination_rect.1,
                    region.destination_rect.2,
                    region.destination_rect.3,
                )?;
                Some(MoveRect {
                    source_x: u32::try_from(region.source_point.0).ok()?,
                    source_y: u32::try_from(region.source_point.1).ok()?,
                    destination,
                })
            })
            .collect();

        Ok(CapturedFrame {
            width,
            height,
            stride: width.checked_mul(4).ok_or(CaptureError::InvalidFrame)?,
            bgra,
            dirty_regions,
            move_regions,
            pointer_position: metadata.pointer_position,
            pointer_visible: metadata.pointer_visible,
            accumulated_frames: metadata.accumulated_frames,
            presentation_counter: metadata.last_present_time,
        })
    }
}

fn duration_ms(timeout: Duration) -> u32 {
    u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX)
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn rect_from_edges(left: i32, top: i32, right: i32, bottom: i32) -> Option<DirtyRect> {
    if left < 0 || top < 0 || right <= left || bottom <= top {
        return None;
    }
    Some(DirtyRect {
        x: left as u32,
        y: top as u32,
        width: (right - left) as u32,
        height: (bottom - top) as u32,
    })
}

fn map_capture_error(error: DxgiCaptureError) -> CaptureError {
    match error {
        DxgiCaptureError::Timeout => CaptureError::Timeout,
        DxgiCaptureError::AccessDenied => CaptureError::AccessDenied,
        DxgiCaptureError::AccessLost => CaptureError::AccessLost,
        DxgiCaptureError::RefreshFailure => CaptureError::RefreshFailure,
        DxgiCaptureError::Fail(error) => CaptureError::Capture(error.to_string()),
    }
}
