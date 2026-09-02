use std::sync::mpsc;
use std::time::Instant;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayInfo {
    pub logical_width: u32,
    pub logical_height: u32,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub scale_factor_milli: u32,
}

impl DisplayInfo {
    pub fn scale_factor(self) -> f32 {
        self.scale_factor_milli as f32 / 1_000.0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CaptureConfig {
    pub width: u32,
    pub height: u32,
    /// Zero captures at the display's native refresh rate.
    pub frames_per_second: u32,
    pub capture_audio: bool,
}

impl CaptureConfig {
    pub fn native(display: DisplayInfo, capture_audio: bool) -> Self {
        Self {
            width: display.pixel_width,
            height: display.pixel_height,
            frames_per_second: 0,
            capture_audio,
        }
    }
}

#[derive(Debug)]
pub struct CaptureFrame {
    pub width: u32,
    pub height: u32,
    pub bytes_per_row: usize,
    pub bgra: Vec<u8>,
    pub captured_at: Instant,
}

#[derive(Debug)]
pub enum CaptureEvent {
    Video(CaptureFrame),
    /// ScreenCaptureKit is configured for 48 kHz stereo Float32 interleaved PCM.
    Audio {
        pcm_f32_le: Vec<u8>,
        captured_at: Instant,
    },
    Stopped(String),
}

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("ScreenCaptureKit is available only on macOS")]
    Unsupported,
    #[error("Screen Recording access is not granted")]
    PermissionDenied,
    #[error("no display is available")]
    NoDisplay,
    #[error("ScreenCaptureKit failed: {0}")]
    ScreenCaptureKit(String),
    #[error("capture setup timed out")]
    Timeout,
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use block2::RcBlock;
    use core_graphics::display::CGDisplay;
    use objc2::ffi::NSInteger;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{define_class, msg_send, AnyThread, DeclaredClass};
    use objc2_core_media::CMSampleBuffer;
    use objc2_core_video::{
        kCVPixelFormatType_32BGRA, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
        CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth,
        CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    };
    use objc2_foundation::{NSArray, NSError};
    use objc2_screen_capture_kit::{
        SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamOutput,
        SCStreamOutputType,
    };
    use std::ptr::NonNull;
    use std::sync::Mutex;
    use std::time::Duration;

    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
    }

    struct SinkIvars {
        sender: Mutex<mpsc::Sender<CaptureEvent>>,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[ivars = SinkIvars]
        struct FrameSink;

        unsafe impl NSObjectProtocol for FrameSink {}

        unsafe impl SCStreamOutput for FrameSink {
            #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
            unsafe fn __stream_did_output_sample_buffer_of_type(
                &self,
                _stream: *mut SCStream,
                sample_buffer: *mut AnyObject,
                output_type: NSInteger,
            ) {
                if sample_buffer.is_null() {
                    return;
                }
                let sample = &*(sample_buffer.cast::<CMSampleBuffer>());
                let event = if output_type == SCStreamOutputType::Screen.0 {
                    copy_video_frame(sample).map(CaptureEvent::Video)
                } else if output_type == SCStreamOutputType::Audio.0 {
                    copy_audio_frame(sample).map(|pcm_f32_le| CaptureEvent::Audio {
                        pcm_f32_le,
                        captured_at: Instant::now(),
                    })
                } else {
                    None
                };
                if let Some(event) = event {
                    if let Ok(sender) = self.ivars().sender.lock() {
                        let _ = sender.send(event);
                    }
                }
            }
        }
    );

    impl FrameSink {
        fn new(sender: mpsc::Sender<CaptureEvent>) -> Retained<Self> {
            let this = Self::alloc().set_ivars(SinkIvars {
                sender: Mutex::new(sender),
            });
            unsafe { msg_send![super(this), init] }
        }
    }

    unsafe fn copy_video_frame(sample: &CMSampleBuffer) -> Option<CaptureFrame> {
        let image = sample.image_buffer()?;
        if CVPixelBufferGetPixelFormatType(&image) != kCVPixelFormatType_32BGRA {
            return None;
        }
        let flags = CVPixelBufferLockFlags::ReadOnly;
        if CVPixelBufferLockBaseAddress(&image, flags) != 0 {
            return None;
        }
        let width = CVPixelBufferGetWidth(&image);
        let height = CVPixelBufferGetHeight(&image);
        let bytes_per_row = CVPixelBufferGetBytesPerRow(&image);
        let base = CVPixelBufferGetBaseAddress(&image).cast::<u8>();
        let bgra = if base.is_null() {
            None
        } else {
            Some(std::slice::from_raw_parts(base, bytes_per_row * height).to_vec())
        };
        let _ = CVPixelBufferUnlockBaseAddress(&image, flags);
        bgra.map(|bgra| CaptureFrame {
            width: width as u32,
            height: height as u32,
            bytes_per_row,
            bgra,
            captured_at: Instant::now(),
        })
    }

    unsafe fn copy_audio_frame(sample: &CMSampleBuffer) -> Option<Vec<u8>> {
        let block = sample.data_buffer()?;
        let length = block.data_length();
        if length == 0 {
            return None;
        }
        let mut bytes = vec![0_u8; length];
        let destination = NonNull::new(bytes.as_mut_ptr().cast()).expect("Vec pointer is non-null");
        if block.copy_data_bytes(0, length, destination) != 0 {
            return None;
        }
        Some(bytes)
    }

    pub struct MacScreenCapture {
        display_info: DisplayInfo,
        stream: Retained<SCStream>,
        _sink: Retained<FrameSink>,
        _queue: dispatch2::DispatchRetained<dispatch2::DispatchQueue>,
    }

    impl MacScreenCapture {
        pub fn display_info() -> Result<DisplayInfo, CaptureError> {
            let display = CGDisplay::main();
            let logical = display.bounds().size;
            let pixel_width = display.pixels_wide() as u32;
            let pixel_height = display.pixels_high() as u32;
            if pixel_width == 0
                || pixel_height == 0
                || logical.width <= 0.0
                || logical.height <= 0.0
            {
                return Err(CaptureError::NoDisplay);
            }
            let scale = pixel_width as f64 / logical.width;
            Ok(DisplayInfo {
                logical_width: logical.width.round() as u32,
                logical_height: logical.height.round() as u32,
                pixel_width,
                pixel_height,
                scale_factor_milli: (scale * 1_000.0).round() as u32,
            })
        }

        pub fn start(
            config: CaptureConfig,
        ) -> Result<(Self, mpsc::Receiver<CaptureEvent>), CaptureError> {
            // Note: In headless / CI / VM environments, CGPreflightScreenCaptureAccess
            // may return false even when ScreenCaptureKit functions or when running via ssh.
            let _ = unsafe { CGPreflightScreenCaptureAccess() };

            objc2::rc::autoreleasepool(|_| unsafe {
                let (content_tx, content_rx) =
                    mpsc::channel::<Result<*mut SCShareableContent, String>>();
                let completion = RcBlock::new(
                    move |content: *mut SCShareableContent, error: *mut NSError| {
                        if !error.is_null() {
                            let msg = (*error).localizedDescription().to_string();
                            let _ = content_tx.send(Err(msg));
                        } else if !content.is_null() {
                            let retained = Retained::retain(content).expect("non-null SCK content");
                            let _ = content_tx.send(Ok(Retained::into_raw(retained)));
                        } else {
                            let _ = content_tx.send(Err("null content and null error".into()));
                        }
                    },
                );
                SCShareableContent::getShareableContentWithCompletionHandler(&completion);
                let raw_content = content_rx
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(|_| CaptureError::Timeout)?
                    .map_err(CaptureError::ScreenCaptureKit)?;
                let content = Retained::from_raw(raw_content).ok_or_else(|| {
                    CaptureError::ScreenCaptureKit("null shareable content".into())
                })?;
                let displays = content.displays();
                if displays.count() == 0 {
                    return Err(CaptureError::NoDisplay);
                }
                let main_id = CGDisplay::main().id;
                let display = (0..displays.count())
                    .map(|index| displays.objectAtIndex(index))
                    .find(|display| display.displayID() == main_id)
                    .unwrap_or_else(|| displays.objectAtIndex(0));

                let filter = SCContentFilter::initWithDisplay_excludingWindows(
                    SCContentFilter::alloc(),
                    &display,
                    &NSArray::new(),
                );
                let stream_config = SCStreamConfiguration::new();
                stream_config.setWidth(config.width as usize);
                stream_config.setHeight(config.height as usize);
                stream_config.setQueueDepth(3);
                stream_config.setShowsCursor(true);
                stream_config.setPixelFormat(kCVPixelFormatType_32BGRA);
                if config.frames_per_second > 0 {
                    stream_config.setMinimumFrameInterval(objc2_core_media::CMTime::new(
                        1,
                        config.frames_per_second as i32,
                    ));
                } else {
                    stream_config.setMinimumFrameInterval(objc2_core_media::kCMTimeZero);
                }
                stream_config.setCapturesAudio(config.capture_audio);
                stream_config.setSampleRate(48_000);
                stream_config.setChannelCount(2);

                let stream = SCStream::initWithFilter_configuration_delegate(
                    SCStream::alloc(),
                    &filter,
                    &stream_config,
                    None,
                );
                let (sender, receiver) = mpsc::channel();
                let sink = FrameSink::new(sender);
                let queue = dispatch2::DispatchQueue::new("erd-host.sck-output", None);
                stream
                    .addStreamOutput_type_sampleHandlerQueue_error(
                        ProtocolObject::from_ref(&*sink),
                        SCStreamOutputType::Screen,
                        Some(&queue),
                    )
                    .map_err(|error| CaptureError::ScreenCaptureKit(error.to_string()))?;
                if config.capture_audio {
                    stream
                        .addStreamOutput_type_sampleHandlerQueue_error(
                            ProtocolObject::from_ref(&*sink),
                            SCStreamOutputType::Audio,
                            Some(&queue),
                        )
                        .map_err(|error| CaptureError::ScreenCaptureKit(error.to_string()))?;
                }

                let (started_tx, started_rx) = mpsc::sync_channel(1);
                let started = RcBlock::new(move |error: *mut NSError| {
                    let result = if error.is_null() {
                        Ok(())
                    } else {
                        Err((*error).localizedDescription().to_string())
                    };
                    let _ = started_tx.send(result);
                });
                stream.startCaptureWithCompletionHandler(Some(&started));
                started_rx
                    .recv_timeout(Duration::from_secs(10))
                    .map_err(|_| CaptureError::Timeout)?
                    .map_err(CaptureError::ScreenCaptureKit)?;

                Ok((
                    Self {
                        display_info: Self::display_info()?,
                        stream,
                        _sink: sink,
                        _queue: queue,
                    },
                    receiver,
                ))
            })
        }

        pub fn display(&self) -> DisplayInfo {
            self.display_info
        }

        pub fn stop(self) -> Result<(), CaptureError> {
            let (stopped_tx, stopped_rx) = mpsc::sync_channel(1);
            let stopped = RcBlock::new(move |error: *mut NSError| {
                let result = unsafe {
                    if error.is_null() {
                        Ok(())
                    } else {
                        Err((*error).localizedDescription().to_string())
                    }
                };
                let _ = stopped_tx.send(result);
            });
            unsafe { self.stream.stopCaptureWithCompletionHandler(Some(&stopped)) };
            stopped_rx
                .recv_timeout(Duration::from_secs(5))
                .map_err(|_| CaptureError::Timeout)?
                .map_err(CaptureError::ScreenCaptureKit)
        }
    }

    pub use MacScreenCapture as PlatformScreenCapture;
}

#[cfg(target_os = "macos")]
pub use macos::PlatformScreenCapture as ScreenCapture;

#[cfg(not(target_os = "macos"))]
pub struct ScreenCapture;

#[cfg(not(target_os = "macos"))]
impl ScreenCapture {
    pub fn display_info() -> Result<DisplayInfo, CaptureError> {
        Err(CaptureError::Unsupported)
    }

    pub fn start(
        _config: CaptureConfig,
    ) -> Result<(Self, mpsc::Receiver<CaptureEvent>), CaptureError> {
        Err(CaptureError::Unsupported)
    }

    pub fn stop(self) -> Result<(), CaptureError> {
        Err(CaptureError::Unsupported)
    }
}
