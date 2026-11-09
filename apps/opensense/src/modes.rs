//! Performance modes: the three firmware modes plus user modes based on
//! them, each with its own fan curves, Windows power mode and CPU boost.
//! Also the curve maths and the software fan-curve loop used by the tray
//! agent (the Acer firmware has no fan curves of its own).

use std::time::{Duration, Instant};

use opensense_hw::device::CustomFan;
use opensense_proto::mode::OperationMode;
use opensense_sys::config::{AppConfig, FanModeCfg, ModeProfile};
use opensense_sys::power::PowerMode;

use crate::i18n::{tr, Lang};
use crate::worker::{Command, WorkerHandle};

/// Built-in modes in display order (low to high).
pub const BUILTIN: [(&str, OperationMode); 3] =
    [("quiet", OperationMode::Quiet), ("default", OperationMode::Default), ("extreme", OperationMode::Extreme)];

/// User modes are "custom1" ..= "custom17".
const MAX_CUSTOM: u32 = 17;

pub type Curve = [[f32; 2]; 8];

/// Measured Custom-mode fan speed per percentage. The firmware keeps the
/// fans at roughly 2,500 RPM even at 0 %.
const FAN_CURVE: [(u8, f32); 6] = [(0, 2500.0), (10, 2700.0), (20, 3750.0), (40, 4900.0), (60, 6000.0), (100, 7900.0)];

/// Approximate Custom-mode fan speed for a percentage, rounded to 50 RPM.
pub fn approx_rpm(pct: u8) -> u32 {
    let pct = pct.min(100);
    let rpm = FAN_CURVE
        .windows(2)
        .find(|w| pct <= w[1].0)
        .map(|w| {
            let (p0, r0) = w[0];
            let (p1, r1) = w[1];
            r0 + (r1 - r0) * f32::from(pct - p0) / f32::from(p1 - p0)
        })
        .unwrap_or(FAN_CURVE[FAN_CURVE.len() - 1].1);
    (rpm / 50.0).round() as u32 * 50
}

pub fn base_mode(key: &str) -> Option<OperationMode> {
    BUILTIN.iter().find(|(k, _)| *k == key).map(|(_, m)| *m)
}

pub fn is_custom(key: &str) -> bool {
    base_mode(key).is_none()
}

/// Key of the current mode.
pub fn current_key(cfg: &AppConfig) -> String {
    if let Some(k) = cfg.current_mode.as_ref().filter(|k| base_mode(k).is_some() || cfg.profiles.contains_key(*k)) {
        return k.clone();
    }
    cfg.operation_mode.clone().filter(|m| base_mode(m).is_some()).unwrap_or_else(|| "default".into())
}

/// Firmware mode a mode key runs on.
pub fn base_of(cfg: &AppConfig, key: &str) -> OperationMode {
    base_mode(key).or_else(|| cfg.profiles.get(key).and_then(|p| base_mode(&p.base))).unwrap_or(OperationMode::Default)
}

/// Settings of a mode, with defaults filled in (curves normalised).
pub fn profile(cfg: &AppConfig, key: &str) -> ModeProfile {
    let mut p = cfg.profiles.get(key).cloned().unwrap_or_default();
    let base = base_of(cfg, key);
    p.base = base.label().to_ascii_lowercase();
    if p.cpu_curve.len() != 8 {
        p.cpu_curve = to_stored(&default_curve(base, false));
    }
    if p.gpu_curve.len() != 8 {
        p.gpu_curve = to_stored(&default_curve(base, true));
    }
    p
}

/// Mutable settings of a mode, created with defaults when missing.
pub fn profile_mut<'a>(cfg: &'a mut AppConfig, key: &str) -> &'a mut ModeProfile {
    if !cfg.profiles.contains_key(key) {
        let p = profile(cfg, key);
        cfg.profiles.insert(key.to_owned(), p);
    }
    cfg.profiles.get_mut(key).expect("inserted above")
}

