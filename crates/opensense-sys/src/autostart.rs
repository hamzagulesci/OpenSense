//! Start-at-logon. An elevated app cannot use the Run key without a UAC
//! prompt at every logon, so a Task Scheduler task with "run with highest
//! privileges" starts the tray agent instead.
//!
//! The task is created from XML because `schtasks /Create` on its own uses
//! defaults that suit desktops, not laptops: "start only on AC power",
//! "stop when switching to battery" and a 3-day run-time limit.

use std::process::Command;

use crate::SysError;

pub const TASK_NAME: &str = "OpenSense";

fn schtasks(args: &[&str]) -> Result<std::process::Output, SysError> {
    let mut cmd = Command::new("schtasks.exe");
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    Ok(cmd.output()?)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn task_xml(exe: &str, user: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Starts the OpenSense tray process at sign-in.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe}</Command>
      <Arguments>--tray</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
        exe = xml_escape(exe),
        user = xml_escape(user),
    )
}

/// Whether the task exists. Reads Task Scheduler's registry cache, which is
/// instant; falls back to `schtasks /Query` (~100 ms, one process) when the
/// cache cannot be read.
pub fn is_enabled() -> bool {
    #[cfg(windows)]
    {
        use windows::core::HSTRING;
        use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
        use windows::Win32::System::Registry::{RegCloseKey, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ};
        let path =
            HSTRING::from(format!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Schedule\TaskCache\Tree\{TASK_NAME}"));
        let mut key = HKEY::default();
        let r = unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, &path, 0, KEY_READ, &mut key) };
        if r.is_ok() {
            unsafe {
                let _ = RegCloseKey(key);
            }
            return true;
        }
        if r == ERROR_FILE_NOT_FOUND {
            return false;
        }
    }
    schtasks(&["/Query", "/TN", TASK_NAME]).map(|o| o.status.success()).unwrap_or(false)
}

/// Registers the task for the current user, launching this executable's
/// tray agent.
pub fn enable() -> Result<(), SysError> {
    enable_for(&std::env::current_exe()?)
}

/// Registers the task to start `exe --tray` at sign-in.
pub fn enable_for(exe: &std::path::Path) -> Result<(), SysError> {
    let user = match (std::env::var("USERDOMAIN"), std::env::var("USERNAME")) {
        (Ok(d), Ok(u)) => format!("{d}\\{u}"),
        (_, Ok(u)) => u,
        _ => return Err(SysError::Config("cannot determine the current user".into())),
    };
    let xml = task_xml(&exe.display().to_string(), &user);

    // Task Scheduler expects the file in UTF-16 LE with a BOM.
    let mut bytes = vec![0xFF, 0xFE];
    for unit in xml.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    // Not a fixed name: another process of the same user could otherwise
    // prepare or swap the file this elevated process hands to schtasks.
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    let path = std::env::temp_dir().join(format!("opensense-task-{}-{nonce:08x}.xml", std::process::id()));
    std::fs::write(&path, bytes)?;
    let out = schtasks(&["/Create", "/F", "/TN", TASK_NAME, "/XML", &path.display().to_string()]);
    let _ = std::fs::remove_file(&path);
    let out = out?;
    if out.status.success() {
        Ok(())
    } else {
        Err(SysError::Win(String::from_utf8_lossy(&out.stderr).trim().to_owned()))
    }
}

pub fn disable() -> Result<(), SysError> {
    if !is_enabled() {
        return Ok(());
    }
    let out = schtasks(&["/Delete", "/F", "/TN", TASK_NAME])?;
    if out.status.success() {
        Ok(())
    } else {
        Err(SysError::Win(String::from_utf8_lossy(&out.stderr).trim().to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_has_laptop_friendly_settings_and_escapes() {
        let x = task_xml(r"C:\A & B\opensense.exe", r"PC\user");
        assert!(x.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
        assert!(x.contains("<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>"));
        assert!(x.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(x.contains(r"C:\A &amp; B\opensense.exe"));
    }
}
