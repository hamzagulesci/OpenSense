//! Windows settings as they were before OpenSense first changed them, so
//! `opensense --restore-system` (run by the uninstaller) can put them back.
//!
//! Kept in their own file next to `config.toml` (`originals.toml`), written
//! only here: the windows and the tray agent save the whole settings file
//! from copies they loaded earlier, which would drop a value remembered in
//! between.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::power::PowerMode;
use crate::tweaks::{self, setting};
use crate::SysError;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Originals {
    /// Modern Standby network (AC, DC) of the active plan before OpenSense
    /// first turned it off.
    pub standby_network: Option<[u32; 2]>,
    /// Processor boost mode (AC, DC) per power plan (GUID as 32 hex
    /// digits, see [`plan_key`]) before OpenSense first changed it there.
    pub cpu_boost: BTreeMap<String, [u32; 2]>,
    /// Active power plan ([`plan_key`]) before OpenSense first changed the
    /// plan or the power mode.
    pub power_plan: Option<String>,
    /// Power mode of that plan ([`PowerMode::key`] of an overlay), when the
    /// plan has power modes.
    pub power_mode: Option<String>,
}

/// The power plan and power mode to remember before OpenSense changes them:
/// the current ones, unless a plan is remembered already. `mode` is the
/// current [`crate::power::power_mode`]; only a power mode (overlay) is kept,
/// a plan without power modes is the plan itself.
fn power_to_remember(
    saved_plan: Option<&str>,
    plan: u128,
    mode: Option<PowerMode>,
) -> Option<(String, Option<String>)> {
    if saved_plan.is_some() {
        return None;
    }
    let overlay = mode.filter(|m| !matches!(m, PowerMode::Plan(_))).map(PowerMode::key);
    Some((plan_key(plan), overlay))
}

/// The remembered power plan and power mode, if a plan is remembered.
pub fn remembered_power(o: &Originals) -> Option<(u128, Option<PowerMode>)> {
    let plan = parse_plan_key(o.power_plan.as_deref()?)?;
    let mode = o.power_mode.as_deref().and_then(PowerMode::from_key).filter(|m| !matches!(m, PowerMode::Plan(_)));
    Some((plan, mode))
}

/// Called by [`crate::power::set_power_mode`] before it changes the active
/// plan or the power mode from `plan` / `mode`. Only the first value is kept.
pub(crate) fn remember_power_mode(plan: u128, mode: Option<PowerMode>) {
    if Originals::load().power_plan.is_some() {
        return;
    }
    let r = Originals::update(|o| {
        if let Some((p, m)) = power_to_remember(o.power_plan.as_deref(), plan, mode) {
            o.power_plan = Some(p);
            o.power_mode = m;
        }
    });
    if let Err(e) = r {
        tracing::warn!("cannot remember the power plan: {e}");
    }
}

/// Key of a power plan in [`Originals::cpu_boost`].
pub fn plan_key(plan: u128) -> String {
    format!("{plan:032x}")
}

/// Power plan GUID of a [`plan_key`].
pub fn parse_plan_key(key: &str) -> Option<u128> {
    u128::from_str_radix(key, 16).ok()
}

impl Originals {
    pub fn path() -> Option<PathBuf> {
        Some(crate::config::AppConfig::path()?.with_file_name("originals.toml"))
    }

    /// The remembered values; empty when nothing was remembered (or the
    /// file cannot be read).
    pub fn load() -> Self {
        Self::path().map_or_else(Self::default, |p| Self::load_from(&p))
    }

