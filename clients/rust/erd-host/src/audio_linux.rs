//! PipeWire/PulseAudio system-monitor capture for Linux.
//!
//! `pw-record` is preferred on PipeWire. `parec` is the compatibility fallback
//! and works with PipeWire's PulseAudio server as well as native PulseAudio.
//! Both commands are configured for 48 kHz, stereo, interleaved native-endian
//! `f32`, matching the v3 media contract exactly.
//!
//! A specific PipeWire monitor node can be supplied through
//! `ERD_AUDIO_MONITOR`. Otherwise PipeWire follows its configured default
//! target; for PulseAudio, the backend resolves the default sink and appends
//! `.monitor`. On Arch/Omarchy install `pipewire-audio` (and normally
//! `pipewire-pulse`). Runtime QA must confirm that the selected node is the
//! system-output monitor rather than a microphone.

use std::{
    env,
    io::{self, Read},
    process::{Child, ChildStdout, Command, Stdio},
};

use erd_proto::{AudioFragment, AudioFragmentHeader, MAX_AUDIO_FRAGMENT_BYTES};
use thiserror::Error;

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u16 = 2;
pub const BYTES_PER_SAMPLE: usize = size_of::<f32>();
pub const BYTES_PER_SAMPLE_FRAME: usize = CHANNELS as usize * BYTES_PER_SAMPLE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioBackend {
    PipeWire,
    PulseAudio,
}

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("install PipeWire (`pw-record`) or PulseAudio (`parec`) capture tools")]
    Unavailable,
    #[error("audio capture command failed: {0}")]
    Command(String),
    #[error("audio I/O failed: {0}")]
    Io(#[from] io::Error),
}

pub struct LinuxAudioCapture {
    backend: AudioBackend,
    child: Child,
    stdout: ChildStdout,
    pending: Vec<u8>,
}

impl LinuxAudioCapture {
    pub fn start() -> Result<Self, AudioError> {
        if command_available("pw-record") {
            let mut command = Command::new("pw-record");
            command.args([
                "--raw",
                "--rate",
                "48000",
                "--channels",
                "2",
                "--format",
                "f32",
            ]);
            if let Some(target) = env::var_os("ERD_AUDIO_MONITOR") {
                command.arg("--target").arg(target);
            }
            return Self::spawn(AudioBackend::PipeWire, command);
        }

        if command_available("parec") {
            let source = env::var("ERD_AUDIO_MONITOR")
                .ok()
                .or_else(default_pulse_monitor_source);
            let mut command = Command::new("parec");
            command.args([
                "--raw",
                "--format=float32ne",
                "--rate=48000",
                "--channels=2",
            ]);
            if let Some(source) = source {
                command.arg(format!("--device={source}"));
            }
            return Self::spawn(AudioBackend::PulseAudio, command);
        }

        Err(AudioError::Unavailable)
    }

    fn spawn(backend: AudioBackend, mut command: Command) -> Result<Self, AudioError> {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AudioError::Command("capture stdout was unavailable".into()))?;
        Ok(Self {
            backend,
            child,
            stdout,
            pending: Vec::with_capacity(BYTES_PER_SAMPLE_FRAME * 2),
        })
    }

    pub fn backend(&self) -> AudioBackend {
        self.backend
    }

    /// Reads complete stereo sample frames into `output`. Short pipe reads are
    /// retained so a partial f32/stereo frame never shifts channel alignment.
    pub fn read_interleaved_f32(&mut self, output: &mut [f32]) -> Result<usize, AudioError> {
        let sample_capacity = output.len() - (output.len() % CHANNELS as usize);
        if sample_capacity == 0 {
            return Ok(0);
        }
        let byte_capacity = sample_capacity * BYTES_PER_SAMPLE;
        while self.pending.len() < BYTES_PER_SAMPLE_FRAME {
            let mut chunk = vec![0_u8; byte_capacity.max(BYTES_PER_SAMPLE_FRAME)];
            let read = self.stdout.read(&mut chunk)?;
            if read == 0 {
                return Ok(0);
            }
            self.pending.extend_from_slice(&chunk[..read]);
        }

        let complete_bytes = self.pending.len().min(byte_capacity);
        let complete_bytes = complete_bytes - (complete_bytes % BYTES_PER_SAMPLE_FRAME);
        for (sample, bytes) in output[..complete_bytes / BYTES_PER_SAMPLE]
            .iter_mut()
            .zip(self.pending[..complete_bytes].chunks_exact(BYTES_PER_SAMPLE))
        {
            *sample = f32::from_ne_bytes(bytes.try_into().expect("one native f32"));
        }
        self.pending.drain(..complete_bytes);
        Ok(complete_bytes / BYTES_PER_SAMPLE)
    }
}

impl Drop for LinuxAudioCapture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Splits one PCM block into the exact v3 UDP fragmentation contract.
pub fn fragment_audio(frame_id: u32, pcm: &[u8]) -> Vec<AudioFragment> {
    if pcm.is_empty() {
        return Vec::new();
    }
    let count = pcm.len().div_ceil(MAX_AUDIO_FRAGMENT_BYTES);
    if count > u16::MAX as usize {
        return Vec::new();
    }
    pcm.chunks(MAX_AUDIO_FRAGMENT_BYTES)
        .enumerate()
        .map(|(index, data)| AudioFragment {
            header: AudioFragmentHeader {
                frame_id,
                fragment_index: index as u16,
                fragment_count: count as u16,
            },
            data: data.to_vec(),
        })
        .collect()
}

fn command_available(name: &str) -> bool {
    env::var_os("PATH")
        .into_iter()
        .flat_map(|path| env::split_paths(&path).collect::<Vec<_>>())
        .map(|directory| directory.join(name))
        .any(|candidate| candidate.is_file())
}

fn default_pulse_monitor_source() -> Option<String> {
    let output = Command::new("pactl")
        .args(["get-default-sink"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| format!("{}.monitor", String::from_utf8_lossy(&output.stdout).trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragments_audio_at_wire_limit() {
        let pcm = vec![7_u8; MAX_AUDIO_FRAGMENT_BYTES * 2 + 3];
        let fragments = fragment_audio(42, &pcm);
        assert_eq!(fragments.len(), 3);
        assert_eq!(fragments[0].header.frame_id, 42);
        assert_eq!(fragments[0].header.fragment_index, 0);
        assert_eq!(fragments[2].header.fragment_index, 2);
        assert_eq!(fragments[2].header.fragment_count, 3);
        assert_eq!(fragments[2].data, vec![7; 3]);
        assert_eq!(
            fragments
                .into_iter()
                .flat_map(|fragment| fragment.data)
                .collect::<Vec<_>>(),
            pcm
        );
    }

    #[test]
    fn empty_audio_does_not_create_invalid_zero_count_fragment() {
        assert!(fragment_audio(1, &[]).is_empty());
    }
}
