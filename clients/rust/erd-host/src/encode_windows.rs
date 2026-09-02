//! Media Foundation video encoding for the Windows host.
//!
//! The primary backend is a synchronous Media Foundation Transform (MFT). It
//! prefers a hardware transform, negotiates HEVC first with H.264 fallback,
//! accepts NV12 frames, disables B-frames where the codec exposes `ICodecAPI`,
//! and applies ABR bitrate changes through both the output media type and
//! `CODECAPI_AVEncCommonMeanBitRate`.
//!
//! [`NvencAvailability`] is the alternative NVIDIA path: the `nvenc` crate
//! dynamically loads `nvEncodeAPI64.dll`, so deployments can select an NVENC
//! implementation without shipping or statically linking the SDK. The MFT path
//! remains the compatibility default and may itself resolve to NVIDIA's MFT.
//!
//! Output is normalized to EclipticRD's AVCC contract: every NAL unit has a
//! four-byte big-endian length prefix, and keyframes include cached VPS/SPS/PPS
//! (or SPS/PPS for H.264) before the access unit.
//!
//! CI validates all Windows bindings. Runtime QA still requires real Intel,
//! AMD, and NVIDIA systems to exercise driver selection, format negotiation,
//! bitrate reconfiguration, keyframe requests, and sustained encode load.

use std::{mem::ManuallyDrop, ptr, slice};

use thiserror::Error;
use windows::{
    core::{Interface, GUID},
    Win32::{
        Media::MediaFoundation::{
            eAVEncCommonRateControlMode_LowDelayVBR, CODECAPI_AVEncCommonLowLatency,
            CODECAPI_AVEncCommonMeanBitRate, CODECAPI_AVEncCommonRateControlMode,
            CODECAPI_AVEncMPVDefaultBPictureCount, CODECAPI_AVEncMPVGOPSize,
            CODECAPI_AVEncVideoForceKeyFrame, ICodecAPI, IMFActivate, IMFMediaBuffer, IMFMediaType,
            IMFSample, IMFTransform, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
            MFMediaType_Video, MFSampleExtension_CleanPoint, MFShutdown, MFStartup, MFTEnumEx,
            MFVideoFormat_H264, MFVideoFormat_HEVC, MFVideoFormat_NV12,
            MFVideoInterlace_Progressive, MFSTARTUP_FULL, MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG, MFT_ENUM_FLAG_ALL, MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER,
            MFT_MESSAGE_COMMAND_DRAIN, MFT_MESSAGE_COMMAND_FLUSH,
            MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, MFT_MESSAGE_NOTIFY_END_OF_STREAM,
            MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER,
            MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES, MFT_OUTPUT_STREAM_PROVIDES_SAMPLES,
            MFT_REGISTER_TYPE_INFO, MF_E_NOTACCEPTING, MF_E_TRANSFORM_NEED_MORE_INPUT,
            MF_E_TRANSFORM_STREAM_CHANGE, MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
            MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_MPEG_SEQUENCE_HEADER,
            MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE, MF_VERSION,
        },
        System::{
            Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED},
            Variant::VARIANT,
        },
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    Hevc,
    H264,
}

impl VideoCodec {
    fn media_subtype(self) -> GUID {
        match self {
            Self::Hevc => MFVideoFormat_HEVC,
            Self::H264 => MFVideoFormat_H264,
        }
    }

