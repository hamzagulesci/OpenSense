//! Gives the fans back when Windows signs out, restarts or shuts down while
//! the tray agent drives them with a software curve.
//!
//! Windows sends `WM_QUERYENDSESSION` / `WM_ENDSESSION` only to top-level
//! windows (not to message-only ones), and may end the process as soon as
//! `WM_ENDSESSION` returns, so the agent keeps a hidden top-level window and
//! restores the fans inside that message. The window belongs to the thread
//! that created it; the tray loop's `PeekMessageW` / `DispatchMessageW`
//! delivers its messages. "End task" in Task Manager cannot be caught.

use std::sync::Mutex;

use opensense_hw::TransportKind;
use opensense_sys::config::FanConfig;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, WM_ENDSESSION, WM_QUERYENDSESSION, WNDCLASSW,
    WS_EX_TOOLWINDOW, WS_POPUP,
};

use crate::worker::Command;

/// What to restore and how to reach the firmware.
struct State {
    kind: TransportKind,
    profile: opensense_hw::device::DeviceProfile,
    /// Fan setting to restore; `None` while no curve is active.
    restore: Option<FanConfig>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

/// Hidden top-level window that receives the end-of-session messages;
/// destroyed when dropped.
pub struct SessionEnd {
    hwnd: Option<HWND>,
}

impl SessionEnd {
    /// Creates the window on the calling thread (the tray loop's thread).
    pub fn new(kind: TransportKind, profile: opensense_hw::device::DeviceProfile) -> Self {
        *STATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(State { kind, profile, restore: None });
        let class = w!("OpenSense.SessionEnd");
        let hwnd = unsafe {
            let instance = GetModuleHandleW(None).unwrap_or_default();
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: instance.into(),
                lpszClassName: class,
                ..Default::default()
            };
            // Fails harmlessly when the class exists already.
            RegisterClassW(&wc);
            CreateWindowExW(WS_EX_TOOLWINDOW, class, w!(""), WS_POPUP, 0, 0, 0, 0, None, None, instance, None)
        };
        match hwnd {
            Ok(h) => Self { hwnd: Some(h) },
            Err(e) => {
                tracing::warn!("no end-of-session window; fans are not restored at sign-out: {e}");
                Self { hwnd: None }
            }
        }
    }

    /// The fan setting to restore at sign-out / shutdown, or `None` while
    /// no software curve drives the fans. Call after each change.
    pub fn set_restore(&self, restore: Option<FanConfig>) {
        if let Some(s) = STATE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            s.restore = restore;
        }
    }
}

impl Drop for SessionEnd {
    fn drop(&mut self) {
        if let Some(h) = self.hwnd.take() {
            let _ = unsafe { DestroyWindow(h) };
        }
        *STATE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// Sets the fans to `fan` over a connection of its own (the worker thread
/// may be busy or stuck).
pub fn restore_fans(kind: TransportKind, profile: &opensense_hw::device::DeviceProfile, fan: &FanConfig) {
    let result = opensense_hw::open_transport(kind).and_then(|t| {
        let dev = opensense_hw::NitroDevice::new(t, profile.clone());
        match crate::apply::fan(fan) {
            Command::FansMax => dev.set_fans_max(),
            Command::FansCustom(c) => dev.set_fans_custom(c),
            _ => dev.set_fans_auto(),
        }
    });
    match result {
        Ok(()) => tracing::info!("session ending: fans restored to {:?}", fan.mode),
        Err(e) => tracing::error!("session ending but the fans could not be restored: {e}"),
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // Never block the sign-out.
        WM_QUERYENDSESSION => LRESULT(1),
        // wParam != 0: the session really ends (not cancelled).
        WM_ENDSESSION => {
            if wparam.0 != 0 {
                let taken = STATE
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .as_mut()
                    .and_then(|s| s.restore.take().map(|f| (s.kind, s.profile.clone(), f)));
                match taken {
                    Some((kind, profile, fan)) => restore_fans(kind, &profile, &fan),
                    None => tracing::info!("session ending: no fan curve active"),
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
