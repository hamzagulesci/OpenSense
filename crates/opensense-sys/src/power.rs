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
        // 255 = unknown: report that rather than "on battery".
        if s.ACLineStatus > 1 {
            return Err(SysError::Win("AC line status unknown".into()));
        }
        Ok(PowerStatus {
            on_ac: s.ACLineStatus == 1,
            battery_percent: (s.BatteryLifePercent <= 100).then_some(s.BatteryLifePercent),
            charging: s.BatteryFlag != 255 && s.BatteryFlag & 8 != 0,
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

    pub(super) fn friendly_name(g: &GUID) -> String {
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

/// Windows "power mode" (Settings > System > Power), applied as a power
/// overlay on a balanced plan, or a whole plan such as High or Ultimate
/// Performance (Windows then fixes the power mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerMode {
    BestEfficiency,
    Balanced,
    BestPerformance,
    /// A power plan without power modes, by GUID.
    Plan(u128),
}

/// The classic High Performance plan.
pub const PLAN_HIGH: u128 = 0x8c5e7fda_e8bf_4a96_9a85_a6e23a8c635c;

impl PowerMode {
    /// The three power modes of a balanced plan.
    pub const OVERLAYS: [PowerMode; 3] = [PowerMode::BestEfficiency, PowerMode::Balanced, PowerMode::BestPerformance];

    /// English label of a power mode (plans are shown by their own name).
    pub fn label(self) -> &'static str {
        match self {
            PowerMode::BestEfficiency => "Best Power Efficiency",
            PowerMode::Balanced => "Balanced",
            PowerMode::BestPerformance => "Best Performance",
            PowerMode::Plan(_) => "Power plan",
        }
    }

    pub fn key(self) -> String {
        match self {
            PowerMode::BestEfficiency => "efficiency".into(),
            PowerMode::Balanced => "balanced".into(),
            PowerMode::BestPerformance => "performance".into(),
            PowerMode::Plan(g) => format!("plan:{g:032x}"),
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        match k {
            "high" => Some(PowerMode::Plan(PLAN_HIGH)),
            _ => match k.strip_prefix("plan:") {
                Some(hex) => u128::from_str_radix(hex, 16).ok().map(PowerMode::Plan),
                None => Self::OVERLAYS.into_iter().find(|m| m.key() == k),
            },
        }
    }
}

/// Values of the processor "performance boost mode" setting.
pub const BOOST_MODES: [&str; 7] = [
    "Disabled",
    "Enabled",
    "Aggressive",
    "Efficient Enabled",
    "Efficient Aggressive",
    "Aggressive at Guaranteed",
    "Efficient at Guaranteed",
];

#[cfg(windows)]
mod modes {
    use super::*;
    use windows::core::GUID;
    use windows::Win32::Foundation::{LocalFree, ERROR_SUCCESS, HLOCAL};
    use windows::Win32::System::Power::{
        PowerEnumerate, PowerGetActiveScheme, PowerReadACValueIndex, PowerReadDCValueIndex, PowerSetActiveScheme,
        PowerWriteACValueIndex, PowerWriteDCValueIndex, ACCESS_SCHEME,
    };

    const OVERLAY_EFFICIENCY: GUID = GUID::from_u128(0x961cc777_2547_4f9d_8174_7d86181b8a7a);
    const OVERLAY_BALANCED: GUID = GUID::zeroed();
    const OVERLAY_PERFORMANCE: GUID = GUID::from_u128(0xded574b5_45a0_4f42_8737_46345c09c238);
    const PLAN_BALANCED: GUID = GUID::from_u128(0x381b4222_f694_41f0_9685_ff5bb260df2e);
    const NO_SUBGROUP: GUID = GUID::from_u128(0xfea3413e_7e05_4911_9a71_700331f1c294);
    const PERSONALITY: GUID = GUID::from_u128(0x245d8541_3943_4422_b025_13a784f679b7);
    /// Personality value of plans built on Balanced.
    const BALANCED_PERSONALITY: u32 = 2;
    const SUB_PROCESSOR: GUID = GUID::from_u128(0x54533251_82be_4824_96c1_47b60b740d00);
    const BOOST_MODE: GUID = GUID::from_u128(0xbe337238_0d82_4146_a960_4f3749d470c7);

    // Undocumented powrprof exports used by the Settings app; not in the
    // import library, so they are looked up at run time.
    type RawProc = unsafe extern "system" fn() -> isize;
    type GetOverlay = unsafe extern "system" fn(*mut GUID) -> u32;
    type SetOverlay = unsafe extern "system" fn(GUID) -> u32;

    fn proc(name: windows::core::PCSTR) -> Option<unsafe extern "system" fn() -> isize> {
        use windows::core::w;
        use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
        let module = unsafe { LoadLibraryW(w!("powrprof.dll")) }.ok()?;
        unsafe { GetProcAddress(module, name) }
    }

    #[allow(non_snake_case)]
    unsafe fn PowerGetEffectiveOverlayScheme(guid: *mut GUID) -> u32 {
        match proc(windows::core::s!("PowerGetEffectiveOverlayScheme")) {
            Some(f) => unsafe { std::mem::transmute::<RawProc, GetOverlay>(f)(guid) },
            None => 1,
        }
    }

    #[allow(non_snake_case)]
    unsafe fn PowerSetActiveOverlayScheme(guid: GUID) -> u32 {
        match proc(windows::core::s!("PowerSetActiveOverlayScheme")) {
            Some(f) => unsafe { std::mem::transmute::<RawProc, SetOverlay>(f)(guid) },
            None => 1,
        }
    }

    fn active() -> Option<GUID> {
        unsafe {
            let mut p: *mut GUID = std::ptr::null_mut();
            if PowerGetActiveScheme(None, &mut p) != ERROR_SUCCESS || p.is_null() {
                return None;
            }
            let g = *p;
            let _ = LocalFree(HLOCAL(p.cast()));
            Some(g)
        }
    }

    /// Battery saver / Energy saver is on (Windows then owns the overlay).
    pub fn energy_saver() -> bool {
        use windows::core::w;
        use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD};
        let mut v = 0u32;
        let mut size = 4u32;
        let r = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                w!("System\\CurrentControlSet\\Control\\Power"),
                w!("EnergySaverState"),
                RRF_RT_REG_DWORD,
                None,
                Some(std::ptr::addr_of_mut!(v).cast()),
                Some(&mut size),
            )
        };
        r.is_ok() && v == 1
    }

    /// Windows offers power modes only on plans built on Balanced; on High
    /// or Ultimate Performance it fixes the mode itself.
    fn has_power_modes(plan: &GUID) -> bool {
        if *plan == PLAN_BALANCED {
            return true;
        }
        let mut v = 0u32;
        let rc = unsafe { PowerReadACValueIndex(None, Some(plan), Some(&NO_SUBGROUP), Some(&PERSONALITY), &mut v) };
        rc == ERROR_SUCCESS && v == BALANCED_PERSONALITY
    }

    /// Installed plans without power modes (High Performance, Ultimate
    /// Performance, vendor plans), with their names.
    pub fn plans_without_modes() -> Vec<(PowerMode, String)> {
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
            if !has_power_modes(&g) {
                out.push((PowerMode::Plan(g.to_u128()), super::imp::friendly_name(&g)));
            }
        }
        out
    }

    pub fn power_mode() -> Option<PowerMode> {
        let plan = active()?;
        if !has_power_modes(&plan) {
            return Some(PowerMode::Plan(plan.to_u128()));
        }
        let mut g = GUID::zeroed();
        if unsafe { PowerGetEffectiveOverlayScheme(&mut g) } != 0 {
            return None;
        }
        Some(match g {
            OVERLAY_EFFICIENCY => PowerMode::BestEfficiency,
            OVERLAY_PERFORMANCE => PowerMode::BestPerformance,
            _ => PowerMode::Balanced,
        })
    }

    pub fn set_power_mode(m: PowerMode) -> Result<(), SysError> {
        let current = active();
        if let PowerMode::Plan(g) = m {
            let g = GUID::from_u128(g);
            if current != Some(g) {
                check(unsafe { PowerSetActiveScheme(None, Some(&g)) }.0, "PowerSetActiveScheme")?;
            }
            return Ok(());
        }
        // Power modes need a balanced plan; leave High / Ultimate Performance.
        if !current.is_some_and(|c| has_power_modes(&c)) {
            check(unsafe { PowerSetActiveScheme(None, Some(&PLAN_BALANCED)) }.0, "PowerSetActiveScheme")?;
        }
        if energy_saver() {
            return Ok(());
        }
        let target = match m {
            PowerMode::BestEfficiency => OVERLAY_EFFICIENCY,
            PowerMode::BestPerformance => OVERLAY_PERFORMANCE,
            _ => OVERLAY_BALANCED,
        };
        let mut g = GUID::zeroed();
        if unsafe { PowerGetEffectiveOverlayScheme(&mut g) } == 0 && g == target {
            return Ok(());
        }
        check(unsafe { PowerSetActiveOverlayScheme(target) }, "PowerSetActiveOverlayScheme")
    }

    pub fn cpu_boost() -> Option<u8> {
        let scheme = active()?;
        let mut v = 0u32;
        let rc = unsafe { PowerReadACValueIndex(None, Some(&scheme), Some(&SUB_PROCESSOR), Some(&BOOST_MODE), &mut v) };
        (rc == ERROR_SUCCESS).then_some(v.min(6) as u8)
    }

    pub fn set_cpu_boost(v: u8) -> Result<(), SysError> {
        let scheme = active().ok_or_else(|| SysError::Win("no active power plan".into()))?;
        let v = u32::from(v.min(6));
        // Both the plugged-in and the battery value must already match.
        let (mut ac, mut dc) = (u32::MAX, u32::MAX);
        let same = unsafe {
            PowerReadACValueIndex(None, Some(&scheme), Some(&SUB_PROCESSOR), Some(&BOOST_MODE), &mut ac)
                == ERROR_SUCCESS
                && PowerReadDCValueIndex(None, Some(&scheme), Some(&SUB_PROCESSOR), Some(&BOOST_MODE), &mut dc) == 0
        } && ac == v
            && dc == v;
        if same {
            return Ok(());
        }
        // Remember the plan's own value for `opensense --restore-system`.
        if ac != u32::MAX && dc != u32::MAX {
            crate::originals::remember_cpu_boost(scheme.to_u128(), (ac, dc));
        }
        unsafe {
            check(PowerWriteACValueIndex(None, &scheme, Some(&SUB_PROCESSOR), Some(&BOOST_MODE), v).0, "write AC")?;
            check(PowerWriteDCValueIndex(None, &scheme, Some(&SUB_PROCESSOR), Some(&BOOST_MODE), v), "write DC")?;
            check(PowerSetActiveScheme(None, Some(&scheme)).0, "PowerSetActiveScheme")?;
        }
        Ok(())
    }

    fn check(rc: u32, what: &str) -> Result<(), SysError> {
        if rc == 0 {
            Ok(())
        } else {
            Err(SysError::Win(format!("{what}: {rc}")))
        }
    }
}

