//! `GetGamingSysInfo` sensor reads.

use serde::{Deserialize, Serialize};

/// Sensor index.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Sensor {
    CpuTemp = 1,
    CpuFan = 2,
    SystemTemp = 3,
    SystemFan = 4,
    FrostCore = 5,
    Gpu1Fan = 6,
    System2Temp = 7,
    System2Fan = 8,
    Gpu2Fan = 9,
    Gpu1Temp = 10,
    Gpu2Temp = 11,
}

impl Sensor {
    pub const ALL: [Sensor; 11] = [
        Sensor::CpuTemp,
        Sensor::CpuFan,
        Sensor::SystemTemp,
        Sensor::SystemFan,
        Sensor::FrostCore,
        Sensor::Gpu1Fan,
        Sensor::System2Temp,
        Sensor::System2Fan,
        Sensor::Gpu2Fan,
        Sensor::Gpu1Temp,
        Sensor::Gpu2Temp,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Sensor::CpuTemp => "CPU temperature",
            Sensor::CpuFan => "CPU fan",
            Sensor::SystemTemp => "System temperature",
            Sensor::SystemFan => "System fan",
            Sensor::FrostCore => "FrostCore",
            Sensor::Gpu1Fan => "GPU fan",
            Sensor::System2Temp => "System 2 temperature",
            Sensor::System2Fan => "System 2 fan",
            Sensor::Gpu2Fan => "GPU 2 fan",
            Sensor::Gpu1Temp => "GPU temperature",
            Sensor::Gpu2Temp => "GPU 2 temperature",
        }
    }

    /// `GetGamingSysInfo` input: function 1 (health info) with the index in
    /// bits 8..16.
    pub fn request(self) -> u32 {
        1 | (u32::from(self as u8) << 8)
    }

    /// Decodes a reply: low byte is the status (0 = ok), bits 8..24 the
    /// value (°C for temperatures, RPM for fans).
    pub fn decode(reply: u64) -> Option<u16> {
        (reply & 0xFF == 0).then_some(((reply >> 8) & 0xFFFF) as u16)
    }
}

/// `GetGamingSysInfo` input 2: battery-boost state lives in bits 40..48 of
/// the reply.
pub const SYSINFO_BATTERY: u32 = 2;

pub fn celsius_to_fahrenheit(c: f32) -> f32 {
    c * 9.0 / 5.0 + 32.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_and_decode() {
        assert_eq!(Sensor::CpuTemp.request(), 0x0101);
        assert_eq!(Sensor::Gpu1Temp.request(), 0x0A01);
        assert_eq!(Sensor::decode(0x0000_0000_0014_9A00), Some(0x149A));
        assert_eq!(Sensor::decode(0x0000_0000_0000_3C03), None);
    }
}
