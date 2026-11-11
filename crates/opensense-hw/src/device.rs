//! High-level device API: one method per feature, each mapping to the
//! firmware word(s) it needs.

use opensense_proto::battery::{self, BatteryHealth};
use opensense_proto::fan::{self, FanBehavior, FanGroup};
use opensense_proto::lighting::{self, Direction, Effect, Rgb, ZONES};
use opensense_proto::mode::{self, BacklightTimeout, GpuMode, OperationMode};
use opensense_proto::sensor::Sensor;
use opensense_proto::GamingMethod as M;

use crate::{GamingTransport, HwError, WmiValue};

/// Per-model knobs (optional OEM registry/ini values, sensible defaults).
#[derive(Debug, Clone)]
pub struct DeviceProfile {
    /// Keyboard-backlight function number (132 by default).
    pub backlight_hotkey: u8,
    /// `HW_Support.ini [ZoneColorAdjust]`.
    pub zone_color_adjust: [f32; 3],
    /// `PD_Support == 1`: read AC state from firmware instead of Windows.
    pub pd_support: bool,
}

impl Default for DeviceProfile {
    fn default() -> Self {
        Self { backlight_hotkey: 132, zone_color_adjust: [1.0, 1.0, 1.0], pd_support: true }
    }
}

/// Features the firmware answers for, probed with read-only calls so
/// OpenSense adapts to each model without vendor configuration files.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// Quiet/Default/Extreme platform profile (misc setting 0x0B).
    pub operation_modes: bool,
    /// 4-zone RGB keyboard (`GetGamingRgbKb`).
    pub rgb_keyboard: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SensorReadings {
    pub cpu_temp: Option<u16>,
    pub gpu_temp: Option<u16>,
    pub cpu_fan_rpm: Option<u16>,
    pub gpu_fan_rpm: Option<u16>,
}

