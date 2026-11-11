//! Hardware compatibility tests, built on [`NitroDevice`].
//!
//! Tests come in three safety levels:
//! - [`Level::ReadOnly`]: only firmware getters; safe to run automatically.
//! - [`Level::Confirm`]: briefly change a setting, read it back and restore
//!   it; run only after the user agreed.
//! - [`Level::Restart`]: needs a reboot or a lasting change (GPU MUX). Never
//!   run; only reported as present or not.
//!
//! A feature the firmware merely answers for is reported as
//! [`Outcome::Detected`]; only a confirmed change (written and read back, or
//! a measured effect) is [`Outcome::Verified`].

use std::time::Duration;

use opensense_proto::lighting::{Rgb, ZONES};
use opensense_proto::mode::OperationMode;
use opensense_proto::sensor::Sensor;
use opensense_proto::GamingMethod as M;

use crate::device::{NitroDevice, StaticLighting};
use crate::HwError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Level {
    ReadOnly,
    Confirm,
    Restart,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Changed and confirmed (read back, or a measured effect).
    Verified,
    /// The firmware answers for the feature; not exercised.
    Detected,
    /// The firmware reports the feature as absent.
    NotSupported,
    /// A call failed or the result was wrong.
    Failed,
    /// Not run (needs confirmation, or never run automatically).
    NotRun,
}

/// One test's result. `detail` is short technical text (values, status
/// codes); it never contains user data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestResult {
    pub id: &'static str,
    pub level: Level,
    pub outcome: Outcome,
    pub detail: String,
}

impl TestResult {
    fn new(id: &'static str, level: Level, outcome: Outcome, detail: impl Into<String>) -> Self {
        Self { id, level, outcome, detail: detail.into() }
    }

    fn error(id: &'static str, level: Level, e: &HwError) -> Self {
        Self::new(id, level, Outcome::Failed, e.to_string())
    }
}

/// Every test, with its level, in display order.
pub const TESTS: [(&str, Level); 19] = [
    ("connection", Level::ReadOnly),
    ("cpu_temp", Level::ReadOnly),
    ("gpu_temp", Level::ReadOnly),
    ("cpu_fan_rpm", Level::ReadOnly),
    ("gpu_fan_rpm", Level::ReadOnly),
    ("operation_mode", Level::ReadOnly),
    ("rgb_keyboard", Level::ReadOnly),
    ("charge_limit", Level::ReadOnly),
    ("coolboost", Level::ReadOnly),
    ("lcd_overdrive", Level::ReadOnly),
    ("backlight_timeout", Level::ReadOnly),
    ("gpu_mux", Level::ReadOnly),
    ("raw_firmware", Level::ReadOnly),
    ("fan_max", Level::Confirm),
    ("mode_switch", Level::Confirm),
    ("rgb_write", Level::Confirm),
    ("charge_limit_write", Level::Confirm),
    ("gpu_mux_switch", Level::Restart),
    ("overdrive_write", Level::Restart),
];

pub fn level_of(id: &str) -> Option<Level> {
    TESTS.iter().find(|(t, _)| *t == id).map(|(_, l)| *l)
}

/// Plausible ranges; anything else is reported as a failed reading.
const TEMP_RANGE: std::ops::RangeInclusive<u16> = 1..=115;
const RPM_RANGE: std::ops::RangeInclusive<u16> = 0..=12_000;