    fn parameter_set_types(self) -> &'static [u8] {
        match self {
            Self::Hevc => &[32, 33, 34],
            Self::H264 => &[7, 8],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncoderBackend {
    MediaFoundationHardware,
    MediaFoundationSoftware,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderConfig {
    pub width: u32,
    pub height: u32,
    pub bitrate: u32,
    pub fps: u32,
    pub keyframe_interval: u32,
    pub preferred_codec: VideoCodec,
}

impl EncoderConfig {
    fn validate(self) -> Result<Self, EncodeError> {
        if self.width == 0
            || self.height == 0
            || self.width % 2 != 0
            || self.height % 2 != 0
            || self.bitrate == 0
            || self.fps == 0
        {
            return Err(EncodeError::InvalidConfiguration);
        }
        Ok(self)
    }

    fn frame_duration_hns(self) -> i64 {
        10_000_000_i64 / i64::from(self.fps)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub is_key_frame: bool,
    pub timestamp_hns: i64,
    pub codec: VideoCodec,
}

#[derive(Debug, Error)]
pub enum EncodeError {
    #[error(
        "encoder dimensions must be non-zero, even NV12 sizes and bitrate/fps must be positive"
    )]
    InvalidConfiguration,
    #[error("input NV12 frame has the wrong length: expected {expected}, got {actual}")]
    InvalidFrameLength { expected: usize, actual: usize },
    #[error("no Media Foundation {0:?} encoder transform is available")]
    TransformUnavailable(VideoCodec),
    #[error("Media Foundation call failed: {0}")]
    MediaFoundation(#[from] windows::core::Error),
    #[error("Media Foundation returned an output sample without a buffer")]
    MissingOutput,
    #[error("encoded access unit is malformed: {0}")]
    MalformedBitstream(&'static str),
}

struct MediaFoundationRuntime {
    com_initialized: bool,
}

impl MediaFoundationRuntime {
    fn start() -> Result<Self, EncodeError> {
        let status = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let com_initialized = status.is_ok();
        // RPC_E_CHANGED_MODE means COM was initialized differently on this
        // thread. Media Foundation remains usable, but we must not uninitialize.
        if status.is_err() && status.0 != 0x8001_0106_u32 as i32 {
            return Err(windows::core::Error::from(status).into());
        }
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL)? };
        Ok(Self { com_initialized })
    }
}

impl Drop for MediaFoundationRuntime {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
            if self.com_initialized {
                CoUninitialize();
            }
        }
    }
}

/// Availability probe for the optional dynamically loaded NVENC backend.
pub struct NvencAvailability;

impl NvencAvailability {
    pub fn probe() -> Result<u32, String> {
        let library = nvenc::nvenc_init().map_err(|error| error.to_string())?;
        library
            .get_max_version()
            .map_err(|error| format!("{error:?}"))
    }
}

pub struct MediaFoundationEncoder {
    _runtime: MediaFoundationRuntime,
    transform: IMFTransform,
    output_type: IMFMediaType,
    codec_api: Option<ICodecAPI>,
    config: EncoderConfig,
    backend: EncoderBackend,
    codec: VideoCodec,
    frame_index: u64,
    force_keyframe: bool,
    parameter_sets: Vec<Vec<u8>>,
}

