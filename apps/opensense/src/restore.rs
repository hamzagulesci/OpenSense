//! `opensense --restore-system`, run by the uninstaller after the tray agent
//! quit: undoes the Windows settings OpenSense changed. Values remembered
//! before the first change are put back; without one the Windows default of
//! the plan is used. A setting is only touched while it still holds the
//! value OpenSense sets, so a later change made by the user is kept.
//!
//! Fans, lighting, the operation mode and the charge limit live in the
//! firmware and are not changed here.

use opensense_sys::config::AppConfig;
use opensense_sys::originals::{self, Originals};
use opensense_sys::tweaks::{self, setting};

/// Plugged-in PCIe link state to write back, if OpenSense still holds it
/// off (0). `default` is the plan's Windows default.
fn aspm_target(current_ac: u32, aspm_off: bool, previous: Option<u32>, default: u32) -> Option<u32> {
    (current_ac == 0 && (aspm_off || previous.is_some())).then(|| previous.filter(|&p| p != 0).unwrap_or(default))
}

/// Modern Standby network (AC, DC) to write back, if OpenSense still holds
/// it off (0, 0).
fn standby_target(current: (u32, u32), off: bool, saved: Option<[u32; 2]>, default: (u32, u32)) -> Option<(u32, u32)> {
    (current == (0, 0) && (off || saved.is_some())).then(|| saved.map_or(default, |[ac, dc]| (ac, dc)))
}

/// Plugged-in lid action to write back, if clamshell mode still holds it
/// at "do nothing" (0).
fn lid_target(current_ac: u32, clamshell: bool, saved: Option<u32>, default: u32) -> Option<u32> {
    (current_ac == 0 && (clamshell || saved.is_some())).then(|| saved.filter(|&v| v != 0).unwrap_or(default.max(1)))
}

fn log(what: &str, r: Result<(), opensense_sys::SysError>) {
    match r {
        Ok(()) => tracing::info!("restore-system: {what} restored"),
        Err(e) => tracing::warn!("restore-system: cannot restore {what}: {e}"),
    }
}

pub fn run() {
    tracing::info!("restore-system: undoing Windows settings changed by OpenSense");
    let cfg = AppConfig::load();
    let orig = Originals::load();
    let plan = tweaks::active_plan().ok();
    let default = |s| plan.and_then(|p| tweaks::read_default(p, s).ok());

    // PCIe link state power management (plugged in only).
    if let Ok((ac, _)) = tweaks::read(setting::ASPM) {
        let d = default(setting::ASPM).map_or(1, |d| d.0);
        if let Some(v) = aspm_target(ac, cfg.aspm_off, cfg.aspm_previous, d) {
            log("PCIe link state", tweaks::write(setting::ASPM, Some(v), None));
        }
    }

    // Networking in Modern Standby.
    if let Ok(cur) = tweaks::read(setting::STANDBY_NETWORK) {
        let d = default(setting::STANDBY_NETWORK).unwrap_or((1, 2));
        if let Some((ac, dc)) = standby_target(cur, cfg.standby_network_off, orig.standby_network, d) {
            log("Modern Standby network", tweaks::write(setting::STANDBY_NETWORK, Some(ac), Some(dc)));
        }
    }

    // Lid action forced by clamshell mode (a killed agent cannot undo it).
    if let Ok((ac, _)) = tweaks::read(setting::LID_ACTION) {
        let d = default(setting::LID_ACTION).map_or(1, |d| d.0);
        if let Some(v) = lid_target(ac, cfg.clamshell, cfg.clamshell_default_lid, d) {
            log("lid action", tweaks::write(setting::LID_ACTION, Some(v), None));
        }
    }

    // CPU boost mode, per plan OpenSense changed.
    if !orig.cpu_boost.is_empty() {
        for (key, [ac, dc]) in &orig.cpu_boost {
            let Some(p) = originals::parse_plan_key(key) else { continue };
            log("CPU boost", tweaks::write_plan(p, setting::CPU_BOOST, Some(*ac), Some(*dc)));
        }
    } else if cfg.profiles.values().any(|p| p.cpu_boost.is_some()) {
        // Set by a version that did not remember it: the plan's default.
        if let (Some(p), Some((ac, dc))) = (plan, default(setting::CPU_BOOST)) {
            log("CPU boost", tweaks::write_plan(p, setting::CPU_BOOST, Some(ac), Some(dc)));
        }
    }

    // NVIDIA clock lock (nvidia-smi -rgc), when a mode had a limit.
    if cfg.profiles.values().any(|p| p.gpu_clock_limit.is_some_and(|l| l < crate::nvidia::CLOCK_MAX)) {
        crate::nvidia::set_clock_limit(None, true);
    }

    // Acer services stopped by OpenSense: start type back and started.
    if !cfg.service_start_types.is_empty() {
        tweaks::set_services(true);
    }
    tracing::info!("restore-system: done");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aspm_only_while_still_off() {
        assert_eq!(aspm_target(0, true, Some(2), 1), Some(2));
        assert_eq!(aspm_target(0, true, None, 1), Some(1));
        assert_eq!(aspm_target(0, false, Some(2), 1), Some(2));
        assert_eq!(aspm_target(0, false, None, 1), None);
        assert_eq!(aspm_target(2, true, Some(1), 1), None);
        assert_eq!(aspm_target(0, true, Some(0), 2), Some(2));
    }

    #[test]
    fn standby_only_while_still_off() {
        assert_eq!(standby_target((0, 0), true, Some([1, 1]), (1, 2)), Some((1, 1)));
        assert_eq!(standby_target((0, 0), true, None, (1, 2)), Some((1, 2)));
        assert_eq!(standby_target((0, 0), false, Some([2, 2]), (1, 2)), Some((2, 2)));
        assert_eq!(standby_target((0, 0), false, None, (1, 2)), None);
        assert_eq!(standby_target((1, 2), true, Some([1, 1]), (1, 2)), None);
    }

    #[test]
    fn lid_only_while_forced_to_do_nothing() {
        assert_eq!(lid_target(0, true, Some(2), 1), Some(2));
        assert_eq!(lid_target(0, false, Some(3), 1), Some(3));
        assert_eq!(lid_target(0, true, None, 1), Some(1));
        assert_eq!(lid_target(0, true, None, 0), Some(1));
        assert_eq!(lid_target(0, false, None, 1), None);
        assert_eq!(lid_target(1, true, Some(2), 1), None);
    }
}