/// Custom fan state for both groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CustomFan {
    /// `None` = this fan stays on Auto inside Custom mode.
    pub cpu_percent: Option<u8>,
    pub gpu_percent: Option<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StaticLighting {
    pub zones: [Rgb; ZONES],
    pub enabled: [bool; ZONES],
    pub brightness_percent: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DynamicLighting {
    pub effect: Effect,
    pub speed: u8,
    pub brightness_percent: u8,
    pub direction: Direction,
    pub color: Rgb,
}

pub struct NitroDevice {
    transport: Box<dyn GamingTransport>,
    profile: DeviceProfile,
}

fn check(method: M, status: u32) -> Result<(), HwError> {
    if fan::set_ok(status) {
        Ok(())
    } else {
        Err(HwError::Firmware { method: method.wmi_name(), status })
    }
}

impl NitroDevice {
    pub fn new(transport: Box<dyn GamingTransport>, profile: DeviceProfile) -> Self {
        Self { transport, profile }
    }

    pub fn transport_name(&self) -> &'static str {
        self.transport.name()
    }

    pub fn profile(&self) -> &DeviceProfile {
        &self.profile
    }

    fn set(&self, method: M, word: u64) -> Result<(), HwError> {
        tracing::debug!(method = method.wmi_name(), word = format_args!("{word:#x}"), "set");
        check(method, self.transport.set_u64(method, word)?)
    }

    fn get(&self, method: M, input: u32) -> Result<u64, HwError> {
        let v = self.transport.get_u32(method, input)?;
        tracing::trace!(
            method = method.wmi_name(),
            input = format_args!("{input:#x}"),
            out = format_args!("{v:#x}"),
            "get"
        );
        Ok(v)
    }

    /// Raw `Set*` call returning the status word, for diagnostics.
    pub fn raw_set(&self, method: M, input: u64) -> Result<u32, HwError> {
        self.transport.set_u64(method, input)
    }

    /// Raw byte-array `Set*` call (u64 as 16 LE bytes), for diagnostics.
    pub fn raw_set_bytes(&self, method: M, input: u64) -> Result<u32, HwError> {
        self.transport.set_bytes(method, &lighting::kb_backlight_bytes(input))
    }

    /// Raw `Get*` call, for diagnostics.
    pub fn raw_get(&self, method: M, input: u32) -> Result<u64, HwError> {
        self.get(method, input)
    }

    /// Read-only capability probe.
    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            operation_modes: matches!(self.operation_mode(), Ok(Some(_))),
            rgb_keyboard: self.get(M::GetGamingRgbKb, 1).is_ok_and(|v| v & 0xFF == 0),
        }
    }

    // ---- sensors -------------------------------------------------------

    pub fn sensor(&self, s: Sensor) -> Result<Option<u16>, HwError> {
        Ok(Sensor::decode(self.get(M::GetGamingSysInfo, s.request())?))
    }

    /// Temperatures (only if > 0) and fan speeds.
    pub fn readings(&self) -> Result<SensorReadings, HwError> {
        let pos = |v: Option<u16>| v.filter(|&x| x > 0);
        Ok(SensorReadings {
            cpu_temp: pos(self.sensor(Sensor::CpuTemp)?),
            gpu_temp: pos(self.sensor(Sensor::Gpu1Temp)?),
            cpu_fan_rpm: self.sensor(Sensor::CpuFan)?,
            gpu_fan_rpm: self.sensor(Sensor::Gpu1Fan)?,
        })
    }

    // ---- fans ----------------------------------------------------------

    pub fn set_fans_auto(&self) -> Result<(), HwError> {
        self.set(M::SetGamingFanBehavior, fan::all_fans(FanBehavior::Auto))
    }

    pub fn set_fans_max(&self) -> Result<(), HwError> {
        self.set(M::SetGamingFanBehavior, fan::all_fans(FanBehavior::Max))
    }

    /// `set_all_custom_fan_state`: one behavior word for both fans, then a
    /// speed word per fan (sent even for fans left on Auto).
    pub fn set_fans_custom(&self, c: CustomFan) -> Result<(), HwError> {
        let b = |p: Option<u8>| if p.is_some() { FanBehavior::Custom } else { FanBehavior::Auto };
        self.set(
            M::SetGamingFanBehavior,
            fan::behavior_word(&[(FanGroup::Cpu, b(c.cpu_percent)), (FanGroup::Gpu, b(c.gpu_percent))]),
        )?;
        let speeds = self
            .set(M::SetGamingFanSpeed, fan::speed_word(FanGroup::Cpu, c.cpu_percent.unwrap_or(0)))
            .and_then(|()| self.set(M::SetGamingFanSpeed, fan::speed_word(FanGroup::Gpu, c.gpu_percent.unwrap_or(0))));
        if speeds.is_err() {
            // Custom behaviour without a valid speed: do not leave the fans
            // at whatever the firmware last held.
            let _ = self.set_fans_auto();
        }
        speeds
    }

    pub fn coolboost(&self) -> Result<Option<bool>, HwError> {
        Ok(fan::coolboost_decode(self.get(M::ApgeGetFunction, fan::COOLBOOST_GET)?))
    }

    pub fn set_coolboost(&self, on: bool) -> Result<(), HwError> {
        self.set(M::ApgeSetFunction, fan::coolboost_set_word(on))
    }

    // ---- operation mode / power ---------------------------------------

    pub fn operation_mode(&self) -> Result<Option<OperationMode>, HwError> {
        let r = self.get(M::GetGamingMiscSetting, mode::misc_get_word(mode::MISC_PLATFORM_PROFILE))?;
        Ok(mode::byte1_if_ok(r).and_then(OperationMode::from_u8))
    }

    pub fn set_operation_mode(&self, m: OperationMode) -> Result<(), HwError> {
        self.set(M::SetGamingMiscSetting, mode::operation_mode_set_word(m))?;
        if m.forces_auto_fan() {
            self.set_fans_auto()?;
        }
        Ok(())
    }

    /// AC adapter present, from firmware when the model reports USB-PD.
    pub fn ac_from_firmware(&self) -> Result<Option<bool>, HwError> {
        if !self.profile.pd_support {
            return Ok(None);
        }
        Ok(mode::ac_decode(self.get(M::ApgeGetFunction, mode::PROFILE_GET_AC)?))
    }

    pub fn battery_boost(&self) -> Result<Option<bool>, HwError> {
        Ok(mode::battery_boost_decode(self.get(M::GetGamingSysInfo, opensense_proto::sensor::SYSINFO_BATTERY)?))
    }

    // ---- GPU MUX -------------------------------------------------------

    pub fn gpu_mode_supported(&self) -> Result<bool, HwError> {
        let r = self.get(M::GetGamingMiscSetting, mode::misc_get_word(mode::MISC_GPU_MODE_SUPPORT))?;
        Ok(mode::byte1_if_ok(r) == Some(3))
    }

    pub fn gpu_mode(&self) -> Result<Option<GpuMode>, HwError> {
        let r = self.get(M::GetGamingMiscSetting, mode::misc_get_word(mode::MISC_GPU_MODE))?;
        Ok(mode::byte1_if_ok(r).and_then(GpuMode::from_u8))
    }

    /// Takes effect after a reboot.
    pub fn set_gpu_mode(&self, m: GpuMode) -> Result<(), HwError> {
        self.set(M::SetGamingMiscSetting, mode::misc_set_word(mode::MISC_GPU_MODE, m as u8))
    }

    // ---- advanced settings --------------------------------------------

    pub fn lcd_overdrive(&self) -> Result<Option<bool>, HwError> {
        Ok(mode::lcd_overdrive_decode(self.get(M::GetGamingProfile, 0)?))
    }

    pub fn set_lcd_overdrive(&self, on: bool) -> Result<(), HwError> {
        self.set(M::SetGamingProfile, mode::lcd_overdrive_set_word(on))
    }

    pub fn backlight_timeout(&self) -> Result<Option<BacklightTimeout>, HwError> {
        let r = self.get(M::ApgeGetFunction, mode::backlight_get_word(self.profile.backlight_hotkey))?;
        Ok(mode::backlight_decode(r))
    }

    pub fn set_backlight_timeout(&self, s: BacklightTimeout) -> Result<(), HwError> {
        self.set(M::ApgeSetFunction, mode::backlight_set_word(self.profile.backlight_hotkey, s))
    }

    // ---- battery ---------------------------------------------------------

    /// Health-mode (80 % charge limit) and calibration status.
    pub fn battery_health(&self) -> Result<BatteryHealth, HwError> {
        let out = self.transport.call_raw(
            battery::CLASS,
            battery::GET_STATUS,
            &[
                ("uBatteryNo", WmiValue::Int(battery::BATTERY_NO.into())),
                ("uFunctionQuery", WmiValue::Int(battery::FN_HEALTH_MODE.into())),
                ("uReserved", WmiValue::Bytes(vec![0, 0])),
            ],
            &["uFunctionList", "uFunctionStatus"],
        )?;
        let list = out.first().and_then(WmiValue::as_int).unwrap_or(0) as u8;
        Ok(battery::decode(list, out.get(1).map(WmiValue::as_bytes).unwrap_or(&[])))
    }

    /// Turns the 80 % charge limit (Acer "health mode") on or off.
    ///
    /// Succeeds only when the limit reads back as requested: some firmware
    /// answers without a `uReturn` status, so the status alone is not proof.
    pub fn set_charge_limit(&self, on: bool) -> Result<(), HwError> {
        let out = self.transport.call_raw(
            battery::CLASS,
            battery::SET_CONTROL,
            &[
                ("uBatteryNo", WmiValue::Int(battery::BATTERY_NO.into())),
                ("uFunctionMask", WmiValue::Int(battery::FN_HEALTH_MODE.into())),
                ("uFunctionStatus", WmiValue::Int(u32::from(on))),
                ("uReservedIn", WmiValue::Bytes(vec![0; 5])),
            ],
            &["uReturn"],
        )?;
        if let Some(status) = out.first().and_then(WmiValue::as_int).filter(|&s| s != 0) {
            return Err(HwError::Firmware { method: battery::SET_CONTROL, status });
        }
        // Give slow firmware one short second chance before calling it failed.
        for attempt in 0..2 {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
            if self.battery_health()?.limit_active == on {
                return Ok(());
            }
        }
        Err(HwError::NotApplied(battery::SET_CONTROL))
    }

    // ---- keyboard lighting ---------------------------------------------

    fn kb_backlight(&self, word: u64) -> Result<(), HwError> {
        tracing::debug!(word = format_args!("{word:#x}"), "SetGamingKBBacklight");
        let status = self.transport.set_bytes(M::SetGamingKbBacklight, &lighting::kb_backlight_bytes(word))?;
        check(M::SetGamingKbBacklight, status)
    }

    /// Static colours: zone mask, brightness, then colours.
    pub fn apply_static_lighting(&self, l: &StaticLighting) -> Result<(), HwError> {
        // Some firmware rejects the zone mask (status 1), so it is
        // best-effort and disabled zones are additionally painted black.
        if let Err(e) = self.set(M::SetGamingLedBehavior, lighting::zone_mask_word(l.enabled)) {
            tracing::debug!("zone mask not supported: {e}");
        }
        self.kb_backlight(lighting::static_brightness_word(l.brightness_percent))?;
        for (i, c) in l.zones.iter().enumerate() {
            let c = if l.enabled[i] { c.adjusted(self.profile.zone_color_adjust) } else { Rgb::default() };
            self.set(M::SetGamingRgbKb, lighting::zone_color_word(i, c))?;
        }
        Ok(())
    }

    pub fn apply_dynamic_lighting(&self, d: &DynamicLighting) -> Result<(), HwError> {
        let color = d.color.adjusted(self.profile.zone_color_adjust);
        self.kb_backlight(lighting::effect_word(d.effect, d.speed, d.brightness_percent, d.direction, color))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockTransport;
    use std::sync::Arc;

    struct Shared(Arc<MockTransport>);
    impl GamingTransport for Shared {
        fn name(&self) -> &'static str {
            "shared-mock"
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
    }

    fn device() -> (NitroDevice, Arc<MockTransport>) {
        let mock = Arc::new(MockTransport::default());
        (NitroDevice::new(Box::new(Shared(mock.clone())), DeviceProfile::default()), mock)
    }

    #[test]
    fn custom_fan_sequence() {
        let (dev, mock) = device();
        dev.set_fans_custom(CustomFan { cpu_percent: Some(70), gpu_percent: None }).unwrap();
        assert_eq!(
            mock.calls(),
            vec![(M::SetGamingFanBehavior, 0x43_0009), (M::SetGamingFanSpeed, 0x4601), (M::SetGamingFanSpeed, 0x0004),]
        );
    }

    #[test]
    fn quiet_mode_forces_auto_fans_and_reads_back() {
        let (dev, mock) = device();
        dev.set_operation_mode(OperationMode::Quiet).unwrap();
        assert_eq!(mock.calls(), vec![(M::SetGamingMiscSetting, 0x000B), (M::SetGamingFanBehavior, 0x41_0009)]);
        dev.set_operation_mode(OperationMode::Extreme).unwrap();
        assert_eq!(dev.operation_mode().unwrap(), Some(OperationMode::Extreme));
    }

    #[test]
    fn static_lighting_order() {
        let (dev, mock) = device();
        let l = StaticLighting {
            zones: [Rgb::new(255, 0, 0); 4],
            enabled: [true, true, false, false],
            brightness_percent: 100,
        };
        dev.apply_static_lighting(&l).unwrap();
        let calls = mock.calls();
        assert_eq!(calls[0], (M::SetGamingLedBehavior, 0x0300_0000_0008));
        assert_eq!(calls[1].0, M::SetGamingKbBacklight);
        assert_eq!(calls[2], (M::SetGamingRgbKb, 0xFF01));
        assert_eq!(calls[3], (M::SetGamingRgbKb, 0xFF02));
        assert_eq!(calls[4], (M::SetGamingRgbKb, 0x04));
        assert_eq!(calls[5], (M::SetGamingRgbKb, 0x08));
        assert_eq!(calls.len(), 6);
    }

    /// Battery firmware that answers the write without a `uReturn` and
    /// applies it only when `applies` is set.
    struct SilentBattery {
        applies: bool,
        active: std::sync::Mutex<bool>,
    }
    impl GamingTransport for SilentBattery {
        fn name(&self) -> &'static str {
            "silent-battery"
        }
        fn set_u64(&self, _: M, _: u64) -> Result<u32, HwError> {
            Ok(0)
        }
        fn get_u32(&self, _: M, _: u32) -> Result<u64, HwError> {
            Ok(0)
        }
        fn set_bytes(&self, _: M, _: &[u8]) -> Result<u32, HwError> {
            Ok(0)
        }
        fn get_bytes(&self, _: M, _: u32) -> Result<Vec<u8>, HwError> {
            Ok(vec![0; 16])
        }
        fn call_raw(
            &self,
            _class: &'static str,
            method: &'static str,
            inputs: &[(&'static str, WmiValue)],
            _outputs: &[&'static str],
        ) -> Result<Vec<WmiValue>, HwError> {
            let mut active = self.active.lock().unwrap();
            if method == battery::SET_CONTROL {
                if self.applies {
                    *active = inputs.iter().any(|(n, v)| *n == "uFunctionStatus" && v.as_int() == Some(1));
                }
                return Ok(Vec::new());
            }
            Ok(vec![WmiValue::Int(3), WmiValue::Bytes(vec![u8::from(*active), 0, 0, 0, 0])])
        }
    }

    #[test]
    fn charge_limit_success_means_read_back() {
        let dev = |applies| {
            NitroDevice::new(Box::new(SilentBattery { applies, active: Default::default() }), DeviceProfile::default())
        };
        dev(true).set_charge_limit(true).unwrap();
        assert!(matches!(dev(false).set_charge_limit(true), Err(HwError::NotApplied(_))));
        // Already in the requested state: nothing to change, so it is fine.
        dev(false).set_charge_limit(false).unwrap();
    }

    #[test]
    fn mock_charge_limit_round_trip() {
        let mock = MockTransport::default();
        let dev = NitroDevice::new(Box::new(mock), DeviceProfile::default());
        dev.set_charge_limit(true).unwrap();
        assert!(dev.battery_health().unwrap().limit_active);
        dev.set_charge_limit(false).unwrap();
        assert!(!dev.battery_health().unwrap().limit_active);
    }

    #[test]
    fn sensors_decode() {
        let (dev, _) = device();
        let r = dev.readings().unwrap();
        assert!(r.cpu_temp.is_some() && r.cpu_fan_rpm.is_some());
    }
}
