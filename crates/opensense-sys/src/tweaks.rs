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
    /// Processor performance boost mode (see `power::BOOST_MODES`).
    pub const CPU_BOOST: (u128, u128) =
        (0x54533251_82be_4824_96c1_47b60b740d00, 0xbe337238_0d82_4146_a960_4f3749d470c7);
    /// Power scheme personality (0 High performance, 1 Power saver,
    /// 2 Balanced).
    pub const PERSONALITY: (u128, u128) =
        (0xfea3413e_7e05_4911_9a71_700331f1c294, 0x245d8541_3943_4422_b025_13a784f679b7);
}

/// Default plan of each scheme personality (High performance, Power saver,
/// Balanced); `PowerRead*DefaultIndex` takes these.
const PERSONALITY_PLANS: [u128; 3] = [
    0x8c5e7fda_e8bf_4a96_9a85_a6e23a8c635c,
    0xa1841308_3541_4fab_bc81_f71556f20b4a,
    0x381b4222_f694_41f0_9685_ff5bb260df2e,
];

/// The personality plan for a personality value (Balanced when unknown).
fn personality_plan(personality: Option<u32>) -> u128 {
    personality.and_then(|p| PERSONALITY_PLANS.get(p as usize).copied()).unwrap_or(PERSONALITY_PLANS[2])
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
        PowerGetActiveScheme, PowerReadACDefaultIndex, PowerReadACValueIndex, PowerReadDCDefaultIndex,
        PowerReadDCValueIndex, PowerSetActiveScheme, PowerWriteACValueIndex, PowerWriteDCValueIndex,
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

    /// GUID of the active power plan.
    pub fn active_plan() -> Result<u128, SysError> {
        active().map(|g| g.to_u128())
    }

    /// (AC, DC) value of a power setting in the active plan.
    pub fn read(s: (u128, u128)) -> Result<(u32, u32), SysError> {
        read_in(active()?, s)
    }

    /// (AC, DC) value of a power setting in the plan `plan`.
    pub fn read_plan(plan: u128, s: (u128, u128)) -> Result<(u32, u32), SysError> {
        read_in(GUID::from_u128(plan), s)
    }

    /// (AC, DC) Windows default of a power setting for a plan of the same
    /// kind (personality) as `plan`.
    pub fn read_default(plan: u128, s: (u128, u128)) -> Result<(u32, u32), SysError> {
        let personality = read_plan(plan, setting::PERSONALITY).ok().map(|v| v.0);
        let base = GUID::from_u128(personality_plan(personality));
        let (sub, set) = (GUID::from_u128(s.0), GUID::from_u128(s.1));
        let (mut ac, mut dc) = (0u32, 0u32);
        unsafe {
            if PowerReadACDefaultIndex(None, &base, Some(&sub), &set, &mut ac) != 0
                || PowerReadDCDefaultIndex(None, &base, Some(&sub), &set, &mut dc) != 0
            {
                return Err(SysError::Win("cannot read power setting default".into()));
            }
        }
        Ok((ac, dc))
    }

    /// Writes the AC and DC value of a power setting in the plan `plan`
    /// (re-activated when it is the active one). Unchanged values are not
    /// written.
    pub fn write_plan(plan: u128, s: (u128, u128), ac: Option<u32>, dc: Option<u32>) -> Result<(), SysError> {
        write_in(GUID::from_u128(plan), s, ac, dc)
    }

    fn read_in(scheme: GUID, s: (u128, u128)) -> Result<(u32, u32), SysError> {
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
        if s == setting::STANDBY_NETWORK && ac == Some(0) && dc == Some(0) {
            crate::originals::remember_standby_network();
        }
        write_in(active()?, s, ac, dc)
    }

    fn write_in(scheme: GUID, s: (u128, u128), ac: Option<u32>, dc: Option<u32>) -> Result<(), SysError> {
        let current = read_in(scheme, s).ok();
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
            // Re-activating applies the change; another plan stays inactive.
            if dirty && active().is_ok_and(|a| a == scheme) {
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
pub use imp::{active_plan, idle_seconds, monitor_count, read, read_default, read_plan, write, write_plan};

#[cfg(not(windows))]
pub fn read(_s: (u128, u128)) -> Result<(u32, u32), SysError> {
    Err(SysError::Unsupported)
}
#[cfg(not(windows))]
pub fn active_plan() -> Result<u128, SysError> {
    Err(SysError::Unsupported)
}
#[cfg(not(windows))]
pub fn read_plan(_plan: u128, _s: (u128, u128)) -> Result<(u32, u32), SysError> {
    Err(SysError::Unsupported)
}
#[cfg(not(windows))]
pub fn read_default(_plan: u128, _s: (u128, u128)) -> Result<(u32, u32), SysError> {
    let _ = personality_plan;
    Err(SysError::Unsupported)
}
#[cfg(not(windows))]
pub fn write_plan(_plan: u128, _s: (u128, u128), _ac: Option<u32>, _dc: Option<u32>) -> Result<(), SysError> {
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

/// Service start types (`SERVICE_*_START` values of the service control
/// manager).
pub mod start_type {
    pub const AUTO: u32 = 2;
    pub const DEMAND: u32 = 3;
    pub const DISABLED: u32 = 4;
}

/// Start type each stopped vendor service had before OpenSense stopped it,
/// keyed by service name; restored when the services are started again.
pub type SavedStartTypes = std::collections::BTreeMap<String, u32>;

/// What stopping a service with start type `current` does: the start type
/// to remember (if any) and the one to set (if any). Only automatic start
/// is lowered to manual; a manual or disabled service keeps its type. An
/// already remembered type wins over the manual type OpenSense set itself.
fn plan_stop(current: u32, saved: Option<u32>) -> (Option<u32>, Option<u32>) {
    let remember = if current == start_type::DEMAND { saved } else { Some(current) };
    let set = (current == start_type::AUTO).then_some(start_type::DEMAND);
    (remember, set)
}

/// Vendor services that Acer's installers register with automatic start.
/// Assumed for all of [`ACER_SERVICES`]; not confirmed on every model.
const INSTALLED_AUTO: [&str; 7] = ACER_SERVICES;

/// What starting the service `name` does: the start type to restore (if it
/// differs from `current`) and whether to start it. A remembered start type
/// wins. With nothing remembered, a manual service that Acer installs as
/// automatic goes back to automatic: OpenSense 0.2.2 lowered it to manual
/// without remembering the type. A service that was disabled before (or is
/// disabled with nothing remembered) is not started.
fn plan_start(name: &str, current: u32, saved: Option<u32>) -> (Option<u32>, bool) {
    // Service names are case-insensitive in the service control manager.
    let installed_auto = INSTALLED_AUTO.iter().any(|n| n.eq_ignore_ascii_case(name));
    let target =
        saved.unwrap_or(if current == start_type::DEMAND && installed_auto { start_type::AUTO } else { current });
    ((target != current).then_some(target), target != start_type::DISABLED)
}

#[cfg(windows)]
mod svc {
    use windows::core::{Error, PCWSTR};
    use windows::Win32::Foundation::{ERROR_SERVICE_ALREADY_RUNNING, ERROR_SERVICE_DOES_NOT_EXIST};
    use windows::Win32::System::Services::*;

    /// A service control manager handle, closed when dropped.
    pub struct Sc(SC_HANDLE);

    impl Drop for Sc {
        fn drop(&mut self) {
            let _ = unsafe { CloseServiceHandle(self.0) };
        }
    }

    pub fn manager() -> Result<Sc, Error> {
        unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT) }.map(Sc)
    }

    pub const QUERY: u32 = SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG;
    pub const CONTROL: u32 = QUERY | SERVICE_CHANGE_CONFIG | SERVICE_START | SERVICE_STOP;

    /// `Ok(None)` = not installed. Decided by the error code, not by text.
    pub fn open(scm: &Sc, name: &str, access: u32) -> Result<Option<Sc>, Error> {
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        match unsafe { OpenServiceW(scm.0, PCWSTR(wide.as_ptr()), access) } {
            Ok(h) => Ok(Some(Sc(h))),
            Err(e) if e.code() == ERROR_SERVICE_DOES_NOT_EXIST.to_hresult() => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn running(s: &Sc) -> bool {
        let mut st = SERVICE_STATUS::default();
        unsafe { QueryServiceStatus(s.0, &mut st) }.is_ok() && st.dwCurrentState == SERVICE_RUNNING
    }

    pub fn start_type(s: &Sc) -> Result<u32, Error> {
        let mut need = 0u32;
        let _ = unsafe { QueryServiceConfigW(s.0, None, 0, &mut need) };
        // u64 elements keep the buffer aligned for the struct's pointers.
        let mut buf = vec![0u64; (need as usize).div_ceil(8).max(8)];
        let size = (buf.len() * 8) as u32;
        let cfg = buf.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>();
        unsafe {
            QueryServiceConfigW(s.0, Some(cfg), size, &mut need)?;
            Ok((*cfg).dwStartType.0)
        }
    }

    pub fn set_start_type(s: &Sc, t: u32) -> Result<(), Error> {
        let none = PCWSTR::null();
        unsafe {
            ChangeServiceConfigW(
                s.0,
                ENUM_SERVICE_TYPE(SERVICE_NO_CHANGE),
                SERVICE_START_TYPE(t),
                SERVICE_ERROR(SERVICE_NO_CHANGE),
                none,
                none,
                None,
                none,
                none,
                none,
                none,
            )
        }
    }

    pub fn start(s: &Sc) -> Result<(), Error> {
        match unsafe { StartServiceW(s.0, None) } {
            Err(e) if e.code() == ERROR_SERVICE_ALREADY_RUNNING.to_hresult() => Ok(()),
            r => r,
        }
    }

    pub fn stop(s: &Sc) -> Result<(), Error> {
        let mut st = SERVICE_STATUS::default();
        unsafe { ControlService(s.0, SERVICE_CONTROL_STOP, &mut st) }
    }
}

/// Vendor services matching `keep`, queried through the service control
/// manager (works on every Windows display language).
#[cfg(windows)]
fn services_where(keep: impl Fn(&svc::Sc) -> bool) -> Vec<&'static str> {
    let Ok(scm) = svc::manager() else { return Vec::new() };
    ACER_SERVICES
        .iter()
        .copied()
        .filter(|name| match svc::open(&scm, name, svc::QUERY) {
            Ok(Some(s)) => keep(&s),
            Ok(None) => false,
            Err(e) => {
                tracing::debug!("cannot open service {name}: {e}");
                false
            }
        })
        .collect()
}

/// Vendor services that are installed and running.
pub fn running_services() -> Vec<&'static str> {
    #[cfg(windows)]
    return services_where(svc::running);
    #[cfg(not(windows))]
    Vec::new()
}

/// Installed vendor services (running or not).
pub fn installed_services() -> Vec<&'static str> {
    #[cfg(windows)]
    return services_where(|_| true);
    #[cfg(not(windows))]
    Vec::new()
}

/// Stops or starts the installed vendor services. Stopping lowers
/// automatic start to manual and remembers the previous start type in
/// `saved`; starting restores it (and forgets it), or with nothing
/// remembered puts a manual Acer service back to automatic (see
/// [`plan_start`]).
pub fn set_services_with(run: bool, saved: &mut SavedStartTypes) {
    #[cfg(windows)]
    {
        let scm = match svc::manager() {
            Ok(scm) => scm,
            Err(e) => {
                tracing::warn!("cannot open the service control manager: {e}");
                return;
            }
        };
        for name in ACER_SERVICES {
            let s = match svc::open(&scm, name, svc::CONTROL) {
                Ok(Some(s)) => s,
                Ok(None) => continue,
                Err(e) => {
                    tracing::warn!("cannot open service {name}: {e}");
                    continue;
                }
            };
            let current = match svc::start_type(&s) {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!("cannot read start type of {name}: {e}");
                    continue;
                }
            };
            if run {
                let (set, start) = plan_start(name, current, saved.get(name).copied());
                if let Some(t) = set {
                    if let Err(e) = svc::set_start_type(&s, t) {
                        tracing::warn!("cannot restore start type of {name}: {e}");
                        continue;
                    }
                }
                saved.remove(name);
                if !start {
                    tracing::info!("{name} is disabled; not starting it");
                } else if let Err(e) = svc::start(&s) {
                    tracing::warn!("cannot start {name}: {e}");
                }
            } else {
                let (remember, set) = plan_stop(current, saved.get(name).copied());
                if let Some(t) = remember {
                    saved.insert(name.to_string(), t);
                }
                if svc::running(&s) {
                    if let Err(e) = svc::stop(&s) {
                        tracing::warn!("cannot stop {name}: {e}");
                    }
                }
                if let Some(t) = set {
                    if let Err(e) = svc::set_start_type(&s, t) {
                        tracing::warn!("cannot change start type of {name}: {e}");
                    }
                }
            }
        }
    }
    #[cfg(not(windows))]
    let _ = (run, saved);
}

