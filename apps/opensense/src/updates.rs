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
    /// SHA-256 of the zip as published by GitHub (lowercase hex).
    pub zip_sha256: Option<String>,
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
    // The Windows zip among the assets, with its digest. Each asset object
    // starts with its API url (".../releases/assets/<id>"), so splitting
    // there keeps every digest with its own download url.
    let asset = json
        .split("/releases/assets/")
        .skip(1)
        .find(|a| json_str(a, "browser_download_url").is_some_and(|u| u.ends_with(".zip") && u.contains("windows")));
    let zip = asset.and_then(|a| json_str(a, "browser_download_url")).map(str::to_owned);
    let zip_sha256 = asset
        .and_then(|a| json_str(a, "digest"))
        .and_then(|d| d.strip_prefix("sha256:"))
        .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase);
    Some(Release { version, page, zip, zip_sha256 })
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

/// SHA-256 of a file as lowercase hex.
fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut f, &mut hasher).map_err(|e| e.to_string())?;
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Downloads the release zip, checks it against the SHA-256 GitHub
/// published for it, and replaces the executables next to this one.
/// Running executables are renamed first (Windows allows that), so the new
/// files take effect on the next start. Without a published digest
/// nothing is installed. If replacing any file fails, every file already
/// replaced is put back.
pub fn install(zip_url: &str, sha256: Option<&str>) -> Result<(), String> {
    let expected = sha256.ok_or("the release publishes no checksum for the download")?;
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
    let actual = sha256_file(&zip)?;
    if !actual.eq_ignore_ascii_case(expected) {
        let _ = std::fs::remove_dir_all(&work);
        tracing::warn!("update rejected: checksum {actual} does not match {expected}");
        return Err("the download is damaged or was altered (checksum mismatch)".into());
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
    // (new file, backup of the old one) for every file replaced so far.
    let mut replaced: Vec<(PathBuf, PathBuf)> = Vec::new();
    let rollback = |replaced: &[(PathBuf, PathBuf)]| {
        for (dst, old) in replaced.iter().rev() {
            let _ = std::fs::remove_file(dst);
            let _ = std::fs::rename(old, dst);
        }
    };
    for name in ["opensense.exe", "opensense-cli.exe"] {
        let Some(src) = find(&unpacked, name) else { continue };
        let dst = dir.join(name);
        let old = dir.join(format!("{name}.old"));
        let _ = std::fs::remove_file(&old);
        if dst.exists() {
            if let Err(e) = std::fs::rename(&dst, &old) {
                rollback(&replaced);
                return Err(format!("{name}: {e}"));
            }
        }
        if let Err(e) = std::fs::copy(&src, &dst) {
            let _ = std::fs::rename(&old, &dst);
            rollback(&replaced);
            return Err(format!("{name}: {e}"));
        }
        replaced.push((dst, old));
    }
    let _ = std::fs::remove_dir_all(&work);
    if replaced.is_empty() {
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

    const HASH_SETUP: &str = "be3ade783b0f6f8283e3b4f1d5c40aa0da3f4ee0b8431fa77316d9532d8ba1e6";
    const HASH_ZIP: &str = "11A8C798AF7DDA1982A47F9D1CAFCC25870BB3C57E7D6C2011A3447E810DB4A8";

    /// Shape of GitHub's releases/latest answer (trimmed).
    fn release_json(zip_digest: &str) -> String {
        format!(
            r#"{{"url":"https://api.github.com/repos/a/b/releases/1","html_url":"https://github.com/a/b/releases/tag/v0.2.0","tag_name":"v0.2.0",
            "assets":[
              {{"url":"https://api.github.com/repos/a/b/releases/assets/2","name":"OpenSense-v0.2.0-setup.exe",
                "uploader":{{"url":"https://api.github.com/users/x"}},"digest":"sha256:{HASH_SETUP}",
                "browser_download_url":"https://github.com/a/b/releases/download/v0.2.0/OpenSense-v0.2.0-setup.exe"}},
              {{"url":"https://api.github.com/repos/a/b/releases/assets/3","name":"OpenSense-v0.2.0-windows-x64.zip",
                "uploader":{{"url":"https://api.github.com/users/x"}},{zip_digest}
                "browser_download_url":"https://github.com/a/b/releases/download/v0.2.0/OpenSense-v0.2.0-windows-x64.zip"}}]}}"#
        )
    }

    #[test]
    fn parses_release_json() {
        let r = parse_release(&release_json(&format!(r#""digest":"sha256:{HASH_ZIP}","#))).unwrap();
        assert_eq!(r.version, "0.2.0");
        assert!(r.page.ends_with("v0.2.0"));
        assert!(r.zip.unwrap().ends_with("windows-x64.zip"));
        // The zip's own digest, not the installer's listed before it.
        assert_eq!(r.zip_sha256.as_deref(), Some(HASH_ZIP.to_ascii_lowercase().as_str()));
    }

    #[test]
    fn missing_or_malformed_digest_is_none() {
        assert_eq!(parse_release(&release_json("")).unwrap().zip_sha256, None);
        let bad = parse_release(&release_json(r#""digest":"sha256:1234","#)).unwrap();
        assert_eq!(bad.zip_sha256, None);
    }

    #[test]
    fn install_refuses_without_checksum() {
        assert!(install("https://invalid.example/x.zip", None).unwrap_err().contains("checksum"));
    }

    #[test]
    fn sha256_of_file() {
        let path = std::env::temp_dir().join("opensense-sha-test.txt");
        std::fs::write(&path, b"abc").unwrap();
        let h = sha256_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(h, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }
}
