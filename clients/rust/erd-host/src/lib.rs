#[cfg(target_os = "linux")]
pub mod audio_linux;
#[cfg(target_os = "linux")]
pub mod capture_linux;
#[cfg(target_os = "macos")]
pub mod capture_macos;
#[cfg(target_os = "windows")]
pub mod capture_windows;
#[cfg(target_os = "linux")]
pub mod clipboard_linux;
#[cfg(target_os = "windows")]
pub mod clipboard_windows;
#[cfg(target_os = "linux")]
pub mod encode_linux;
#[cfg(target_os = "macos")]
pub mod encode_vt;
#[cfg(target_os = "windows")]
pub mod encode_windows;
#[cfg(target_os = "linux")]
pub mod inject_linux;
#[cfg(target_os = "macos")]
pub mod inject_macos;
#[cfg(target_os = "windows")]
pub mod inject_windows;
pub mod session;
pub mod windows_logic;

#[cfg(target_os = "macos")]
pub use capture_macos::{CaptureConfig, CaptureEvent, CaptureFrame, ScreenCapture};
#[cfg(target_os = "windows")]
pub use capture_windows::WindowsCapture;
#[cfg(target_os = "windows")]
pub use clipboard_windows::WindowsClipboard;
#[cfg(target_os = "macos")]
pub use encode_vt::{EncodedFrame, EncoderConfig, VideoToolboxEncoder};
#[cfg(target_os = "windows")]
pub use encode_windows::MediaFoundationEncoder;
#[cfg(target_os = "macos")]
pub use inject_macos::{accessibility_is_trusted, request_accessibility, InputInjector};
#[cfg(target_os = "windows")]
pub use inject_windows::WindowsInputInjector;
#[cfg(target_os = "linux")]
pub use session::focused_output_name;
pub use session::{
    random_pin, ConsentPrompt, DisplayInfo, HostConfig, HostServer, PairingRecord, PairingStore,
    SessionState, TimestampStats, VideoFrame,
};
