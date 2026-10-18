//! Operation modes, GPU MUX, battery boost, AC state, LCD overdrive and
//! keyboard-backlight timeout.

use serde::{Deserialize, Serialize};

/// Platform profile. The UI offers Quiet, Default and Extreme; the other
/// values exist in the firmware interface.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OperationMode {
    Quiet = 0,
    Default = 1,
    Render = 2,
    Edit = 3,
    Extreme = 4,
    Turbo = 5,
    Eco = 6,
}

impl OperationMode {
    /// The modes shown in the UI.
    pub const UI: [OperationMode; 3] = [OperationMode::Quiet, OperationMode::Default, OperationMode::Extreme];

    pub fn from_u8(v: u8) -> Option<Self> {
        use OperationMode::*;
        Some(match v {
            0 => Quiet,
            1 => Default,
            2 => Render,
            3 => Edit,
            4 => Extreme,
            5 => Turbo,
            6 => Eco,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            OperationMode::Quiet => "Quiet",
            OperationMode::Default => "Default",
            OperationMode::Render => "Render",
            OperationMode::Edit => "Edit",
            OperationMode::Extreme => "Extreme",
            OperationMode::Turbo => "Turbo",
            OperationMode::Eco => "Eco",
        }
    }

    /// Extreme requires AC power and battery boost (`disable_GPU_overclock_control`).
    pub fn needs_full_power(self) -> bool {
        !matches!(self, OperationMode::Quiet | OperationMode::Default | OperationMode::Eco)
    }

    /// Quiet keeps both fans on Auto.
    pub fn forces_auto_fan(self) -> bool {
        self == OperationMode::Quiet
    }
}

/// Misc-setting id that carries the platform/operation profile.
///
/// Written through `SetGamingMiscSetting(0x0B | value << 8)`, the same
/// platform-profile setting the Linux `acer-wmi` driver uses (QUIET=0,
/// BALANCED=1, PERFORMANCE=4, TURBO=5, ECO=6).
pub const MISC_PLATFORM_PROFILE: u8 = 0x0B;
/// Misc-setting id of the GPU (MUX) mode.
pub const MISC_GPU_MODE: u8 = 0x02;
/// Misc-setting id reporting MUX support (value 3 = switchable).
pub const MISC_GPU_MODE_SUPPORT: u8 = 0x09;

/// `SetGamingMiscSetting` input.
pub fn misc_set_word(id: u8, value: u8) -> u64 {
    u64::from(id) | (u64::from(value) << 8)
}

/// `GetGamingMiscSetting` input.
pub fn misc_get_word(id: u8) -> u32 {
    u32::from(id)
}

/// Decodes a misc/profile-setting reply: status in the low byte (0 = ok),
/// value in the next byte.
pub fn byte1_if_ok(reply: u64) -> Option<u8> {
    (reply & 0xFF == 0).then_some(((reply >> 8) & 0xFF) as u8)
}

pub fn operation_mode_set_word(mode: OperationMode) -> u64 {
    misc_set_word(MISC_PLATFORM_PROFILE, mode as u8)
}

/// GPU MUX mode (`Set_System_GPUMode`): 1 = hybrid (Optimus), 2 = dGPU only.
/// Takes effect after a reboot.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuMode {
    Hybrid = 1,
    Discrete = 2,
    Unknown3 = 3,
}

impl GpuMode {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(GpuMode::Hybrid),
            2 => Some(GpuMode::Discrete),
            3 => Some(GpuMode::Unknown3),
            _ => None,
        }
    }
}

// ---- GetGamingSysInfo -------------------------------------------------------

/// Battery boost (the full-power adapter is attached): `GetGamingSysInfo(2)`,
/// status ok and byte 5 == 1.
pub fn battery_boost_decode(reply: u64) -> Option<bool> {
    (reply & 0xFF == 0).then_some((reply >> 40) & 0xFF == 1)
}

// ---- APGeAction.GetFunction / SetFunction -----------------------------------

/// AC adapter (USB-PD aware) state: `APGeAction.GetFunction(0x0A)`, bit 0
/// of byte 1. Only meaningful when `PD_Support == 1`.
pub const PROFILE_GET_AC: u32 = 0x0A;

pub fn ac_decode(reply: u64) -> Option<bool> {
    byte1_if_ok(reply).map(|b| b & 1 == 1)
}

/// USB-C DisplayPort wired to the dGPU: `APGeAction.GetFunction(0x108)`, bit 1 of byte 1.
pub const PROFILE_GET_TYPEC_DP: u32 = 0x108;

/// Keyboard backlight auto-off. `hotkey` is the per-model
/// keyboard-backlight function number (132 by default).
pub fn backlight_get_word(hotkey: u8) -> u32 {
    0x01 | (u32::from(hotkey) << 8) | 0x08_0000
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BacklightTimeout {
    /// Backlight turns off after 30 s idle.
    pub auto_off: bool,
    /// Brightness in percent.
    pub brightness: u8,
}

pub fn backlight_decode(reply: u64) -> Option<BacklightTimeout> {
    (reply & 0xFF == 0)
        .then_some(BacklightTimeout { auto_off: (reply >> 40) & 0xFF == 30, brightness: ((reply >> 32) & 0xFF) as u8 })
}

pub fn backlight_set_word(hotkey: u8, state: BacklightTimeout) -> u64 {
    0x02 | (u64::from(hotkey) << 8)
        | 0x08_0000
        | (u64::from(state.brightness.min(100)) << 32)
        | (if state.auto_off { 30u64 << 40 } else { 0 })
}

// ---- Set/GetGamingProfile ------------------------------------------------------

/// LCD overdrive.
pub fn lcd_overdrive_set_word(on: bool) -> u64 {
    0x10 | (u64::from(on) << 48)
}

/// `GetGamingProfile(0)` reply: LCD overdrive in byte 6.
/// `None` when unsupported: the firmware answers 0xFF in that byte.
pub fn lcd_overdrive_decode(reply: u64) -> Option<bool> {
    if reply & 0xFF != 0 {
        return None;
    }
    match (reply >> 48) & 0xFF {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words() {
        assert_eq!(operation_mode_set_word(OperationMode::Extreme), 0x040B);
        assert_eq!(misc_set_word(MISC_GPU_MODE, GpuMode::Discrete as u8), 0x0202);
        assert_eq!(backlight_get_word(132), 0x08_8401);
        let s = BacklightTimeout { auto_off: true, brightness: 100 };
        assert_eq!(backlight_set_word(132, s), 0x1E64_0008_8402);
        assert_eq!(backlight_decode(0x1E64_0000_0000), Some(s));
        assert_eq!(battery_boost_decode(0x0100_0000_0000), Some(true));
        assert_eq!(ac_decode(0x0100), Some(true));
        assert_eq!(lcd_overdrive_set_word(true), 0x0001_0000_0000_0010);
    }
}
