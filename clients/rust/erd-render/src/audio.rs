use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BufferSize, SampleFormat, SampleRate, Stream, StreamConfig,
};
use thiserror::Error;

pub const AUDIO_SAMPLE_RATE: u32 = 48_000;
pub const AUDIO_CHANNELS: u16 = 2;

#[derive(Debug, Clone)]
pub struct AudioQueue {
    samples: Arc<Mutex<VecDeque<f32>>>,
    volume: Arc<Mutex<f32>>,
    muted: Arc<Mutex<bool>>,
}

impl Default for AudioQueue {
    fn default() -> Self {
        Self {
            samples: Arc::new(Mutex::new(VecDeque::new())),
            volume: Arc::new(Mutex::new(1.0)),
            muted: Arc::new(Mutex::new(false)),
        }
    }
}

impl AudioQueue {
    pub fn push_pcm_bytes(&self, bytes: &[u8]) -> Result<(), AudioError> {
        if bytes.len() % std::mem::size_of::<f32>() != 0 {
            return Err(AudioError::MisalignedPcm(bytes.len()));
        }
        let mut queue = self.samples.lock().map_err(|_| AudioError::Poisoned)?;
        queue.reserve(bytes.len() / 4);
        for sample in bytes.chunks_exact(4) {
            queue.push_back(f32::from_le_bytes(
                sample.try_into().expect("four-byte chunk"),
            ));
        }
        Ok(())
    }

    pub fn push_samples(&self, samples: impl IntoIterator<Item = f32>) -> Result<(), AudioError> {
        self.samples
            .lock()
            .map_err(|_| AudioError::Poisoned)?
            .extend(samples);
        Ok(())
    }

    pub fn queued_samples(&self) -> Result<usize, AudioError> {
        Ok(self.samples.lock().map_err(|_| AudioError::Poisoned)?.len())
    }

    pub fn set_volume(&self, volume: f32) -> Result<(), AudioError> {
        *self.volume.lock().map_err(|_| AudioError::Poisoned)? = volume.clamp(0.0, 1.0);
        Ok(())
    }

    pub fn set_muted(&self, muted: bool) -> Result<(), AudioError> {
        *self.muted.lock().map_err(|_| AudioError::Poisoned)? = muted;
        Ok(())
    }

    pub fn drain_into(&self, output: &mut [f32]) -> Result<usize, AudioError> {
        let muted = *self.muted.lock().map_err(|_| AudioError::Poisoned)?;
        let volume = *self.volume.lock().map_err(|_| AudioError::Poisoned)?;
        let mut samples = self.samples.lock().map_err(|_| AudioError::Poisoned)?;
        let mut consumed = 0;
        for destination in output {
            if let Some(sample) = samples.pop_front() {
                *destination = if muted { 0.0 } else { sample * volume };
                consumed += 1;
            } else {
                *destination = 0.0;
            }
        }
        Ok(consumed)
    }
}

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("no default audio output device")]
    NoOutputDevice,
    #[error("default output uses {0:?}, but the v3 audio path requires f32")]
    UnsupportedSampleFormat(SampleFormat),
    #[error("PCM byte count {0} is not aligned to f32 samples")]
    MisalignedPcm(usize),
    #[error("audio queue lock was poisoned")]
    Poisoned,
    #[error("audio device error: {0}")]
    Device(String),
}

/// Active CPAL 48 kHz stereo f32 output stream backed by [`AudioQueue`].
pub struct CpalAudioOutput {
    queue: AudioQueue,
    stream: Stream,
}

impl CpalAudioOutput {
    pub fn start(queue: AudioQueue) -> Result<Self, AudioError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or(AudioError::NoOutputDevice)?;
        let supported = device
            .supported_output_configs()
            .map_err(|error| AudioError::Device(error.to_string()))?
            .find(|config| {
                config.channels() == AUDIO_CHANNELS
                    && config.sample_format() == SampleFormat::F32
                    && config.min_sample_rate().0 <= AUDIO_SAMPLE_RATE
                    && config.max_sample_rate().0 >= AUDIO_SAMPLE_RATE
            })
            .ok_or(AudioError::UnsupportedSampleFormat(
                device
                    .default_output_config()
                    .map_err(|error| AudioError::Device(error.to_string()))?
                    .sample_format(),
            ))?;
        let config = StreamConfig {
            channels: AUDIO_CHANNELS,
            sample_rate: SampleRate(AUDIO_SAMPLE_RATE),
            buffer_size: BufferSize::Default,
        };
        let callback_queue = queue.clone();
        let stream = device
            .build_output_stream(
                &config,
                move |output: &mut [f32], _| {
                    let _ = callback_queue.drain_into(output);
                },
                |error| tracing::error!(%error, "CPAL output stream failed"),
                None,
            )
            .map_err(|error| AudioError::Device(error.to_string()))?;
        let _ = supported;
        stream
            .play()
            .map_err(|error| AudioError::Device(error.to_string()))?;
        Ok(Self { queue, stream })
    }

    pub fn queue(&self) -> &AudioQueue {
        &self.queue
    }

    pub fn stream(&self) -> &Stream {
        &self.stream
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_five_second_stream_drains_without_underrun() {
        let queue = AudioQueue::default();
        let sample_count = AUDIO_SAMPLE_RATE as usize * AUDIO_CHANNELS as usize * 5;
        queue
            .push_samples((0..sample_count).map(|sample| (sample as f32 * 0.01).sin()))
            .unwrap();
        let mut output = vec![0.0; sample_count];
        assert_eq!(queue.drain_into(&mut output).unwrap(), sample_count);
        assert_eq!(queue.queued_samples().unwrap(), 0);
        assert!(output.iter().any(|sample| *sample != 0.0));
    }
}
