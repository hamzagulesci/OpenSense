//! AC/battery status and power plans (`GetSystemPowerStatus`,
//! `PowerGetActiveScheme`, `PowerSetActiveScheme`, `PowerEnumerate`,
//! `PowerReadFriendlyName`).

use serde::{Deserialize, Serialize};

use crate::SysError;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerStatus {
    pub on_ac: bool,
    /// 0..=100, `None` when unknown / no battery.
    pub battery_percent: Option<u8>,
    pub charging: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerPlan {
    pub guid: String,
    pub name: String,
    pub active: bool,
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::core::GUID;
    use windows::Win32::Foundation::{LocalFree, ERROR_SUCCESS, HLOCAL};
    use windows::Win32::System::Power::{
        GetSystemPowerStatus, PowerEnumerate, PowerGetActiveScheme, PowerReadFriendlyName, PowerSetActiveScheme,
        ACCESS_SCHEME, SYSTEM_POWER_STATUS,
    };

    pub fn status() -> Result<PowerStatus, SysError> {
        let mut s = SYSTEM_POWER_STATUS::default();
        unsafe { GetSystemPowerStatus(&mut s)? };
        Ok(PowerStatus {
            on_ac: s.ACLineStatus == 1,
            battery_percent: (s.BatteryLifePercent <= 100).then_some(s.BatteryLifePercent),
            charging: s.BatteryFlag & 8 != 0,
        })
    }

    fn guid_string(g: &GUID) -> String {
        format!("{g:?}").to_ascii_lowercase()
    }

    fn active_guid() -> Result<GUID, SysError> {
        unsafe {
            let mut p: *mut GUID = std::ptr::null_mut();
            let rc = PowerGetActiveScheme(None, &mut p);
            if rc != ERROR_SUCCESS || p.is_null() {
                return Err(SysError::Win(format!("PowerGetActiveScheme: {rc:?}")));
            }
            let g = *p;
            let _ = LocalFree(HLOCAL(p.cast()));
            Ok(g)
        }
    }

    fn friendly_name(g: &GUID) -> String {
        unsafe {
            let mut size = 0u32;
            let _ = PowerReadFriendlyName(None, Some(g), None, None, None, &mut size);
            if size == 0 {
                return String::new();
            }
            let mut buf = vec![0u8; size as usize];
            if PowerReadFriendlyName(None, Some(g), None, None, Some(buf.as_mut_ptr()), &mut size) != ERROR_SUCCESS {
                return String::new();
            }
            let wide: Vec<u16> = buf.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            String::from_utf16_lossy(&wide).trim_end_matches('\0').to_owned()
        }
    }

    pub fn plans() -> Result<Vec<PowerPlan>, SysError> {
        let active = active_guid().ok();
        let mut out = Vec::new();
        for index in 0.. {
            let mut g = GUID::zeroed();
            let mut size = std::mem::size_of::<GUID>() as u32;
            let rc = unsafe {
                PowerEnumerate(None, None, None, ACCESS_SCHEME, index, Some((&mut g as *mut GUID).cast()), &mut size)
            };
            if rc != ERROR_SUCCESS {
                break;
            }
            out.push(PowerPlan { guid: guid_string(&g), name: friendly_name(&g), active: Some(g) == active });
        }
        Ok(out)
    }

    pub fn set_active(guid: &str) -> Result<(), SysError> {
        let hex: String = guid.trim().trim_matches(['{', '}']).chars().filter(|c| *c != '-').collect();
        let g = (hex.len() == 32)
            .then(|| u128::from_str_radix(&hex, 16).ok())
            .flatten()
            .map(GUID::from_u128)
            .ok_or_else(|| SysError::Config(format!("bad GUID {guid}")))?;
        let rc = unsafe { PowerSetActiveScheme(None, Some(&g)) };
        if rc == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(SysError::Win(format!("PowerSetActiveScheme: {rc:?}")))
        }
    }
}

#[cfg(windows)]
pub use imp::{plans, set_active, status};

#[cfg(not(windows))]
pub fn status() -> Result<PowerStatus, SysError> {
    Err(SysError::Unsupported)
}
#[cfg(not(windows))]
pub fn plans() -> Result<Vec<PowerPlan>, SysError> {
    Err(SysError::Unsupported)
}
#[cfg(not(windows))]
pub fn set_active(_guid: &str) -> Result<(), SysError> {
    Err(SysError::Unsupported)
}
