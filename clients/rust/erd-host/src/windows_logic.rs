//! Windows host pure logic kept target-independent for unit tests.

use erd_proto::Modifiers;

/// Wire audio format produced by every host: interleaved f32 LE stereo at 48 kHz,
/// matching the client's cpal output contract in `erd-render::audio`.
pub const WIRE_AUDIO_CHANNELS: usize = 2;
pub const WIRE_AUDIO_SAMPLE_RATE: u32 = 48_000;

/// Sample layout of a WASAPI loopback mix format relevant to wire conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceAudioFormat {
    pub channels: usize,
    pub sample_rate: u32,
    /// `false` for 16-bit PCM, `true` for IEEE f32 (shared-mode mix format norm).
    pub is_float: bool,
}

impl Default for SourceAudioFormat {
    fn default() -> Self {
        Self {
            channels: WIRE_AUDIO_CHANNELS,
            sample_rate: WIRE_AUDIO_SAMPLE_RATE,
            is_float: true,
        }
    }
}

/// Converts one WASAPI loopback packet to the wire format: interleaved f32 LE
/// stereo at 48 kHz. Downmixes multi-channel by keeping the first two channels,
/// duplicates mono, converts 16-bit PCM, and linearly resamples other rates.
pub fn convert_to_wire_audio(pcm: &[u8], source: &SourceAudioFormat) -> Vec<u8> {
    if source.channels == 0 {
        return Vec::new();
    }
    let bytes_per_sample = if source.is_float { 4 } else { 2 };
    let frame_bytes = bytes_per_sample * source.channels;
    let frame_count = pcm.len() / frame_bytes.max(1);
    if frame_count == 0 {
        return Vec::new();
    }

    let mut samples = Vec::with_capacity(frame_count * WIRE_AUDIO_CHANNELS);
    for frame in 0..frame_count {
        let read_sample = |channel: usize| -> f32 {
            let offset = frame * frame_bytes + channel * bytes_per_sample;
            if source.is_float {
                f32::from_le_bytes(pcm[offset..offset + 4].try_into().expect("4 bytes"))
            } else {
                i16::from_le_bytes(pcm[offset..offset + 2].try_into().expect("2 bytes")) as f32
                    / 32_768.0
            }
        };
        if source.channels == 1 {
            let mono = read_sample(0);
            samples.push(mono);
            samples.push(mono);
        } else {
            samples.push(read_sample(0));
            samples.push(read_sample(1));
        }
    }
    // `samples` is now frame-sequential stereo at the source rate.
    let output = if source.sample_rate == WIRE_AUDIO_SAMPLE_RATE {
        samples
    } else {
        let source_frames = frame_count;
        let output_frames = (source_frames as u64 * u64::from(WIRE_AUDIO_SAMPLE_RATE)
            / u64::from(source.sample_rate)) as usize;
        let step = f64::from(source.sample_rate) / f64::from(WIRE_AUDIO_SAMPLE_RATE);
        let mut resampled = Vec::with_capacity(output_frames * WIRE_AUDIO_CHANNELS);
        for frame in 0..output_frames {
            let position = frame as f64 * step;
            let index = position.floor() as usize;
            let next = (index + 1).min(source_frames - 1);
            let frac = (position - index as f64) as f32;
            for channel in 0..WIRE_AUDIO_CHANNELS {
                let left = samples[index * WIRE_AUDIO_CHANNELS + channel];
                let right = samples[next * WIRE_AUDIO_CHANNELS + channel];
                resampled.push(left + (right - left) * frac);
            }
        }
        resampled
    };
    let mut bytes = Vec::with_capacity(output.len() * 4);
    for sample in output {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

const ABSOLUTE_AXIS_MAX: f64 = 65_535.0;

/// Windows virtual desktop bounds in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualDesktop {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Target display bounds within the virtual desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetDisplay {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl TargetDisplay {
    pub fn from_virtual_desktop(desktop: VirtualDesktop) -> Self {
        Self {
            x: desktop.x,
            y: desktop.y,
            width: desktop.width,
            height: desktop.height,
        }
    }
}

/// Maps normalized protocol coordinates into the Windows absolute-input range.
pub fn normalize_absolute_pointer(
    normalized_x: f32,
    normalized_y: f32,
    target: TargetDisplay,
    desktop: VirtualDesktop,
) -> (i32, i32) {
    if target.width == 0 || target.height == 0 || desktop.width <= 1 || desktop.height <= 1 {
        return (0, 0);
    }
    let local_x = finite_unit(normalized_x) * target.width.saturating_sub(1) as f64;
    // Protocol convention inverts Y (1.0 - y) for legacy macOS compatibility.
    // Invert it back so (0,0) is top-left on Windows.
    let local_y = (1.0 - finite_unit(normalized_y)) * target.height.saturating_sub(1) as f64;
    let desktop_x = (target.x as f64 + local_x - desktop.x as f64)
        .clamp(0.0, desktop.width.saturating_sub(1) as f64);
    let desktop_y = (target.y as f64 + local_y - desktop.y as f64)
        .clamp(0.0, desktop.height.saturating_sub(1) as f64);
    (
        (desktop_x * ABSOLUTE_AXIS_MAX / desktop.width.saturating_sub(1) as f64).round() as i32,
        (desktop_y * ABSOLUTE_AXIS_MAX / desktop.height.saturating_sub(1) as f64).round() as i32,
    )
}

fn finite_unit(value: f32) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0) as f64
    } else {
        0.0
    }
}

