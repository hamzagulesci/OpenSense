//! Turns the saved configuration into hardware commands. Shared by the
//! window and the tray agent.

use opensense_hw::device::{CustomFan, DynamicLighting, StaticLighting};
use opensense_proto::lighting::{self, Direction, Effect, Rgb};
use opensense_sys::config::{AppConfig, FanConfig, FanModeCfg, LightingConfig, LightingKind};
use opensense_sys::oem::OemInfo;

use crate::worker::Command;

pub fn effect_from_name(s: &str) -> Effect {
    Effect::DYNAMIC.into_iter().find(|e| e.label().eq_ignore_ascii_case(s)).unwrap_or(Effect::Wave)
}

pub fn fan(f: &FanConfig) -> Command {
    match f.mode {
        FanModeCfg::Auto => Command::FansAuto,
        FanModeCfg::Max => Command::FansMax,
        FanModeCfg::Custom => Command::FansCustom(CustomFan {
            cpu_percent: (!f.cpu_auto).then_some(f.cpu_percent),
            gpu_percent: (!f.gpu_auto).then_some(f.gpu_percent),
        }),
    }
}

pub fn lighting(l: &LightingConfig) -> Command {
    let brightness = lighting::level_to_percent(l.brightness_level);
    match l.kind {
        LightingKind::Static => Command::StaticLighting(StaticLighting {
            zones: std::array::from_fn(|i| Rgb::from_hex(&l.zones[i]).unwrap_or_default()),
            enabled: l.zone_enabled,
            brightness_percent: brightness,
        }),
        LightingKind::Dynamic => Command::DynamicLighting(DynamicLighting {
            effect: effect_from_name(&l.effect),
            speed: l.speed,
            brightness_percent: brightness,
            direction: if l.direction == "right" { Direction::Right } else { Direction::Left },
            color: Rgb::from_hex(&l.effect_color).unwrap_or_default(),
        }),
    }
}

/// Commands that restore mode, fans and lighting.
pub fn saved_state(cfg: &AppConfig, oem: &OemInfo) -> Vec<Command> {
    let key = crate::modes::current_key(cfg);
    let mut out = if cfg.operation_mode.is_some() || cfg.current_mode.is_some() {
        crate::modes::commands(cfg, &key)
    } else if crate::modes::profile(cfg, &key).apply_curve {
        Vec::new()
    } else {
        vec![fan(&cfg.fan)]
    };
    if oem.rgb_keyboard {
        out.push(lighting(&cfg.lighting));
    }
    // Targets between 80 and 100 % are kept by the tray agent.
    match cfg.charge_target() {
        Some(80) => out.push(Command::ChargeLimit(true)),
        Some(100) => out.push(Command::ChargeLimit(false)),
        _ => {}
    }
    out
}

/// How far below a charge target charging starts again.
const CHARGE_HYSTERESIS: u8 = 3;

/// Firmware 80 % cap state needed for a charge target, or `None` to leave
/// it as it is. The firmware only knows "80 %" and "no limit"; a target in
/// between is held by turning the cap on once the battery reaches the target
/// (charging stops) and off again when it has dropped a few percent.
pub fn charge_cap_for(target: u8, percent: Option<u8>, cap_on: bool) -> Option<bool> {
    let want = match target {
        0..=80 => true,
        100.. => false,
        t => match percent {
            Some(p) if p >= t => true,
            Some(p) if p + CHARGE_HYSTERESIS <= t => false,
            _ => return None,
        },
    };
    (want != cap_on).then_some(want)
}

/// Label the worker records after a GPU mode write.
pub const GPU_SET_LABEL: &str = "GPU mode set, reboot to apply";

/// Progress of a GPU mode write started with [`begin_gpu_write`].
pub enum WriteState {
    Pending,
    Done,
    /// Firmware error text; `None` when the firmware did not answer.
    Failed(Option<String>),
}

/// Sends the GPU mode, clearing the last result so the answer can be told
/// apart from an earlier one.
pub fn begin_gpu_write(worker: &crate::worker::WorkerHandle, mode: opensense_proto::mode::GpuMode) {
    worker.shared.lock().last_action = None;
    let _ = worker.tx.send(Command::GpuMode(mode));
}

pub fn gpu_write_state(shared: &crate::worker::Shared, since: std::time::Instant) -> WriteState {
    match shared.lock().last_action.clone() {
        Some((label, None)) if label == GPU_SET_LABEL => WriteState::Done,
        Some((label, Some(e))) if label == GPU_SET_LABEL => WriteState::Failed(Some(e)),
        _ if since.elapsed() > std::time::Duration::from_secs(10) => WriteState::Failed(None),
        _ => WriteState::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charge_cap_holds_target() {
        // Fixed ends map straight to the firmware.
        assert_eq!(charge_cap_for(80, Some(50), false), Some(true));
        assert_eq!(charge_cap_for(100, Some(90), true), Some(false));
        assert_eq!(charge_cap_for(100, Some(90), false), None);
        // 90 %: stop at 90, resume at 87.
        assert_eq!(charge_cap_for(90, Some(85), false), None);
        assert_eq!(charge_cap_for(90, Some(90), false), Some(true));
        assert_eq!(charge_cap_for(90, Some(89), true), None);
        assert_eq!(charge_cap_for(90, Some(87), true), Some(false));
        assert_eq!(charge_cap_for(90, None, true), None);
    }
}
