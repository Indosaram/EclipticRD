//! Shared bounded encoded-frame ordering and recovery for CLI and desktop.
use anyhow::{bail, Result};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

pub const FRAME_QUEUE_CAPACITY: usize = 4;
/// Preserve the receive instant across asynchronous decoding.
pub type QueuedFrame = (crate::AssembledFrame, Instant);

pub struct FrameQueue {
    state: std::sync::Mutex<FrameQueueState>,
    ready: std::sync::Condvar,
}

struct FrameQueueState {
    frames: std::collections::VecDeque<QueuedFrame>,
    last_frame_id: Option<u32>,
    recovering: bool,
    stopped: bool,
}

impl FrameQueue {
    pub fn new() -> Self {
        Self {
            state: std::sync::Mutex::new(FrameQueueState {
                frames: std::collections::VecDeque::with_capacity(FRAME_QUEUE_CAPACITY),
                last_frame_id: None,
                recovering: false,
                stopped: false,
            }),
            ready: std::sync::Condvar::new(),
        }
    }

    /// Admit a frame without blocking the producer. True requests one keyframe.
    pub fn push(&self, frame: QueuedFrame) -> Result<bool> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("frame queue poisoned"))?;
        if state.stopped {
            bail!("frame queue stopped");
        }
        let key = frame.0.header.is_key_frame;
        let id = frame.0.header.frame_id;
        let mut discontinuity = false;
        if let Some(last) = state.last_frame_id {
            let advance = id.wrapping_sub(last);
            // Half-range serial ordering rejects late frames across u32 wrap.
            if advance == 0 || advance >= (1 << 31) {
                return Ok(false);
            }
            discontinuity = advance != 1;
        }
        state.last_frame_id = Some(id);
        let mut request_keyframe = false;
        if discontinuity || state.frames.len() == FRAME_QUEUE_CAPACITY {
            // Dropping a reference invalidates the entire pending chain.
            state.frames.clear();
            request_keyframe = !state.recovering && !key;
            state.recovering = true;
        }
        if state.recovering && !key {
            return Ok(request_keyframe);
        }
        state.recovering = false;
        state.frames.push_back(frame);
        self.ready.notify_one();
        Ok(request_keyframe)
    }

    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> std::result::Result<QueuedFrame, mpsc::RecvTimeoutError> {
        let state = self
            .state
            .lock()
            .map_err(|_| mpsc::RecvTimeoutError::Disconnected)?;
        let (mut state, _) = self
            .ready
            .wait_timeout_while(state, timeout, |state| {
                state.frames.is_empty() && !state.stopped
            })
            .map_err(|_| mpsc::RecvTimeoutError::Disconnected)?;
        if state.stopped {
            return Err(mpsc::RecvTimeoutError::Disconnected);
        }
        state
            .frames
            .pop_front()
            .ok_or(mpsc::RecvTimeoutError::Timeout)
    }

    /// Invalidate dependents of a failed in-flight decode. An already queued
    /// independent keyframe and its successors remain usable.
    pub fn decode_failed(&self) -> Result<bool> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("frame queue poisoned"))?;
        if state.stopped {
            return Ok(false);
        }
        if let Some(key) = state
            .frames
            .iter()
            .position(|frame| frame.0.header.is_key_frame)
        {
            state.frames.drain(..key);
            state.recovering = false;
            return Ok(false);
        }
        state.frames.clear();
        let request = !state.recovering;
        state.recovering = true;
        Ok(request)
    }

    /// Wait for a frame or explicit stop, without polling the decoder worker.
    pub fn recv(&self) -> std::result::Result<QueuedFrame, mpsc::RecvError> {
        let state = self.state.lock().map_err(|_| mpsc::RecvError)?;
        let mut state = self
            .ready
            .wait_while(state, |state| state.frames.is_empty() && !state.stopped)
            .map_err(|_| mpsc::RecvError)?;
        if state.stopped {
            return Err(mpsc::RecvError);
        }
        state.frames.pop_front().ok_or(mpsc::RecvError)
    }

    pub fn stop(&self) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("frame queue poisoned"))?;
        state.stopped = true;
        state.frames.clear();
        self.ready.notify_all();
        Ok(())
    }
}

