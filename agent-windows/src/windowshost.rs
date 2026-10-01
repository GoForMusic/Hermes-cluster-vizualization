//! The CPU and memory of a Windows machine, from `kernel32` only (`GetSystemTimes`, `GlobalMemoryStatusEx`): nothing that a minimal Nano Server
//! image might not have. Only Windows can be read; elsewhere the sampler exists so that the project builds, vets and tests everywhere, and says
//! so when it is used.

use anyhow::Result;
#[cfg(windows)]
use hermes_agentkit::host::CpuTracker;
use hermes_agentkit::host::{HostSample, ISampler};

#[derive(Default)]
pub struct WindowsSampler {
    #[cfg(windows)]
    cpu: CpuTracker,
}

#[cfg(windows)]
mod win32 {
    //! The two Win32 calls. This is the only place in the workspace that needs `unsafe`.
    #![allow(unsafe_code)]

    use anyhow::{Result, bail};
    use hermes_agentkit::host::CpuTimes;
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::GetSystemTimes;

    fn filetime(f: FILETIME) -> u64 {
        (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime)
    }

    /// Cumulative CPU time. Kernel time already includes idle time.
    pub fn cpu_times() -> Result<CpuTimes> {
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut idle, mut kernel, mut user) = (zero, zero, zero);
        // SAFETY: the three pointers are to live, writable FILETIME values, which is all the call needs.
        if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
            bail!("GetSystemTimes failed: {}", std::io::Error::last_os_error());
        }
        Ok(CpuTimes {
            total: filetime(kernel) + filetime(user),
            idle: filetime(idle),
        })
    }

    /// Physical memory in bytes: (in use, total).
    pub fn memory() -> Option<(u64, u64)> {
        // SAFETY: MEMORYSTATUSEX is a plain C struct of integers, for which all zeroes is a valid value.
        let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        status.dwLength = u32::try_from(std::mem::size_of::<MEMORYSTATUSEX>()).ok()?;
        // SAFETY: the pointer is to a live MEMORYSTATUSEX whose length field is set, as the call requires.
        (unsafe { GlobalMemoryStatusEx(&mut status) } != 0).then(|| {
            (
                status.ullTotalPhys - status.ullAvailPhys,
                status.ullTotalPhys,
            )
        })
    }
}

#[cfg(windows)]
impl ISampler for WindowsSampler {
    fn sample(&mut self) -> Result<HostSample> {
        let cpu = self.cpu.update(win32::cpu_times()?);
        let mib = |bytes: u64| bytes as f64 / (1u64 << 20) as f64;
        let (used, total) = win32::memory().unwrap_or_default();
        Ok(HostSample {
            cpu,
            mem_used_mib: mib(used),
            mem_total_mib: mib(total),
        })
    }
}

#[cfg(not(windows))]
impl ISampler for WindowsSampler {
    fn sample(&mut self) -> Result<HostSample> {
        anyhow::bail!("the Windows sampler only works on Windows")
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn off_windows_it_says_it_cannot_read_anything() {
        assert!(
            WindowsSampler::default()
                .sample()
                .unwrap_err()
                .to_string()
                .contains("only works on Windows")
        );
    }
}
