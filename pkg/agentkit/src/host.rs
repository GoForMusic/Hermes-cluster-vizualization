//! How an agent reads the machine it runs on. The trait lives here so that collectors do not depend on any operating system; each agent
//! (Linux, Windows) provides its own [`ISampler`].

use anyhow::Result;

/// One reading of the host.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HostSample {
    /// The share of the last interval the CPU was busy, in percent. `None` on the first reading: there is nothing to compare with yet.
    pub cpu: Option<f64>,
    pub mem_used_mib: f64,
    pub mem_total_mib: f64,
}

/// Reads the host's CPU and memory.
pub trait ISampler: Send {
    fn sample(&mut self) -> Result<HostSample>;
}

/// Cumulative CPU counters: `total` is all CPU time, `idle` the part spent doing nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuTimes {
    pub total: u64,
    pub idle: u64,
}

/// The busy share between two readings of the counters, in percent. Both operating systems expose such counters, so the arithmetic is shared.
pub fn cpu_percent(prev: CpuTimes, cur: CpuTimes) -> f64 {
    let dt = cur.total as f64 - prev.total as f64;
    if dt <= 0.0 || cur.total < prev.total {
        return 0.0;
    }
    (1.0 - (cur.idle as f64 - prev.idle as f64) / dt) * 100.0
}

/// Keeps the one reading `cpu_percent` needs a previous one to compare against. Both `ISampler` impls (Linux, Windows) read counters
/// their own way but need the exact same "remember the last reading, `None` on the first one" bookkeeping around them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuTracker(Option<CpuTimes>);

impl CpuTracker {
    /// The busy share since the last call, or `None` on the first one. Always remembers `cur` for next time.
    pub fn update(&mut self, cur: CpuTimes) -> Option<f64> {
        self.0.replace(cur).map(|prev| cpu_percent(prev, cur))
    }
}

#[cfg(test)]
#[path = "../tests/unit/host.rs"]
mod tests;
