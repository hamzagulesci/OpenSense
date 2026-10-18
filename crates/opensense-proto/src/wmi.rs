//! `root\WMI` method table: `AcerGamingFunction` plus `APGeAction`.
//!
//! Method names and signatures come from the WMI class definitions the
//! firmware publishes. The `AcerGamingFunction` ACPI method ids
//! (declaration order) match the Linux `acer-wmi` driver
//! (5 = GetGamingSysInfo, 14 = SetGamingFanBehavior, 22/23 = misc).

/// Shape of a method's in/out parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// `gmInput: u64 -> gmOutput: u32`
    SetU64,
    /// `gmInput: u32 -> gmOutput: u64`
    GetU32,
    /// `gmInput: u8[] -> gmOutput: u32`
    SetBytes,
    /// `gmInput: u32 -> gmOutput: u8[], gmReturn: u8`
    GetBytes,
    /// `-> gmOutput: u64`
    GetNoInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GamingMethod {
    SetGamingProfile,
    SetGamingLed,
    GetGamingProfile,
    GetGamingLed,
    GetGamingSysInfo,
    SetGamingRgbKb,
    GetGamingRgbKb,
    SetGamingProfileSetting,
    GetGamingProfileSetting,
    SetGamingLedBehavior,
    GetGamingLedBehavior,
    SetGamingLedColor,
    GetGamingLedColor,
    SetGamingFanBehavior,
    GetGamingFanBehavior,
    SetGamingFanSpeed,
    GetGamingFanSpeed,
    SetGamingFanTable,
    GetGamingFanTable,
    SetGamingKbBacklight,
    GetGamingKbBacklight,
    SetGamingMiscSetting,
    GetGamingMiscSetting,
    /// `APGeAction.SetFunction(uiInput: u64) -> uiOutput: u32`
    ApgeSetFunction,
    /// `APGeAction.GetFunction(uiInput: u32) -> uiOutput: u64`
    ApgeGetFunction,
}

impl GamingMethod {
    pub const ALL: [GamingMethod; 25] = {
        use GamingMethod::*;
        [
            SetGamingProfile,
            SetGamingLed,
            GetGamingProfile,
            GetGamingLed,
            GetGamingSysInfo,
            SetGamingRgbKb,
            GetGamingRgbKb,
            SetGamingProfileSetting,
            GetGamingProfileSetting,
            SetGamingLedBehavior,
            GetGamingLedBehavior,
            SetGamingLedColor,
            GetGamingLedColor,
            SetGamingFanBehavior,
            GetGamingFanBehavior,
            SetGamingFanSpeed,
            GetGamingFanSpeed,
            SetGamingFanTable,
            GetGamingFanTable,
            SetGamingKbBacklight,
            GetGamingKbBacklight,
            SetGamingMiscSetting,
            GetGamingMiscSetting,
            ApgeSetFunction,
            ApgeGetFunction,
        ]
    };

    /// Looks a method up by WMI name (`SetGamingRgbKb`) or qualified name
    /// (`APGeAction.GetFunction`), case-insensitively.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|m| m.wmi_name().eq_ignore_ascii_case(name) || m.qualified_name().eq_ignore_ascii_case(name))
    }

    pub const fn wmi_name(self) -> &'static str {
        use GamingMethod::*;
        match self {
            SetGamingProfile => "SetGamingProfile",
            SetGamingLed => "SetGamingLED",
            GetGamingProfile => "GetGamingProfile",
            GetGamingLed => "GetGamingLED",
            GetGamingSysInfo => "GetGamingSysInfo",
            SetGamingRgbKb => "SetGamingRgbKb",
            GetGamingRgbKb => "GetGamingRgbKb",
            SetGamingProfileSetting => "SetGamingProfileSetting",
            GetGamingProfileSetting => "GetGamingProfileSetting",
            SetGamingLedBehavior => "SetGamingLEDBehavior",
            GetGamingLedBehavior => "GetGamingLEDBehavior",
            SetGamingLedColor => "SetGamingLEDColor",
            GetGamingLedColor => "GetGamingLEDColor",
            SetGamingFanBehavior => "SetGamingFanBehavior",
            GetGamingFanBehavior => "GetGamingFanBehavior",
            SetGamingFanSpeed => "SetGamingFanSpeed",
            GetGamingFanSpeed => "GetGamingFanSpeed",
            SetGamingFanTable => "SetGamingFanTable",
            GetGamingFanTable => "GetGamingFanTable",
            SetGamingKbBacklight => "SetGamingKBBacklight",
            GetGamingKbBacklight => "GetGamingKBBacklight",
            SetGamingMiscSetting => "SetGamingMiscSetting",
            GetGamingMiscSetting => "GetGamingMiscSetting",
            ApgeSetFunction => "SetFunction",
            ApgeGetFunction => "GetFunction",
        }
    }

    /// WMI class that declares this method.
    pub const fn wmi_class(self) -> &'static str {
        match self {
            GamingMethod::ApgeSetFunction | GamingMethod::ApgeGetFunction => APGE_CLASS,
            _ => WMI_CLASS,
        }
    }

    /// Names of the input and output parameters.
    pub const fn param_names(self) -> (&'static str, &'static str) {
        match self {
            GamingMethod::ApgeSetFunction | GamingMethod::ApgeGetFunction => ("uiInput", "uiOutput"),
            _ => ("gmInput", "gmOutput"),
        }
    }

    /// `Class.Method`, for messages.
    pub fn qualified_name(self) -> String {
        format!("{}.{}", self.wmi_class(), self.wmi_name())
    }

    pub const fn shape(self) -> Shape {
        use GamingMethod::*;
        match self {
            SetGamingLed | SetGamingKbBacklight => Shape::SetBytes,
            GetGamingLed | GetGamingKbBacklight => Shape::GetBytes,
            GetGamingFanTable => Shape::GetNoInput,
            GetGamingProfile
            | GetGamingSysInfo
            | GetGamingRgbKb
            | GetGamingProfileSetting
            | GetGamingLedBehavior
            | GetGamingLedColor
            | GetGamingFanBehavior
            | GetGamingFanSpeed
            | GetGamingMiscSetting
            | ApgeGetFunction => Shape::GetU32,
            _ => Shape::SetU64,
        }
    }
}

pub const WMI_NAMESPACE: &str = r"ROOT\WMI";
pub const WMI_CLASS: &str = "AcerGamingFunction";
pub const APGE_CLASS: &str = "APGeAction";