/// Runs every read-only test. Makes no firmware writes.
pub fn read_only(dev: &NitroDevice) -> Vec<TestResult> {
    let r = Level::ReadOnly;
    let mut out = vec![TestResult::new("connection", r, Outcome::Verified, dev.transport_name())];

    for (id, sensor, range) in [
        ("cpu_temp", Sensor::CpuTemp, TEMP_RANGE),
        ("gpu_temp", Sensor::Gpu1Temp, TEMP_RANGE),
        ("cpu_fan_rpm", Sensor::CpuFan, RPM_RANGE),
        ("gpu_fan_rpm", Sensor::Gpu1Fan, RPM_RANGE),
    ] {
        out.push(match dev.sensor(sensor) {
            Ok(Some(v)) if range.contains(&v) => TestResult::new(id, r, Outcome::Verified, v.to_string()),
            Ok(Some(v)) => TestResult::new(id, r, Outcome::Failed, format!("implausible value {v}")),
            Ok(None) => TestResult::new(id, r, Outcome::NotSupported, ""),
            Err(e) => TestResult::error(id, r, &e),
        });
    }

    out.push(match dev.operation_mode() {
        Ok(Some(m)) => TestResult::new("operation_mode", r, Outcome::Detected, format!("{m:?}")),
        Ok(None) => TestResult::new("operation_mode", r, Outcome::NotSupported, ""),
        Err(e) => TestResult::error("operation_mode", r, &e),
    });
    let caps = dev.capabilities();
    out.push(TestResult::new(
        "rgb_keyboard",
        r,
        if caps.rgb_keyboard { Outcome::Detected } else { Outcome::NotSupported },
        "",
    ));
    out.push(match dev.battery_health() {
        Ok(b) if b.limit_supported => TestResult::new(
            "charge_limit",
            r,
            Outcome::Detected,
            format!("80% limit {}", if b.limit_active { "on" } else { "off" }),
        ),
        Ok(_) => TestResult::new("charge_limit", r, Outcome::NotSupported, ""),
        Err(e) => TestResult::error("charge_limit", r, &e),
    });
    for (id, value) in [
        ("coolboost", dev.coolboost().map(|v| v.map(|b| format!("{b}")))),
        ("lcd_overdrive", dev.lcd_overdrive().map(|v| v.map(|b| format!("{b}")))),
        (
            "backlight_timeout",
            dev.backlight_timeout()
                .map(|v| v.map(|t| format!("auto-off {}, {}%", if t.auto_off { "on" } else { "off" }, t.brightness))),
        ),
    ] {
        out.push(match value {
            Ok(Some(v)) => TestResult::new(id, r, Outcome::Detected, v),
            Ok(None) => TestResult::new(id, r, Outcome::NotSupported, ""),
            Err(e) => TestResult::error(id, r, &e),
        });
    }
    out.push(match (dev.gpu_mode_supported(), dev.gpu_mode()) {
        (Ok(true), Ok(m)) => TestResult::new(
            "gpu_mux",
            r,
            Outcome::Detected,
            m.map_or_else(|| "unknown".to_owned(), |m| format!("{m:?}")),
        ),
        (Ok(false), _) => TestResult::new("gpu_mux", r, Outcome::NotSupported, ""),
        (Err(e), _) | (_, Err(e)) => TestResult::error("gpu_mux", r, &e),
    });
    // Raw answers of a few getters, to map models that behave differently
    // (sensor bitmap, supported-mode mask, MUX flags, gaming profile).
    let raw: Vec<String> = [
        ("sysinfo0", M::GetGamingSysInfo, 0x00),
        ("misc0a", M::GetGamingMiscSetting, 0x0A),
        ("misc09", M::GetGamingMiscSetting, 0x09),
        ("misc0b", M::GetGamingMiscSetting, 0x0B),
        ("profile0", M::GetGamingProfile, 0x00),
    ]
    .into_iter()
    .map(|(name, m, input)| match dev.raw_get(m, input) {
        Ok(v) => format!("{name}={v:#x}"),
        Err(_) => format!("{name}=error"),
    })
    .collect();
    out.push(TestResult::new("raw_firmware", r, Outcome::Detected, raw.join(" ")));

    for (id, level) in TESTS.iter().filter(|(_, l)| *l == Level::Confirm) {
        out.push(TestResult::new(id, *level, Outcome::NotRun, ""));
    }
    // Restart-level features are never run; report whether the firmware
    // has them, from the matching read-only result.
    for (id, from) in [("gpu_mux_switch", "gpu_mux"), ("overdrive_write", "lcd_overdrive")] {
        let present = out.iter().any(|r| r.id == from && r.outcome == Outcome::Detected);
        out.push(if present {
            TestResult::new(id, Level::Restart, Outcome::NotRun, "supported; not run, needs a restart")
        } else {
            TestResult::new(id, Level::Restart, Outcome::NotSupported, "")
        });
    }
    out
}

