//! Elevation helpers. `AcerGamingFunction` is only readable by
//! administrators, so OpenSense checks up front and can relaunch itself
//! elevated (the `runas` verb shows the UAC prompt).

#[cfg(windows)]
pub fn is_elevated() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    false
}

/// Relaunches the current executable with the UAC prompt. Returns `Ok(())`
/// when the elevated copy was started; the caller should then exit.
#[cfg(windows)]
pub fn relaunch_elevated(args: &str) -> Result<(), crate::SysError> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let exe = std::env::current_exe()?;
    let exe = HSTRING::from(exe.as_os_str());
    let args = HSTRING::from(args);
    let verb = HSTRING::from("runas");
    let h = unsafe { ShellExecuteW(None, &verb, &exe, &args, PCWSTR::null(), SW_SHOWNORMAL) };
    // ShellExecute returns a value > 32 on success.
    if h.0 as isize > 32 {
        Ok(())
    } else {
        Err(crate::SysError::Win(format!("ShellExecute(runas) failed: {}", h.0 as isize)))
    }
}

#[cfg(not(windows))]
pub fn relaunch_elevated(_args: &str) -> Result<(), crate::SysError> {
    Err(crate::SysError::Unsupported)
}
