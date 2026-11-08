//! Update check against OpenSense's GitHub releases and in-place update.
//! Uses `curl.exe`, which ships with Windows 10 and 11, so no HTTP stack
//! is linked into the app.

use std::path::{Path, PathBuf};
use std::process::Command;

pub const REPO: &str = env!("CARGO_PKG_REPOSITORY");
/// Re-check at most this often (seconds).
pub const CHECK_EVERY: u64 = 12 * 3600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Version without a leading "v", e.g. "0.2.0".
    pub version: String,
    pub page: String,
    pub zip: Option<String>,
}

fn hidden(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Value of the first `"key": "value"` pair in a JSON text.
fn json_str<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let at = text.find(&format!("\"{key}\""))?;
    let rest = &text[at + key.len() + 2..];
    let start = rest.find('"')? + 1;
    let end = rest[start..].find('"')? + start;
    Some(&rest[start..end])
}

pub fn parse_release(json: &str) -> Option<Release> {
    let version = json_str(json, "tag_name")?.trim_start_matches(['v', 'V']).to_owned();
    let page = json_str(json, "html_url").unwrap_or(REPO).to_owned();
    // The Windows zip among the assets.
    let zip = json
        .match_indices("\"browser_download_url\"")
        .filter_map(|(i, _)| json_str(&json[i..], "browser_download_url"))
        .find(|u| u.ends_with(".zip") && u.contains("windows"))
        .map(str::to_owned);
    Some(Release { version, page, zip })
}

/// Latest published release, or `None` when offline.
pub fn latest() -> Option<Release> {
    let api = REPO.replace("https://github.com/", "https://api.github.com/repos/") + "/releases/latest";
    let out = hidden(Command::new("curl.exe").args(["-s", "-f", "-m", "15", "-H", "User-Agent: OpenSense", &api]))
        .output()
        .ok()?;
    out.status.success().then(|| parse_release(&String::from_utf8_lossy(&out.stdout))).flatten()
}

fn parts(v: &str) -> Vec<u32> {
    v.split(['.', '-']).map(|p| p.parse().unwrap_or(0)).collect()
}

/// `candidate` is a newer version than `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let (a, b) = (parts(candidate), parts(current));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Removes executables left over from the previous update.
pub fn cleanup() {
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        for name in ["opensense.exe.old", "opensense-cli.exe.old"] {
            let _ = std::fs::remove_file(dir.join(name));
        }
    }
}

/// Downloads the release zip and replaces the executables next to this
/// one. Running executables are renamed first (Windows allows that), so
/// the new files take effect on the next start.
pub fn install(zip_url: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("no folder")?.to_path_buf();
    let work: PathBuf = std::env::temp_dir().join("opensense-update");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let zip = work.join("update.zip");
    let ok = hidden(Command::new("curl.exe").args(["-s", "-f", "-L", "-m", "300", "-o"]).arg(&zip).arg(zip_url))
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        return Err("download failed".into());
    }
    let unpacked = work.join("files");
    let script =
        format!("Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force", zip.display(), unpacked.display());
    let ok = hidden(Command::new("powershell.exe").args(["-NoProfile", "-NonInteractive", "-Command", &script]))
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        return Err("cannot unpack the update".into());
    }
    let mut replaced = 0;
    for name in ["opensense.exe", "opensense-cli.exe"] {
        let Some(src) = find(&unpacked, name) else { continue };
        let dst = dir.join(name);
        let old = dir.join(format!("{name}.old"));
        let _ = std::fs::remove_file(&old);
        if dst.exists() {
            std::fs::rename(&dst, &old).map_err(|e| format!("{name}: {e}"))?;
        }
        if let Err(e) = std::fs::copy(&src, &dst) {
            // Put the old file back.
            let _ = std::fs::rename(&old, &dst);
            return Err(format!("{name}: {e}"));
        }
        replaced += 1;
    }
    let _ = std::fs::remove_dir_all(&work);
    if replaced == 0 {
        return Err("the update contains no OpenSense files".into());
    }
    Ok(())
}

fn find(dir: &Path, name: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let p = entry.path();
        if p.is_dir() {
            if let Some(f) = find(&p, name) {
                return Some(f);
            }
        } else if p.file_name().is_some_and(|f| f.eq_ignore_ascii_case(name)) {
            return Some(p);
        }
    }
    None
}

/// BIOS version and date from the SMBIOS registry copy.
pub fn bios() -> Option<(String, String)> {
    #[cfg(windows)]
    {
        use windows::core::{w, PCWSTR};
        use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
        let read = |name: PCWSTR| -> Option<String> {
            let mut buf = [0u16; 128];
            let mut size = (buf.len() * 2) as u32;
            unsafe {
                RegGetValueW(
                    HKEY_LOCAL_MACHINE,
                    w!("HARDWARE\\DESCRIPTION\\System\\BIOS"),
                    name,
                    RRF_RT_REG_SZ,
                    None,
                    Some(buf.as_mut_ptr().cast()),
                    Some(&mut size),
                )
                .ok()
                .ok()?;
            }
            let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Some(String::from_utf16_lossy(&buf[..len]))
        };
        Some((read(w!("BIOSVersion"))?, read(w!("BIOSReleaseDate")).unwrap_or_default()))
    }
    #[cfg(not(windows))]
    None
}

/// NVIDIA driver version reported by `nvidia-smi`.
pub fn nvidia_driver() -> Option<String> {
    let out = hidden(Command::new("nvidia-smi.exe").args(["--query-gpu=driver_version", "--format=csv,noheader"]))
        .output()
        .ok()?;
    let v = String::from_utf8_lossy(&out.stdout).lines().next()?.trim().to_owned();
    (out.status.success() && !v.is_empty()).then_some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1", "0.1.0"));
    }

    #[test]
    fn parses_release_json() {
        let json = r#"{"url":"x","html_url":"https://github.com/a/b/releases/tag/v0.2.0","tag_name":"v0.2.0",
            "assets":[{"browser_download_url":"https://github.com/a/b/releases/download/v0.2.0/OpenSense-v0.2.0-windows-x64.zip"}]}"#;
        let r = parse_release(json).unwrap();
        assert_eq!(r.version, "0.2.0");
        assert!(r.page.ends_with("v0.2.0"));
        assert!(r.zip.unwrap().ends_with("windows-x64.zip"));
    }
}