/// Waiting between steps; tests pass a no-op so they run instantly.
pub type Sleep<'a> = &'a dyn Fn(Duration);

/// Re-applies the user's saved keyboard lighting after the RGB test.
pub type RestoreLighting<'a> = &'a dyn Fn(&NitroDevice) -> Result<(), HwError>;

/// Runs one confirm-level test: change, verify, restore. `restore_lighting`
/// is the user's saved lighting, re-applied after the RGB test (the
/// firmware cannot report the current colours). Restart-level ids are
/// refused.
pub fn run_confirmed(
    dev: &NitroDevice,
    id: &str,
    restore_lighting: Option<RestoreLighting>,
    sleep: Sleep,
) -> TestResult {
    let c = Level::Confirm;
    match id {
        "fan_max" => fan_max(dev, sleep),
        "mode_switch" => mode_switch(dev, sleep),
        "rgb_write" => {
            let red = StaticLighting {
                zones: [Rgb { r: 255, g: 0, b: 0 }; ZONES],
                enabled: [true; ZONES],
                brightness_percent: 100,
            };
            let res = dev.apply_static_lighting(&red);
            sleep(Duration::from_secs(2));
            let restored = restore_lighting.map(|f| f(dev));
            match (res, restored) {
                (Err(e), _) => TestResult::error("rgb_write", c, &e),
                (Ok(()), Some(Err(e))) => {
                    TestResult::new("rgb_write", c, Outcome::Failed, format!("accepted, restore failed: {e}"))
                }
                // The firmware cannot report colours: accepted means detected,
                // the user's eyes confirm the rest.
                (Ok(()), _) => TestResult::new("rgb_write", c, Outcome::Detected, "firmware accepted the colours"),
            }
        }
        "charge_limit_write" => charge_limit(dev),
        _ => TestResult::new(
            static_id(id),
            level_of(id).unwrap_or(Level::Restart),
            Outcome::NotRun,
            "not run: needs a restart",
        ),
    }
}

/// Test ids are the static strings from [`TESTS`]; map back to them.
fn static_id(id: &str) -> &'static str {
    TESTS.iter().find(|(t, _)| *t == id).map_or("unknown", |(t, _)| t)
}

/// Gain in RPM that counts as "the fans sped up".
const FAN_GAIN: u16 = 1000;
/// Above this the fans may have no 1000 RPM left to gain.
const FAN_ALREADY_FAST: u16 = 5000;

/// Max fans for up to 10 s; verified when either fan speeds up clearly.
/// Always ends with the fans on Auto.
fn fan_max(dev: &NitroDevice, sleep: Sleep) -> TestResult {
    let c = Level::Confirm;
    // Fan speeds only (a missing temperature must not spoil the test).
    let rpm = |dev: &NitroDevice| {
        let fans = [dev.sensor(Sensor::CpuFan), dev.sensor(Sensor::Gpu1Fan)];
        fans.into_iter().filter_map(|r| r.ok().flatten()).filter(|v| RPM_RANGE.contains(v)).max()
    };
    // Without a baseline a normal idle reading would look like a gain.
    let Some(start) = rpm(dev) else {
        return TestResult::new("fan_max", c, Outcome::Failed, "no fan speed reading");
    };
    let goal = start.saturating_add(FAN_GAIN);
    if let Err(e) = dev.set_fans_max() {
        let _ = dev.set_fans_auto();
        return TestResult::error("fan_max", c, &e);
    }
    let mut peak = start;
    for _ in 0..10 {
        sleep(Duration::from_secs(1));
        peak = peak.max(rpm(dev).unwrap_or(0));
        if peak >= goal {
            break;
        }
    }
    let restored = dev.set_fans_auto();
    let detail = format!("{start} -> {peak} RPM");
    match restored {
        Err(e) => TestResult::new("fan_max", c, Outcome::Failed, format!("{detail}; restoring Auto failed: {e}")),
        Ok(()) if peak >= goal => TestResult::new("fan_max", c, Outcome::Verified, detail),
        // Already spinning fast (hot laptop): accepted, but no gain to measure.
        Ok(()) if start >= FAN_ALREADY_FAST => {
            TestResult::new("fan_max", c, Outcome::Detected, format!("{detail}: fans were already fast"))
        }
        Ok(()) => TestResult::new("fan_max", c, Outcome::Failed, format!("{detail}: no clear change")),
    }
}

