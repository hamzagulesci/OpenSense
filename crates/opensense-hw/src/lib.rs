//! Hardware access for OpenSense.
//!
//! Every feature maps to `root\WMI` methods (`AcerGamingFunction`,
//! `APGeAction`, `BatteryControl`) provided by the firmware through
//! Windows' built-in ACPI-WMI driver. Calling them needs administrator
//! rights.

use opensense_proto::GamingMethod;

pub mod device;
pub mod mock;
#[cfg(windows)]
pub mod wmi;

pub use device::NitroDevice;

#[derive(Debug, thiserror::Error)]
pub enum HwError {
    #[error("access denied: AcerGamingFunction requires administrator rights")]
    AccessDenied,
    #[error("AcerGamingFunction WMI class not found (unsupported machine or missing Acer WMI driver)")]
    NotPresent,
    #[error("WMI error: {0}")]
    Wmi(String),
    #[error("firmware rejected {method} (status {status:#x})")]
    Firmware { method: &'static str, status: u32 },
    #[error("{0} has an incompatible parameter shape")]
    Shape(&'static str),
    #[error("no hardware transport available on this platform")]
    NoTransport,
    #[error("{0} is not supported by this transport")]
    Unsupported(&'static str),
}

/// A WMI method argument or result for [`GamingTransport::call_raw`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WmiValue {
    Int(u32),
    Bytes(Vec<u8>),
}

impl WmiValue {
    pub fn as_int(&self) -> Option<u32> {
        match self {
            WmiValue::Int(v) => Some(*v),
            WmiValue::Bytes(_) => None,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        match self {
            WmiValue::Bytes(b) => b,
            WmiValue::Int(_) => &[],
        }
    }
}

/// One round trip to an `AcerGamingFunction` method.
pub trait GamingTransport: Send + Sync {
    fn name(&self) -> &'static str;
    /// `Set*` style call: 64-bit input, 32-bit status output.
    fn set_u64(&self, method: GamingMethod, input: u64) -> Result<u32, HwError>;
    /// `Get*` style call: 32-bit input, 64-bit output.
    fn get_u32(&self, method: GamingMethod, input: u32) -> Result<u64, HwError>;
    /// Byte-array input, 32-bit status output.
    fn set_bytes(&self, method: GamingMethod, input: &[u8]) -> Result<u32, HwError>;
    /// 32-bit input, byte-array output.
    fn get_bytes(&self, method: GamingMethod, input: u32) -> Result<Vec<u8>, HwError>;
    /// Any `root\WMI` method with named in/out parameters (used for
    /// `BatteryControl`). Returns the requested outputs in order.
    fn call_raw(
        &self,
        class: &'static str,
        method: &'static str,
        inputs: &[(&'static str, WmiValue)],
        outputs: &[&'static str],
    ) -> Result<Vec<WmiValue>, HwError> {
        let _ = (class, inputs, outputs);
        Err(HwError::Unsupported(method))
    }
}

/// Which transport to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportKind {
    #[default]
    Wmi,
    Mock,
}

impl std::str::FromStr for TransportKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "wmi" => Ok(Self::Wmi),
            "mock" => Ok(Self::Mock),
            other => Err(format!("unknown transport '{other}' (wmi|mock)")),
        }
    }
}

/// Opens a transport of the requested kind.
pub fn open_transport(kind: TransportKind) -> Result<Box<dyn GamingTransport>, HwError> {
    match kind {
        TransportKind::Mock => Ok(Box::new(mock::MockTransport::default())),
        #[cfg(windows)]
        TransportKind::Wmi => Ok(Box::new(wmi::WmiTransport::connect()?)),
        #[cfg(not(windows))]
        TransportKind::Wmi => Err(HwError::NoTransport),
    }
}
