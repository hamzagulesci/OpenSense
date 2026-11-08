//! Battery details from the battery class driver (`IOCTL_BATTERY_*`):
//! charge/discharge rate, capacities, wear and cycle count. Needs no
//! administrator rights and no vendor software.

use crate::SysError;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BatteryInfo {
    /// Positive while charging, negative while discharging (mW).
    pub rate_mw: Option<i32>,
    pub remaining_mwh: Option<u32>,
    pub full_mwh: Option<u32>,
    pub design_mwh: Option<u32>,
    pub cycles: Option<u32>,
}

impl BatteryInfo {
    /// Full-charge capacity as a share of the design capacity, in percent.
    pub fn health_percent(&self) -> Option<f32> {
        match (self.full_mwh, self.design_mwh) {
            (Some(f), Some(d)) if d > 0 => Some((f as f32 / d as f32 * 100.0).min(100.0)),
            _ => None,
        }
    }

    /// Charge level with one decimal, from the capacities.
    pub fn charge_percent(&self) -> Option<f32> {
        match (self.remaining_mwh, self.full_mwh) {
            (Some(r), Some(f)) if f > 0 => Some((r as f32 / f as f32 * 100.0).min(100.0)),
            _ => None,
        }
    }

    pub fn rate_watts(&self) -> Option<f32> {
        self.rate_mw.map(|r| r as f32 / 1000.0)
    }
}

#[cfg(windows)]
mod imp {
    use std::sync::OnceLock;

    use super::*;
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Devices::DeviceAndDriverInstallation::{
        SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
        SetupDiGetDeviceInterfaceDetailW, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA,
        SP_DEVICE_INTERFACE_DETAIL_DATA_W,
    };
    use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, HWND};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::Power::{
        BatteryInformation, BATTERY_CAPACITY_RELATIVE, BATTERY_INFORMATION, BATTERY_QUERY_INFORMATION, BATTERY_STATUS,
        BATTERY_UNKNOWN_CAPACITY, BATTERY_UNKNOWN_RATE, BATTERY_WAIT_STATUS, GUID_DEVICE_BATTERY,
        IOCTL_BATTERY_QUERY_INFORMATION, IOCTL_BATTERY_QUERY_STATUS, IOCTL_BATTERY_QUERY_TAG,
    };
    use windows::Win32::System::IO::DeviceIoControl;

    /// Device path of the first battery; looked up once.
    fn device_path() -> Option<&'static str> {
        static PATH: OnceLock<Option<String>> = OnceLock::new();
        PATH.get_or_init(|| unsafe {
            let set = SetupDiGetClassDevsW(
                Some(&GUID_DEVICE_BATTERY),
                PCWSTR::null(),
                HWND::default(),
                DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
            )
            .ok()?;
            let mut iface = SP_DEVICE_INTERFACE_DATA {
                cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                ..Default::default()
            };
            let path = (|| {
                SetupDiEnumDeviceInterfaces(set, None, &GUID_DEVICE_BATTERY, 0, &mut iface).ok()?;
                let mut size = 0u32;
                let _ = SetupDiGetDeviceInterfaceDetailW(set, &iface, None, 0, Some(&mut size), None);
                if size == 0 {
                    return None;
                }
                // u64 storage keeps the detail struct aligned.
                let mut buf = vec![0u64; (size as usize).div_ceil(8)];
                let detail = buf.as_mut_ptr().cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
                (*detail).cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
                SetupDiGetDeviceInterfaceDetailW(set, &iface, Some(detail), size, None, None).ok()?;
                let start = std::ptr::addr_of!((*detail).DevicePath).cast::<u16>();
                let max = (size as usize - 4) / 2;
                let chars = std::slice::from_raw_parts(start, max);
                let len = chars.iter().position(|&c| c == 0).unwrap_or(max);
                Some(String::from_utf16_lossy(&chars[..len]))
            })();
            let _ = SetupDiDestroyDeviceInfoList(set);
            path
        })
        .as_deref()
    }

    fn ioctl<I: Copy, O: Default>(h: HANDLE, code: u32, input: &I) -> Option<O> {
        let mut out = O::default();
        let mut returned = 0u32;
        unsafe {
            DeviceIoControl(
                h,
                code,
                Some(std::ptr::from_ref(input).cast()),
                std::mem::size_of::<I>() as u32,
                Some(std::ptr::from_mut(&mut out).cast()),
                std::mem::size_of::<O>() as u32,
                Some(&mut returned),
                None,
            )
            .ok()?;
        }
        Some(out)
    }

    pub fn info() -> Result<BatteryInfo, SysError> {
        let path = device_path().ok_or(SysError::Unsupported)?;
        let h = unsafe {
            CreateFileW(
                &HSTRING::from(path),
                (GENERIC_READ | GENERIC_WRITE).0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                HANDLE::default(),
            )?
        };
        let result = (|| {
            let tag: u32 = ioctl(h, IOCTL_BATTERY_QUERY_TAG, &0u32).filter(|&t| t != 0)?;
            let q = BATTERY_QUERY_INFORMATION { BatteryTag: tag, InformationLevel: BatteryInformation, AtRate: 0 };
            let bi: BATTERY_INFORMATION = ioctl(h, IOCTL_BATTERY_QUERY_INFORMATION, &q)?;
            let ws = BATTERY_WAIT_STATUS { BatteryTag: tag, ..Default::default() };
            let st: BATTERY_STATUS = ioctl(h, IOCTL_BATTERY_QUERY_STATUS, &ws)?;
            // Relative batteries report percentages, not mWh.
            let absolute = bi.Capabilities & BATTERY_CAPACITY_RELATIVE == 0;
            let known = |v: u32| (absolute && v != 0 && v != BATTERY_UNKNOWN_CAPACITY).then_some(v);
            Some(BatteryInfo {
                rate_mw: (absolute && st.Rate != BATTERY_UNKNOWN_RATE as i32).then_some(st.Rate),
                remaining_mwh: known(st.Capacity),
                full_mwh: known(bi.FullChargedCapacity),
                design_mwh: known(bi.DesignedCapacity),
                cycles: (bi.CycleCount > 0).then_some(bi.CycleCount),
            })
        })();
        unsafe {
            let _ = CloseHandle(h);
        }
        result.ok_or_else(|| SysError::Win("battery query failed".into()))
    }
}

#[cfg(windows)]
pub use imp::info;

#[cfg(not(windows))]
pub fn info() -> Result<BatteryInfo, SysError> {
    Err(SysError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_values() {
        let b = BatteryInfo {
            rate_mw: Some(14_500),
            remaining_mwh: Some(40_000),
            full_mwh: Some(50_000),
            design_mwh: Some(57_000),
            cycles: Some(120),
        };
        assert_eq!(b.charge_percent(), Some(80.0));
        assert!((b.health_percent().unwrap() - 87.7).abs() < 0.1);
        assert_eq!(b.rate_watts(), Some(14.5));
        assert_eq!(BatteryInfo::default().health_percent(), None);
    }
}
