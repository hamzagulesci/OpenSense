//! Operating-system side of OpenSense: everything read or changed without
//! going through the firmware interface.

pub mod autostart;
pub mod config;
pub mod cpu;
#[cfg(windows)]
pub mod gpu_load;
pub mod gpu_name;
pub mod oem;
pub mod power;
pub mod privilege;

#[derive(Debug, thiserror::Error)]
pub enum SysError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Windows API error: {0}")]
    Win(String),
    #[error("config error: {0}")]
    Config(String),
    #[error("not supported on this platform")]
    Unsupported,
}

#[cfg(windows)]
impl From<windows::core::Error> for SysError {
    fn from(e: windows::core::Error) -> Self {
        SysError::Win(e.to_string())
    }
}
