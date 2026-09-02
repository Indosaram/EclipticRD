use std::time::{Duration, Instant};

const LOSS_THRESHOLD: f64 = 0.05;
const REDUCTION_FACTOR: f64 = 0.75;
const STABLE_RECOVERY_AFTER: Duration = Duration::from_secs(30);

/// Adaptive bitrate controller matching the client protocol contract.
#[derive(Debug, Clone)]
pub struct AbrController {
    initial_bitrate: i32,
    current_bitrate: i32,
    min_bitrate: i32,
    max_bitrate: i32,
    stable_since: Option<Instant>,
}

impl AbrController {
    pub fn new(initial_bitrate: i32, min_bitrate: i32, max_bitrate: i32) -> Self {
        assert!(min_bitrate > 0);
        assert!(min_bitrate <= initial_bitrate && initial_bitrate <= max_bitrate);
        Self {
            initial_bitrate,
            current_bitrate: initial_bitrate,
            min_bitrate,
            max_bitrate,
            stable_since: None,
        }
    }

    pub fn current_bitrate(&self) -> i32 {
        self.current_bitrate
    }

    /// Returns a new target bitrate when a control update must be emitted.
    pub fn evaluate(&mut self, loss_ratio: f64, now: Instant) -> Option<i32> {
        if loss_ratio > LOSS_THRESHOLD {
            self.stable_since = None;
            let reduced = ((self.current_bitrate as f64) * REDUCTION_FACTOR) as i32;
            return self.update(reduced.max(self.min_bitrate));
        }

        if loss_ratio <= LOSS_THRESHOLD {
            let stable_since = self.stable_since.get_or_insert(now);
            if now.saturating_duration_since(*stable_since) >= STABLE_RECOVERY_AFTER {
                *stable_since = now;
                let recovered = if self.current_bitrate < self.initial_bitrate {
                    self.initial_bitrate
                } else {
                    ((self.current_bitrate as f64) / REDUCTION_FACTOR) as i32
                };
                return self.update(recovered.min(self.max_bitrate));
            }
        }
        None
    }

    fn update(&mut self, target: i32) -> Option<i32> {
        if target == self.current_bitrate {
            return None;
        }
        self.current_bitrate = target;
        Some(target)
    }
}