/// Switches to another mode, reads it back, switches back.
fn mode_switch(dev: &NitroDevice, sleep: Sleep) -> TestResult {
    let c = Level::Confirm;
    let original = match dev.operation_mode() {
        Ok(Some(m)) => m,
        Ok(None) => return TestResult::new("mode_switch", c, Outcome::NotSupported, ""),
        Err(e) => return TestResult::error("mode_switch", c, &e),
    };
    let target = if original == OperationMode::Default { OperationMode::Quiet } else { OperationMode::Default };
    let wait_for = |m: OperationMode| {
        (0..10).any(|_| {
            sleep(Duration::from_millis(150));
            matches!(dev.operation_mode(), Ok(Some(x)) if x == m)
        })
    };
    let switched = dev.set_operation_mode(target).map(|()| wait_for(target));
    let back = dev.set_operation_mode(original).map(|()| wait_for(original));
    let detail = format!("{original:?} -> {target:?} -> {original:?}");
    match (switched, back) {
        (Err(e), _) | (_, Err(e)) => TestResult::new("mode_switch", c, Outcome::Failed, format!("{detail}: {e}")),
        (Ok(true), Ok(true)) => TestResult::new("mode_switch", c, Outcome::Verified, detail),
        (Ok(false), _) => TestResult::new("mode_switch", c, Outcome::Failed, format!("{detail}: not read back")),
        (Ok(true), Ok(false)) => {
            TestResult::new("mode_switch", c, Outcome::Failed, format!("{detail}: original mode not read back"))
        }
    }
}

