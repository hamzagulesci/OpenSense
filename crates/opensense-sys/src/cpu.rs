//! CPU load, clock and name straight from Windows (`GetSystemTimes`,
//! `CallNtPowerInformation`, the processor registry key). Avoids a general
//! system-information library and its allocations.

#[derive(Debug, Clone, Copy, Default)]
pub struct CpuSample {
    /// Total load, 0..=100.
    pub usage: f32,
    /// Average current core clock in MHz (0 when unavailable).
    pub frequency_mhz: u64,
}

#[derive(Default)]
pub struct CpuMonitor {
    /// (idle, kernel + user) at the previous sample, in 100 ns units.
    last: Option<(u64, u64)>,
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Power::{CallNtPowerInformation, ProcessorInformation};
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    use windows::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};
    use windows::Win32::System::Threading::GetSystemTimes;

    /// `PROCESSOR_POWER_INFORMATION` (not exported by the bindings).
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct ProcessorPowerInformation {
        number: u32,
        max_mhz: u32,
        current_mhz: u32,
        mhz_limit: u32,
        max_idle_state: u32,
        current_idle_state: u32,
    }

    fn ft(f: FILETIME) -> u64 {
        (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime)
    }

    impl CpuMonitor {
        pub fn new() -> Self {
            Self { last: Self::times() }
        }

        fn times() -> Option<(u64, u64)> {
            let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
            unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.ok()?;
            // Kernel time already includes idle time.
            Some((ft(idle), ft(kernel) + ft(user)))
        }

        pub fn brand(&self) -> String {
            let key = HSTRING::from(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0");
            let value = HSTRING::from("ProcessorNameString");
            let mut buf = [0u16; 128];
            let mut len = (buf.len() * 2) as u32;
            let rc = unsafe {
                RegGetValueW(
                    HKEY_LOCAL_MACHINE,
                    &key,
                    &value,
                    RRF_RT_REG_SZ,
                    None,
                    Some(buf.as_mut_ptr().cast()),
                    Some(&mut len),
                )
            };
            if rc.is_err() {
                return String::new();
            }
            let n = (len as usize / 2).saturating_sub(1);
            String::from_utf16_lossy(&buf[..n]).trim().to_owned()
        }

        fn frequency_mhz() -> u64 {
            let mut info = SYSTEM_INFO::default();
            unsafe { GetSystemInfo(&mut info) };
            let n = info.dwNumberOfProcessors.max(1) as usize;
            let mut buf = vec![ProcessorPowerInformation::default(); n];
            let size = (n * std::mem::size_of::<ProcessorPowerInformation>()) as u32;
            let status =
                unsafe { CallNtPowerInformation(ProcessorInformation, None, 0, Some(buf.as_mut_ptr().cast()), size) };
            if status.is_err() {
                return 0;
            }
            buf.iter().map(|p| u64::from(p.current_mhz)).sum::<u64>() / n as u64
        }

        pub fn sample(&mut self) -> CpuSample {
            let now = Self::times();
            let usage = match (self.last, now) {
                (Some((i0, t0)), Some((i1, t1))) if t1 > t0 => {
                    let busy = (t1 - t0).saturating_sub(i1 - i0);
                    (busy as f64 / (t1 - t0) as f64 * 100.0) as f32
                }
                _ => 0.0,
            };
            self.last = now;
            CpuSample { usage: usage.clamp(0.0, 100.0), frequency_mhz: Self::frequency_mhz() }
        }
    }
}

#[cfg(not(windows))]
impl CpuMonitor {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn brand(&self) -> String {
        String::new()
    }
    pub fn sample(&mut self) -> CpuSample {
        CpuSample::default()
    }
}
