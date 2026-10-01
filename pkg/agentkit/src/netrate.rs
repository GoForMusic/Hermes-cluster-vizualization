//! Turns the cumulative network byte counters that kubelets and container engines report into throughput in Mb/s. Shared by the collectors:
//! what they read differs, the arithmetic does not.

use std::collections::{HashMap, HashSet};

/// One reading of a cumulative counter pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub rx: u64,
    pub tx: u64,
    /// When the counters were read, in milliseconds.
    pub at_ms: i64,
}

/// Remembers the previous sample of each thing it measures (a pod, a container).
#[derive(Debug, Default)]
pub struct Tracker {
    last: HashMap<String, Sample>,
}

fn mbps(bytes: u64, secs: f64) -> f64 {
    bytes as f64 * 8.0 / 1e6 / secs
}

impl Tracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records `s` and returns the throughput since the previous sample of the same key, in megabits per second (received, sent). `None`
    /// when there is nothing to compare with yet (the first sample), when time did not advance, or when a counter went backwards (the pod or
    /// container restarted): the new value is then the baseline.
    pub fn update(&mut self, key: &str, s: Sample) -> Option<(f64, f64)> {
        let prev = self.last.insert(key.to_string(), s)?;
        let secs = (s.at_ms - prev.at_ms) as f64 / 1000.0;
        if secs <= 0.0 || s.rx < prev.rx || s.tx < prev.tx {
            return None;
        }
        Some((mbps(s.rx - prev.rx, secs), mbps(s.tx - prev.tx, secs)))
    }

    /// Forgets everything that is not in `keys`, so that pods and containers that are gone do not pile up.
    pub fn keep(&mut self, keys: &HashSet<String>) {
        self.last.retain(|k, _| keys.contains(k));
    }
}

#[cfg(test)]
#[path = "../tests/unit/netrate.rs"]
mod tests;