impl MediaFoundationEncoder {
    pub fn new(config: EncoderConfig) -> Result<Self, EncodeError> {
        let config = config.validate()?;
        let runtime = MediaFoundationRuntime::start()?;
        let codecs = match config.preferred_codec {
            VideoCodec::Hevc => [VideoCodec::Hevc, VideoCodec::H264],
            VideoCodec::H264 => [VideoCodec::H264, VideoCodec::Hevc],
        };

        let mut last_error = None;
        for codec in codecs {
            match create_transform(codec) {
                Ok((transform, backend)) => {
                    return Self::configure(runtime, transform, backend, codec, config);
                }
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or(EncodeError::TransformUnavailable(config.preferred_codec)))
    }

    fn configure(
        runtime: MediaFoundationRuntime,
        transform: IMFTransform,
        backend: EncoderBackend,
        codec: VideoCodec,
        config: EncoderConfig,
    ) -> Result<Self, EncodeError> {
        let output_type = video_type(codec.media_subtype(), config)?;
        let input_type = video_type(MFVideoFormat_NV12, config)?;
        unsafe {
            // Encoders generally require output first so the desired profile is
            // known while enumerating supported input formats.
            transform.SetOutputType(0, &output_type, 0)?;
            transform.SetInputType(0, &input_type, 0)?;
        }

        let codec_api = transform.cast::<ICodecAPI>().ok();
        if let Some(api) = &codec_api {
            set_codec_u32(
                api,
                &CODECAPI_AVEncCommonRateControlMode,
                eAVEncCommonRateControlMode_LowDelayVBR.0 as u32,
            );
            set_codec_u32(api, &CODECAPI_AVEncCommonMeanBitRate, config.bitrate);
            set_codec_bool(api, &CODECAPI_AVEncCommonLowLatency, true);
            set_codec_u32(api, &CODECAPI_AVEncMPVDefaultBPictureCount, 0);
            set_codec_u32(
                api,
                &CODECAPI_AVEncMPVGOPSize,
                config.keyframe_interval.max(1),
            );
        }

        unsafe {
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
        }
        let parameter_sets = media_type_parameter_sets(&output_type, codec);
        Ok(Self {
            _runtime: runtime,
            transform,
            output_type,
            codec_api,
            config,
            backend,
            codec,
            frame_index: 0,
            force_keyframe: false,
            parameter_sets,
        })
    }

    pub fn backend(&self) -> EncoderBackend {
        self.backend
    }

    pub fn codec(&self) -> VideoCodec {
        self.codec
    }

    /// Input is tightly packed NV12: Y plane followed by interleaved UV.
    pub fn encode_nv12(&mut self, nv12: &[u8]) -> Result<Option<EncodedFrame>, EncodeError> {
        let expected = nv12_len(self.config.width, self.config.height)?;
        if nv12.len() != expected {
            return Err(EncodeError::InvalidFrameLength {
                expected,
                actual: nv12.len(),
            });
        }
        if self.force_keyframe {
            if let Some(api) = &self.codec_api {
                set_codec_bool(api, &CODECAPI_AVEncVideoForceKeyFrame, true);
            }
            self.force_keyframe = false;
        }

        let timestamp = i64::try_from(self.frame_index)
            .unwrap_or(i64::MAX)
            .saturating_mul(self.config.frame_duration_hns());
        let sample = sample_from_bytes(nv12, timestamp, self.config.frame_duration_hns())?;
        match unsafe { self.transform.ProcessInput(0, &sample, 0) } {
            Ok(()) => {}
            Err(error) if error.code() == MF_E_NOTACCEPTING => {
                if let Some(output) = self.take_output()? {
                    return Ok(Some(output));
                }
                unsafe { self.transform.ProcessInput(0, &sample, 0)? };
            }
            Err(error) => return Err(error.into()),
        }
        self.frame_index = self.frame_index.saturating_add(1);
        self.take_output()
    }

    pub fn force_key_frame(&mut self) {
        self.force_keyframe = true;
    }

    /// Applies protocol ABR messages immediately when the selected MFT exposes
    /// dynamic bitrate control. The output media type is updated as a fallback.
    pub fn update_bitrate(&mut self, bitrate: u32) -> Result<(), EncodeError> {
        if bitrate == 0 {
            return Err(EncodeError::InvalidConfiguration);
        }
        self.config.bitrate = bitrate;
        unsafe { self.output_type.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)? };
        if let Some(api) = &self.codec_api {
            set_codec_u32(api, &CODECAPI_AVEncCommonMeanBitRate, bitrate);
        }
        Ok(())
    }

