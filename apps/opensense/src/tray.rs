//! Lightweight tray agent (`opensense --tray`).
//!
//! The egui window holds an OpenGL context that costs ~100 MB with the
//! NVIDIA driver, so instead of hiding the window OpenSense closes it and
//! keeps only this agent running: no renderer, no OS monitors, just the
//! firmware worker, a tray icon and a Win32 message loop. "Show" starts the
//! window process again.

use std::time::Duration;

use opensense_hw::TransportKind;
use opensense_proto::mode::OperationMode;
use opensense_sys::config::AppConfig;
use opensense_sys::config::{FanModeCfg, TempUnit};
use opensense_sys::oem::OemInfo;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, TrayIconBuilder, TrayIconEvent};
use windows::core::PWSTR;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HWND, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, OpenEventW, OpenMutexW, OpenProcess, QueryFullProcessImageNameW, SetEvent,
    EVENT_MODIFY_STATE, INFINITE, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, SYNCHRONIZATION_SYNCHRONIZE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, FindWindowExW, GetWindowThreadProcessId, IsIconic, KillTimer, MsgWaitForMultipleObjects,
    PeekMessageW, SetForegroundWindow, SetTimer, ShowWindow, TranslateMessage, MSG, PM_REMOVE, QS_ALLINPUT, SW_RESTORE,
    WM_QUIT, WM_TIMER,
};

use crate::i18n::{tr, Lang};
use crate::worker::{self, Command};

const AGENT_MUTEX: PCWSTR = w!("Local\\OpenSense.TrayAgent");
/// Signalled by the window after it saved a setting the agent shows.
const CONFIG_EVENT: PCWSTR = w!("Local\\OpenSense.ConfigChanged");
const WINDOW_TITLE: &str = "OpenSense";

/// True when a tray agent is already running in this session.
pub fn is_running() -> bool {
    match unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, AGENT_MUTEX) } {
        Ok(h) => {
            let _ = unsafe { CloseHandle(h) };
            true
        }
        Err(_) => false,
    }
}

/// Starts the agent unless it is running. Returns true if it was started
/// now (it then applies the saved settings itself). Without administrator
/// rights the agent could not reach the firmware, so it is not started.
pub fn ensure_running(mock: bool) -> bool {
    if is_running() || !(mock || opensense_sys::privilege::is_elevated()) {
        return false;
    }
    spawn_detached(if mock { &["--tray", "--mock"] } else { &["--tray"] })
}

/// Starts another OpenSense process that is not a child of this one (via
/// `cmd /c start`), so Task Manager lists the window and the tray agent
/// separately instead of grouping them.
pub fn spawn_detached(args: &[&str]) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let Ok(exe) = std::env::current_exe() else { return false };
    // Built by hand: `start` needs an empty "" title before the quoted path,
    // which std's argument quoting would escape.
    let line = format!("/c start \"\" \"{}\" {}", exe.display(), args.join(" "));
    std::process::Command::new("cmd.exe")
        .raw_arg(line)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| tracing::warn!("cannot start OpenSense process: {e}"))
        .is_ok()
}

/// Tells a running tray agent to reload the configuration (language).
pub fn notify_config_changed() {
    if let Ok(h) = unsafe { OpenEventW(EVENT_MODIFY_STATE, false, CONFIG_EVENT) } {
        unsafe {
            let _ = SetEvent(h);
            let _ = CloseHandle(h);
        }
    }
}

/// True when `hwnd` belongs to a process running this same executable, so a
/// folder or another app titled "OpenSense" is never picked.
fn is_own_window(hwnd: HWND) -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else { return false };
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) };
    let _ = unsafe { CloseHandle(process) };
    ok.is_ok() && String::from_utf16_lossy(&buf[..len as usize]).eq_ignore_ascii_case(&exe.display().to_string())
}

/// Brings the window to the front, or starts the window process.
fn show_window(mock: bool) {
    let title = HSTRING::from(WINDOW_TITLE);
    let mut after = HWND::default();
    while let Ok(hwnd) = unsafe { FindWindowExW(None, after, PCWSTR::null(), &title) } {
        if is_own_window(hwnd) {
            unsafe {
                if IsIconic(hwnd).as_bool() {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                }
                let _ = SetForegroundWindow(hwnd);
            }
            return;
        }
        after = hwnd;
    }
    spawn_detached(if mock { &["--mock"] } else { &[] });
}

/// Persists a tray-menu choice so the window and the next start agree.
fn save_choice(update: impl FnOnce(&mut AppConfig)) {
    let mut cfg = AppConfig::load();
    update(&mut cfg);
    if let Err(e) = cfg.save() {
        tracing::warn!("cannot save settings: {e}");
    }
}

