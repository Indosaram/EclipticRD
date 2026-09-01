// P0-SPIKE: ScreenCaptureKit reachable from Rust? (plan PIVOT v2)
// Gate: stream starts, receives >=5 frames in 5s. Exit 0 = FFI viable.
#![allow(non_upper_case_globals)]

use block2::RcBlock;
use objc2::ffi::NSInteger;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread, DeclaredClass, MainThreadMarker};
use objc2_foundation::{NSArray, NSDefaultRunLoopMode, NSDate, NSError, NSRunLoop};
use objc2_screen_capture_kit::{
    SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamOutput,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};

define_class!(
    #[unsafe(super(NSObject))]
    #[ivars = Arc<AtomicUsize>]
    struct FrameSink;

    unsafe impl NSObjectProtocol for FrameSink {}

    unsafe impl SCStreamOutput for FrameSink {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        unsafe fn __stream_didOutputSampleBuffer_ofType(
            &self,
            _stream: *mut SCStream,
            _sample_buffer: *mut AnyObject,
            of_type: NSInteger,
        ) {
            let n = self.ivars().fetch_add(1, Ordering::SeqCst) + 1;
            if n <= 5 {
                println!("frame #{} received (output type {})", n, of_type);
            }
        }
    }
);

impl FrameSink {
    fn new(frames: Arc<AtomicUsize>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(frames);
        unsafe { msg_send![super(this), init] }
    }
}

fn pump(seconds: f64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs_f64(seconds);
    while std::time::Instant::now() < deadline {
        unsafe {
            NSRunLoop::mainRunLoop().runMode_beforeDate(NSDefaultRunLoopMode, &NSDate::new());
        }
    }
}

fn main() {
    let _mtm = MainThreadMarker::new().expect("SCK requires the main thread");
    objc2::rc::autoreleasepool(|_| {
    unsafe {
        let (tx, rx) = mpsc::channel::<*mut SCShareableContent>();
        let completion = RcBlock::new(
            move |content: *mut SCShareableContent, err: *mut NSError| {
                if err.is_null() && !content.is_null() {
                    // The completion returns an autoreleased object: retain it
                    // before crossing threads, and hand off our +1 via into_raw.
                    let retained = unsafe { Retained::retain(content) }.unwrap();
                    let displays = retained.displays();
                    let d = displays.objectAtIndex(0);
                    println!("[spike] display: {}x{}", d.width(), d.height());
                    let _ = tx.send(Retained::into_raw(retained) as *mut SCShareableContent);
                } else {
                    let msg = if err.is_null() {
                        String::from("null content")
                    } else {
                        (*err).localizedDescription().to_string()
                    };
                    eprintln!("shareable content error: {}", msg);
                }
            },
        );
        SCShareableContent::getShareableContentWithCompletionHandler(&completion);
        
        let content: *mut SCShareableContent =
            match rx.recv_timeout(std::time::Duration::from_secs(10)) {
                Ok(ptr) => ptr,
                Err(_) => {
                    eprintln!(
                        "FAIL: no shareable content in 10s — grant Screen Recording to this terminal, rerun"
                    );
                    std::process::exit(2);
                }
            };

        let displays = (&*content).displays();
        let display = displays.objectAtIndex(0);
        println!("display: {}x{}", display.width(), display.height());

        let filter = SCContentFilter::initWithDisplay_excludingWindows(
            SCContentFilter::alloc(),
            &display,
            &NSArray::new(),
        );
        let config = SCStreamConfiguration::new();
        config.setWidth(1280);
        config.setHeight(800);
        config.setQueueDepth(3);
        config.setShowsCursor(true);
        config.setPixelFormat(1111970369); // 'BGRA'

        let stream = SCStream::initWithFilter_configuration_delegate(
            SCStream::alloc(),
            &filter,
            &config,
            None,
        );

        let frames = Arc::new(AtomicUsize::new(0));
        let sink = FrameSink::new(frames.clone());
        let queue = dispatch2::Queue::new("spike-sck-frames", None);
        let ok = stream
            .addStreamOutput_type_sampleHandlerQueue_error(
                ProtocolObject::from_ref(&*sink),
                objc2_screen_capture_kit::SCStreamOutputType::Screen,
                Some(&queue),
            )
            .is_ok();
        println!("addStreamOutput ok={}", ok);

        let started = RcBlock::new(|_e: *mut NSError| {
            println!("stream started");
        });
        stream.startCaptureWithCompletionHandler(Some(&started));
        pump(5.0);

        let stopped = RcBlock::new(|_e: *mut NSError| {});
        stream.stopCaptureWithCompletionHandler(Some(&stopped));
        pump(0.2);

        let total = frames.load(Ordering::SeqCst);
        println!("TOTAL_FRAMES={}", total);
        if total >= 5 {
            println!("SPIKE_PASS: ScreenCaptureKit is viable from Rust");
        } else {
            println!("SPIKE_FAIL: frames < 5");
            std::process::exit(1);
        }
    }
    });
}
