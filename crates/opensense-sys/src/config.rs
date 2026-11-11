//! OpenSense settings, stored as TOML in `%APPDATA%\OpenSense\config\config.toml`.
//!
//! Everything is kept per user in one human-readable file.

use std::path::{Path, PathBuf};

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
    /// Plugged-in PCIe link power setting the power plan had before
    /// `aspm_off` turned it off; restored when the option is turned off.
    pub aspm_previous: Option<u32>,
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
    /// Start type each Acer service had before OpenSense stopped it
    /// (see `tweaks::set_services`).
    pub service_start_types: std::collections::BTreeMap<String, u32>,
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
            aspm_previous: None,
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
            service_start_types: Default::default(),
        }
    }
}

impl AppConfig {
    pub fn path() -> Option<PathBuf> {
        directories::ProjectDirs::from("", "", "OpenSense").map(|d| d.config_dir().join("config.toml"))
    }

    /// Loads the config. A missing file gives the defaults; an invalid field
    /// falls back to its default (logged) while the rest is kept, and a copy
    /// of the file is saved as `config.toml.bad` whenever anything was
    /// dropped.
    pub fn load() -> Self {
        Self::path().map_or_else(Self::default, |p| Self::load_from(&p))
    }

    /// Loads the file, changes it with `f` and saves it, all under the
    /// cross-process write lock, so only the fields `f` touches change and
    /// a concurrent save from another window is not overwritten with a
    /// stale copy. Returns the saved config.
    pub fn update(f: impl FnOnce(&mut AppConfig)) -> Result<AppConfig, SysError> {
        let path = Self::path().ok_or_else(|| SysError::Config("no config directory".into()))?;
        Self::update_at(&path, f)
    }

    fn update_at(path: &Path, f: impl FnOnce(&mut AppConfig)) -> Result<AppConfig, SysError> {
        let _lock = WriteLock::acquire();
        let mut cfg = Self::load_from(path);
        f(&mut cfg);
        cfg.write_to(path)?;
        Ok(cfg)
    }

    /// Parses config text, keeping every valid field. Returns the config and
    /// the dotted names of the fields that were invalid and replaced by
    /// their defaults. Fails only when the text is not TOML at all.
    pub fn parse_lenient(text: &str) -> Result<(Self, Vec<String>), toml::de::Error> {
        if let Ok(cfg) = toml::from_str(text) {
            return Ok((cfg, Vec::new()));
        }
        let file: toml::Table = toml::from_str(text)?;
        // Every struct is `#[serde(default)]`, so an empty table is the
        // default config; add the file's fields one by one and keep each
        // one that still deserializes.
        let mut root = toml::Table::new();
        let mut dropped = Vec::new();
        merge_valid(&mut root, &mut Vec::new(), file, &mut dropped);
        let cfg = toml::Value::Table(root).try_into().unwrap_or_default();
        Ok((cfg, dropped))
    }

    fn load_from(path: &Path) -> Self {
        // Another OpenSense process may be replacing the file right now
        // (sharing violation); defaults returned for that would later be
        // saved over the real settings, so retry before giving up.
        let mut read = std::fs::read_to_string(path);
        for _ in 0..5 {
            match &read {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    read = std::fs::read_to_string(path);
                }
                _ => break,
            }
        }
        let keep_copy = |why: &dyn std::fmt::Display| {
            // Keep the broken file for the user instead of overwriting it.
            let backup = path.with_extension("toml.bad");
            tracing::warn!("invalid {}: {why}; saved a copy as {}", path.display(), backup.display());
            let _ = std::fs::copy(path, backup);
        };
        match read {
            Ok(text) => match Self::parse_lenient(&text) {
                Ok((cfg, dropped)) => {
                    if !dropped.is_empty() {
                        keep_copy(&format_args!("reset to default: {}", dropped.join(", ")));
                    }
                    cfg
                }
                Err(e) => {
                    keep_copy(&e);
                    Self::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => {
                keep_copy(&e);
                Self::default()
            }
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

    /// Saves this whole config. Prefer [`AppConfig::update`] for a change
    /// made from a copy that may be stale.
    pub fn save(&self) -> Result<(), SysError> {
        let path = Self::path().ok_or_else(|| SysError::Config("no config directory".into()))?;
        let _lock = WriteLock::acquire();
        self.write_to(&path)
    }

    /// Writes the file; the caller holds the [`WriteLock`].
    fn write_to(&self, path: &Path) -> Result<(), SysError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| SysError::Config(e.to_string()))?;
        // Several OpenSense processes save this file: one writer at a
        // time, each with its own temp file, flushed before the rename so
        // a crash or power loss leaves the old or the new file, never half.
        let tmp = path.with_extension(format!("toml.{}.tmp", std::process::id()));
        let written = (|| {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(text.as_bytes())?;
            f.sync_all()
        })();
        if let Err(e) = written.and_then(|()| std::fs::rename(&tmp, path)) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        Ok(())
    }
}

/// The table at `path` inside `root` (created by [`merge_valid`] before it
/// descends).
fn table_at<'a>(root: &'a mut toml::Table, path: &[String]) -> &'a mut toml::Table {
    path.iter().fold(root, |t, k| match t.get_mut(k) {
        Some(toml::Value::Table(sub)) => sub,
        _ => unreachable!("merge_valid creates every table it descends into"),
    })
}

