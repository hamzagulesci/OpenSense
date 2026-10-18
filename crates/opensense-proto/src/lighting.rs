//! 4-zone RGB keyboard.
//!
//! | WMI method              | Purpose                         |
//! |-------------------------|---------------------------------|
//! | `SetGamingRgbKb(u64)`   | static colour of one zone       |
//! | `SetGamingLEDBehavior`  | which zones are lit             |
//! | `SetGamingKBBacklight`  | brightness and dynamic effects  |
//!
//! `SetGamingKBBacklight` takes a byte array; OpenSense builds a 64-bit word
//! and sends its little-endian bytes padded to 16 bytes, the buffer size the
//! Linux community driver uses for this method.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Per-channel correction (`HW_Support.ini [ZoneColorAdjust]`); each
    /// channel is sent as `floor(c * adj)`.
    pub fn adjusted(self, adj: [f32; 3]) -> Self {
        let f = |c: u8, a: f32| ((f32::from(c) * a).floor()).clamp(0.0, 255.0) as u8;
        Self { r: f(self.r, adj[0]), g: f(self.g, adj[1]), b: f(self.b, adj[2]) }
    }

    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('#');
        if s.len() != 6 {
            return None;
        }
        let v = u32::from_str_radix(s, 16).ok()?;
        Some(Self::new((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }
}

/// Keyboard zones, left to right.
pub const ZONES: usize = 4;

/// Static colour of one zone (0..4) for `SetGamingRgbKb`.
pub fn zone_color_word(zone: usize, c: Rgb) -> u64 {
    debug_assert!(zone < ZONES);
    (1u64 << zone) | (u64::from(c.r) << 8) | (u64::from(c.g) << 16) | (u64::from(c.b) << 24)
}

/// Lit-zone mask for `SetGamingLEDBehavior` (bit i = zone i).
pub fn zone_mask_word(enabled: [bool; ZONES]) -> u64 {
    let mask = enabled.iter().enumerate().fold(0u64, |m, (i, &on)| m | (u64::from(on) << i));
    0x08 | (mask << 40)
}

/// Brightness level (1..=5) to percent.
/// Level 1 is "off".
pub fn level_to_percent(level: u8) -> u8 {
    (level.clamp(1, 5) - 1) * 25
}

pub fn percent_to_level(percent: u8) -> u8 {
    (percent.min(100) / 25) + 1
}

/// Dynamic effects. Discriminants are the firmware effect ids.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Effect {
    Static = 0,
    Breathing = 1,
    Neon = 2,
    Wave = 3,
    Shifting = 4,
    Zoom = 5,
}

impl Effect {
    pub const DYNAMIC: [Effect; 5] = [Effect::Breathing, Effect::Neon, Effect::Wave, Effect::Shifting, Effect::Zoom];

    pub fn label(self) -> &'static str {
        match self {
            Effect::Static => "Static",
            Effect::Breathing => "Breathing",
            Effect::Neon => "Neon",
            Effect::Wave => "Wave",
            Effect::Shifting => "Shifting",
            Effect::Zoom => "Zoom",
        }
    }

    pub fn uses_color(self) -> bool {
        matches!(self, Effect::Breathing | Effect::Shifting | Effect::Zoom | Effect::Wave)
    }

    pub fn uses_direction(self) -> bool {
        matches!(self, Effect::Wave | Effect::Shifting)
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Direction {
    /// UI label "Direction Right".
    Right = 1,
    /// UI label "Direction Left" (default).
    #[default]
    Left = 2,
}

/// Static backlight brightness (`Change_Keyboard_Brightness`).
pub fn static_brightness_word(percent: u8) -> u64 {
    u64::from(percent.min(100)) << 16
}

/// Dynamic effect (`Set_Dynamic_Keyboard`). `speed` is 1..=9.
pub fn effect_word(effect: Effect, speed: u8, brightness_percent: u8, direction: Direction, color: Rgb) -> u64 {
    let dir = if effect.uses_direction() { direction as u64 } else { 0 };
    let color = if effect.uses_color() { color } else { Rgb::default() };
    u64::from(effect as u8)
        | (u64::from(speed.clamp(1, 9)) << 8)
        | (u64::from(brightness_percent.min(100)) << 16)
        | (if effect == Effect::Wave { 0x08u64 << 24 } else { 0 })
        | (dir << 32)
        | (u64::from(color.r) << 40)
        | (u64::from(color.g) << 48)
        | (u64::from(color.b) << 56)
}

/// Byte buffer for `SetGamingKBBacklight`.
pub fn kb_backlight_bytes(word: u64) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&word.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts() {
        assert_eq!(zone_color_word(2, Rgb::new(0x11, 0x22, 0x33)), 0x3322_1104);
        assert_eq!(zone_mask_word([true, false, true, true]), 0x0D00_0000_0008);
        assert_eq!(static_brightness_word(75), 0x4B_0000);
        let w = effect_word(Effect::Wave, 5, 100, Direction::Left, Rgb::new(1, 2, 3));
        assert_eq!(w, 0x0302_0102_0864_0503);
        let w = effect_word(Effect::Neon, 9, 50, Direction::Left, Rgb::new(9, 9, 9));
        assert_eq!(w, 0x0032_0902);
        assert_eq!(level_to_percent(5), 100);
        assert_eq!(percent_to_level(50), 3);
        assert_eq!(Rgb::from_hex("#FF8000"), Some(Rgb::new(255, 128, 0)));
        assert_eq!(Rgb::new(200, 10, 10).adjusted([0.8, 1.0, 1.0]), Rgb::new(160, 10, 10));
    }
}
