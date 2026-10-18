//! In-memory stand-in for `AcerGamingFunction`, used for UI development and
//! tests on machines without the Acer firmware interface.

use std::collections::HashMap;
use std::time::Instant;

use opensense_proto::sensor::Sensor;
use opensense_proto::GamingMethod;
use parking_lot::Mutex;

use crate::{GamingTransport, HwError, WmiValue};

pub struct MockTransport {
    started: Instant,
    state: Mutex<MockState>,
}

#[derive(Default)]
struct MockState {
    /// Last input written per Set method.
    writes: HashMap<GamingMethod, u64>,
    /// Profile/misc settings keyed by function id (low byte).
    settings: HashMap<(GamingMethod, u8), u64>,
    log: Vec<(GamingMethod, u64)>,
    charge_limit: bool,
}

impl Default for MockTransport {
    fn default() -> Self {
        let mut state = MockState::default();
        // Start in Default mode like a freshly booted laptop.
        state.settings.insert((GamingMethod::SetGamingMiscSetting, opensense_proto::mode::MISC_PLATFORM_PROFILE), 1);
        Self { started: Instant::now(), state: Mutex::new(state) }
    }
}

impl MockTransport {
    /// Every Set call made so far, oldest first.
    pub fn calls(&self) -> Vec<(GamingMethod, u64)> {
        self.state.lock().log.clone()
    }

    fn wave(&self, base: f32, amp: f32, period_s: f32) -> u16 {
        let t = self.started.elapsed().as_secs_f32();
        (base + amp * (t * std::f32::consts::TAU / period_s).sin()).max(0.0) as u16
    }

    fn sensor(&self, s: Sensor) -> Option<u16> {
        let fan_max = self.state.lock().writes.get(&GamingMethod::SetGamingFanBehavior).copied() == Some(0x82_0009);
        Some(match s {
            Sensor::CpuTemp => self.wave(58.0, 12.0, 23.0),
            Sensor::Gpu1Temp => self.wave(51.0, 9.0, 31.0),
            Sensor::CpuFan if fan_max => 5800,
            Sensor::Gpu1Fan if fan_max => 5600,
            Sensor::CpuFan => self.wave(3200.0, 900.0, 17.0),
            Sensor::Gpu1Fan => self.wave(3000.0, 800.0, 19.0),
            _ => return None,
        })
    }
}

impl GamingTransport for MockTransport {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn set_u64(&self, method: GamingMethod, input: u64) -> Result<u32, HwError> {
        let mut st = self.state.lock();
        st.writes.insert(method, input);
        st.log.push((method, input));
        if matches!(method, GamingMethod::ApgeSetFunction | GamingMethod::SetGamingMiscSetting) {
            st.settings.insert((method, (input & 0xFF) as u8), input >> 8);
        }
        Ok(0)
    }

    fn get_u32(&self, method: GamingMethod, input: u32) -> Result<u64, HwError> {
        match method {
            GamingMethod::GetGamingSysInfo => {
                let idx = ((input >> 8) & 0xFF) as u8;
                let sensor = Sensor::ALL.iter().copied().find(|s| *s as u8 == idx);
                Ok(match (input & 0xFF, sensor.and_then(|s| self.sensor(s))) {
                    (1, Some(v)) => u64::from(v) << 8,
                    _ => 0x03,
                })
            }
            GamingMethod::ApgeGetFunction | GamingMethod::GetGamingMiscSetting => {
                let set = if method == GamingMethod::ApgeGetFunction {
                    GamingMethod::ApgeSetFunction
                } else {
                    GamingMethod::SetGamingMiscSetting
                };
                let st = self.state.lock();
                let v = st.settings.get(&(set, (input & 0xFF) as u8)).copied().unwrap_or(0);
                Ok((v & 0xFF) << 8)
            }
            _ => Ok(0),
        }
    }

    fn set_bytes(&self, method: GamingMethod, input: &[u8]) -> Result<u32, HwError> {
        let mut word = [0u8; 8];
        let n = input.len().min(8);
        word[..n].copy_from_slice(&input[..n]);
        self.set_u64(method, u64::from_le_bytes(word))
    }

    fn get_bytes(&self, _method: GamingMethod, _input: u32) -> Result<Vec<u8>, HwError> {
        Ok(vec![0; 16])
    }

    fn call_raw(
        &self,
        class: &'static str,
        method: &'static str,
        inputs: &[(&'static str, WmiValue)],
        _outputs: &[&'static str],
    ) -> Result<Vec<WmiValue>, HwError> {
        use opensense_proto::battery;
        if class != battery::CLASS {
            return Err(HwError::Unsupported(method));
        }
        let mut st = self.state.lock();
        if method == battery::SET_CONTROL {
            let on = inputs.iter().find(|(n, _)| *n == "uFunctionStatus").and_then(|(_, v)| v.as_int()) == Some(1);
            st.charge_limit = on;
            return Ok(vec![WmiValue::Int(0)]);
        }
        Ok(vec![WmiValue::Int(3), WmiValue::Bytes(vec![u8::from(st.charge_limit), 0, 0, 0, 0])])
    }
}
