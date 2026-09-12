//! Linux text clipboard synchronization.
//!
//! Wayland uses `wl-copy`/`wl-paste` from `wl-clipboard`; X11/XWayland falls
//! back to `xclip`. The Wayland path inspects advertised MIME types before
//! reading the text and suppresses the macOS concealed/transient marker types.
//! X11 has no portable equivalent for `org.nspasteboard.ConcealedType`, so the
//! fallback cannot export an entry-level concealed marker; password managers
//! should disable clipboard ownership or expose only non-sensitive selections.
//!
//! A CLI clipboard has no `NSPasteboard.changeCount`. The equivalent state is
//! the content hash plus a short remote-write suppression period. This prevents
//! a remote write from echoing while still allowing a later, genuinely changed
//! local value to be sent.

use std::{
    env,
    ffi::OsString,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use thiserror::Error;

pub const MAX_CLIPBOARD_BYTES: usize = 4 * 1024;
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(500);
pub const DEFAULT_ECHO_PERIOD: Duration = Duration::from_secs(2);
const MAX_TYPE_LIST_BYTES: usize = 64 * 1024;
const CONCEALED_TYPES: [&str; 2] = [
    "org.nspasteboard.ConcealedType",
    "org.nspasteboard.TransientType",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardBackend {
    Wayland,
    X11,
}

impl ClipboardBackend {
    pub fn detect() -> Result<Self, ClipboardError> {
        if env::var_os("WAYLAND_DISPLAY").is_some()
            && executable_in_path("wl-copy").is_some()
            && executable_in_path("wl-paste").is_some()
        {
            return Ok(Self::Wayland);
        }
        if env::var_os("DISPLAY").is_some() && executable_in_path("xclip").is_some() {
            return Ok(Self::X11);
        }
        Err(ClipboardError::Unavailable)
    }
}

#[derive(Debug, Error)]
pub enum ClipboardError {
    #[error("install wl-clipboard for Wayland or xclip for X11 clipboard support")]
    Unavailable,
    #[error("clipboard command failed: {0}")]
    Command(String),
    #[error("clipboard text is not valid UTF-8")]
    InvalidUtf8,
    #[error("clipboard text exceeds the 4 KiB protocol limit")]
    TooLarge,
    #[error("clipboard I/O failed: {0}")]
    Io(#[from] io::Error),
}

/// Stable FNV-1a content hash used by the polling state machine.
pub fn content_hash(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Pure echo/dedup state, separated from process I/O for deterministic tests.
#[derive(Debug, Clone)]
pub struct ClipboardEchoSuppressor {
    echo_period: Duration,
    last_observed_hash: Option<u64>,
    remote_write: Option<(u64, Instant)>,
}

impl ClipboardEchoSuppressor {
    pub fn new(echo_period: Duration) -> Self {
        Self {
            echo_period,
            last_observed_hash: None,
            remote_write: None,
        }
    }

    pub fn record_remote_write(&mut self, text: &str, now: Instant) {
        let hash = content_hash(text);
        self.last_observed_hash = Some(hash);
        self.remote_write = Some((hash, now));
    }

    /// Returns true only for a new local value that should cross the wire.
    pub fn observe_local(&mut self, text: &str, now: Instant) -> bool {
        let hash = content_hash(text);
        if let Some((remote_hash, written_at)) = self.remote_write {
            if remote_hash == hash && now.saturating_duration_since(written_at) <= self.echo_period
            {
                self.last_observed_hash = Some(hash);
                return false;
            }
            if now.saturating_duration_since(written_at) > self.echo_period {
                self.remote_write = None;
            }
        }
        if self.last_observed_hash == Some(hash) {
            return false;
        }
        self.last_observed_hash = Some(hash);
        true
    }
}

pub struct LinuxClipboard {
    backend: ClipboardBackend,
    suppressor: ClipboardEchoSuppressor,
}

impl LinuxClipboard {
    pub fn new() -> Result<Self, ClipboardError> {
        Self::with_backend(ClipboardBackend::detect())
    }

    pub fn with_backend(
        backend: Result<ClipboardBackend, ClipboardError>,
    ) -> Result<Self, ClipboardError> {
        Ok(Self {
            backend: backend?,
            suppressor: ClipboardEchoSuppressor::new(DEFAULT_ECHO_PERIOD),
        })
    }

    pub fn backend(&self) -> ClipboardBackend {
        self.backend
    }

    /// Poll once. The host session should call this every
    /// [`DEFAULT_POLL_INTERVAL`].
    pub fn poll(&mut self, now: Instant) -> Result<Option<String>, ClipboardError> {
        if self.backend == ClipboardBackend::Wayland && self.wayland_selection_is_concealed()? {
            return Ok(None);
        }

        let bytes = match self.backend {
            ClipboardBackend::Wayland => read_command_bounded(
                Command::new("wl-paste").args([
                    "--no-newline",
                    "--type",
                    "text/plain;charset=utf-8",
                ]),
                MAX_CLIPBOARD_BYTES,
            )
            .or_else(|error| {
                // Some producers advertise only `text/plain`.
                if matches!(error, ClipboardError::Command(_)) {
                    read_command_bounded(
                        Command::new("wl-paste").args(["--no-newline", "--type", "text/plain"]),
                        MAX_CLIPBOARD_BYTES,
                    )
                } else {
                    Err(error)
                }
            })?,
            ClipboardBackend::X11 => read_command_bounded(
                Command::new("xclip").args(["-selection", "clipboard", "-out"]),
                MAX_CLIPBOARD_BYTES,
            )?,
        };

        if bytes.is_empty() {
            return Ok(None);
        }
        let text = String::from_utf8(bytes).map_err(|_| ClipboardError::InvalidUtf8)?;
        Ok(self.suppressor.observe_local(&text, now).then_some(text))
    }

    pub fn apply_remote_text(&mut self, text: &str, now: Instant) -> Result<(), ClipboardError> {
        if text.len() > MAX_CLIPBOARD_BYTES {
            return Err(ClipboardError::TooLarge);
        }
        let mut command = match self.backend {
            ClipboardBackend::Wayland => {
                let mut command = Command::new("wl-copy");
                command.args(["--type", "text/plain;charset=utf-8"]);
                command
            }
            ClipboardBackend::X11 => {
                let mut command = Command::new("xclip");
                command.args(["-selection", "clipboard", "-in"]);
                command
            }
        };
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        child
            .stdin
            .take()
            .ok_or_else(|| ClipboardError::Command("clipboard stdin was unavailable".into()))?
            .write_all(text.as_bytes())?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Err(ClipboardError::Command(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        self.suppressor.record_remote_write(text, now);
        Ok(())
    }

    fn wayland_selection_is_concealed(&self) -> Result<bool, ClipboardError> {
        let types = read_command_bounded(
            Command::new("wl-paste").arg("--list-types"),
            MAX_TYPE_LIST_BYTES,
        )?;
        let types = String::from_utf8(types).map_err(|_| ClipboardError::InvalidUtf8)?;
        Ok(types
            .lines()
            .any(|mime| CONCEALED_TYPES.contains(&mime.trim())))
    }
}

fn read_command_bounded(command: &mut Command, limit: usize) -> Result<Vec<u8>, ClipboardError> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let mut bytes = Vec::with_capacity(limit.min(1024));
    child
        .stdout
        .take()
        .ok_or_else(|| ClipboardError::Command("clipboard stdout was unavailable".into()))?
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)?;
    let output = child.wait_with_output()?;
    if bytes.len() > limit {
        return Err(ClipboardError::TooLarge);
    }
    if !output.status.success() {
        return Err(ClipboardError::Command(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(bytes)
}

fn executable_in_path(name: &str) -> Option<PathBuf> {
    let path: OsString = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_equal_content_equally() {
        assert_eq!(content_hash("hello"), content_hash("hello"));
        assert_ne!(content_hash("hello"), content_hash("hello!"));
    }

    #[test]
    fn suppresses_remote_echo_and_unchanged_local_content() {
        let start = Instant::now();
        let mut state = ClipboardEchoSuppressor::new(Duration::from_secs(2));
        state.record_remote_write("remote", start);
        assert!(!state.observe_local("remote", start + Duration::from_millis(500)));
        assert!(!state.observe_local("remote", start + Duration::from_secs(3)));
        assert!(state.observe_local("local", start + Duration::from_secs(3)));
        assert!(!state.observe_local("local", start + Duration::from_secs(4)));
    }

    #[test]
    fn four_kibibyte_limit_is_utf8_bytes() {
        assert_eq!("é".repeat(2048).len(), MAX_CLIPBOARD_BYTES);
        assert!("é".repeat(2049).len() > MAX_CLIPBOARD_BYTES);
    }
}