/// Convert a Windows VK to the protocol's macOS physical-key code.
pub fn vk_to_macos_keycode(vk: u16) -> Option<u16> {
    KEY_MAP
        .iter()
        .find_map(|&(macos, windows)| (windows == vk).then_some(macos))
}

/// Convert a protocol/macOS physical-key code to a Windows VK.
pub fn macos_keycode_to_vk(key_code: u16) -> Option<u16> {
    KEY_MAP
        .iter()
        .find_map(|&(macos, windows)| (macos == key_code).then_some(windows))
}

/// Maps v3 modifier bits to the left-side Win32 VKs used by the injector.
pub fn modifier_vks(modifiers: Modifiers) -> Vec<u16> {
    [
        (Modifiers::SHIFT, 0xA0),
        (Modifiers::CONTROL, 0xA2),
        (Modifiers::OPTION, 0xA4),
        (Modifiers::COMMAND, 0x5B),
        (Modifiers::CAPS_LOCK, 0x14),
    ]
    .into_iter()
    .filter_map(|(modifier, vk)| modifiers.contains(modifier).then_some(vk))
    .collect()
}

/// Returns Win32 MOUSEEVENTF flags for mouse button down/up events.
pub fn mouse_button_flags(event_type: erd_proto::InputEventType) -> u32 {
    const MOUSEEVENTF_LEFTDOWN: u32 = 0x0002;
    const MOUSEEVENTF_LEFTUP: u32 = 0x0004;
    const MOUSEEVENTF_RIGHTDOWN: u32 = 0x0008;
    const MOUSEEVENTF_RIGHTUP: u32 = 0x0010;
    const MOUSEEVENTF_MIDDLEDOWN: u32 = 0x0020;
    const MOUSEEVENTF_MIDDLEUP: u32 = 0x0040;

    match event_type {
        erd_proto::InputEventType::LeftMouseDown => MOUSEEVENTF_LEFTDOWN,
        erd_proto::InputEventType::LeftMouseUp => MOUSEEVENTF_LEFTUP,
        erd_proto::InputEventType::RightMouseDown => MOUSEEVENTF_RIGHTDOWN,
        erd_proto::InputEventType::RightMouseUp => MOUSEEVENTF_RIGHTUP,
        erd_proto::InputEventType::MiddleMouseDown => MOUSEEVENTF_MIDDLEDOWN,
        erd_proto::InputEventType::MiddleMouseUp => MOUSEEVENTF_MIDDLEUP,
        _ => 0,
    }
}

