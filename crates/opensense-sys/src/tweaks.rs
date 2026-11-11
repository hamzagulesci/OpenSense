//! Windows settings offered in the Extra window: PCIe link power, network
//! in Modern Standby, lid action (clamshell), hibernate timeout, external
//! displays and the vendor services that compete for the firmware.

use crate::SysError;

/// (subgroup, setting) GUIDs of power settings, as u128.
pub mod setting {
    pub const ASPM: (u128, u128) = (0x501a4d13_42af_4429_9fd1_a8218c268e20, 0xee12f906_d277_404b_b6da_e5fa1a576df5);
    /// "No subgroup" holds the Modern Standby connectivity setting.
    pub const STANDBY_NETWORK: (u128, u128) =
        (0xfea3413e_7e05_4911_9a71_700331f1c294, 0xf15576e8_98b7_4186_b944_eafa664402d9);
    pub const LID_ACTION: (u128, u128) =
        (0x4f971e89_eebd_4455_a8de_9e59040e7347, 0x5ca83367_6e45_459f_a27b_476b1d01c936);
    pub const HIBERNATE_AFTER: (u128, u128) =
        (0x238c9fa8_0aad_41ed_83f4_97be242c8f20, 0x9d7815a6_7ee4_497e_8888_515a05f02364);
    /// Sleep after (seconds idle; 0 = never).
    pub const SLEEP_AFTER: (u128, u128) =
        (0x238c9fa8_0aad_41ed_83f4_97be242c8f20, 0x29f6c1db_86da_48c5_9fdb_f2b67b1f44da);
    /// Turn off the display after (seconds idle; 0 = never).
    pub const DISPLAY_OFF_AFTER: (u128, u128) =
        (0x7516b95f_f776_4464_8c53_06167f40cc99, 0x3c0bc021_c8a8_4e07_a973_6b14cbcb2b7e);
}

/// Vendor services that drive the same firmware interface as OpenSense.
pub const ACER_SERVICES: [&str; 7] =
    ["PSSvc", "AcerQAAgent", "ACCSvc", "ACCStd", "AcerSysMonitorService", "AcerLightingService", "AcerDIAgent"];

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::GUID;
    use windows::Win32::Foundation::{LocalFree, ERROR_SUCCESS, HLOCAL};
    use windows::Win32::System::Power::{
        PowerGetActiveScheme, PowerReadACValueIndex, PowerReadDCValueIndex, PowerSetActiveScheme,
        PowerWriteACValueIndex, PowerWriteDCValueIndex,
    };

    fn active() -> Result<GUID, SysError> {
        unsafe {
            let mut p: *mut GUID = std::ptr::null_mut();
            if PowerGetActiveScheme(None, &mut p) != ERROR_SUCCESS || p.is_null() {
                return Err(SysError::Win("no active power plan".into()));
            }
            let g = *p;
            let _ = LocalFree(HLOCAL(p.cast()));
            Ok(g)
        }
    }

    /// (AC, DC) value of a power setting in the active plan.
    pub fn read(s: (u128, u128)) -> Result<(u32, u32), SysError> {
        let scheme = active()?;
        let (sub, set) = (GUID::from_u128(s.0), GUID::from_u128(s.1));
        let (mut ac, mut dc) = (0u32, 0u32);
        unsafe {
            if PowerReadACValueIndex(None, Some(&scheme), Some(&sub), Some(&set), &mut ac) != ERROR_SUCCESS
                || PowerReadDCValueIndex(None, Some(&scheme), Some(&sub), Some(&set), &mut dc) != 0
            {
                return Err(SysError::Win("cannot read power setting".into()));
            }
        }
        Ok((ac, dc))
    }

    /// Writes the AC and/or DC value of a power setting and re-activates the
    /// plan so it takes effect. Unchanged values are not written.
    pub fn write(s: (u128, u128), ac: Option<u32>, dc: Option<u32>) -> Result<(), SysError> {
        let current = read(s).ok();
        let scheme = active()?;
        let (sub, set) = (GUID::from_u128(s.0), GUID::from_u128(s.1));
        let mut dirty = false;
        unsafe {
            if let Some(v) = ac.filter(|&v| current.is_none_or(|c| c.0 != v)) {
                if PowerWriteACValueIndex(None, &scheme, Some(&sub), Some(&set), v) != ERROR_SUCCESS {
                    return Err(SysError::Win("cannot write power setting".into()));
                }
                dirty = true;
            }
            if let Some(v) = dc.filter(|&v| current.is_none_or(|c| c.1 != v)) {
                if PowerWriteDCValueIndex(None, &scheme, Some(&sub), Some(&set), v) != 0 {
                    return Err(SysError::Win("cannot write power setting".into()));
                }
                dirty = true;
            }
            if dirty {
                let _ = PowerSetActiveScheme(None, Some(&scheme));
            }
        }
        Ok(())
    }

    /// Monitors attached to the desktop.
    pub fn monitor_count() -> u32 {
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CMONITORS};
        unsafe { GetSystemMetrics(SM_CMONITORS) }.max(0) as u32
    }

    /// Seconds since the last keyboard or mouse input.
    pub fn idle_seconds() -> u32 {
        use windows::Win32::System::SystemInformation::GetTickCount;
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
        let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
        if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
            return 0;
        }
        unsafe { GetTickCount() }.wrapping_sub(info.dwTime) / 1000
    }
}

#[cfg(windows)]
pub use imp::{idle_seconds, monitor_count, read, write};

#[cfg(not(windows))]
pub fn read(_s: (u128, u128)) -> Result<(u32, u32), SysError> {
    Err(SysError::Unsupported)
}
#[cfg(not(windows))]
pub fn write(_s: (u128, u128), _ac: Option<u32>, _dc: Option<u32>) -> Result<(), SysError> {
    Err(SysError::Unsupported)
}
#[cfg(not(windows))]
pub fn monitor_count() -> u32 {
    1
}
#[cfg(not(windows))]
pub fn idle_seconds() -> u32 {
    0
}

fn sc(args: &[&str]) -> Option<String> {
    let mut cmd = std::process::Command::new("sc.exe");
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Vendor services that are installed and running.
pub fn running_services() -> Vec<&'static str> {
    ACER_SERVICES.iter().copied().filter(|s| sc(&["query", s]).is_some_and(|o| o.contains("RUNNING"))).collect()
}

/// Installed vendor services (running or not).
pub fn installed_services() -> Vec<&'static str> {
    ACER_SERVICES.iter().copied().filter(|s| sc(&["query", s]).is_some_and(|o| o.contains("STATE"))).collect()
}

/// Stops (and sets to manual start) or starts the installed vendor services.
pub fn set_services(run: bool) {
    for s in installed_services() {
        if run {
            let _ = sc(&["config", s, "start=", "auto"]);
            let _ = sc(&["start", s]);
        } else {
            let _ = sc(&["stop", s]);
            let _ = sc(&["config", s, "start=", "demand"]);
        }
    }
}
