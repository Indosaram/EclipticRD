use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Fixed-capacity lock-free or single-threaded ring buffer for low-latency per-frame measurement.
#[derive(Debug, Clone)]
pub struct LatencyRecorder {
    samples: Vec<u64>,
    capacity: usize,
    index: usize,
    count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatencyStats {
    pub frames: u64,
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub max_us: u64,
}

impl Default for LatencyRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl LatencyRecorder {
    pub const DEFAULT_CAPACITY: usize = 4096;

    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            samples: Vec::with_capacity(capacity),
            capacity,
            index: 0,
            count: 0,
        }
    }

    /// Records a per-frame sample from capture timestamp (if present, else receive Instant),
    /// decode timestamp, and present timestamp.
    pub fn record_sample(
        &mut self,
        capture_ts: Option<Instant>,
        decode_ts: Instant,
        present_ts: Instant,
    ) {
        let start = capture_ts.unwrap_or(decode_ts);
        let latency = present_ts.saturating_duration_since(start);
        self.record_duration(latency);
    }

    /// Ingests a frame latency as a `Duration`.
    pub fn record_duration(&mut self, latency: Duration) {
        let us = latency.as_micros().min(u64::MAX as u128) as u64;
        self.record_us(us);
    }

    /// Ingests a latency sample directly in microseconds.
    pub fn record_us(&mut self, latency_us: u64) {
        if self.samples.len() < self.capacity {
            self.samples.push(latency_us);
        } else {
            self.samples[self.index] = latency_us;
        }
        self.index = (self.index + 1) % self.capacity;
        self.count = self.count.saturating_add(1);
    }

    pub fn frame_count(&self) -> u64 {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn stats(&self) -> LatencyStats {
        if self.samples.is_empty() {
            return LatencyStats {
                frames: 0,
                p50_us: 0,
                p95_us: 0,
                p99_us: 0,
                max_us: 0,
            };
        }

        let mut sorted = self.samples.clone();
        sorted.sort_unstable();

        let len = sorted.len();
        let p50_us = percentile_sorted(&sorted, 0.50);
        let p95_us = percentile_sorted(&sorted, 0.95);
        let p99_us = percentile_sorted(&sorted, 0.99);
        let max_us = sorted[len - 1];

        LatencyStats {
            frames: self.count,
            p50_us,
            p95_us,
            p99_us,
            max_us,
        }
    }

    pub fn stats_json(&self) -> String {
        serde_json::to_string(&self.stats()).unwrap_or_else(|_| {
            r#"{"frames":0,"p50_us":0,"p95_us":0,"p99_us":0,"max_us":0}"#.to_string()
        })
    }
}

fn percentile_sorted(sorted: &[u64], percentile: f64) -> u64 {
    let len = sorted.len();
    if len == 0 {
        return 0;
    }
    if len == 1 {
        return sorted[0];
    }
    let rank = ((len - 1) as f64 * percentile.clamp(0.0, 1.0)).round() as usize;
    sorted[rank.min(len - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_recorder_stats() {
        let recorder = LatencyRecorder::new();
        assert_eq!(recorder.frame_count(), 0);
        assert!(recorder.is_empty());
        let stats = recorder.stats();
        assert_eq!(stats.frames, 0);
        assert_eq!(stats.p50_us, 0);
        assert_eq!(stats.p95_us, 0);
        assert_eq!(stats.p99_us, 0);
        assert_eq!(stats.max_us, 0);
        assert_eq!(
            recorder.stats_json(),
            r#"{"frames":0,"p50_us":0,"p95_us":0,"p99_us":0,"max_us":0}"#
        );
    }

    #[test]
    fn single_sample_stats() {
        let mut recorder = LatencyRecorder::new();
        recorder.record_us(1234);
        assert_eq!(recorder.frame_count(), 1);
        assert!(!recorder.is_empty());
        let stats = recorder.stats();
        assert_eq!(stats.frames, 1);
        assert_eq!(stats.p50_us, 1234);
        assert_eq!(stats.p95_us, 1234);
        assert_eq!(stats.p99_us, 1234);
        assert_eq!(stats.max_us, 1234);
        assert_eq!(
            recorder.stats_json(),
            r#"{"frames":1,"p50_us":1234,"p95_us":1234,"p99_us":1234,"max_us":1234}"#
        );
    }

    #[test]
    fn synthetic_1000_sample_distribution_sanity() {
        let mut recorder = LatencyRecorder::with_capacity(1000);
        for i in 1..=1000 {
            recorder.record_us(i);
        }
        assert_eq!(recorder.frame_count(), 1000);
        let stats = recorder.stats();
        assert_eq!(stats.frames, 1000);
        assert!(
            stats.p50_us <= stats.p95_us,
            "p50 ({}) should be <= p95 ({})",
            stats.p50_us,
            stats.p95_us
        );
        assert!(
            stats.p95_us <= stats.p99_us,
            "p95 ({}) should be <= p99 ({})",
            stats.p95_us,
            stats.p99_us
        );
        assert!(
            stats.p99_us <= stats.max_us,
            "p99 ({}) should be <= max ({})",
            stats.p99_us,
            stats.max_us
        );
        assert_eq!(stats.max_us, 1000);
        // With 1..=1000 uniformly spaced, p50 should be around 500, p95 around 950, p99 around 990
        assert!((490..=515).contains(&stats.p50_us));
        assert!((940..=960).contains(&stats.p95_us));
        assert!((980..=995).contains(&stats.p99_us));

        let json = recorder.stats_json();
        let parsed: LatencyStats = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, stats);
    }

    #[test]
    fn ring_buffer_wraparound_maintains_latest_capacity() {
        let mut recorder = LatencyRecorder::with_capacity(10);
        for i in 1..=20 {
            recorder.record_us(i);
        }
        assert_eq!(recorder.frame_count(), 20);
        let stats = recorder.stats();
        assert_eq!(stats.frames, 20);
        // The buffer contains 11..=20
        assert_eq!(stats.max_us, 20);
        assert!(stats.p50_us >= 15);
    }
}