/// All modes in menu order: built-in, then user modes by number.
pub fn ordered(cfg: &AppConfig) -> Vec<String> {
    let mut out: Vec<String> = BUILTIN.iter().map(|(k, _)| (*k).to_owned()).collect();
    let mut custom: Vec<(u32, String)> = cfg
        .profiles
        .keys()
        .filter_map(|k| k.strip_prefix("custom").and_then(|n| n.parse().ok()).map(|n| (n, k.clone())))
        .collect();
    custom.sort();
    out.extend(custom.into_iter().map(|(_, k)| k));
    out
}

pub fn display_name(cfg: &AppConfig, lang: Lang, key: &str) -> String {
    match base_mode(key) {
        Some(m) => tr(lang, m.label()).to_owned(),
        None => cfg.profiles.get(key).map(|p| p.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| key.into()),
    }
}

/// Adds a user mode copied from the current one; returns its key.
pub fn add_custom(cfg: &mut AppConfig, lang: Lang) -> Option<String> {
    let n = (1..=MAX_CUSTOM).find(|n| !cfg.profiles.contains_key(&format!("custom{n}")))?;
    let key = format!("custom{n}");
    let mut p = profile(cfg, &current_key(cfg));
    p.name = format!("{} {n}", tr(lang, "Custom"));
    cfg.profiles.insert(key.clone(), p);
    Some(key)
}

pub fn can_add(cfg: &AppConfig) -> bool {
    cfg.profiles.keys().filter(|k| is_custom(k)).count() < MAX_CUSTOM as usize
}

pub fn remove_custom(cfg: &mut AppConfig, key: &str) {
    if is_custom(key) {
        cfg.profiles.remove(key);
        if cfg.current_mode.as_deref() == Some(key) {
            cfg.current_mode = Some("default".into());
        }
    }
}

pub fn default_power_mode(base: OperationMode) -> PowerMode {
    match base {
        OperationMode::Quiet => PowerMode::BestEfficiency,
        OperationMode::Extreme => PowerMode::BestPerformance,
        _ => PowerMode::Balanced,
    }
}

pub fn power_mode_of(p: &ModeProfile, base: OperationMode) -> PowerMode {
    p.power_mode.as_deref().and_then(PowerMode::from_key).unwrap_or_else(|| default_power_mode(base))
}

/// Applies the Windows side of a mode (power mode, CPU boost, GPU clock
/// limit). Errors are logged; they never block the firmware part.
pub fn apply_windows(p: &ModeProfile, base: OperationMode) {
    // A mode left at its default power mode keeps a plan the user picked in
    // Windows (High / Ultimate Performance) instead of switching it away.
    let keep_plan = p.power_mode.is_none()
        && matches!(opensense_sys::power::power_mode(), Some(opensense_sys::power::PowerMode::Plan(_)));
    if !keep_plan {
        if let Err(e) = opensense_sys::power::set_power_mode(power_mode_of(p, base)) {
            tracing::warn!("power mode: {e}");
        }
    }
    if let Some(b) = p.cpu_boost {
        if let Err(e) = opensense_sys::power::set_cpu_boost(b) {
            tracing::warn!("cpu boost: {e}");
        }
    }
    crate::nvidia::apply_clock_limit(p.gpu_clock_limit);
}

/// Firmware commands for a mode. With an active fan curve the fans are left
/// to the tray agent's loop.
pub fn commands(cfg: &AppConfig, key: &str) -> Vec<Command> {
    let p = profile(cfg, key);
    let base = base_of(cfg, key);
    let mut out = vec![Command::Mode(base)];
    if !p.apply_curve {
        out.push(crate::apply::fan(&cfg.fan));
    }
    out
}

/// Switches to a mode: stores it, sends the firmware commands and applies
/// the Windows settings. The caller saves the config.
pub fn switch(cfg: &mut AppConfig, key: &str, worker: &WorkerHandle) {
    let base = base_of(cfg, key);
    cfg.current_mode = Some(key.to_owned());
    cfg.operation_mode = Some(base.label().to_ascii_lowercase());
    if !profile(cfg, key).apply_curve && base.forces_auto_fan() {
        cfg.fan.mode = FanModeCfg::Auto;
    }
    for c in commands(cfg, key) {
        let _ = worker.tx.send(c);
    }
    let p = profile(cfg, key);
    std::thread::spawn(move || apply_windows(&p, base));
}