    pub fn flush(&mut self) -> Result<Vec<EncodedFrame>, EncodeError> {
        unsafe {
            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)?;
            self.transform
                .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)?;
        }
        let mut frames = Vec::new();
        while let Some(frame) = self.take_output()? {
            frames.push(frame);
        }
        unsafe {
            self.transform
                .ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0)?
        };
        Ok(frames)
    }

    fn take_output(&mut self) -> Result<Option<EncodedFrame>, EncodeError> {
        let stream_info = unsafe { self.transform.GetOutputStreamInfo(0)? };
        let transform_provides_sample = stream_info.dwFlags
            & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32
                | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0 as u32)
            != 0;
        let sample = if transform_provides_sample {
            None
        } else {
            let sample = unsafe { MFCreateSample()? };
            let capacity = stream_info.cbSize.max(1);
            let buffer = unsafe { MFCreateMemoryBuffer(capacity)? };
            unsafe { sample.AddBuffer(&buffer)? };
            Some(sample)
        };
        let mut output = [MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: 0,
            pSample: ManuallyDrop::new(sample),
            dwStatus: 0,
            pEvents: ManuallyDrop::new(None),
        }];
        let mut status = 0;
        let result = unsafe { self.transform.ProcessOutput(0, &mut output, &mut status) };
        let output_sample = unsafe { ManuallyDrop::take(&mut output[0].pSample) };
        let _events = unsafe { ManuallyDrop::take(&mut output[0].pEvents) };
        match result {
            Ok(()) => {}
            Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(None),
            Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                let new_type = unsafe { self.transform.GetOutputAvailableType(0, 0)? };
                unsafe { self.transform.SetOutputType(0, &new_type, 0)? };
                self.output_type = new_type;
                self.parameter_sets = media_type_parameter_sets(&self.output_type, self.codec);
                return self.take_output();
            }
            Err(error) => return Err(error.into()),
        }

        let sample = output_sample.ok_or(EncodeError::MissingOutput)?;
        let timestamp_hns = unsafe { sample.GetSampleTime().unwrap_or_default() };
        let is_key_frame = unsafe {
            sample
                .GetUINT32(&MFSampleExtension_CleanPoint)
                .unwrap_or_default()
                != 0
        };
        let bytes = sample_bytes(&sample)?;
        let mut nalus = parse_access_unit(&bytes)?;
        if is_key_frame {
            let fresh = parameter_sets(&nalus, self.codec);
            if !fresh.is_empty() {
                self.parameter_sets = fresh;
            }
            if !self.parameter_sets.is_empty() {
                let present_types: Vec<u8> = nalus
                    .iter()
                    .filter_map(|nalu| nalu_type(nalu, self.codec))
                    .collect();
                let mut prefixed = Vec::new();
                for parameter_set in &self.parameter_sets {
                    let Some(kind) = nalu_type(parameter_set, self.codec) else {
                        continue;
                    };
                    if !present_types.contains(&kind) {
                        prefixed.push(parameter_set.clone());
                    }
                }
                prefixed.append(&mut nalus);
                nalus = prefixed;
            }
        }

        Ok(Some(EncodedFrame {
            data: write_avcc(&nalus)?,
            is_key_frame,
            timestamp_hns,
            codec: self.codec,
        }))
    }
}

impl Drop for MediaFoundationEncoder {
    fn drop(&mut self) {
        unsafe {
            let _ = self
                .transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0);
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
        }
    }
}

fn create_transform(codec: VideoCodec) -> Result<(IMFTransform, EncoderBackend), EncodeError> {
    let hardware_flags = MFT_ENUM_FLAG(MFT_ENUM_FLAG_HARDWARE.0 | MFT_ENUM_FLAG_SORTANDFILTER.0);
    if let Some(transform) = enumerate_transform(codec, hardware_flags)? {
        return Ok((transform, EncoderBackend::MediaFoundationHardware));
    }
    if let Some(transform) = enumerate_transform(codec, MFT_ENUM_FLAG_ALL)? {
        return Ok((transform, EncoderBackend::MediaFoundationSoftware));
    }
    Err(EncodeError::TransformUnavailable(codec))
}

fn enumerate_transform(
    codec: VideoCodec,
    flags: MFT_ENUM_FLAG,
) -> Result<Option<IMFTransform>, EncodeError> {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: codec.media_subtype(),
    };
    let mut activations: *mut Option<IMFActivate> = ptr::null_mut();
    let mut count = 0;
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            flags,
            Some(&input),
            Some(&output),
            &mut activations,
            &mut count,
        )?;
    }
    if count == 0 || activations.is_null() {
        return Ok(None);
    }

    let mut selected = None;
    unsafe {
        let entries = slice::from_raw_parts_mut(activations, count as usize);
        for entry in entries {
            let Some(activation) = entry.take() else {
                continue;
            };
            if selected.is_none() {
                selected = Some(activation.ActivateObject::<IMFTransform>()?);
            }
        }
        CoTaskMemFree(Some(activations.cast()));
    }
    Ok(selected)
}

fn video_type(subtype: GUID, config: EncoderConfig) -> Result<IMFMediaType, EncodeError> {
    let media_type = unsafe { MFCreateMediaType()? };
    unsafe {
        media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        media_type.SetGUID(&MF_MT_SUBTYPE, &subtype)?;
        media_type.SetUINT64(
            &MF_MT_FRAME_SIZE,
            (u64::from(config.width) << 32) | u64::from(config.height),
        )?;
        media_type.SetUINT64(&MF_MT_FRAME_RATE, (u64::from(config.fps) << 32) | 1)?;
        media_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1_u64 << 32) | 1)?;
        media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        media_type.SetUINT32(&MF_MT_AVG_BITRATE, config.bitrate)?;
    }
    Ok(media_type)
}

