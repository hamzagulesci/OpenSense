//! Fan control: `SetGamingFanBehavior` / `SetGamingFanSpeed` words and
//! CoolBoost.

use serde::{Deserialize, Serialize};

/// Fan group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FanGroup {
    Cpu,
    Gpu,
}

impl FanGroup {
    /// Bit in the low 16 bits of a FanGroupBehaviorSet word.
    const fn behavior_mask(self) -> u64 {
        match self {
            FanGroup::Cpu => 0x01,
            FanGroup::Gpu => 0x08,
        }
    }
    /// Shift of the 2-bit behavior code for this group.
    const fn behavior_shift(self) -> u32 {
        match self {
            FanGroup::Cpu => 16,
            FanGroup::Gpu => 22,
        }
    }
    /// Selector byte of a FanGroupSpeedSet word.
    const fn speed_selector(self) -> u64 {
        match self {
            FanGroup::Cpu => 0x01,
            FanGroup::Gpu => 0x04,
        }
    }
}

/// Per-fan behavior code used by the firmware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum FanBehavior {
    Auto = 1,
    Max = 2,
    Custom = 3,
}

/// Builds a `SetGamingFanBehavior` input.
pub fn behavior_word(fans: &[(FanGroup, FanBehavior)]) -> u64 {
    fans.iter().fold(0u64, |acc, &(g, b)| acc | g.behavior_mask() | ((b as u64) << g.behavior_shift()))
}

/// Both fans to the same behavior (`set_all_fan_mode`).
/// Auto -> 0x410009, Max -> 0x820009.
pub fn all_fans(behavior: FanBehavior) -> u64 {
    behavior_word(&[(FanGroup::Cpu, behavior), (FanGroup::Gpu, behavior)])
}

/// Builds a `SetGamingFanSpeed` input. `percent` is clamped
/// to 0..=100.
pub fn speed_word(group: FanGroup, percent: u8) -> u64 {
    group.speed_selector() | (u64::from(percent.min(100)) << 8)
}

/// Status byte of a Set* reply: zero means success.
pub fn set_ok(status: u32) -> bool {
    status & 0xFF == 0
}

/// CoolBoost: `APGeAction.SetFunction` word.
pub fn coolboost_set_word(on: bool) -> u64 {
    7 | (u64::from(on) << 16)
}

/// CoolBoost readback input for `APGeAction.GetFunction`.
pub const COOLBOOST_GET: u32 = 0x207;

pub fn coolboost_decode(reply: u64) -> Option<bool> {
    (reply & 0xFF == 0).then_some((reply >> 8) & 0xFF == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fan_words() {
        assert_eq!(all_fans(FanBehavior::Auto), 0x41_0009);
        assert_eq!(all_fans(FanBehavior::Max), 0x82_0009);
        assert_eq!(all_fans(FanBehavior::Custom), 0xC3_0009);
        assert_eq!(behavior_word(&[(FanGroup::Cpu, FanBehavior::Custom)]), 0x3_0001);
        assert_eq!(behavior_word(&[(FanGroup::Gpu, FanBehavior::Custom)]), 0xC0_0008);
        assert_eq!(speed_word(FanGroup::Cpu, 70), 0x4601);
        assert_eq!(speed_word(FanGroup::Gpu, 100), 0x6404);
        assert_eq!(speed_word(FanGroup::Gpu, 200), 0x6404);
        assert_eq!(coolboost_set_word(true), 0x1_0007);
        assert_eq!(coolboost_decode(0x100), Some(true));
        assert_eq!(coolboost_decode(0x001), None);
    }
}