// ---- curves ---------------------------------------------------------------------

fn table(temps: [u8; 8], duty: [u8; 8]) -> Curve {
    std::array::from_fn(|i| [f32::from(temps[i]), f32::from(duty[i])])
}

/// Starting curves per firmware mode.
pub fn default_curve(base: OperationMode, gpu: bool) -> Curve {
    let raw = match (base, gpu) {
        (OperationMode::Quiet, false) => table([30, 49, 59, 66, 71, 80, 90, 100], [0, 0, 3, 12, 20, 28, 34, 41]),
        (OperationMode::Quiet, true) => table([30, 49, 59, 66, 71, 80, 90, 100], [0, 0, 4, 17, 27, 35, 40, 45]),
        (OperationMode::Extreme, false) => table([30, 63, 68, 72, 76, 80, 84, 98], [17, 26, 34, 41, 52, 67, 81, 90]),
        (OperationMode::Extreme, true) => table([30, 63, 68, 72, 76, 80, 84, 98], [22, 31, 38, 45, 57, 71, 85, 95]),
        (_, false) => table([30, 58, 61, 64, 72, 77, 81, 98], [0, 8, 17, 22, 34, 41, 48, 69]),
        (_, true) => table([30, 58, 61, 64, 72, 77, 81, 98], [0, 12, 22, 29, 38, 45, 52, 74]),
    };
    normalize(raw)
}

/// Strictly increasing temperatures and non-decreasing duty.
pub fn normalize(mut c: Curve) -> Curve {
    for i in 1..8 {
        if c[i][0] <= c[i - 1][0] {
            c[i][0] = (c[i - 1][0] + 1.0).min(110.0);
        }
        if c[i][1] < c[i - 1][1] {
            c[i][1] = c[i - 1][1];
        }
    }
    c
}

/// Curve from the settings file. The safety floor is applied here too, so
/// a hand-edited file cannot set hot temperatures to slow fans.
pub fn from_stored(v: &[[u8; 2]]) -> Option<Curve> {
    (v.len() == 8).then(|| {
        let mut c = normalize(std::array::from_fn(|i| [f32::from(v[i][0]), f32::from(v[i][1])]));
        for p in &mut c {
            p[1] = p[1].clamp(0.0, 100.0).max(safety_min(p[0]));
        }
        c
    })
}

pub fn to_stored(c: &Curve) -> Vec<[u8; 2]> {
    c.iter().map(|p| [p[0].round().clamp(0.0, 110.0) as u8, p[1].clamp(0.0, 100.0) as u8]).collect()
}

/// Lowest duty allowed at a temperature: above 70 °C the fans may not be
/// set slower than this line (12 % at 80 °C, 36 % at 100 °C).
pub fn safety_min(temp: f32) -> f32 {
    ((temp - 70.0) * 1.2).max(0.0)
}

/// Moves point `k` and keeps the curve monotonic: later points are pushed
/// up / right, earlier points down / left.
pub fn move_point(c: &mut Curve, k: usize, mut x: f32, mut y: f32, clamp_grid: bool) {
    x = x.clamp(20.0, 110.0);
    y = y.clamp(0.0, 100.0).max(safety_min(x));
    if clamp_grid {
        let lo = 30.0 + 10.0 * k as f32;
        x = x.clamp(lo, lo + 9.0);
    }
    c[k] = [x, y];
    for p in c.iter_mut().skip(k + 1) {
        p[0] = p[0].max(x);
        p[1] = p[1].max(y);
    }
    for p in c.iter_mut().take(k) {
        p[0] = p[0].min(x);
        p[1] = p[1].min(y);
    }
}

/// Duty for a temperature, linearly interpolated between the points.
pub fn duty(c: &Curve, temp: f32) -> f32 {
    if temp <= c[0][0] {
        return c[0][1];
    }
    for w in c.windows(2) {
        let ([t0, d0], [t1, d1]) = (w[0], w[1]);
        if temp <= t1 {
            return if t1 > t0 { d0 + (d1 - d0) * (temp - t0) / (t1 - t0) } else { d1 };
        }
    }
    c[7][1]
}