    fn load_from(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!("invalid {}: {e}", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    /// Loads the file, changes it with `f` and saves it when `f` changed
    /// anything, under a cross-process lock.
    pub fn update(f: impl FnOnce(&mut Originals)) -> Result<(), SysError> {
        let path = Self::path().ok_or_else(|| SysError::Config("no config directory".into()))?;
        Self::update_at(&path, f)
    }

    fn update_at(path: &Path, f: impl FnOnce(&mut Originals)) -> Result<(), SysError> {
        let _lock = Lock::acquire();
        let mut o = Self::load_from(path);
        let before = o.clone();
        f(&mut o);
        if o == before {
            return Ok(());
        }
        let text = toml::to_string_pretty(&o).map_err(|e| SysError::Config(e.to_string()))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension(format!("toml.{}.tmp", std::process::id()));
        let written = (|| {
            use std::io::Write;
            let mut file = std::fs::File::create(&tmp)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()
        })();
        if let Err(e) = written.and_then(|()| std::fs::rename(&tmp, path)) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        Ok(())
    }
}

/// The Modern Standby value to remember before OpenSense turns networking
/// off (writes 0, 0): the current one, unless something is remembered
/// already or it is off already (then it cannot be told apart from a
/// change an older OpenSense made, and the Windows default is used).
fn standby_to_remember(current: (u32, u32), saved: Option<[u32; 2]>) -> Option<[u32; 2]> {
    (saved.is_none() && current != (0, 0)).then_some([current.0, current.1])
}

/// Called by [`tweaks::write`] before it turns networking in Modern
/// Standby off.
pub(crate) fn remember_standby_network() {
    let Ok(current) = tweaks::read(setting::STANDBY_NETWORK) else { return };
    let r = Originals::update(|o| {
        if let Some(v) = standby_to_remember(current, o.standby_network) {
            o.standby_network = Some(v);
        }
    });
    if let Err(e) = r {
        tracing::warn!("cannot remember the Modern Standby setting: {e}");
    }
}

/// Called by [`crate::power::set_cpu_boost`] before it changes the boost
/// mode of `plan` from `current` (AC, DC). Only the first value per plan is
/// kept. When a mode already has a boost setting but nothing is remembered
/// for this plan, an OpenSense version that did not remember may have
/// changed it already, so the Windows default is remembered instead.
pub(crate) fn remember_cpu_boost(plan: u128, current: (u32, u32)) {
    let key = plan_key(plan);
    if Originals::load().cpu_boost.contains_key(&key) {
        return;
    }
    let legacy = crate::config::AppConfig::load().profiles.values().any(|p| p.cpu_boost.is_some());
    let value = if legacy { tweaks::read_default(plan, setting::CPU_BOOST).unwrap_or(current) } else { current };
    let r = Originals::update(|o| {
        o.cpu_boost.entry(key).or_insert([value.0, value.1]);
    });
    if let Err(e) = r {
        tracing::warn!("cannot remember the CPU boost setting: {e}");
    }
}

/// Cross-process lock around writes of the file (a named mutex; released
/// when dropped). Waits at most 2 s, then writes anyway.
struct Lock(#[cfg(windows)] Option<windows::Win32::Foundation::HANDLE>);

impl Lock {
    fn acquire() -> Self {
        #[cfg(windows)]
        {
            use windows::core::w;
            use windows::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};
            let h = unsafe { CreateMutexW(None, false, w!("Local\\OpenSense.OriginalsWrite")) }.ok();
            if let Some(h) = h {
                let _ = unsafe { WaitForSingleObject(h, 2000) };
            }
            Self(h)
        }
        #[cfg(not(windows))]
        Self()
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        #[cfg(windows)]
        if let Some(h) = self.0 {
            unsafe {
                let _ = windows::Win32::System::Threading::ReleaseMutex(h);
                let _ = windows::Win32::Foundation::CloseHandle(h);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standby_remembers_only_first_non_off_value() {
        assert_eq!(standby_to_remember((1, 2), None), Some([1, 2]));
        assert_eq!(standby_to_remember((0, 0), None), None);
        assert_eq!(standby_to_remember((1, 1), Some([1, 2])), None);
        // Half off is not OpenSense's own value.
        assert_eq!(standby_to_remember((0, 2), None), Some([0, 2]));
    }

    #[test]
    fn power_remembers_first_plan_and_overlay_only() {
        let balanced = 0x381b4222_f694_41f0_9685_ff5bb260df2e;
        let key = plan_key(balanced);
        assert_eq!(
            power_to_remember(None, balanced, Some(PowerMode::BestEfficiency)),
            Some((key.clone(), Some("efficiency".into())))
        );
        // A plan without power modes: only the plan.
        let high = crate::power::PLAN_HIGH;
        assert_eq!(power_to_remember(None, high, Some(PowerMode::Plan(high))), Some((plan_key(high), None)));
        // Power mode unreadable: the plan alone.
        assert_eq!(power_to_remember(None, balanced, None), Some((key.clone(), None)));
        // Something remembered already: kept.
        assert_eq!(power_to_remember(Some(&key), high, None), None);
    }

    #[test]
    fn remembered_power_parses_back() {
        let mut o = Originals::default();
        assert_eq!(remembered_power(&o), None);
        o.power_mode = Some("performance".into());
        // A mode without a plan is not used.
        assert_eq!(remembered_power(&o), None);
        o.power_plan = Some(plan_key(5));
        assert_eq!(remembered_power(&o), Some((5, Some(PowerMode::BestPerformance))));
        o.power_mode = None;
        assert_eq!(remembered_power(&o), Some((5, None)));
        // A stored plan key in the mode field is ignored.
        o.power_mode = Some(PowerMode::Plan(9).key());
        assert_eq!(remembered_power(&o), Some((5, None)));
        o.power_plan = Some("not hex".into());
        assert_eq!(remembered_power(&o), None);
    }

    #[test]
    fn plan_keys_round_trip() {
        let plan = 0x381b4222_f694_41f0_9685_ff5bb260df2e;
        assert_eq!(plan_key(plan), "381b4222f69441f09685ff5bb260df2e");
        assert_eq!(parse_plan_key(&plan_key(plan)), Some(plan));
        assert_eq!(parse_plan_key(&plan_key(7)), Some(7));
        assert_eq!(parse_plan_key("not hex"), None);
    }

    #[test]
    fn update_keeps_first_value_and_skips_unchanged_writes() {
        let dir = std::env::temp_dir().join(format!("opensense-test-{}-originals", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("originals.toml");
        // No change: no file is created.
        Originals::update_at(&path, |_| {}).unwrap();
        assert!(!path.exists());
        Originals::update_at(&path, |o| {
            o.standby_network = Some([1, 2]);
            o.cpu_boost.entry(plan_key(5)).or_insert([2, 1]);
        })
        .unwrap();
        Originals::update_at(&path, |o| {
            o.cpu_boost.entry(plan_key(5)).or_insert([0, 0]);
        })
        .unwrap();
        let o = Originals::load_from(&path);
        assert_eq!(o.standby_network, Some([1, 2]));
        assert_eq!(o.cpu_boost[&plan_key(5)], [2, 1]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
