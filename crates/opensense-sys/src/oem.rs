//! Read-only access to the per-model data Acer's installer leaves behind
//! (`HKLM\SOFTWARE\OEM\NitroSense`, `C:\ProgramData\oem\NitroSense\*.ini`).
//! Everything has a sensible default so OpenSense also works without it.

use std::path::Path;

/// Machine generation from the optional OEM `Feature.ini [MachineType] Type`.
/// Only used until the firmware capability probe has answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineType {
    /// Fans and power plans only.
    Basic,
    /// With RGB keyboard.
    RgbKeyboard,
    /// With operation modes.
    PerformanceModes,
    Unknown(i32),
}

#[derive(Debug, Clone)]
pub struct OemInfo {
    pub model: Option<String>,
    pub machine_type: MachineType,
    pub backlight_hotkey: u8,
    pub pd_support: bool,
    pub rgb_keyboard: bool,
    pub zone_color_adjust: [f32; 3],
}

impl Default for OemInfo {
    fn default() -> Self {
        Self {
            model: None,
            machine_type: MachineType::PerformanceModes,
            backlight_hotkey: 132,
            pd_support: false,
            rgb_keyboard: true,
            zone_color_adjust: [1.0, 1.0, 1.0],
        }
    }
}

const OEM_DIR: &str = r"C:\ProgramData\oem\NitroSense";

/// Minimal INI lookup (case-insensitive section/key).
pub fn ini_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut in_section = false;
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_section = line[1..line.len() - 1].trim().eq_ignore_ascii_case(section);
        } else if in_section {
            if let Some((k, v)) = line.split_once('=') {
                if k.trim().eq_ignore_ascii_case(key) {
                    return Some(v.trim().to_owned());
                }
            }
        }
    }
    None
}

fn read_ini(name: &str) -> Option<String> {
    std::fs::read_to_string(Path::new(OEM_DIR).join(name)).ok()
}

#[cfg(windows)]
fn reg_dword(name: &str) -> Option<u32> {
    reg_read(name).and_then(|v| match v {
        RegValue::Dword(d) => Some(d),
        RegValue::Sz(s) => s.trim().parse().ok(),
    })
}

#[cfg(windows)]
fn reg_string(name: &str) -> Option<String> {
    reg_read(name).map(|v| match v {
        RegValue::Dword(d) => d.to_string(),
        RegValue::Sz(s) => s,
    })
}

#[cfg(windows)]
enum RegValue {
    Dword(u32),
    Sz(String),
}

#[cfg(windows)]
fn reg_read(name: &str) -> Option<RegValue> {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{
        RegGetValueW, HKEY_LOCAL_MACHINE, REG_DWORD, REG_SZ, REG_VALUE_TYPE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    };
    let key = HSTRING::from(opensense_reg_root());
    let value = HSTRING::from(name);
    let mut ty = REG_VALUE_TYPE::default();
    let mut buf = [0u8; 512];
    let mut len = buf.len() as u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &key,
            &value,
            RRF_RT_REG_DWORD | RRF_RT_REG_SZ,
            Some(&mut ty),
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len),
        )
    };
    if rc.is_err() {
        return None;
    }
    if ty == REG_DWORD {
        Some(RegValue::Dword(u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])))
    } else if ty == REG_SZ {
        let wide: Vec<u16> =
            buf[..len as usize].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        Some(RegValue::Sz(String::from_utf16_lossy(&wide).trim_end_matches('\0').to_owned()))
    } else {
        None
    }
}

fn opensense_reg_root() -> &'static str {
    r"SOFTWARE\OEM\NitroSense"
}

impl OemInfo {
    pub fn detect() -> Self {
        let mut info = OemInfo::default();
        if let Some(t) = read_ini("Feature.ini").and_then(|s| ini_value(&s, "MachineType", "Type")) {
            info.machine_type = match t.parse::<i32>() {
                Ok(0) => MachineType::Basic,
                Ok(1) => MachineType::RgbKeyboard,
                Ok(2) => MachineType::PerformanceModes,
                Ok(n) => MachineType::Unknown(n),
                Err(_) => MachineType::Unknown(-1),
            };
        }
        if let Some(hw) = read_ini("HW_Support.ini") {
            for (i, k) in ["R", "G", "B"].iter().enumerate() {
                if let Some(v) = ini_value(&hw, "ZoneColorAdjust", k).and_then(|v| v.parse::<f32>().ok()) {
                    info.zone_color_adjust[i] = v.clamp(0.0, 2.0);
                }
            }
        }
        #[cfg(windows)]
        {
            info.model = reg_string("Model_Name_1st");
            if let Some(v) = reg_dword("BK_Hotkey_Number") {
                info.backlight_hotkey = v.min(255) as u8;
            }
            info.pd_support = reg_dword("PD_Support") == Some(1);
            if let Some(v) = reg_dword("RGB_Keyboard_Support") {
                info.rgb_keyboard = v == 1;
            }
        }
        info
    }

    pub fn has_operation_modes(&self) -> bool {
        self.machine_type == MachineType::PerformanceModes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini() {
        let t = "[MachineType]\nType=2\n[YearType]\nYear = 2022\n";
        assert_eq!(ini_value(t, "machinetype", "type").as_deref(), Some("2"));
        assert_eq!(ini_value(t, "YearType", "Year").as_deref(), Some("2022"));
        assert_eq!(ini_value(t, "Nope", "Type"), None);
    }
}
