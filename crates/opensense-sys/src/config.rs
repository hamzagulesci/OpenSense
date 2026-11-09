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

/// Settings of one performance mode: a built-in one ("quiet", "default",
/// "extreme") or a user mode ("custom1", ...) based on one of them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModeProfile {
    /// Display name of a user mode (empty for built-in modes).
    pub name: String,
    /// Firmware mode the profile runs on: quiet, default or extreme.
    pub base: String,
    /// Fan curves: 8 points of (°C, %), empty = default curve.
    pub cpu_curve: Vec<[u8; 2]>,
    pub gpu_curve: Vec<[u8; 2]>,
    /// Drive the fans from the curves (software loop in the tray agent).
    pub apply_curve: bool,
    /// Windows power mode key (efficiency/balanced/performance/high);
    /// `None` = the default for the base mode.
    pub power_mode: Option<String>,
    /// Processor boost mode index (0..=6); `None` = leave Windows alone.
    pub cpu_boost: Option<u8>,
    /// NVIDIA core clock limit in MHz; `None` = no limit.
    pub gpu_clock_limit: Option<u32>,
}

impl Default for ModeProfile {
    fn default() -> Self {
        Self {
            name: String::new(),
            base: "default".into(),
            cpu_curve: Vec::new(),
            gpu_curve: Vec::new(),
            apply_curve: false,
            power_mode: None,
            cpu_boost: None,
            gpu_clock_limit: None,
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
    /// Older on/off form of the charge limit (80 % or none); read only to
    /// migrate to `charge_target`.
    pub charge_limit: Option<bool>,
    /// Charge target in percent (80..=100), `None` = leave as is.
    pub charge_target: Option<u8>,
    pub fan: FanConfig,
    pub lighting: LightingConfig,
    pub temp_unit: TempUnit,
    /// Sensor poll period.
    pub poll_ms: u64,
    /// Samples kept for the charts.
    pub history_len: usize,
    /// Keep the tray agent running after the window closes.
    pub close_to_tray: bool,
    /// Main window theme: "windows" (follow the Windows app theme), "dark"
    /// or "light".
    pub ui_mode: String,
    /// Fan readings as RPM (false = percent of the highest speed seen).
    pub fan_rpm: bool,
    /// Battery charge shown in Wh instead of percent.
    pub charge_watt: bool,
    /// One-time charge to 100 % is active (cleared at full charge).
    pub charge_full: bool,
    /// Keep the main window above other windows.
    pub topmost: bool,
    /// Monochrome tray icon.
    pub bw_icon: bool,
    /// Current performance mode key; `None` = follow `operation_mode`.
    pub current_mode: Option<String>,
    /// Per-mode settings, keyed by mode key.
    pub profiles: std::collections::BTreeMap<String, ModeProfile>,
    /// Keep fan-curve points in their 10 °C columns.
    pub fan_clamp: bool,
    /// Highest CPU / GPU fan speed measured by "Calibrate".
    pub fan_max_rpm: Option<[u16; 2]>,
    /// Global keyboard shortcuts (Ctrl+Shift+F5 etc.).
    pub hotkeys: bool,
    /// Keyboard backlight off after this many idle seconds (0 = never),
    /// plugged in / on battery.
    pub kb_timeout_ac: u32,
    pub kb_timeout_battery: u32,
    /// Disable PCIe link power saving while plugged in.
    pub aspm_off: bool,
    /// Disable networking during Modern Standby.
    pub standby_network_off: bool,
    /// Keep the laptop awake with the lid closed when plugged in and an
    /// external display is connected.
    pub clamshell: bool,
    /// Lid action to restore when clamshell mode ends.
    pub clamshell_default_lid: Option<u32>,
    /// No on-screen notification when a shortcut changes something.
    pub disable_osd: bool,
    /// Mode last used on AC power / on battery (restored when the power
    /// source changes).
    pub mode_ac: Option<String>,
    pub mode_dc: Option<String>,
    /// Unix time of the last update check and the version it found.
    pub update_checked: Option<u64>,
    pub latest_version: Option<String>,
    /// Never check GitHub for updates.
    pub skip_updates: bool,
    /// In-game metrics overlay on, and only shown over games.
    pub overlay: bool,
    pub overlay_game_only: bool,
    /// Overlay layout: 0 Light, 1 Default, 2 Full.
    pub overlay_mode: u8,
    /// Overlay position (screen pixels), size in percent and background
    /// opacity (0..=255).
    pub overlay_pos: Option<[i32; 2]>,
    pub overlay_scale: u16,
    pub overlay_alpha: u8,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            language: "auto".into(),
            theme: "dark".into(),
            apply_on_start: true,
            operation_mode: None,
            charge_limit: None,
            charge_target: None,
            fan: FanConfig::default(),
            lighting: LightingConfig::default(),
            temp_unit: TempUnit::Celsius,
            poll_ms: 1000,
            history_len: 720,
            close_to_tray: true,
            ui_mode: "windows".into(),
            fan_rpm: true,
            charge_watt: false,
            charge_full: false,
            topmost: false,
            bw_icon: false,
            current_mode: None,
            profiles: Default::default(),
            fan_clamp: true,
            fan_max_rpm: None,
            hotkeys: true,
            kb_timeout_ac: 0,
            kb_timeout_battery: 60,
            aspm_off: false,
            standby_network_off: false,
            clamshell: false,
            clamshell_default_lid: None,
            disable_osd: false,
            mode_ac: None,
            mode_dc: None,
            update_checked: None,
            latest_version: None,
            skip_updates: false,
            overlay: false,
            overlay_game_only: false,
            overlay_mode: 1,
            overlay_pos: None,
            overlay_scale: 100,
            overlay_alpha: 128,
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

    /// Charge target, falling back to the older on/off setting.
    pub fn charge_target(&self) -> Option<u8> {
        self.charge_target.map(|t| t.clamp(80, 100)).or(self.charge_limit.map(|on| if on { 80 } else { 100 }))
    }

    pub fn set_charge_target(&mut self, target: u8) {
        self.charge_target = Some(target.clamp(80, 100));
        self.charge_limit = None;
    }

    pub fn save(&self) -> Result<(), SysError> {
        let path = Self::path().ok_or_else(|| SysError::Config("no config directory".into()))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| SysError::Config(e.to_string()))?;
        // Several OpenSense processes save this file: one writer at a
        // time, each with its own temp file, flushed before the rename so
        // a crash or power loss leaves the old or the new file, never half.
        let _lock = WriteLock::acquire();
        let tmp = path.with_extension(format!("toml.{}.tmp", std::process::id()));
        let written = (|| {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(text.as_bytes())?;
            f.sync_all()
        })();
        if let Err(e) = written.and_then(|()| std::fs::rename(&tmp, &path)) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        Ok(())
    }
}

/// Cross-process lock around config writes (a named mutex; released when
/// dropped). Waits at most 2 s, then writes anyway rather than lose the
/// change.
struct WriteLock(#[cfg(windows)] Option<windows::Win32::Foundation::HANDLE>);

impl WriteLock {
    fn acquire() -> Self {
        #[cfg(windows)]
        {
            use windows::core::w;
            use windows::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};
            let h = unsafe { CreateMutexW(None, false, w!("Local\\OpenSense.ConfigWrite")) }.ok();
            if let Some(h) = h {
                // WAIT_OBJECT_0 or WAIT_ABANDONED both mean we own it now.
                let _ = unsafe { WaitForSingleObject(h, 2000) };
            }
            Self(h)
        }
        #[cfg(not(windows))]
        Self()
    }
}

impl Drop for WriteLock {
    fn drop(&mut self) {
        #[cfg(windows)]
        if let Some(h) = self.0 {
            unsafe {
                let _ = windows::Win32::System::Threading::ReleaseMutex(h);
                let _ = windows::Win32::Foundation::CloseHandle(h);
            }
        }
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