/// Adds the fields of `incoming` (the file's table at `path`) to `root` one
/// at a time, keeping each one with which `root` still deserializes as an
/// [`AppConfig`]. An invalid table is merged field by field, so one bad
/// value only drops itself; dropped names are added to `dropped`.
fn merge_valid(root: &mut toml::Table, path: &mut Vec<String>, incoming: toml::Table, dropped: &mut Vec<String>) {
    let valid = |root: &toml::Table| toml::Value::Table(root.clone()).try_into::<AppConfig>().is_ok();
    for (key, value) in incoming {
        let prev = table_at(root, path).insert(key.clone(), value.clone());
        if valid(root) {
            continue;
        }
        let here = table_at(root, path);
        match prev {
            Some(p) => here.insert(key.clone(), p),
            None => here.remove(&key),
        };
        let name = path.iter().chain(Some(&key)).cloned().collect::<Vec<_>>().join(".");
        let toml::Value::Table(sub) = value else {
            dropped.push(name);
            continue;
        };
        // Descend: start from an empty table (all defaults) at this key.
        let here = table_at(root, path);
        if !matches!(here.get(&key), Some(toml::Value::Table(_))) {
            here.insert(key.clone(), toml::Value::Table(toml::Table::new()));
            if !valid(root) {
                table_at(root, path).remove(&key);
                dropped.push(name);
                continue;
            }
        }
        path.push(key);
        merge_valid(root, path, sub, dropped);
        path.pop();
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

    #[test]
    fn invalid_field_only_resets_itself() {
        let text = r##"
poll_ms = 2000
current_mode = "custom1"
temp_unit = "kelvin"
history_len = -5

[fan]
mode = "turbo"
cpu_percent = 70

[lighting]
zones = ["#00FF00", "#00FF00", "#00FF00"]
effect = "neon"

[profiles.custom1]
name = "Mine"
base = "extreme"
cpu_boost = 900

[profiles.quiet]
apply_curve = true
"##;
        let (c, mut dropped) = AppConfig::parse_lenient(text).unwrap();
        dropped.sort();
        assert_eq!(dropped, ["fan.mode", "history_len", "lighting.zones", "profiles.custom1.cpu_boost", "temp_unit"]);
        let d = AppConfig::default();
        assert_eq!(c.poll_ms, 2000);
        assert_eq!(c.current_mode.as_deref(), Some("custom1"));
        assert_eq!(c.temp_unit, d.temp_unit);
        assert_eq!(c.history_len, d.history_len);
        assert_eq!(c.fan.mode, FanModeCfg::Auto);
        assert_eq!(c.fan.cpu_percent, 70);
        assert_eq!(c.lighting.zones, d.lighting.zones);
        assert_eq!(c.lighting.effect, "neon");
        let p = &c.profiles["custom1"];
        assert_eq!((p.name.as_str(), p.base.as_str(), p.cpu_boost), ("Mine", "extreme", None));
        assert!(c.profiles["quiet"].apply_curve);
    }

    #[test]
    fn whole_table_of_wrong_type_is_dropped() {
        let (c, dropped) = AppConfig::parse_lenient("fan = 3\npoll_ms = 500\n").unwrap();
        assert_eq!(dropped, ["fan"]);
        assert_eq!((c.fan, c.poll_ms), (FanConfig::default(), 500));
        assert!(AppConfig::parse_lenient("this is = = not toml").is_err());
    }

    fn temp_config(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("opensense-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("config.toml")
    }

    #[test]
    fn load_keeps_bad_copy_only_when_something_was_dropped() {
        let path = temp_config("bad-copy");
        let bad = path.with_extension("toml.bad");
        std::fs::write(&path, "poll_ms = 1500\n").unwrap();
        assert_eq!(AppConfig::load_from(&path).poll_ms, 1500);
        assert!(!bad.exists());
        std::fs::write(&path, "poll_ms = 1500\ntheme = 7\n").unwrap();
        let c = AppConfig::load_from(&path);
        assert_eq!((c.poll_ms, c.theme.as_str()), (1500, "dark"));
        assert_eq!(std::fs::read_to_string(&bad).unwrap(), "poll_ms = 1500\ntheme = 7\n");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn update_changes_only_what_it_touches() {
        let path = temp_config("update");
        // Missing file: starts from the defaults.
        let c = AppConfig::update_at(&path, |c| c.poll_ms = 1234).unwrap();
        assert_eq!(c.poll_ms, 1234);
        // Another process saves a different field in the meantime...
        let mut other = AppConfig::load_from(&path);
        other.theme = "light".into();
        other.write_to(&path).unwrap();
        // ...and an update from a stale copy's point of view keeps it.
        let c = AppConfig::update_at(&path, |c| c.fan.cpu_percent = 80).unwrap();
        assert_eq!((c.poll_ms, c.theme.as_str(), c.fan.cpu_percent), (1234, "light", 80));
        assert_eq!(AppConfig::load_from(&path), c);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
