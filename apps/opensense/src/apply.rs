//! Turns the saved configuration into hardware commands. Shared by the
//! window and the tray agent.

use opensense_hw::device::{CustomFan, DynamicLighting, StaticLighting};
use opensense_proto::lighting::{self, Direction, Effect, Rgb};
use opensense_proto::mode::OperationMode;
use opensense_sys::config::{AppConfig, FanConfig, FanModeCfg, LightingConfig, LightingKind};
use opensense_sys::oem::OemInfo;

use crate::worker::Command;

pub fn mode_from_name(s: &str) -> Option<OperationMode> {
    OperationMode::UI.into_iter().find(|m| m.label().eq_ignore_ascii_case(s))
}

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
    let mut out = Vec::with_capacity(3);
    if let Some(m) = cfg.operation_mode.as_deref().and_then(mode_from_name) {
        out.push(Command::Mode(m));
    }
    // Quiet mode already forces Auto fans.
    if cfg.operation_mode.as_deref().and_then(mode_from_name) != Some(OperationMode::Quiet) {
        out.push(fan(&cfg.fan));
    }
    if oem.rgb_keyboard {
        out.push(lighting(&cfg.lighting));
    }
    if let Some(on) = cfg.charge_limit {
        out.push(Command::ChargeLimit(on));
    }
    out
}