/// [`set_services_with`], remembering the start types in the OpenSense
/// settings (`service_start_types`).
pub fn set_services(run: bool) {
    let mut saved = crate::config::AppConfig::load().service_start_types;
    set_services_with(run, &mut saved);
    if let Err(e) = crate::config::AppConfig::update(|c| c.service_start_types = saved) {
        tracing::warn!("cannot save service start types: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::start_type::*;
    use super::*;

    #[test]
    fn stop_remembers_and_lowers_only_auto() {
        assert_eq!(plan_stop(AUTO, None), (Some(AUTO), Some(DEMAND)));
        assert_eq!(plan_stop(DEMAND, None), (None, None));
        assert_eq!(plan_stop(DISABLED, None), (Some(DISABLED), None));
        // Stopped twice: the manual type OpenSense set does not overwrite.
        assert_eq!(plan_stop(DEMAND, Some(AUTO)), (Some(AUTO), None));
    }

    #[test]
    fn personality_picks_default_plan() {
        assert_eq!(personality_plan(Some(0)), 0x8c5e7fda_e8bf_4a96_9a85_a6e23a8c635c);
        assert_eq!(personality_plan(Some(1)), 0xa1841308_3541_4fab_bc81_f71556f20b4a);
        assert_eq!(personality_plan(Some(2)), 0x381b4222_f694_41f0_9685_ff5bb260df2e);
        assert_eq!(personality_plan(Some(9)), personality_plan(None));
        assert_eq!(personality_plan(None), 0x381b4222_f694_41f0_9685_ff5bb260df2e);
    }

    #[test]
    fn start_restores_previous_type() {
        assert_eq!(plan_start("PSSvc", DEMAND, Some(AUTO)), (Some(AUTO), true));
        assert_eq!(plan_start("PSSvc", AUTO, None), (None, true));
        assert_eq!(plan_start("PSSvc", DISABLED, None), (None, false));
        assert_eq!(plan_start("PSSvc", DISABLED, Some(DISABLED)), (None, false));
        // A remembered type always wins, also a remembered manual one.
        assert_eq!(plan_start("PSSvc", DEMAND, Some(DEMAND)), (None, true));
        assert_eq!(plan_start("PSSvc", AUTO, Some(DEMAND)), (Some(DEMAND), true));
        // Remembered as disabled: put back, not started.
        assert_eq!(plan_start("PSSvc", DEMAND, Some(DISABLED)), (Some(DISABLED), false));
    }

    #[test]
    fn start_without_memory_returns_acer_services_to_auto() {
        // Stopped by 0.2.2, which set manual without remembering.
        for name in ACER_SERVICES {
            assert_eq!(plan_start(name, DEMAND, None), (Some(AUTO), true), "{name}");
        }
        // Not a known Acer service: left as it is.
        assert_eq!(plan_start("Spooler", DEMAND, None), (None, true));
        // Disabled with nothing remembered stays disabled.
        assert_eq!(plan_start("AcerQAAgent", DISABLED, None), (None, false));
        // Service names are case-insensitive.
        assert_eq!(plan_start("pssvc", DEMAND, None), (Some(AUTO), true));
    }
}
