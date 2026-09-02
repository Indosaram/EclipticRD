//! FFmpeg video encoding for the Linux host.
//!
//! The preferred encoders are `hevc_vaapi` and `h264_vaapi`. They use libva
//! through FFmpeg's VAAPI hardware-device and hardware-frame contexts. BGRA
//! capture frames are converted to NV12 and uploaded to VAAPI surfaces. If no
//! VAAPI encoder/device can be opened, `libx264` is opened with `ultrafast` +
//! `zerolatency`, no B-frames, and a one-frame-thread policy.
//!
//! Output is always AVCC-style: every NAL unit has a four-byte big-endian
//! length prefix. FFmpeg's Annex-B output is converted when necessary. On
//! keyframes the cached VPS/SPS/PPS (HEVC) or SPS/PPS (H.264) are prepended,
//! matching the macOS VideoToolbox contract.
//!
//! Runtime QA on the deployment host must exercise the actual GPU. For Arch,
//! install `ffmpeg`, `libva`, and the vendor VA driver (`libva-mesa-driver` or
//! `intel-media-driver`), then confirm `vainfo` and `ffmpeg -encoders` expose
//! the selected codec.

use std::{ffi::CString, ptr};

use ffmpeg_next as ffmpeg;
use thiserror::Error;

use ffmpeg::{
    codec,
    format::Pixel,
    frame,
    software::scaling::{context::Context as ScaleContext, flag::Flags as ScaleFlags},
    Dictionary, Packet,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    Hevc,
    H264,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderBackend {
    Vaapi,
    X264,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderConfig {
    pub width: u32,
    pub height: u32,
    pub bitrate: usize,
    pub fps: u32,
    pub keyframe_interval: u32,
    pub preferred_codec: VideoCodec,
}

impl EncoderConfig {
    pub fn validate(self) -> Result<Self, EncodeError> {
        if self.width == 0 || self.height == 0 || self.fps == 0 || self.bitrate == 0 {
            return Err(EncodeError::InvalidConfig);
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub is_key_frame: bool,
    pub codec: VideoCodec,
    pub pts: i64,
}

#[derive(Debug, Error)]
pub enum EncodeError {
    #[error("encoder dimensions, FPS, and bitrate must be non-zero")]
    InvalidConfig,
    #[error("BGRA frame length/stride does not match the configured dimensions")]
    InvalidFrame,
    #[error("no VAAPI encoder or libx264 fallback is available")]
    EncoderUnavailable,
    #[error("FFmpeg error: {0}")]
    Ffmpeg(#[from] ffmpeg::Error),
    #[error("encoded packet is neither valid Annex-B nor AVCC")]
    InvalidBitstream,
}

struct VaapiResources {
    device: *mut ffmpeg::ffi::AVBufferRef,
    frames: *mut ffmpeg::ffi::AVBufferRef,
}

impl Drop for VaapiResources {
    fn drop(&mut self) {
        unsafe {
            ffmpeg::ffi::av_buffer_unref(&mut self.frames);
            ffmpeg::ffi::av_buffer_unref(&mut self.device);
        }
    }
}

struct OpenEncoder {
    encoder: ffmpeg::codec::encoder::video::Encoder,
    backend: EncoderBackend,
    codec: VideoCodec,
    scaler: ScaleContext,
    vaapi: Option<VaapiResources>,
}

pub struct LinuxVideoEncoder {
    config: EncoderConfig,
    open: OpenEncoder,
    next_pts: i64,
    force_keyframe: bool,
    parameter_sets: Vec<Vec<u8>>,
}

impl LinuxVideoEncoder {
    pub fn new(config: EncoderConfig) -> Result<Self, EncodeError> {
        let config = config.validate()?;
        ffmpeg::init()?;

        let attempts = match config.preferred_codec {
            VideoCodec::Hevc => [VideoCodec::Hevc, VideoCodec::H264],
            VideoCodec::H264 => [VideoCodec::H264, VideoCodec::Hevc],
        };
        let mut last_error = None;
        for codec in attempts {
            match open_vaapi(config, codec) {
                Ok(open) => {
                    return Ok(Self {
                        config,
                        open,
                        next_pts: 0,
                        force_keyframe: true,
                        parameter_sets: Vec::new(),
                    });
                }
                Err(error) => last_error = Some(error),
            }
        }
        match open_x264(config) {
            Ok(open) => Ok(Self {
                config,
                open,
                next_pts: 0,
                force_keyframe: true,
                parameter_sets: Vec::new(),
            }),
            Err(_) => Err(last_error.unwrap_or(EncodeError::EncoderUnavailable)),
        }
    }

    pub fn backend(&self) -> EncoderBackend {
        self.open.backend
    }

    pub fn codec(&self) -> VideoCodec {
        self.open.codec
    }

    pub fn force_key_frame(&mut self) {
        self.force_keyframe = true;
    }

    /// Encodes one tightly packed or padded BGRA frame.
    pub fn encode_bgra(
        &mut self,
        bgra: &[u8],
        stride: usize,
    ) -> Result<Vec<EncodedFrame>, EncodeError> {
        let row_bytes = self.config.width as usize * 4;
        let required = stride
            .checked_mul(self.config.height as usize)
            .ok_or(EncodeError::InvalidFrame)?;
        if stride < row_bytes || bgra.len() < required {
            return Err(EncodeError::InvalidFrame);
        }

        let mut source = frame::Video::new(Pixel::BGRA, self.config.width, self.config.height);
        let source_stride = source.stride(0);
        for row in 0..self.config.height as usize {
            let input_start = row * stride;
            let output_start = row * source_stride;
            source.data_mut(0)[output_start..output_start + row_bytes]
                .copy_from_slice(&bgra[input_start..input_start + row_bytes]);
        }

        let pts = self.next_pts;
        self.next_pts += 1;
        let mut software = frame::Video::empty();
        self.open.scaler.run(&source, &mut software)?;
        software.set_pts(Some(pts));
        if self.force_keyframe {
            software.set_kind(ffmpeg::picture::Type::I);
            self.force_keyframe = false;
        } else {
            software.set_kind(ffmpeg::picture::Type::None);
        }

        if let Some(vaapi) = &self.open.vaapi {
            let mut hardware = frame::Video::empty();
            unsafe {
                let status =
                    ffmpeg::ffi::av_hwframe_get_buffer(vaapi.frames, hardware.as_mut_ptr(), 0);
                if status < 0 {
                    return Err(EncodeError::Ffmpeg(ffmpeg::Error::from(status)));
                }
                let status = ffmpeg::ffi::av_hwframe_transfer_data(
                    hardware.as_mut_ptr(),
                    software.as_ptr(),
                    0,
                );
                if status < 0 {
                    return Err(EncodeError::Ffmpeg(ffmpeg::Error::from(status)));
                }
            }
            hardware.set_pts(Some(pts));
            hardware.set_kind(software.kind());
            self.open.encoder.send_frame(&hardware)?;
        } else {
            self.open.encoder.send_frame(&software)?;
        }

        self.receive_packets()
    }

    pub fn drain(&mut self) -> Result<Vec<EncodedFrame>, EncodeError> {
        self.open.encoder.send_eof()?;
        self.receive_packets()
    }

    fn receive_packets(&mut self) -> Result<Vec<EncodedFrame>, EncodeError> {
        let mut output = Vec::new();
        loop {
            let mut packet = Packet::empty();
            match self.open.encoder.receive_packet(&mut packet) {
                Ok(()) => {
                    let bytes = packet.data().ok_or(EncodeError::InvalidBitstream)?;
                    let nal_units = parse_nal_units(bytes)?;
                    let is_key = packet.is_key();
                    if is_key {
                        let discovered = extract_parameter_sets(self.open.codec, &nal_units);
                        if !discovered.is_empty() {
                            self.parameter_sets = discovered;
                        }
                    }
                    let mut avcc = Vec::new();
                    if is_key {
                        append_unique_parameter_sets(&mut avcc, &self.parameter_sets, &nal_units)?;
                    }
                    for nalu in &nal_units {
                        append_avcc_nalu(&mut avcc, nalu)?;
                    }
                    output.push(EncodedFrame {
                        data: avcc,
                        is_key_frame: is_key,
                        codec: self.open.codec,
                        pts: packet.pts().unwrap_or_default(),
                    });
                }
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::ffi::EAGAIN => break,
                Err(ffmpeg::Error::Eof) => break,
                Err(error) => return Err(EncodeError::Ffmpeg(error)),
            }
        }
        Ok(output)
    }
}

fn base_video_context(
    config: EncoderConfig,
    codec: ffmpeg::Codec,
    pixel_format: Pixel,
) -> Result<ffmpeg::codec::encoder::video::Video, EncodeError> {
    let mut encoder = codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()?;
    encoder.set_width(config.width);
    encoder.set_height(config.height);
    encoder.set_format(pixel_format);
    encoder.set_time_base((1, config.fps as i32));
    encoder.set_frame_rate(Some((config.fps as i32, 1)));
    encoder.set_bit_rate(config.bitrate);
    encoder.set_max_bit_rate(config.bitrate);
    encoder.set_gop(config.keyframe_interval.max(1));
    encoder.set_max_b_frames(0);
    encoder.set_threading(codec::threading::Config::count(1));
    Ok(encoder)
}

fn open_vaapi(config: EncoderConfig, video_codec: VideoCodec) -> Result<OpenEncoder, EncodeError> {
    let name = match video_codec {
        VideoCodec::Hevc => "hevc_vaapi",
        VideoCodec::H264 => "h264_vaapi",
    };
    let codec = codec::encoder::find_by_name(name).ok_or(EncodeError::EncoderUnavailable)?;
    let mut encoder = base_video_context(config, codec, Pixel::VAAPI)?;

    let mut device = ptr::null_mut();
    let device_path = std::env::var("ERD_VAAPI_DEVICE")
        .ok()
        .map(CString::new)
        .transpose()
        .map_err(|_| EncodeError::InvalidConfig)?;
    let status = unsafe {
        ffmpeg::ffi::av_hwdevice_ctx_create(
            &mut device,
            ffmpeg::ffi::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
            device_path
                .as_ref()
                .map_or(ptr::null(), |path| path.as_ptr()),
            ptr::null_mut(),
            0,
        )
    };
    if status < 0 {
        return Err(EncodeError::Ffmpeg(ffmpeg::Error::from(status)));
    }

    let frames = unsafe { ffmpeg::ffi::av_hwframe_ctx_alloc(device) };
    if frames.is_null() {
        unsafe { ffmpeg::ffi::av_buffer_unref(&mut device) };
        return Err(EncodeError::EncoderUnavailable);
    }
    unsafe {
        let frames_context = (*frames).data.cast::<ffmpeg::ffi::AVHWFramesContext>();
        (*frames_context).format = Pixel::VAAPI.into();
        (*frames_context).sw_format = Pixel::NV12.into();
        (*frames_context).width = config.width as i32;
        (*frames_context).height = config.height as i32;
        (*frames_context).initial_pool_size = 4;
        let status = ffmpeg::ffi::av_hwframe_ctx_init(frames);
        if status < 0 {
            let mut frames = frames;
            ffmpeg::ffi::av_buffer_unref(&mut frames);
            ffmpeg::ffi::av_buffer_unref(&mut device);
            return Err(EncodeError::Ffmpeg(ffmpeg::Error::from(status)));
        }
        (*encoder.as_mut_ptr()).hw_frames_ctx = ffmpeg::ffi::av_buffer_ref(frames);
    }

    let mut options = Dictionary::new();
    options.set("rc_mode", "CBR");
    options.set("bf", "0");
    let encoder = match encoder.open_as_with(codec, options) {
        Ok(encoder) => encoder,
        Err(error) => {
            let resources = VaapiResources { device, frames };
            drop(resources);
            return Err(EncodeError::Ffmpeg(error));
        }
    };
    let scaler = ScaleContext::get(
        Pixel::BGRA,
        config.width,
        config.height,
        Pixel::NV12,
        config.width,
        config.height,
        ScaleFlags::FAST_BILINEAR,
    )?;
    Ok(OpenEncoder {
        encoder,
        backend: EncoderBackend::Vaapi,
        codec: video_codec,
        scaler,
        vaapi: Some(VaapiResources { device, frames }),
    })
}

fn open_x264(config: EncoderConfig) -> Result<OpenEncoder, EncodeError> {
    let codec = codec::encoder::find_by_name("libx264").ok_or(EncodeError::EncoderUnavailable)?;
    let encoder = base_video_context(config, codec, Pixel::YUV420P)?;
    let mut options = Dictionary::new();
    options.set("preset", "ultrafast");
    options.set("tune", "zerolatency");
    options.set("bf", "0");
    options.set("sc_threshold", "0");
    options.set("repeat_headers", "1");
    options.set("annexb", "1");
    let encoder = encoder.open_as_with(codec, options)?;
    let scaler = ScaleContext::get(
        Pixel::BGRA,
        config.width,
        config.height,
        Pixel::YUV420P,
        config.width,
        config.height,
        ScaleFlags::FAST_BILINEAR,
    )?;
    Ok(OpenEncoder {
        encoder,
        backend: EncoderBackend::X264,
        codec: VideoCodec::H264,
        scaler,
        vaapi: None,
    })
}

fn append_avcc_nalu(output: &mut Vec<u8>, nalu: &[u8]) -> Result<(), EncodeError> {
    let length = u32::try_from(nalu.len()).map_err(|_| EncodeError::InvalidBitstream)?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(nalu);
    Ok(())
}

fn append_unique_parameter_sets(
    output: &mut Vec<u8>,
    parameter_sets: &[Vec<u8>],
    frame_nalus: &[&[u8]],
) -> Result<(), EncodeError> {
    for parameter_set in parameter_sets {
        if !frame_nalus
            .iter()
            .any(|nalu| *nalu == parameter_set.as_slice())
        {
            append_avcc_nalu(output, parameter_set)?;
        }
    }
    Ok(())
}

fn parse_nal_units(data: &[u8]) -> Result<Vec<&[u8]>, EncodeError> {
    // A four-byte AVCC length of one is byte-identical to an Annex-B start
    // code. Parse a complete AVCC packet first, then fall back to Annex-B.
    parse_avcc(data).or_else(|_| parse_annex_b(data))
}

fn parse_annex_b(data: &[u8]) -> Result<Vec<&[u8]>, EncodeError> {
    let mut starts = Vec::new();
    let mut index = 0;
    while index + 3 <= data.len() {
        let prefix = if data[index..].starts_with(&[0, 0, 0, 1]) {
            Some(4)
        } else if data[index..].starts_with(&[0, 0, 1]) {
            Some(3)
        } else {
            None
        };
        if let Some(length) = prefix {
            starts.push((index, index + length));
            index += length;
        } else {
            index += 1;
        }
    }
    let mut nalus = Vec::new();
    for (position, (_, payload_start)) in starts.iter().enumerate() {
        let payload_end = starts
            .get(position + 1)
            .map_or(data.len(), |(next_start, _)| *next_start);
        let mut trimmed_end = payload_end;
        while trimmed_end > *payload_start && data[trimmed_end - 1] == 0 {
            trimmed_end -= 1;
        }
        if trimmed_end > *payload_start {
            nalus.push(&data[*payload_start..trimmed_end]);
        }
    }
    (!nalus.is_empty())
        .then_some(nalus)
        .ok_or(EncodeError::InvalidBitstream)
}

fn parse_avcc(data: &[u8]) -> Result<Vec<&[u8]>, EncodeError> {
    let mut nalus = Vec::new();
    let mut offset = 0;
    while offset < data.len() {
        if offset + 4 > data.len() {
            return Err(EncodeError::InvalidBitstream);
        }
        let length =
            u32::from_be_bytes(data[offset..offset + 4].try_into().expect("four bytes")) as usize;
        offset += 4;
        if length == 0 || offset + length > data.len() {
            return Err(EncodeError::InvalidBitstream);
        }
        nalus.push(&data[offset..offset + length]);
        offset += length;
    }
    (!nalus.is_empty())
        .then_some(nalus)
        .ok_or(EncodeError::InvalidBitstream)
}

fn extract_parameter_sets(codec: VideoCodec, nalus: &[&[u8]]) -> Vec<Vec<u8>> {
    nalus
        .iter()
        .filter(|nalu| match codec {
            VideoCodec::H264 => matches!(nalu.first().map(|byte| byte & 0x1f), Some(7 | 8)),
            VideoCodec::Hevc => {
                nalu.len() >= 2
                    && matches!(
                        nalu.first().map(|byte| (byte >> 1) & 0x3f),
                        Some(32 | 33 | 34)
                    )
            }
        })
        .map(|nalu| nalu.to_vec())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_annex_b_to_avcc_without_padding() {
        let annex_b = [0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x68, 3];
        let nalus = parse_nal_units(&annex_b).unwrap();
        let mut output = Vec::new();
        for nalu in nalus {
            append_avcc_nalu(&mut output, nalu).unwrap();
        }
        assert_eq!(output, vec![0, 0, 0, 3, 0x67, 1, 2, 0, 0, 0, 2, 0x68, 3]);
    }

    #[test]
    fn detects_h264_and_hevc_parameter_sets() {
        let h264 = [vec![0x67, 1], vec![0x68, 2], vec![0x65, 3]];
        let h264_refs = h264.iter().map(Vec::as_slice).collect::<Vec<_>>();
        assert_eq!(
            extract_parameter_sets(VideoCodec::H264, &h264_refs).len(),
            2
        );

        let hevc = [
            vec![32 << 1, 1],
            vec![33 << 1, 2],
            vec![34 << 1, 3],
            vec![19 << 1, 4],
        ];
        let hevc_refs = hevc.iter().map(Vec::as_slice).collect::<Vec<_>>();
        assert_eq!(
            extract_parameter_sets(VideoCodec::Hevc, &hevc_refs).len(),
            3
        );
    }

    #[test]
    fn accepts_existing_avcc() {
        let avcc = [0, 0, 0, 2, 0x65, 1, 0, 0, 0, 1, 0x41];
        assert_eq!(
            parse_nal_units(&avcc).unwrap(),
            vec![&avcc[4..6], &avcc[10..11]]
        );
    }
}