/// The software fan-curve loop: picks a duty per fan from its curve with a
/// little hysteresis, and says when to write it.
#[derive(Debug, Default)]
pub struct FanLoop {
    /// Duty last written and the temperature it was chosen at, per fan.
    state: [Option<(u8, f32)>; 2],
    last_write: Option<Instant>,
    /// Consecutive ticks without any temperature.
    missing: u32,
}

/// What the fan loop wants written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanStep {
    Custom(CustomFan),
    /// Temperatures are gone: hand the fans back to the firmware, which
    /// reads its own sensors, rather than keep a speed chosen blind.
    Auto,
}

/// Ticks (seconds) without any temperature before the fans go back to the
/// firmware's own control.
pub const MISSING_LIMIT: u32 = 5;

/// Degrees the temperature has to drop before the fans slow down.
const HYSTERESIS_DOWN: f32 = 3.0;
/// Re-send the same speeds this often, in case something else changed them.
const REASSERT: Duration = Duration::from_secs(30);

impl FanLoop {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn active(&self) -> bool {
        self.last_write.is_some()
    }

    /// Next fan speeds, or `None` when nothing needs to be written. A fan
    /// whose own temperature is missing follows the other one; with none
    /// for [`MISSING_LIMIT`] ticks the loop gives the fans back to the
    /// firmware ([`FanStep::Auto`]) and nothing stale is re-sent meanwhile.
    pub fn step(&mut self, curves: [&Curve; 2], temps: [Option<u16>; 2], now: Instant) -> Option<FanStep> {
        let temps = [temps[0].or(temps[1]), temps[1].or(temps[0])];
        if temps[0].is_none() {
            self.missing += 1;
            if self.missing >= MISSING_LIMIT && self.active() {
                self.reset();
                return Some(FanStep::Auto);
            }
            return None;
        }
        self.missing = 0;
        let mut changed = false;
        for i in 0..2 {
            let Some(t) = temps[i].map(f32::from) else { continue };
            let target = duty(curves[i], t).round().clamp(0.0, 100.0) as u8;
            let next = match self.state[i] {
                None => Some((target, t)),
                Some((cur, _)) if target > cur => Some((target, t)),
                Some((cur, set_at)) if target < cur && t <= set_at - HYSTERESIS_DOWN => Some((target, t)),
                _ => None,
            };
            if let Some((d, t)) = next {
                if self.state[i].is_none_or(|(cur, _)| cur.abs_diff(d) >= 2) {
                    self.state[i] = Some((d, t));
                    changed = true;
                }
            }
        }
        let due = self.last_write.is_none_or(|w| now.duration_since(w) >= REASSERT);
        if !(changed || due) {
            return None;
        }
        let (cpu, gpu) = (self.state[0]?, self.state[1]?);
        self.last_write = Some(now);
        Some(FanStep::Custom(CustomFan { cpu_percent: Some(cpu.0), gpu_percent: Some(gpu.0) }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approx_rpm_follows_measured_curve() {
        assert_eq!(approx_rpm(0), 2500);
        assert_eq!(approx_rpm(20), 3750);
        assert_eq!(approx_rpm(30), 4350);
        assert_eq!(approx_rpm(100), 7900);
        assert_eq!(approx_rpm(255), 7900);
    }

    #[test]
    fn default_curves_are_monotonic() {
        for (_, m) in BUILTIN {
            for gpu in [false, true] {
                let c = default_curve(m, gpu);
                assert!(c.windows(2).all(|w| w[1][0] > w[0][0] && w[1][1] >= w[0][1]));
            }
        }
    }

    #[test]
    fn duty_interpolates_and_clamps() {
        let c = table([30, 40, 50, 60, 70, 80, 90, 100], [0, 10, 20, 30, 40, 50, 60, 70]);
        assert_eq!(duty(&c, 20.0), 0.0);
        assert_eq!(duty(&c, 45.0), 15.0);
        assert_eq!(duty(&c, 105.0), 70.0);
    }

    #[test]
    fn move_point_pushes_neighbours() {
        let mut c = table([30, 40, 50, 60, 70, 80, 90, 100], [0, 10, 20, 30, 40, 50, 60, 70]);
        move_point(&mut c, 2, 55.0, 65.0, true);
        assert_eq!(c[2], [55.0, 65.0]);
        assert!(c[3][1] >= 65.0 && c[7][1] >= 65.0);
        move_point(&mut c, 5, 85.0, 0.0, true);
        // Safety line: at 85 °C at least 18 %.
        assert!(c[5][1] >= 18.0 - 0.01);
        assert!(c[0][1] <= c[5][1]);
    }

    #[test]
    fn fan_loop_hysteresis() {
        let c = table([30, 40, 50, 60, 70, 80, 90, 100], [0, 10, 20, 30, 40, 50, 60, 70]);
        let mut l = FanLoop::default();
        let t0 = Instant::now();
        let custom = |s: Option<FanStep>| match s {
            Some(FanStep::Custom(f)) => f,
            other => panic!("expected custom speeds, got {other:?}"),
        };
        let first = custom(l.step([&c, &c], [Some(60), Some(50)], t0));
        assert_eq!((first.cpu_percent, first.gpu_percent), (Some(30), Some(20)));
        // Rising writes at once.
        assert!(l.step([&c, &c], [Some(65), Some(50)], t0).is_some());
        // Dropping 2 °C keeps the speed, 3 °C lowers it.
        assert!(l.step([&c, &c], [Some(63), Some(50)], t0).is_none());
        let down = custom(l.step([&c, &c], [Some(62), Some(50)], t0));
        assert_eq!(down.cpu_percent, Some(32));
        // Re-sent after 30 s even without change.
        assert!(l.step([&c, &c], [Some(62), Some(50)], t0 + Duration::from_secs(31)).is_some());
    }

    fn flat(d: f32) -> Curve {
        std::array::from_fn(|i| [30.0 + 10.0 * i as f32, d])
    }

    #[test]
    fn fan_loop_hands_back_to_firmware_when_sensors_vanish() {
        let c = flat(40.0);
        let mut l = FanLoop::default();
        let t0 = Instant::now();
        assert!(matches!(l.step([&c, &c], [Some(60), Some(55)], t0), Some(FanStep::Custom(_))));
        // A few missing readings: nothing is written (no stale re-send).
        for k in 1..MISSING_LIMIT {
            assert_eq!(l.step([&c, &c], [None, None], t0 + Duration::from_secs(40 * u64::from(k))), None);
        }
        assert_eq!(l.step([&c, &c], [None, None], t0 + Duration::from_secs(400)), Some(FanStep::Auto));
        assert!(!l.active());
        // Still no readings: Auto is not repeated.
        assert_eq!(l.step([&c, &c], [None, None], t0 + Duration::from_secs(401)), None);
        // Readings back: the curve takes over again.
        assert!(matches!(
            l.step([&c, &c], [Some(60), Some(55)], t0 + Duration::from_secs(402)),
            Some(FanStep::Custom(_))
        ));
    }

    #[test]
    fn fan_loop_uses_the_other_temperature_when_one_is_missing() {
        let cpu = flat(30.0);
        let gpu = flat(50.0);
        let mut l = FanLoop::default();
        let step = l.step([&cpu, &gpu], [Some(60), None], Instant::now());
        assert_eq!(step, Some(FanStep::Custom(CustomFan { cpu_percent: Some(30), gpu_percent: Some(50) })));
    }

    #[test]
    fn stored_curve_keeps_the_safety_floor() {
        // Hand-edited: 0 % at 100 °C.
        let stored: Vec<[u8; 2]> = (0..8).map(|i| [30 + 10 * i as u8, 0]).collect();
        let c = from_stored(&stored).unwrap();
        assert!(c[7][1] >= safety_min(100.0));
        assert_eq!(c[0][1], 0.0);
    }
}