/// Toggles the 80 % limit, reads it back, restores it.
fn charge_limit(dev: &NitroDevice) -> TestResult {
    let c = Level::Confirm;
    let original = match dev.battery_health() {
        Ok(b) if b.limit_supported => b.limit_active,
        Ok(_) => return TestResult::new("charge_limit_write", c, Outcome::NotSupported, ""),
        Err(e) => return TestResult::error("charge_limit_write", c, &e),
    };
    let flipped = dev.set_charge_limit(!original).and_then(|()| dev.battery_health());
    let restored = dev.set_charge_limit(original).and_then(|()| dev.battery_health());
    match (flipped, restored) {
        (Err(e), _) | (_, Err(e)) => TestResult::error("charge_limit_write", c, &e),
        (Ok(f), Ok(r)) if f.limit_active == !original && r.limit_active == original => {
            TestResult::new("charge_limit_write", c, Outcome::Verified, "toggled and restored")
        }
        (Ok(_), Ok(r)) => TestResult::new(
            "charge_limit_write",
            c,
            Outcome::Failed,
            format!("not read back; limit now {}", if r.limit_active { "on" } else { "off" }),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::DeviceProfile;
    use crate::mock::MockTransport;

    fn dev() -> NitroDevice {
        NitroDevice::new(Box::new(MockTransport::default()), DeviceProfile::default())
    }

    fn no_wait(_: Duration) {}

    #[test]
    fn read_only_covers_every_test_once_and_writes_nothing() {
        let mock = std::sync::Arc::new(MockTransport::default());
        let d = NitroDevice::new(Box::new(SharedMock(mock.clone())), DeviceProfile::default());
        let results = read_only(&d);
        let ids: Vec<_> = results.iter().map(|r| r.id).collect();
        let expected: Vec<_> = TESTS.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, expected);
        assert!(mock.calls().is_empty(), "read-only tests wrote to the firmware: {:?}", mock.calls());
        assert_eq!(mock.raw_writes(), 0, "read-only tests wrote battery settings");
        // Write tests are listed but not run.
        assert!(results
            .iter()
            .filter(|r| r.level != Level::ReadOnly)
            .all(|r| matches!(r.outcome, Outcome::NotRun | Outcome::NotSupported)));
        assert_eq!(results[0].outcome, Outcome::Verified);
        assert_eq!(results.iter().find(|r| r.id == "cpu_temp").unwrap().outcome, Outcome::Verified);
    }

    #[test]
    fn confirmed_tests_verify_and_restore() {
        let d = dev();
        let before = d.operation_mode().unwrap();
        let r = run_confirmed(&d, "mode_switch", None, &no_wait);
        assert_eq!(r.outcome, Outcome::Verified, "{r:?}");
        assert_eq!(d.operation_mode().unwrap(), before);

        let limit = d.battery_health().unwrap().limit_active;
        let r = run_confirmed(&d, "charge_limit_write", None, &no_wait);
        assert_eq!(r.outcome, Outcome::Verified, "{r:?}");
        assert_eq!(d.battery_health().unwrap().limit_active, limit);

        let mock = std::sync::Arc::new(MockTransport::default());
        let d = NitroDevice::new(Box::new(SharedMock(mock.clone())), DeviceProfile::default());
        let r = run_confirmed(&d, "fan_max", None, &no_wait);
        assert_eq!(r.outcome, Outcome::Verified, "{r:?}");
        // The test ends with the fans back on Auto.
        let auto = opensense_proto::fan::all_fans(opensense_proto::fan::FanBehavior::Auto);
        assert_eq!(mock.calls().last(), Some(&(M::SetGamingFanBehavior, auto)));
    }

    #[test]
    fn rgb_test_restores_saved_lighting() {
        let d = dev();
        let restored = std::cell::Cell::new(false);
        let restore = |_: &NitroDevice| {
            restored.set(true);
            Ok(())
        };
        let r = run_confirmed(&d, "rgb_write", Some(&restore), &no_wait);
        assert_eq!(r.outcome, Outcome::Detected);
        assert!(restored.get());
    }

    #[test]
    fn restart_tests_are_never_run() {
        let mock = std::sync::Arc::new(MockTransport::default());
        let d = NitroDevice::new(Box::new(SharedMock(mock.clone())), DeviceProfile::default());
        for id in ["gpu_mux_switch", "overdrive_write", "nonsense"] {
            assert_eq!(run_confirmed(&d, id, None, &no_wait).outcome, Outcome::NotRun);
        }
        assert!(mock.calls().is_empty());
        assert_eq!(mock.raw_writes(), 0);
    }

    /// Lets a test keep a handle on the mock to inspect its writes.
    struct SharedMock(std::sync::Arc<MockTransport>);
    impl crate::GamingTransport for SharedMock {
        fn name(&self) -> &'static str {
            "mock"
        }
        fn set_u64(&self, m: M, i: u64) -> Result<u32, HwError> {
            self.0.set_u64(m, i)
        }
        fn get_u32(&self, m: M, i: u32) -> Result<u64, HwError> {
            self.0.get_u32(m, i)
        }
        fn set_bytes(&self, m: M, i: &[u8]) -> Result<u32, HwError> {
            self.0.set_bytes(m, i)
        }
        fn get_bytes(&self, m: M, i: u32) -> Result<Vec<u8>, HwError> {
            self.0.get_bytes(m, i)
        }
        fn call_raw(
            &self,
            class: &'static str,
            method: &'static str,
            inputs: &[(&'static str, crate::WmiValue)],
            outputs: &[&'static str],
        ) -> Result<Vec<crate::WmiValue>, HwError> {
            self.0.call_raw(class, method, inputs, outputs)
        }
    }
}