pub fn run(kind: TransportKind) {
    // Single agent per session.
    let mutex = unsafe { CreateMutexW(None, true, AGENT_MUTEX) };
    if mutex.is_err() || unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return;
    }
    let mock = kind == TransportKind::Mock;
    if !mock && !opensense_sys::privilege::is_elevated() {
        tracing::warn!("tray agent needs administrator rights");
        return;
    }
    let cfg = AppConfig::load();
    let mut lang = Lang::resolve(&cfg.language);
    let mut unit = cfg.temp_unit;
    let t = |lang, s| tr(lang, s);
    let oem = OemInfo::detect();
    // Auto-reset event the window sets after changing the language.
    let Ok(config_event) = (unsafe { CreateEventW(None, false, false, CONFIG_EVENT) }) else { return };

    let worker = worker::spawn(kind, &oem, Duration::from_secs(3), 4, false, || {});
    if cfg.apply_on_start {
        for c in crate::apply::saved_state(&cfg, &oem) {
            let _ = worker.tx.send(c);
        }
    }

    let has_modes = oem.has_operation_modes();
    let menu = Menu::new();
    let show = MenuItem::new(t(lang, "Show OpenSense"), true, None);
    let quiet = MenuItem::new(t(lang, "Mode: Quiet"), has_modes, None);
    let default = MenuItem::new(t(lang, "Mode: Default"), has_modes, None);
    let extreme = MenuItem::new(t(lang, "Mode: Extreme"), has_modes, None);
    let fan_auto = MenuItem::new(t(lang, "Fans: Auto"), true, None);
    let fan_max = MenuItem::new(t(lang, "Fans: Max"), true, None);
    let quit = MenuItem::new(t(lang, "Quit"), true, None);
    let labeled = [
        (&show, "Show OpenSense"),
        (&quiet, "Mode: Quiet"),
        (&default, "Mode: Default"),
        (&extreme, "Mode: Extreme"),
        (&fan_auto, "Fans: Auto"),
        (&fan_max, "Fans: Max"),
        (&quit, "Quit"),
    ];
    let sep = PredefinedMenuItem::separator;
    if menu
        .append_items(&[&show, &sep(), &quiet, &default, &extreme, &sep(), &fan_auto, &fan_max, &sep(), &quit])
        .is_err()
    {
        return;
    }
    let Ok(icon) = Icon::from_rgba(crate::icon::rgba(64), 64, 64) else { return };
    let tray = match TrayIconBuilder::new().with_tooltip("OpenSense").with_icon(icon).with_menu(Box::new(menu)).build()
    {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("tray icon unavailable: {e}");
            return;
        }
    };

    // Tooltip refresh timer. The loop sleeps until a window message or the
    // config event arrives; menu/tray events are drained after each wake.
    let timer = unsafe { SetTimer(None, 0, 3000, None) };
    let mut msg = MSG::default();
    'outer: loop {
        let woke = unsafe { MsgWaitForMultipleObjects(Some(&[config_event]), false, INFINITE, QS_ALLINPUT) };
        if woke == WAIT_OBJECT_0 {
            let cfg = AppConfig::load();
            unit = cfg.temp_unit;
            let new_lang = Lang::resolve(&cfg.language);
            if new_lang != lang {
                lang = new_lang;
                for (item, key) in &labeled {
                    item.set_text(t(lang, key));
                }
            }
        }
        let mut tick = false;
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            if msg.message == WM_QUIT {
                break 'outer;
            }
            tick |= msg.message == WM_TIMER;
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if tick {
            // Enable mode entries once the firmware probe has answered.
            if let Some(caps) = worker.shared.lock().caps {
                for item in [&quiet, &default, &extreme] {
                    item.set_enabled(caps.operation_modes);
                }
            }
            let s = worker.shared.lock();
            let temp = |v: Option<u16>| match (v, unit) {
                (None, _) => "--".to_owned(),
                (Some(c), TempUnit::Celsius) => format!("{c}°C"),
                (Some(c), TempUnit::Fahrenheit) => {
                    format!("{:.0}°F", opensense_proto::sensor::celsius_to_fahrenheit(f32::from(c)))
                }
            };
            let mode = s.mode.map(|m| t(lang, m.label())).unwrap_or("--");
            let tip = format!(
                "OpenSense\nCPU {}  ·  GPU {}\n{}: {}",
                temp(s.readings.cpu_temp),
                temp(s.readings.gpu_temp),
                t(lang, "Mode"),
                mode
            );
            drop(s);
            let _ = tray.set_tooltip(Some(tip));
        }
        while let Ok(e) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } = e {
                show_window(mock);
            }
        }
        while let Ok(e) = MenuEvent::receiver().try_recv() {
            let send = |c: Command| {
                let _ = worker.tx.send(c);
            };
            match &e.id {
                id if id == show.id() => show_window(mock),
                id if id == quiet.id() || id == default.id() || id == extreme.id() => {
                    let m = if id == quiet.id() {
                        OperationMode::Quiet
                    } else if id == default.id() {
                        OperationMode::Default
                    } else {
                        OperationMode::Extreme
                    };
                    send(Command::Mode(m));
                    save_choice(|c| {
                        c.operation_mode = Some(m.label().to_ascii_lowercase());
                        if m.forces_auto_fan() {
                            c.fan.mode = FanModeCfg::Auto;
                        }
                    });
                }
                id if id == fan_auto.id() => {
                    send(Command::FansAuto);
                    save_choice(|c| c.fan.mode = FanModeCfg::Auto);
                }
                id if id == fan_max.id() => {
                    send(Command::FansMax);
                    save_choice(|c| c.fan.mode = FanModeCfg::Max);
                }
                id if id == quit.id() => break 'outer,
                _ => {}
            }
        }
    }
    unsafe {
        let _ = KillTimer(None, timer);
        let _ = CloseHandle(config_event);
    }
    drop(tray);
}
