//! The Engine's `/containers/{id}/stats` shape, and the pure CPU/memory math over it — no I/O, no concurrency, just arithmetic on
//! two samples. `task_stats` is what actually fetches these, concurrently, for every task on the node.

use std::collections::HashMap;

use serde::Deserialize;

const MIB: f64 = (1u64 << 20) as f64;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ContainerStats {
    /// Windows: sample times replace `system_cpu_usage`.
    pub read: String,
    pub preread: String,
    pub cpu_stats: CpuStats,
    pub precpu_stats: CpuStats,
    /// Cumulative bytes per network interface of the container.
    pub networks: HashMap<String, NetStats>,
    pub memory_stats: MemoryStats,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct CpuStats {
    pub cpu_usage: CpuUsage,
    pub system_cpu_usage: u64,
    pub online_cpus: u32,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct CpuUsage {
    pub total_usage: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct NetStats {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct MemoryStats {
    pub usage: u64,
    pub stats: HashMap<String, u64>,
    /// Windows
    pub privateworkingset: u64,
}

/// The container's CPU use in millicores (1000 is one full core), like `docker stats` and `kubectl top`.
pub fn cpu_milli(s: &ContainerStats) -> f64 {
    let cpu =
        s.cpu_stats.cpu_usage.total_usage as f64 - s.precpu_stats.cpu_usage.total_usage as f64;
    let system = s.cpu_stats.system_cpu_usage as f64 - s.precpu_stats.system_cpu_usage as f64;
    if cpu <= 0.0 || system <= 0.0 {
        return 0.0;
    }
    let n = if s.cpu_stats.online_cpus == 0 {
        1
    } else {
        s.cpu_stats.online_cpus
    };
    cpu / system * f64::from(n) * 1000.0
}

pub(crate) fn parse_ms(text: &str) -> Option<i64> {
    text.parse::<jiff::Timestamp>()
        .ok()
        .map(|t| t.as_millisecond())
}

/// The same figure for a Windows container: CPU time is in 100 ns units and the wall clock between the two samples is used instead of the
/// system CPU counter (this mirrors `docker stats` on Windows).
pub fn cpu_milli_windows(s: &ContainerStats) -> f64 {
    let (Some(read), Some(pre)) = (parse_ms(&s.read), parse_ms(&s.preread)) else {
        return 0.0;
    };
    let elapsed = (read - pre) as f64 * 10_000.0; // 100 ns intervals between the samples
    let used =
        s.cpu_stats.cpu_usage.total_usage as f64 - s.precpu_stats.cpu_usage.total_usage as f64;
    if elapsed <= 0.0 || used <= 0.0 {
        return 0.0;
    }
    used / elapsed * 1000.0
}

/// Working-set memory: usage minus reclaimable page cache (cgroup v2 `inactive_file`, v1 `cache`).
pub fn mem_mib(s: &ContainerStats) -> f64 {
    let mut used = s.memory_stats.usage;
    for key in ["inactive_file", "total_inactive_file", "cache"] {
        if let Some(cache) = s.memory_stats.stats.get(key).filter(|c| **c <= used) {
            used -= cache;
            break;
        }
    }
    used as f64 / MIB
}

/// The private working set: the number Windows reports for a container.
pub fn mem_mib_windows(s: &ContainerStats) -> f64 {
    s.memory_stats.privateworkingset as f64 / MIB
}

#[cfg(test)]
#[path = "../../tests/unit/stats.rs"]
mod tests;