fn set_codec_u32(api: &ICodecAPI, key: &GUID, value: u32) {
    let value = VARIANT::from(value);
    unsafe {
        let _ = api.SetValue(key, &value);
    }
}

fn set_codec_bool(api: &ICodecAPI, key: &GUID, value: bool) {
    let value = VARIANT::from(value);
    unsafe {
        let _ = api.SetValue(key, &value);
    }
}

fn nv12_len(width: u32, height: u32) -> Result<usize, EncodeError> {
    let pixels = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or(EncodeError::InvalidConfiguration)?;
    pixels
        .checked_add(pixels / 2)
        .ok_or(EncodeError::InvalidConfiguration)
}

fn sample_from_bytes(
    bytes: &[u8],
    timestamp: i64,
    duration: i64,
) -> Result<IMFSample, EncodeError> {
    let length = u32::try_from(bytes.len()).map_err(|_| EncodeError::InvalidConfiguration)?;
    let buffer = unsafe { MFCreateMemoryBuffer(length)? };
    let mut destination = ptr::null_mut();
    unsafe {
        buffer.Lock(&mut destination, None, None)?;
        ptr::copy_nonoverlapping(bytes.as_ptr(), destination, bytes.len());
        if let Err(error) = buffer.Unlock() {
            return Err(error.into());
        }
        buffer.SetCurrentLength(length)?;
        let sample = MFCreateSample()?;
        sample.AddBuffer(&buffer)?;
        sample.SetSampleTime(timestamp)?;
        sample.SetSampleDuration(duration)?;
        Ok(sample)
    }
}

fn sample_bytes(sample: &IMFSample) -> Result<Vec<u8>, EncodeError> {
    let buffer = unsafe { sample.ConvertToContiguousBuffer()? };
    copy_media_buffer(&buffer)
}

fn copy_media_buffer(buffer: &IMFMediaBuffer) -> Result<Vec<u8>, EncodeError> {
    let length = unsafe { buffer.GetCurrentLength()? } as usize;
    let mut pointer = ptr::null_mut();
    unsafe {
        buffer.Lock(&mut pointer, None, None)?;
        let bytes = slice::from_raw_parts(pointer, length).to_vec();
        buffer.Unlock()?;
        Ok(bytes)
    }
}

fn media_type_parameter_sets(media_type: &IMFMediaType, codec: VideoCodec) -> Vec<Vec<u8>> {
    let Ok(size) = (unsafe { media_type.GetBlobSize(&MF_MT_MPEG_SEQUENCE_HEADER) }) else {
        return Vec::new();
    };
    if size == 0 {
        return Vec::new();
    }
    let mut bytes = vec![0_u8; size as usize];
    if unsafe { media_type.GetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, &mut bytes, None) }.is_err() {
        return Vec::new();
    }
    parse_access_unit(&bytes)
        .map(|nalus| parameter_sets(&nalus, codec))
        .unwrap_or_default()
}

/// Accept either Annex B or four-byte length-prefixed MFT/NVENC output.
pub fn parse_access_unit(bytes: &[u8]) -> Result<Vec<Vec<u8>>, EncodeError> {
    if bytes.is_empty() {
        return Err(EncodeError::MalformedBitstream("empty access unit"));
    }
    if is_start_code_at(bytes, 0).is_some() {
        parse_annex_b(bytes)
    } else {
        parse_avcc(bytes)
    }
}

fn parse_avcc(bytes: &[u8]) -> Result<Vec<Vec<u8>>, EncodeError> {
    let mut offset = 0;
    let mut nalus = Vec::new();
    while offset < bytes.len() {
        if bytes.len() - offset < 4 {
            return Err(EncodeError::MalformedBitstream("truncated AVCC NAL length"));
        }
        let length = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        if length == 0 || length > bytes.len() - offset {
            return Err(EncodeError::MalformedBitstream("invalid AVCC NAL length"));
        }
        nalus.push(bytes[offset..offset + length].to_vec());
        offset += length;
    }
    Ok(nalus)
}