/// Stable FNV-1a clipboard hash used for content-level dedup.
pub fn clipboard_content_hash(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClipboardEchoSuppressor {
    last_sequence: Option<u32>,
    last_written_sequence: Option<u32>,
    last_hash: Option<u64>,
}

impl ClipboardEchoSuppressor {
    pub fn start(&mut self, sequence: u32) {
        self.last_sequence = Some(sequence);
        self.last_written_sequence = None;
        self.last_hash = None;
    }

    pub fn record_remote_write(&mut self, sequence: u32, text: &str) {
        self.last_sequence = Some(sequence);
        self.last_written_sequence = Some(sequence);
        self.last_hash = Some(clipboard_content_hash(text));
    }

    pub fn observe_sequence(&mut self, sequence: u32) -> bool {
        if self.last_sequence == Some(sequence) {
            return false;
        }
        self.last_sequence = Some(sequence);
        self.last_written_sequence != Some(sequence)
    }

    pub fn observe_text(&mut self, text: &str) -> bool {
        let hash = clipboard_content_hash(text);
        if self.last_hash == Some(hash) {
            return false;
        }
        self.last_hash = Some(hash);
        true
    }
}

/// Apple ANSI physical key codes mirrored to Win32 virtual keys.
const KEY_MAP: &[(u16, u16)] = &[
    (0, 0x41),
    (1, 0x53),
    (2, 0x44),
    (3, 0x46),
    (4, 0x48),
    (5, 0x47),
    (6, 0x5A),
    (7, 0x58),
    (8, 0x43),
    (9, 0x56),
    (11, 0x42),
    (12, 0x51),
    (13, 0x57),
    (14, 0x45),
    (15, 0x52),
    (16, 0x59),
    (17, 0x54),
    (18, 0x31),
    (19, 0x32),
    (20, 0x33),
    (21, 0x34),
    (22, 0x36),
    (23, 0x35),
    (24, 0xBB),
    (25, 0x39),
    (26, 0x37),
    (27, 0xBD),
    (28, 0x38),
    (29, 0x30),
    (30, 0xDD),
    (31, 0x4F),
    (32, 0x55),
    (33, 0xDB),
    (34, 0x49),
    (35, 0x50),
    (36, 0x0D),
    (37, 0x4C),
    (38, 0x4A),
    (39, 0xDE),
    (40, 0x4B),
    (41, 0xBA),
    (42, 0xDC),
    (43, 0xBC),
    (44, 0xBF),
    (45, 0x4E),
    (46, 0x4D),
    (47, 0xBE),
    (48, 0x09),
    (49, 0x20),
    (50, 0xC0),
    (51, 0x08),
    (53, 0x1B),
    (54, 0x5C),
    (55, 0x5B),
    (56, 0xA0),
    (57, 0x14),
    (58, 0xA4),
    (59, 0xA2),
    (60, 0xA1),
    (61, 0xA5),
    (62, 0xA3),
    (65, 0x6E),
    (67, 0x6A),
    (69, 0x6B),
    (71, 0x90),
    (75, 0x6F),
    (76, 0x0D),
    (78, 0x6D),
    (81, 0xBB),
    (82, 0x60),
    (83, 0x61),
    (84, 0x62),
    (85, 0x63),
    (86, 0x64),
    (87, 0x65),
    (88, 0x66),
    (89, 0x67),
    (91, 0x68),
    (92, 0x69),
    (96, 0x74),
    (97, 0x75),
    (98, 0x76),
    (99, 0x72),
    (100, 0x77),
    (101, 0x78),
    (103, 0x7A),
    (105, 0x7C),
    (106, 0x7F),
    (107, 0x7D),
    (109, 0x79),
    (111, 0x7B),
    (113, 0x7E),
    (114, 0x2D),
    (115, 0x24),
    (116, 0x21),
    (117, 0x2E),
    (118, 0x73),
    (119, 0x23),
    (120, 0x71),
    (121, 0x22),
    (122, 0x70),
    (123, 0x25),
    (124, 0x27),
    (125, 0x28),
    (126, 0x26),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_alphabet_digits_navigation_and_modifiers() {
        assert_eq!(macos_keycode_to_vk(0), Some(0x41));
        assert_eq!(macos_keycode_to_vk(18), Some(0x31));
        assert_eq!(macos_keycode_to_vk(36), Some(0x0D));
        assert_eq!(macos_keycode_to_vk(49), Some(0x20));
        assert_eq!(macos_keycode_to_vk(53), Some(0x1B));
        assert_eq!(macos_keycode_to_vk(123), Some(0x25));
        assert_eq!(macos_keycode_to_vk(126), Some(0x26));
        assert_eq!(macos_keycode_to_vk(u16::MAX), None);
        assert_eq!(vk_to_macos_keycode(0x41), Some(0));
        assert_eq!(vk_to_macos_keycode(0x25), Some(123));
        assert_eq!(vk_to_macos_keycode(0xFF), None);

        assert_eq!(
            modifier_vks(
                Modifiers::SHIFT | Modifiers::CONTROL | Modifiers::OPTION | Modifiers::COMMAND
            ),
            vec![0xA0, 0xA2, 0xA4, 0x5B]
        );

        assert_eq!(
            mouse_button_flags(erd_proto::InputEventType::LeftMouseDown),
            0x0002
        );
        assert_eq!(
            mouse_button_flags(erd_proto::InputEventType::LeftMouseUp),
            0x0004
        );
        assert_eq!(
            mouse_button_flags(erd_proto::InputEventType::RightMouseDown),
            0x0008
        );
        assert_eq!(
            mouse_button_flags(erd_proto::InputEventType::RightMouseUp),
            0x0010
        );
        assert_eq!(
            mouse_button_flags(erd_proto::InputEventType::MiddleMouseDown),
            0x0020
        );
        assert_eq!(
            mouse_button_flags(erd_proto::InputEventType::MiddleMouseUp),
            0x0040
        );
        assert_eq!(mouse_button_flags(erd_proto::InputEventType::MouseMove), 0);
    }

    #[test]
    fn normalizes_target_monitor_into_virtual_desktop() {
        let desktop = VirtualDesktop {
            x: -1920,
            y: 0,
            width: 4480,
            height: 1440,
        };
        let target = TargetDisplay {
            x: 0,
            y: 0,
            width: 2560,
            height: 1440,
        };
        // Wire coordinates: (0.0, 1.0) is top-left, (1.0, 0.0) is bottom-right
        let top_left = normalize_absolute_pointer(0.0, 1.0, target, desktop);
        let bottom_right = normalize_absolute_pointer(1.0, 0.0, target, desktop);
        assert!(top_left.0 > 0);
        assert_eq!(top_left.1, 0);
        assert_eq!(bottom_right.1, 65_535);
        assert!(bottom_right.0 > top_left.0);
        assert_eq!(
            normalize_absolute_pointer(f32::NAN, f32::INFINITY, target, desktop),
            normalize_absolute_pointer(0.0, 0.0, target, desktop)
        );
    }

    #[test]
    fn hashes_and_suppresses_clipboard_echo() {
        assert_eq!(
            clipboard_content_hash("hello"),
            clipboard_content_hash("hello")
        );
        assert_ne!(
            clipboard_content_hash("hello"),
            clipboard_content_hash("hello!")
        );

        let mut state = ClipboardEchoSuppressor::default();
        state.start(10);
        assert!(!state.observe_sequence(10));
        assert!(state.observe_sequence(11));
        assert!(state.observe_text("local"));
        assert!(!state.observe_text("local"));

        state.record_remote_write(12, "remote");
        assert!(!state.observe_sequence(12));
        assert!(!state.observe_text("remote"));
        assert!(state.observe_sequence(13));
        assert!(state.observe_text("other"));
    }

    #[test]
    fn clipboard_hash_is_utf8_byte_based() {
        assert_eq!("é".repeat(2048).len(), 4096);
        assert_eq!(clipboard_content_hash("é"), 0x0ac2_1707_b718_1e01);
    }

    fn read_f32_le(pcm: &[u8]) -> Vec<f32> {
        pcm.chunks_exact(4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .collect()
    }

    #[test]
    fn stereo_48k_float_passes_through() {
        let source = SourceAudioFormat::default();
        let pcm: Vec<u8> = [0.25_f32, -0.5].iter().flat_map(|s| s.to_le_bytes()).collect();
        let wire = convert_to_wire_audio(&pcm, &source);
        assert_eq!(read_f32_le(&wire), vec![0.25, -0.5]);
    }

    #[test]
    fn mono_is_duplicated_and_s16_converted() {
        let source = SourceAudioFormat { channels: 1, is_float: false, ..Default::default() };
        let pcm: Vec<u8> = [16384_i16, -16384].iter().flat_map(|s| s.to_le_bytes()).collect();
        let wire = convert_to_wire_audio(&pcm, &source);
        let out = read_f32_le(&wire);
        // Interleaved stereo: [L0, R0, L1, R1] with L == R == mono sample.
        assert_eq!(out.len(), 4);
        assert!((out[0] - 0.5).abs() < 1e-6 && (out[1] - 0.5).abs() < 1e-6);
        assert!((out[2] + 0.5).abs() < 1e-6 && (out[3] + 0.5).abs() < 1e-6);
    }

    #[test]
    fn multichannel_keeps_front_pair_and_partial_frames_are_dropped() {
        let source = SourceAudioFormat { channels: 6, ..Default::default() };
        let frame: Vec<u8> = [0.1_f32, 0.9, 0.0, 0.0, 0.0, 0.0]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let mut pcm = frame.clone();
        pcm.extend_from_slice(&[0xAB]); // trailing partial sample
        let out = read_f32_le(&convert_to_wire_audio(&pcm, &source));
        assert_eq!(out, vec![0.1, 0.9]);
    }

    #[test]
    fn resampling_scales_frame_count_toward_48k() {
        let source = SourceAudioFormat { sample_rate: 24_000, ..Default::default() };
        // Two stereo frames: (0,0) then (1,1).
        let pcm: Vec<u8> = [0.0_f32, 0.0, 1.0, 1.0]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let out = read_f32_le(&convert_to_wire_audio(&pcm, &source));
        assert_eq!(out.len(), 8); // 2 stereo frames @24k -> 4 stereo frames @48k
        assert_eq!(out[0], 0.0); // frame 0 -> source frame 0
        assert_eq!(out[2], 0.5); // frame 1 -> halfway 0..1
        assert!((out[4] - 1.0).abs() < 1e-6); // frame 2 -> source frame 1
    }
}