impl Default for FrameQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn frame(id: u32, key: bool) -> QueuedFrame {
        (
            crate::AssembledFrame {
                header: erd_proto::FrameHeader {
                    frame_id: id,
                    width: 2,
                    height: 2,
                    is_key_frame: key,
                    total_chunks: 1,
                    total_size: 1,
                },
                data: vec![1],
                timestamp_ms: id,
            },
            Instant::now(),
        )
    }

    #[test]
    fn four_slots_overflow_then_recover_with_original_receive_instant() {
        let queue = FrameQueue::new();
        for id in 0..4 {
            assert!(!queue.push(frame(id, id == 0)).unwrap());
        }
        assert_eq!(queue.state.lock().unwrap().frames.len(), 4);
        assert!(queue.push(frame(4, false)).unwrap());
        assert!(!queue.push(frame(5, false)).unwrap());
        assert_eq!(
            queue.recv_timeout(Duration::ZERO),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        let key = frame(6, true);
        queue.push(key.clone()).unwrap();
        assert_eq!(queue.recv().unwrap(), key);
    }

    #[test]
    fn duplicate_and_late_frames_are_rejected_across_wrap() {
        let queue = FrameQueue::new();
        for (id, key) in [
            (u32::MAX, true),
            (0, false),
            (u32::MAX, true),
            (0, false),
            (1, false),
        ] {
            assert!(!queue.push(frame(id, key)).unwrap());
        }
        for id in [u32::MAX, 0, 1] {
            assert_eq!(queue.recv().unwrap().0.header.frame_id, id);
        }
        assert_eq!(
            queue.recv_timeout(Duration::ZERO),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
    }

    #[test]
    fn decode_failure_discards_pending_dependents_and_requests_only_once() {
        let queue = FrameQueue::new();
        queue.push(frame(0, true)).unwrap();
        queue.recv().unwrap();
        queue.push(frame(1, false)).unwrap();
        queue.push(frame(2, false)).unwrap();
        assert!(queue.decode_failed().unwrap());
        assert!(!queue.decode_failed().unwrap());
        assert!(!queue.push(frame(3, false)).unwrap());
        assert_eq!(
            queue.recv_timeout(Duration::ZERO),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        assert!(!queue.push(frame(4, true)).unwrap());
        assert!(!queue.push(frame(5, false)).unwrap());
        assert_eq!(queue.recv().unwrap().0.header.frame_id, 4);
        assert_eq!(queue.recv().unwrap().0.header.frame_id, 5);
    }

    #[test]
    fn decode_failure_preserves_already_queued_independent_chain() {
        let queue = FrameQueue::new();
        queue.push(frame(0, true)).unwrap();
        queue.recv().unwrap();
        for (id, key) in [(1, false), (2, true), (3, false)] {
            queue.push(frame(id, key)).unwrap();
        }
        assert!(!queue.decode_failed().unwrap());
        assert_eq!(queue.recv().unwrap().0.header.frame_id, 2);
        assert_eq!(queue.recv().unwrap().0.header.frame_id, 3);
    }

    #[test]
    fn stop_wakes_blocking_waiter_and_rejects_producer() {
        let queue = Arc::new(FrameQueue::new());
        let consumer = queue.clone();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            // Signal while holding the queue mutex: stop cannot acquire it
            // until Condvar atomically registers this waiter and unlocks it.
            let state = consumer.state.lock().unwrap();
            entered_tx.send(()).unwrap();
            let state = consumer
                .ready
                .wait_while(state, |state| !state.stopped)
                .unwrap();
            done_tx.send(state.stopped).unwrap();
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        queue.stop().unwrap();
        let completion = done_rx.recv_timeout(Duration::from_secs(2));
        // Release even a broken stop implementation before reporting failure.
        queue.ready.notify_all();
        worker.join().unwrap();
        assert!(completion.unwrap());
        assert_eq!(queue.recv(), Err(mpsc::RecvError));
        assert!(queue.push(frame(0, true)).is_err());
        assert!(!queue.decode_failed().unwrap());
    }
}
