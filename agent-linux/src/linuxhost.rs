//! The CPU and memory of a Linux machine, from `/proc`.

use anyhow::{Context, Result, bail};
use hermes_agentkit::host::{CpuTimes, CpuTracker, HostSample, ISampler};

/// The aggregate `cpu` line of `/proc/stat` (host-wide, even inside a container).
fn parse_cpu_times(stat: &str) -> Result<CpuTimes> {
    for line in stat.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 5 || fields[0] != "cpu" {
            continue;
        }
        let mut times = CpuTimes { total: 0, idle: 0 };
        for (i, v) in fields[1..].iter().enumerate() {
            let n: u64 = v
                .parse()
                .with_context(|| format!("bad number {v:?} in /proc/stat"))?;
            if i < 8 {
                // user nice system idle iowait irq softirq steal
                times.total += n;
            }
            if i == 3 || i == 4 {
                // idle and iowait
                times.idle += n;
            }
        }
        return Ok(times);
    }
    bail!("no cpu line in /proc/stat")
}

/// Used memory (`MemTotal` - `MemAvailable`) and `MemTotal`, in MiB.
fn parse_meminfo(meminfo: &str) -> (f64, f64) {
    let (mut total, mut available) = (0.0, 0.0);
    for line in meminfo.lines() {
        let mut fields = line.split_whitespace();
        let (Some(key), Some(value)) = (fields.next(), fields.next()) else {
            continue;
        };
        let v: f64 = value.parse().unwrap_or(0.0);
        match key {
            "MemTotal:" => total = v,
            "MemAvailable:" => available = v,
            _ => {}
        }
    }
    ((total - available) / 1024.0, total / 1024.0)
}

#[derive(Default)]
pub struct LinuxSampler {
    cpu: CpuTracker,
}

impl ISampler for LinuxSampler {
    fn sample(&mut self) -> Result<HostSample> {
        let cur = parse_cpu_times(
            &std::fs::read_to_string("/proc/stat").context("cannot read /proc/stat")?,
        )?;
        let cpu = self.cpu.update(cur);
        let (mem_used_mib, mem_total_mib) = std::fs::read_to_string("/proc/meminfo")
            .map(|m| parse_meminfo(&m))
            .unwrap_or_default();
        Ok(HostSample {
            cpu,
            mem_used_mib,
            mem_total_mib,
        })
    }
}

#[cfg(test)]
#[path = "../tests/unit/linuxhost.rs"]
mod tests;