fn parse_annex_b(bytes: &[u8]) -> Result<Vec<Vec<u8>>, EncodeError> {
    let mut nalus = Vec::new();
    let mut cursor = 0;
    while let Some((start, prefix)) = find_start_code(bytes, cursor) {
        let nalu_start = start + prefix;
        let next = find_start_code(bytes, nalu_start).map_or(bytes.len(), |(index, _)| index);
        if next > nalu_start {
            nalus.push(bytes[nalu_start..next].to_vec());
        }
        cursor = next;
        if cursor >= bytes.len() {
            break;
        }
    }
    if nalus.is_empty() {
        Err(EncodeError::MalformedBitstream("no Annex B NAL units"))
    } else {
        Ok(nalus)
    }
}

fn find_start_code(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
    (from..bytes.len()).find_map(|index| is_start_code_at(bytes, index).map(|len| (index, len)))
}

fn is_start_code_at(bytes: &[u8], index: usize) -> Option<usize> {
    if bytes.get(index..index + 4) == Some(&[0, 0, 0, 1]) {
        Some(4)
    } else if bytes.get(index..index + 3) == Some(&[0, 0, 1]) {
        Some(3)
    } else {
        None
    }
}

fn write_avcc(nalus: &[Vec<u8>]) -> Result<Vec<u8>, EncodeError> {
    let capacity = nalus
        .iter()
        .try_fold(0_usize, |size, nalu| {
            size.checked_add(4)?.checked_add(nalu.len())
        })
        .ok_or(EncodeError::MalformedBitstream(
            "AVCC output length overflow",
        ))?;
    let mut output = Vec::with_capacity(capacity);
    for nalu in nalus {
        let length = u32::try_from(nalu.len())
            .map_err(|_| EncodeError::MalformedBitstream("NAL unit exceeds u32"))?;
        output.extend_from_slice(&length.to_be_bytes());
        output.extend_from_slice(nalu);
    }
    Ok(output)
}

fn parameter_sets(nalus: &[Vec<u8>], codec: VideoCodec) -> Vec<Vec<u8>> {
    nalus
        .iter()
        .filter(|nalu| {
            nalu_type(nalu, codec).is_some_and(|kind| codec.parameter_set_types().contains(&kind))
        })
        .cloned()
        .collect()
}

fn nalu_type(nalu: &[u8], codec: VideoCodec) -> Option<u8> {
    let first = *nalu.first()?;
    Some(match codec {
        VideoCodec::Hevc => (first >> 1) & 0x3f,
        VideoCodec::H264 => first & 0x1f,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annex_b_is_normalized_to_four_byte_avcc() {
        let annex_b = [0, 0, 0, 1, 0x40, 1, 2, 0, 0, 1, 0x26, 3];
        let nalus = parse_access_unit(&annex_b).unwrap();
        assert_eq!(nalus, vec![vec![0x40, 1, 2], vec![0x26, 3]]);
        assert_eq!(
            write_avcc(&nalus).unwrap(),
            [0, 0, 0, 3, 0x40, 1, 2, 0, 0, 0, 2, 0x26, 3]
        );
    }

    #[test]
    fn avcc_round_trips_and_rejects_truncation() {
        let avcc = [0, 0, 0, 2, 0x42, 1, 0, 0, 0, 1, 0x44];
        assert_eq!(
            write_avcc(&parse_access_unit(&avcc).unwrap()).unwrap(),
            avcc
        );
        assert!(parse_access_unit(&avcc[..avcc.len() - 1]).is_err());
    }

    #[test]
    fn extracts_hevc_and_h264_parameter_sets() {
        let hevc = vec![vec![32 << 1], vec![33 << 1], vec![34 << 1], vec![19 << 1]];
        assert_eq!(parameter_sets(&hevc, VideoCodec::Hevc).len(), 3);
        let h264 = vec![vec![0x67], vec![0x68], vec![0x65]];
        assert_eq!(parameter_sets(&h264, VideoCodec::H264).len(), 2);
    }
}
