//! `opensense --restore-system`, run by the uninstaller after the tray agent
//! quit: undoes the Windows settings OpenSense changed. Values remembered
//! before the first change are put back; without one the Windows default of
//! the plan is used. A setting is only touched while it still holds the
//! value OpenSense sets, so a later change made by the user is kept.
//!
//! The power plan and power mode go back to the ones remembered before
//! OpenSense first changed them (left alone when none is remembered).
//!
//! The firmware is handed back in a neutral state: fans on Auto, the
//! operation mode on Default and the 80 % charge limit off. Keyboard
//! lighting is left as it is. Firmware failures are only logged, and the
//! firmware gets at most [`FIRMWARE_WAIT`], so a hung call cannot block the
//! uninstaller.

use std::time::{Duration, Instant};

use opensense_hw::{NitroDevice, TransportKind};
use opensense_proto::mode::OperationMode;
use opensense_sys::config::AppConfig;
use opensense_sys::originals::{self, Originals};
use opensense_sys::tweaks::{self, setting};

/// Longest wait for the firmware part before the process exits anyway.
const FIRMWARE_WAIT: Duration = Duration::from_secs(20);

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

/// Whether the operation mode must be set to Default: only when the
/// firmware reports a mode (the model has operation modes) and it is
/// another one.
fn mode_needs_reset(current: Option<OperationMode>) -> bool {
    current.is_some_and(|m| m != OperationMode::Default)
}

/// Whether the 80 % charge limit must be turned off.
fn charge_limit_needs_reset(health: Option<opensense_proto::battery::BatteryHealth>) -> bool {
    health.is_some_and(|h| h.limit_supported && h.limit_active)
}

fn log(what: &str, r: Result<(), impl std::fmt::Display>) {
    match r {
        Ok(()) => tracing::info!("restore-system: {what} restored"),
        Err(e) => tracing::warn!("restore-system: cannot restore {what}: {e}"),
    }
}

/// Operation mode Default, fans on Auto and the charge limit off, over a
/// connection of its own (the tray agent has quit).
fn restore_firmware() {
    let oem = opensense_sys::oem::OemInfo::detect();
    let transport = match opensense_hw::open_transport(TransportKind::Wmi) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("restore-system: no firmware connection, firmware left as it is: {e}");
            return;
        }
    };
    let dev = NitroDevice::new(transport, crate::worker::device_profile(&oem));
    match dev.operation_mode() {
        Ok(m) if mode_needs_reset(m) => log("operation mode", dev.set_operation_mode(OperationMode::Default)),
        Ok(_) => {}
        Err(e) => tracing::warn!("restore-system: cannot read the operation mode: {e}"),
    }
    // After the mode: a mode change may set the fans itself.
    log("fans (Auto)", dev.set_fans_auto());
    match dev.battery_health() {
        Ok(h) if charge_limit_needs_reset(Some(h)) => log("charge limit (off)", dev.set_charge_limit(false)),
        Ok(_) => {}
        Err(e) => tracing::warn!("restore-system: cannot read the charge limit: {e}"),
    }
}

pub fn run() {
    tracing::info!("restore-system: undoing Windows and firmware settings changed by OpenSense");
    let started = Instant::now();
    // The firmware in parallel; waited for (with a limit) at the end.
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let firmware = std::thread::Builder::new().name("restore-firmware".into()).spawn(move || {
        restore_firmware();
        let _ = done_tx.send(());
    });
    if let Err(e) = &firmware {
        tracing::warn!("restore-system: cannot start the firmware restore: {e}");
    }
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

    // Power plan and power mode, last: the settings above were changed in
    // the plan that is active now.
    if let Some((p, mode)) = originals::remembered_power(&orig) {
        log("power plan", opensense_sys::power::restore_power_mode(p, mode));
    }

    // NVIDIA clock lock (nvidia-smi -rgc), when a mode had a limit.
    if cfg.profiles.values().any(|p| p.gpu_clock_limit.is_some_and(|l| l < crate::nvidia::CLOCK_MAX)) {
        crate::nvidia::set_clock_limit(None, true);
    }

    // Acer services stopped by OpenSense: start type back and started.
    if !cfg.service_start_types.is_empty() {
        tweaks::set_services(true);
    }

    if firmware.is_ok() {
        let left = FIRMWARE_WAIT.saturating_sub(started.elapsed());
        if done_rx.recv_timeout(left).is_err() {
            tracing::warn!("restore-system: firmware did not answer in time; left as it is");
        }
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
    fn firmware_mode_reset_only_when_another_mode() {
        assert!(mode_needs_reset(Some(OperationMode::Extreme)));
        assert!(mode_needs_reset(Some(OperationMode::Quiet)));
        assert!(!mode_needs_reset(Some(OperationMode::Default)));
        // No operation modes on this model.
        assert!(!mode_needs_reset(None));
    }

    #[test]
    fn charge_limit_reset_only_while_on() {
        use opensense_proto::battery::BatteryHealth;
        let h = |limit_supported, limit_active| BatteryHealth { limit_supported, limit_active, ..Default::default() };
        assert!(charge_limit_needs_reset(Some(h(true, true))));
        assert!(!charge_limit_needs_reset(Some(h(true, false))));
        assert!(!charge_limit_needs_reset(Some(h(false, true))));
        assert!(!charge_limit_needs_reset(None));
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