#[cfg(windows)]
pub use modes::{cpu_boost, energy_saver, plans_without_modes, power_mode, set_cpu_boost, set_power_mode};

#[cfg(not(windows))]
pub fn plans_without_modes() -> Vec<(PowerMode, String)> {
    Vec::new()
}
#[cfg(not(windows))]
pub fn power_mode() -> Option<PowerMode> {
    None
}
#[cfg(not(windows))]
pub fn set_power_mode(_m: PowerMode) -> Result<(), SysError> {
    Err(SysError::Unsupported)
}
#[cfg(not(windows))]
pub fn cpu_boost() -> Option<u8> {
    None
}
#[cfg(not(windows))]
pub fn set_cpu_boost(_v: u8) -> Result<(), SysError> {
    Err(SysError::Unsupported)
}
#[cfg(not(windows))]
pub fn energy_saver() -> bool {
    false
}

#[cfg(windows)]
pub use imp::{plans, set_active, status};

/// Restarts Windows immediately (used after a GPU mode change).
pub fn restart_now() -> Result<(), SysError> {
    let mut cmd = std::process::Command::new("shutdown.exe");
    cmd.args(["/r", "/t", "0"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let status = cmd.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(SysError::Win(format!("shutdown.exe exited with {status}")))
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_mode_keys_round_trip() {
        for m in PowerMode::OVERLAYS.into_iter().chain([PowerMode::Plan(PLAN_HIGH), PowerMode::Plan(7)]) {
            assert_eq!(PowerMode::from_key(&m.key()), Some(m));
        }
        // Older settings stored the High Performance plan as "high".
        assert_eq!(PowerMode::from_key("high"), Some(PowerMode::Plan(PLAN_HIGH)));
        assert_eq!(PowerMode::from_key("nonsense"), None);
    }
}
