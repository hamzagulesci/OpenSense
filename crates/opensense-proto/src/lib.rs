//! Acer gaming-laptop firmware interface (ACPI-WMI).
//!
//! * [`wmi`]: the `root\WMI` method table.
//! * [`fan`], [`sensor`], [`mode`], [`lighting`], [`battery`]: layouts of the
//!   values passed to those methods.

pub mod battery;
pub mod fan;
pub mod lighting;
pub mod mode;
pub mod sensor;
pub mod wmi;

pub use wmi::GamingMethod;
