//! OpenSense settings, stored as TOML in `%APPDATA%\OpenSense\config\config.toml`.
//!
//! Everything is kept per user in one human-readable file.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::SysError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FanModeCfg {
    #[default]
    Auto,
    Max,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TempUnit {
    #[default]
    Celsius,
    Fahrenheit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FanConfig {
    pub mode: FanModeCfg,
    /// Custom-mode speeds in percent (multiples of 10).
    pub cpu_percent: u8,
    pub gpu_percent: u8,
    /// Per-fan "Auto" inside Custom mode.
    pub cpu_auto: bool,
    pub gpu_auto: bool,
    pub coolboost: bool,
    /// One slider drives both fans.
    pub linked: bool,
}

impl Default for FanConfig {
    fn default() -> Self {
        Self {
            mode: FanModeCfg::Auto,
            cpu_percent: 50,
            gpu_percent: 50,
            cpu_auto: false,
            gpu_auto: false,
            coolboost: false,
            linked: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LightingKind {
    #[default]
    Static,
    Dynamic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LightingConfig {
    pub kind: LightingKind,
    /// `#RRGGBB` per zone.
    pub zones: [String; 4],
    pub zone_enabled: [bool; 4],
    /// Brightness level 1..=5 (1 = off).
    pub brightness_level: u8,
    /// Effect name: breathing, neon, wave, shifting, zoom.
    pub effect: String,
    pub speed: u8,
    /// "left" or "right".
    pub direction: String,
    pub effect_color: String,
}

impl Default for LightingConfig {
    fn default() -> Self {
        Self {
            kind: LightingKind::Static,
            zones: ["#FF0000".into(), "#FF0000".into(), "#FF0000".into(), "#FF0000".into()],
            zone_enabled: [true; 4],
            brightness_level: 5,
            effect: "wave".into(),
            speed: 5,
            direction: "left".into(),
            effect_color: "#FF0000".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// UI language: "auto" (follow Windows), "en" or "tr".
    pub language: String,
    /// "dark" (default), "oled" (pure black) or "light".
    pub theme: String,
    /// Reapply fan/mode/lighting settings when OpenSense starts.
    pub apply_on_start: bool,
    /// Operation mode name (quiet/default/extreme), `None` = leave as is.
    pub operation_mode: Option<String>,
    /// 80 % battery charge limit, `None` = leave as is.
    pub charge_limit: Option<bool>,
    pub fan: FanConfig,
    pub lighting: LightingConfig,
    pub temp_unit: TempUnit,
    /// Sensor poll period.
    pub poll_ms: u64,
    /// Samples kept for the charts.
    pub history_len: usize,
    /// Keep the tray agent running after the window closes.
    pub close_to_tray: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            language: "auto".into(),
            theme: "dark".into(),
            apply_on_start: true,
            operation_mode: None,
            charge_limit: None,
            fan: FanConfig::default(),
            lighting: LightingConfig::default(),
            temp_unit: TempUnit::Celsius,
            poll_ms: 1000,
            history_len: 720,
            close_to_tray: true,
        }
    }
}

impl AppConfig {
    pub fn path() -> Option<PathBuf> {
        directories::ProjectDirs::from("", "", "OpenSense").map(|d| d.config_dir().join("config.toml"))
    }

    /// Loads the config, falling back to defaults when missing or invalid.
    pub fn load() -> Self {
        let Some(path) = Self::path() else { return Self::default() };
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                // Keep the broken file for the user instead of overwriting it.
                let backup = path.with_extension("toml.bad");
                tracing::warn!("invalid {}: {e}; saved a copy as {}", path.display(), backup.display());
                let _ = std::fs::copy(&path, backup);
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), SysError> {
        let path = Self::path().ok_or_else(|| SysError::Config("no config directory".into()))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| SysError::Config(e.to_string()))?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_partial() {
        let c = AppConfig::default();
        let text = toml::to_string_pretty(&c).unwrap();
        assert_eq!(toml::from_str::<AppConfig>(&text).unwrap(), c);
        let partial: AppConfig = toml::from_str("poll_ms = 2000\n[fan]\nmode = \"max\"\n").unwrap();
        assert_eq!(partial.poll_ms, 2000);
        assert_eq!(partial.fan.mode, FanModeCfg::Max);
        assert_eq!(partial.fan.cpu_percent, 50);
    }
}
