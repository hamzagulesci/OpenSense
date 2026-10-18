//! Acer battery health control: `root\WMI:BatteryControl`
//! (WMI GUID 79772EC5-04B1-4BFD-843C-61E7F77B6CC9).
//!
//! The same interface is used by the Linux community `acer-wmi-battery`
//! driver. "Health mode" stops charging at 80 %; "calibration" runs a full
//! discharge/charge cycle. Both are reported as bits of `uFunctionList`.

pub const CLASS: &str = "BatteryControl";
pub const GET_STATUS: &str = "GetBatteryHealthControlStatus";
pub const SET_CONTROL: &str = "SetBatteryHealthControl";

/// Battery index used by Acer laptops (single battery).
pub const BATTERY_NO: u8 = 1;
/// `uFunctionQuery` / `uFunctionMask` bit for health mode (80 % limit).
pub const FN_HEALTH_MODE: u8 = 1;
/// Bit for battery calibration.
pub const FN_CALIBRATION: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BatteryHealth {
    /// Firmware supports the 80 % charge limit.
    pub limit_supported: bool,
    /// 80 % charge limit is active.
    pub limit_active: bool,
    pub calibration_supported: bool,
    pub calibration_active: bool,
}

/// Decodes `GetBatteryHealthControlStatus` (`uFunctionList`, `uFunctionStatus[5]`).
pub fn decode(function_list: u8, status: &[u8]) -> BatteryHealth {
    BatteryHealth {
        limit_supported: function_list & FN_HEALTH_MODE != 0,
        limit_active: status.first().is_some_and(|&b| b == 1),
        calibration_supported: function_list & FN_CALIBRATION != 0,
        calibration_active: status.get(1).is_some_and(|&b| b == 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_status() {
        // Both functions supported, everything off.
        let h = decode(3, &[0, 0, 0, 0, 0]);
        assert!(h.limit_supported && h.calibration_supported);
        assert!(!h.limit_active && !h.calibration_active);
        assert!(decode(1, &[1, 0, 0, 0, 0]).limit_active);
    }
}
