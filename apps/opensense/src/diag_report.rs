//! Compatibility report: device facts plus test results, built only from
//! the fields listed in [`Report`]. Nothing is collected and filtered
//! afterwards: serial numbers, UUIDs, MAC addresses, user names and file
//! paths are never read in the first place.

use opensense_hw::diag::{Level, Outcome, TestResult};
use serde::Serialize;

/// Bump when fields change meaning, so collected reports stay comparable.
pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Device {
    pub manufacturer: String,
    pub model: String,
    pub board: String,
    pub bios_version: String,
    pub bios_date: String,
    pub cpu: String,
    pub gpu: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Os {
    pub name: String,
    pub version: String,
    pub build: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Test {
    pub id: &'static str,
    pub level: &'static str,
    pub outcome: &'static str,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Report {
    pub schema: u32,
    pub opensense: &'static str,
    /// Day only (UTC), no time.
    pub date: String,
    pub os: Os,
    pub device: Device,
    pub tests: Vec<Test>,
}

pub fn level_name(l: Level) -> &'static str {
    match l {
        Level::ReadOnly => "read_only",
        Level::Confirm => "confirm",
        Level::Restart => "restart",
    }
}

pub fn outcome_name(o: &Outcome) -> &'static str {
    match o {
        Outcome::Verified => "verified",
        Outcome::Detected => "detected",
        Outcome::NotSupported => "not_supported",
        Outcome::Failed => "failed",
        Outcome::NotRun => "not_run",
    }
}

impl Report {
    pub fn new(os: Os, device: Device, results: &[TestResult], date: String) -> Self {
        Self {
            schema: SCHEMA,
            opensense: env!("CARGO_PKG_VERSION"),
            date,
            os,
            device,
            tests: results
                .iter()
                .map(|r| Test {
                    id: r.id,
                    level: level_name(r.level),
                    outcome: outcome_name(&r.outcome),
                    detail: r.detail.clone(),
                })
                .collect(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Short Markdown table for a GitHub issue.
    pub fn markdown(&self) -> String {
        let d = &self.device;
        let mut s = format!(
            "**Model:** {} {} ({})\n**BIOS:** {} ({})\n**CPU:** {}\n**GPU:** {}\n**OS:** {} {} ({})\n**OpenSense:** {}\n\n| Test | Result | Detail |\n|---|---|---|\n",
            d.manufacturer, d.model, d.board, d.bios_version, d.bios_date, d.cpu, d.gpu,
            self.os.name, self.os.version, self.os.build, self.opensense
        );
        for t in &self.tests {
            s.push_str(&format!("| {} | {} | {} |\n", t.id, t.outcome, t.detail.replace('|', "/")));
        }
        s
    }

    /// New-issue link with the report filled in. Nothing is sent: the user
    /// reviews it on GitHub and submits it there (or not). The JSON is
    /// included when the link stays short enough for browsers.
    pub fn issue_url(&self, repo: &str) -> String {
        let title = format!("Compatibility report: {} (BIOS {})", self.device.model, self.device.bios_version);
        let with_json = format!(
            "{}\n<details><summary>JSON</summary>\n\n```json\n{}\n```\n</details>\n",
            self.markdown(),
            self.to_json()
        );
        let body = if encode(&with_json).len() <= 7000 {
            with_json
        } else {
            format!("{}\n_Attach the saved JSON report here._\n", self.markdown())
        };
        format!("{repo}/issues/new?labels=compatibility&title={}&body={}", encode(&title), encode(&body))
    }
}

/// Percent-encoding for a URL query value.
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Today's date (UTC) as YYYY-MM-DD.
pub fn today() -> String {
    let days = (crate::updates::now_secs() / 86_400) as i64;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Device and OS facts from the registry (SMBIOS copy and Windows version).
#[cfg(windows)]
pub fn collect(cpu: String, gpu: String) -> (Os, Device) {
    const BIOS: &str = r"HARDWARE\DESCRIPTION\System\BIOS";
    const NT: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let build = reg_sz(NT, "CurrentBuild").unwrap_or_default();
    let ubr = reg_dword(NT, "UBR").map(|u| format!(".{u}")).unwrap_or_default();
    // ProductName still says "Windows 10" on Windows 11; the build tells.
    let mut name = reg_sz(NT, "ProductName").unwrap_or_default();
    if build.parse::<u32>().is_ok_and(|b| b >= 22_000) {
        name = name.replace("Windows 10", "Windows 11");
    }
    let os = Os { name, version: reg_sz(NT, "DisplayVersion").unwrap_or_default(), build: format!("{build}{ubr}") };
    let device = Device {
        manufacturer: reg_sz(BIOS, "SystemManufacturer").unwrap_or_default(),
        model: reg_sz(BIOS, "SystemProductName").unwrap_or_default(),
        board: reg_sz(BIOS, "BaseBoardProduct").unwrap_or_default(),
        bios_version: reg_sz(BIOS, "BIOSVersion").unwrap_or_default(),
        bios_date: reg_sz(BIOS, "BIOSReleaseDate").unwrap_or_default(),
        cpu,
        gpu,
    };
    (os, device)
}

#[cfg(windows)]
fn reg_sz(path: &str, name: &str) -> Option<String> {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let mut buf = [0u16; 256];
    let mut size = (buf.len() * 2) as u32;
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(path),
            &HSTRING::from(name),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
        .ok()
        .ok()?;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]).trim().to_owned())
}

#[cfg(windows)]
fn reg_dword(path: &str, name: &str) -> Option<u32> {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD};
    let mut v = 0u32;
    let mut size = 4u32;
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(path),
            &HSTRING::from(name),
            RRF_RT_REG_DWORD,
            None,
            Some(std::ptr::addr_of_mut!(v).cast()),
            Some(&mut size),
        )
        .ok()
        .ok()?;
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Report {
        let results = vec![TestResult {
            id: "cpu_temp",
            level: Level::ReadOnly,
            outcome: Outcome::Verified,
            detail: "61".into(),
        }];
        let device = Device {
            manufacturer: "Acer".into(),
            model: "Nitro Test".into(),
            board: "Board".into(),
            bios_version: "V1.00".into(),
            bios_date: "01/01/2025".into(),
            cpu: "CPU".into(),
            gpu: "GPU".into(),
        };
        let os = Os { name: "Windows 11 Pro".into(), version: "25H2".into(), build: "26300.1".into() };
        Report::new(os, device, &results, "2026-11-09".into())
    }

    #[test]
    fn json_has_exactly_the_allowed_fields() {
        let v: serde_json::Value = serde_json::from_str(&sample().to_json()).unwrap();
        let top: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        assert_eq!(top, ["date", "device", "opensense", "os", "schema", "tests"]);
        let dev: Vec<_> = v["device"].as_object().unwrap().keys().cloned().collect();
        assert_eq!(dev, ["bios_date", "bios_version", "board", "cpu", "gpu", "manufacturer", "model"]);
        assert_eq!(v["tests"][0]["outcome"], "verified");
        assert_eq!(v["tests"][0]["level"], "read_only");
    }

    #[test]
    fn issue_url_is_encoded_and_bounded() {
        let url = sample().issue_url("https://github.com/a/b");
        assert!(url.starts_with("https://github.com/a/b/issues/new?labels=compatibility&title=Compatibility%20report"));
        assert!(!url.contains(' ') && !url.contains('\n'));
        assert!(url.len() < 8000);
    }

    #[test]
    fn date_is_civil() {
        let d = today();
        assert_eq!(d.len(), 10);
        assert!(d.starts_with("20"));
    }
}
