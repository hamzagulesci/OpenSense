//! Lightweight tray agent (`opensense --tray`).
//!
//! The egui window holds an OpenGL context that costs ~100 MB with the
//! NVIDIA driver, so instead of hiding the window OpenSense closes it and
//! keeps only this agent running: no renderer, no OS monitors, just the
//! firmware worker, a tray icon and a Win32 message loop. A left click
//! starts the quick panel process; "Show" starts the window process again.

use std::time::Duration;

use opensense_hw::TransportKind;
use opensense_proto::mode::GpuMode;
use opensense_sys::config::AppConfig;
use opensense_sys::config::TempUnit;
use opensense_sys::oem::OemInfo;
use opensense_sys::SysError;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
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
    WM_HOTKEY, WM_QUIT, WM_TIMER,
};

use crate::i18n::{tr, Lang};
use crate::worker::{self, Command};

const AGENT_MUTEX: PCWSTR = w!("Local\\OpenSense.TrayAgent");
/// Signalled by the window after it saved a setting the agent shows.
const CONFIG_EVENT: PCWSTR = w!("Local\\OpenSense.ConfigChanged");
/// Signalled by the quick panel's "Quit" button.
const QUIT_EVENT: PCWSTR = w!("Local\\OpenSense.Quit");

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

/// Asks a running tray agent to exit.
pub fn request_quit() {
    if let Ok(h) = unsafe { OpenEventW(EVENT_MODIFY_STATE, false, QUIT_EVENT) } {
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

/// The foreground window belongs to another program (not OpenSense).
pub fn foreground_is_foreign() -> bool {
    let hwnd = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
    hwnd.0.is_null() || !is_own_window(hwnd)
}

/// Notices when the user switches to another app: true once the
/// foreground has belonged to another program for a moment, after an
/// OpenSense window had it. A window that never had the foreground (focus
/// stealing prevention) is left alone.
#[derive(Default)]
pub struct AwayWatch {
    had: bool,
    since: Option<std::time::Instant>,
}

impl AwayWatch {
    pub fn reset(&mut self) {
        self.had = true;
        self.since = None;
    }

    /// Call every frame; keeps the window repainting so a switch is seen
    /// even while it has no input.
    pub fn gone(&mut self, ctx: &eframe::egui::Context) -> bool {
        use std::time::{Duration, Instant};
        ctx.request_repaint_after(Duration::from_millis(250));
        if !foreground_is_foreign() {
            self.reset();
            return false;
        }
        if !self.had {
            return false;
        }
        // A short grace period so a window that is still opening can take
        // the focus first.
        match self.since {
            None => {
                self.since = Some(Instant::now());
                false
            }
            Some(t) if t.elapsed() > Duration::from_millis(250) => {
                self.had = false;
                self.since = None;
                true
            }
            Some(_) => false,
        }
    }
}

/// Keeps a window out of the taskbar and Alt+Tab, so it lives only in the
/// tray: Windows lists neither for a window that has an owner and no
/// WS_EX_APPWINDOW. The owner is a hidden window of this process.
/// The window this process keeps tray-only (each window is its own
/// process, so one is enough).
static TRAY_ONLY_WINDOW: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// Re-applies [`tray_only`] if the windowing library put WS_EX_APPWINDOW
/// back (it does when it shows the window), which would bring the taskbar
/// and Alt+Tab entry back. One style read per call; call it every frame.
pub fn keep_tray_only() {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, IsWindowVisible, SetWindowLongPtrW, ShowWindow, GWL_EXSTYLE, SW_HIDE, SW_SHOWNA,
        WS_EX_APPWINDOW,
    };
    let raw = TRAY_ONLY_WINDOW.load(std::sync::atomic::Ordering::Relaxed);
    if raw == 0 {
        return;
    }
    let hwnd = HWND(raw as *mut core::ffi::c_void);
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if ex & WS_EX_APPWINDOW.0 as isize == 0 {
            return;
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex & !(WS_EX_APPWINDOW.0 as isize));
        // The taskbar only re-reads the style when the window is shown.
        if IsWindowVisible(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_HIDE);
            let _ = ShowWindow(hwnd, SW_SHOWNA);
        }
    }
}

pub fn tray_only(hwnd: HWND) {
    TRAY_ONLY_WINDOW.store(hwnd.0 as isize, std::sync::atomic::Ordering::Relaxed);
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWLP_HWNDPARENT, GWL_EXSTYLE,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WINDOW_EX_STYLE, WS_EX_APPWINDOW,
        WS_POPUP,
    };
    let owner = unsafe {
        CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!(""), WS_POPUP, 0, 0, 0, 0, None, None, None, None)
    };
    let Ok(owner) = owner else { return };
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & !(WS_EX_APPWINDOW.0 as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
        SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, owner.0 as isize);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

/// Shows (if hidden) and raises one of OpenSense's windows.
pub fn raise_window(hwnd: HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, IsWindowVisible, SetWindowLongPtrW, GWL_EXSTYLE, SW_SHOW, WS_EX_APPWINDOW,
    };
    unsafe {
        // Hiding through egui puts WS_EX_APPWINDOW back, which would bring
        // the taskbar button back; keep the window tray-only.
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if ex & WS_EX_APPWINDOW.0 as isize != 0 {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex & !(WS_EX_APPWINDOW.0 as isize));
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        } else if !IsWindowVisible(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Closes the main window once it has been hidden for five minutes, so a
/// hidden window does not keep its memory for long.
fn close_idle_main_window() {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::WindowsAndMessaging::{IsWindowVisible, PostMessageW, WM_CLOSE};
    // The window property holds the tick count when the window was hidden.
    for prop in [crate::panel::MARK_PROP, crate::diag_window::MARK_PROP] {
        if let Some((hwnd, lost)) = find_marked_window(prop) {
            let hidden = !unsafe { IsWindowVisible(hwnd) }.as_bool();
            let idle = lost > 1 && (unsafe { GetTickCount() } as usize).wrapping_sub(lost) > 5 * 60 * 1000;
            if hidden && idle {
                let _ = unsafe { PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)) };
            }
        }
    }
}

/// Top-level window of this executable with exactly this title.
pub fn find_own_window(title: &str) -> Option<HWND> {
    let title = HSTRING::from(title);
    let mut after = HWND::default();
    while let Ok(hwnd) = unsafe { FindWindowExW(None, after, PCWSTR::null(), &title) } {
        if is_own_window(hwnd) {
            return Some(hwnd);
        }
        after = hwnd;
    }
    None
}

/// Lets the process started next take the foreground (this process has the
/// right while it handles a tray click or a click in its own window).
fn allow_foreground() {
    use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};
    let _ = unsafe { AllowSetForegroundWindow(ASFW_ANY) };
}

/// The main window of this executable (marked with a window property).
fn find_main_window() -> Option<(HWND, usize)> {
    find_marked_window(crate::panel::MARK_PROP)
}

/// A window of this executable carrying the property `prop`, with its value.
pub fn find_marked_window(prop: &str) -> Option<(HWND, usize)> {
    use windows::Win32::Foundation::{BOOL, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetPropW};
    struct Found(Option<(HWND, usize)>, HSTRING);
    unsafe extern "system" fn visit(hwnd: HWND, lp: LPARAM) -> BOOL {
        let found = unsafe { &mut *(lp.0 as *mut Found) };
        let v = unsafe { GetPropW(hwnd, &found.1) };
        if !v.0.is_null() && is_own_window(hwnd) {
            found.0 = Some((hwnd, v.0 as usize));
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut found = Found(None, HSTRING::from(prop));
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(std::ptr::addr_of_mut!(found) as isize)) };
    found.0
}

/// Left click on the tray icon: opens the main window; when it is open,
/// brings it to the front, or closes it if it was in front (also when it
/// lost focus to this very click less than 300 ms ago).
pub fn toggle_window(mock: bool) {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsWindowVisible, PostMessageW, WM_CLOSE};
    if let Some((hwnd, lost)) = find_main_window() {
        let visible = unsafe { IsWindowVisible(hwnd) }.as_bool();
        let foreground = unsafe { GetForegroundWindow() } == hwnd;
        // The tray click itself takes the focus away, so a window that lost
        // it (and hid) a moment ago was open: the click means "close".
        let just_lost = lost > 1 && (unsafe { GetTickCount() } as usize).wrapping_sub(lost) < 400;
        if (visible && foreground) || just_lost {
            if visible {
                let _ = unsafe { PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)) };
            }
        } else {
            raise_window(hwnd);
        }
        return;
    }
    allow_foreground();
    spawn_detached(if mock { &["--panel", "--mock"] } else { &["--panel"] });
}

/// Saves a settings change field by field. Every process keeps its own copy
/// of the settings, so saving a whole copy could undo what another process
/// saved meanwhile. Instead `edit` is applied to what the file holds now,
/// under the cross-process write lock, and `cfg` becomes that result. When
/// the file cannot be written `edit` is still applied to `cfg`, so the
/// change shows. `edit` should only set the fields the action changed (and
/// set them to values, not toggle what the file holds).
pub fn edit_config(cfg: &mut AppConfig, mut edit: impl FnMut(&mut AppConfig)) -> Result<(), SysError> {
    match AppConfig::update(&mut edit) {
        Ok(fresh) => {
            *cfg = fresh;
            Ok(())
        }
        Err(e) => {
            edit(cfg);
            Err(e)
        }
    }
}

/// [`edit_config`] for the tray agent: a failure is only logged.
fn save_choice(cfg: &mut AppConfig, edit: impl FnMut(&mut AppConfig)) {
    if let Err(e) = edit_config(cfg, edit) {
        tracing::warn!("cannot save settings: {e}");
    }
}

/// Runs automation code against the agent's settings and saves the changes
/// it made, as it made them (see [`crate::automation::Tracked`]). Returns
/// true when something changed. When the automation changed the lighting
/// (and sent it) but the file held other lighting changes from a window,
/// the merged lighting is sent again so the keyboard shows both.
fn run_automation(
    cfg: &mut AppConfig,
    worker: &worker::WorkerHandle,
    run: impl FnOnce(&mut crate::automation::Tracked),
) -> bool {
    let before = cfg.lighting.clone();
    let mut tracked = crate::automation::Tracked::new(cfg);
    run(&mut tracked);
    let edits = tracked.into_edits();
    if edits.is_empty() {
        return false;
    }
    let sent = cfg.lighting.clone();
    save_choice(cfg, |c| edits.apply(c));
    if sent != before && cfg.lighting != sent {
        let _ = worker.tx.send(crate::apply::lighting(&cfg.lighting));
    }
    true
}

/// What a tray-menu entry does.
enum Action {
    Mode(String),
    Gpu(GpuMode),
    BwIcon,
    Overlay,
    OverlayGameOnly,
    Quit,
}

/// Tray menu: modes (checked = current), GPU modes, B/W icon, Quit.
fn build_menu(cfg: &AppConfig, lang: Lang, gpu: Option<GpuMode>, has_modes: bool) -> (Menu, Vec<(MenuId, Action)>) {
    let menu = Menu::new();
    let mut actions = Vec::new();
    let sep = PredefinedMenuItem::separator;
    let current = crate::modes::current_key(cfg);
    let _ = menu.append(&MenuItem::new(tr(lang, "Mode"), false, None));
    for key in crate::modes::ordered(cfg) {
        let name = crate::modes::display_name(cfg, lang, &key);
        let item = CheckMenuItem::new(name, has_modes, key == current, None);
        let _ = menu.append(&item);
        actions.push((item.id().clone(), Action::Mode(key)));
    }
    let _ = menu.append(&sep());
    if let Some(g) = gpu {
        let _ = menu.append(&MenuItem::new(tr(lang, "GPU Mode"), false, None));
        for (m, label) in [(GpuMode::Hybrid, "Standard"), (GpuMode::Discrete, "Ultimate")] {
            let item = CheckMenuItem::new(tr(lang, label), true, g == m, None);
            let _ = menu.append(&item);
            actions.push((item.id().clone(), Action::Gpu(m)));
        }
        let _ = menu.append(&sep());
    }
    let bw = CheckMenuItem::new(tr(lang, "Black and white tray icon"), true, cfg.bw_icon, None);
    let _ = menu.append(&bw);
    actions.push((bw.id().clone(), Action::BwIcon));
    let _ = menu.append(&sep());
    let overlay = CheckMenuItem::new(tr(lang, "Overlay"), true, cfg.overlay, None);
    let _ = menu.append(&overlay);
    actions.push((overlay.id().clone(), Action::Overlay));
    let games = CheckMenuItem::new(tr(lang, "Overlay only in games"), cfg.overlay, cfg.overlay_game_only, None);
    let _ = menu.append(&games);
    actions.push((games.id().clone(), Action::OverlayGameOnly));
    let quit = MenuItem::new(tr(lang, "Quit"), true, None);
    let _ = menu.append(&quit);
    actions.push((quit.id().clone(), Action::Quit));
    (menu, actions)
}

/// Windows taskbar uses the light theme.
fn taskbar_light() -> bool {
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let mut v = 0u32;
    let mut size = 4u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(std::ptr::addr_of_mut!(v).cast()),
            Some(&mut size),
        )
    };
    r.is_ok() && v == 1
}

/// Tray icon: the coloured logo, or a monochrome glyph matching the taskbar.
fn tray_icon(bw: bool) -> Option<Icon> {
    let mut rgba = crate::icon::rgba(64);
    if bw {
        let c = if taskbar_light() { 0u8 } else { 255u8 };
        for px in rgba.as_chunks_mut::<4>().0 {
            px[0] = c;
            px[1] = c;
            px[2] = c;
        }
    }
    Icon::from_rgba(rgba, 64, 64).ok()
}

/// Asks before a GPU mode change and returns (switch, restart now).
fn confirm_gpu(lang: Lang, m: GpuMode) -> (bool, bool) {
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDCANCEL, IDYES, MB_ICONQUESTION, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNOCANCEL,
    };
    let text = if m == GpuMode::Discrete {
        tr(lang, "Ultimate Mode requires restart")
    } else {
        tr(lang, "Switching off Ultimate Mode requires restart")
    };
    let body = format!("{text}.\n\n{}", tr(lang, "Restart now? (No = apply at the next restart)"));
    let r = unsafe {
        MessageBoxW(
            None,
            &HSTRING::from(body),
            &HSTRING::from("OpenSense"),
            MB_YESNOCANCEL | MB_ICONQUESTION | MB_TOPMOST | MB_SETFOREGROUND,
        )
    };
    (r != IDCANCEL, r == IDYES)
}

/// Tooltip text and the readings the agent acts on, from one snapshot.
struct Tick {
    caps: Option<opensense_hw::device::Capabilities>,
    mode: Option<opensense_proto::mode::OperationMode>,
    gpu: Option<GpuMode>,
    cap: Option<bool>,
    percent: Option<u8>,
    temps: [Option<u16>; 2],
    /// Firmware connections made so far (see `Snapshot::connects`).
    connects: u32,
    tip: String,
}

fn read_tick(worker: &worker::WorkerHandle, cfg: &AppConfig, lang: Lang) -> Tick {
    let s = worker.shared.lock();
    let temp = |v: Option<u16>| match (v, cfg.temp_unit) {
        (None, _) => "--".to_owned(),
        (Some(c), TempUnit::Celsius) => format!("{c}°C"),
        (Some(c), TempUnit::Fahrenheit) => {
            format!("{:.0}°F", opensense_proto::sensor::celsius_to_fahrenheit(f32::from(c)))
        }
    };
    let fan = |v: Option<u16>| v.map(|r| format!(" {}: {r}RPM", tr(lang, "Fan"))).unwrap_or_default();
    let mut tip = format!("CPU: {}{}", temp(s.readings.cpu_temp), fan(s.readings.cpu_fan_rpm));
    if s.readings.gpu_temp.is_some() {
        tip.push_str(&format!("\nGPU: {}{}", temp(s.readings.gpu_temp), fan(s.readings.gpu_fan_rpm)));
    }
    if let Some(w) = s.battery_info.and_then(|b| b.rate_watts()).filter(|w| w.abs() >= 0.1) {
        let label = if w > 0.0 { "Charging" } else { "Discharging" };
        tip.push_str(&format!("\n{}: {:.1}W", tr(lang, label), w.abs()));
    }
    Tick {
        caps: s.caps,
        mode: s.mode,
        gpu: s.gpu_mux_supported.then_some(s.gpu_mode.unwrap_or(GpuMode::Hybrid)),
        cap: s.battery.filter(|b| b.limit_supported).map(|b| b.limit_active),
        percent: s.power.battery_percent,
        // Readings the worker has not refreshed for a few seconds (stuck
        // firmware call) count as missing, so they cannot drive the fans.
        temps: if s.readings_at.is_some_and(|t| t.elapsed() < STALE_AFTER) {
            [s.readings.cpu_temp, s.readings.gpu_temp]
        } else {
            [None, None]
        },
        connects: s.connects,
        tip,
    }
}

/// Sensor readings older than this are not used for the fan curve.
const STALE_AFTER: Duration = Duration::from_secs(5);

/// A gap this long between 1 s timer ticks means the PC slept.
const RESUME_GAP: Duration = Duration::from_secs(20);

/// Gives the fans back to their saved setting (normally Auto) when the
/// agent stops while a software curve drives them: on a normal quit and
/// also on a panic, since the guard is dropped while unwinding. It talks
/// to the firmware over its own connection, so it works even when the
/// worker thread is stuck or gone.
struct FanGuard {
    kind: TransportKind,
    profile: opensense_hw::device::DeviceProfile,
    /// Setting to restore; `None` while no curve is active.
    restore: Option<opensense_sys::config::FanConfig>,
}

impl Drop for FanGuard {
    fn drop(&mut self) {
        let Some(fan) = self.restore.take() else { return };
        let result = opensense_hw::open_transport(self.kind).and_then(|t| {
            let dev = opensense_hw::NitroDevice::new(t, self.profile.clone());
            match crate::apply::fan(&fan) {
                Command::FansMax => dev.set_fans_max(),
                Command::FansCustom(c) => dev.set_fans_custom(c),
                _ => dev.set_fans_auto(),
            }
        });
        match result {
            Ok(()) => tracing::info!("fan curve stopped: fans restored to {:?}", fan.mode),
            Err(e) => tracing::error!("fan curve stopped but the fans could not be restored: {e}"),
        }
    }
}

/// Charge target between 80 and 100 % and the one-time full charge.
fn charge_tick(cfg: &mut AppConfig, worker: &worker::WorkerHandle, cap: Option<bool>, percent: Option<u8>) {
    if cfg.charge_full {
        if percent.is_some_and(|p| p > 99) {
            save_choice(cfg, |c| c.charge_full = false);
            let target = cfg.charge_target().unwrap_or(100);
            if let Some(on) = cap.and_then(|cap| crate::apply::charge_cap_for(target, percent, cap)) {
                let _ = worker.tx.send(Command::ChargeLimit(on));
            }
        } else if cap == Some(true) {
            let _ = worker.tx.send(Command::ChargeLimit(false));
        }
    } else if let Some(target) = cfg.charge_target().filter(|t| (81..100).contains(t)) {
        if let Some(on) = cap.and_then(|cap| crate::apply::charge_cap_for(target, percent, cap)) {
            let _ = worker.tx.send(Command::ChargeLimit(on));
        }
    }
}

/// Mode key and stored CPU / GPU curves.
type FanSource = (String, Vec<[u8; 2]>, Vec<[u8; 2]>);

type MenuState = (String, Vec<String>, Option<GpuMode>, (bool, bool, bool), Lang, bool);

/// Starts or stops the overlay to match the settings.
fn sync_overlay(overlay: &mut Option<crate::overlay::Overlay>, cfg: &AppConfig) {
    match (cfg.overlay, overlay.is_some()) {
        (true, false) => *overlay = crate::overlay::Overlay::start(cfg),
        (false, true) => *overlay = None,
        _ => {}
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
    let mut cfg = AppConfig::load();
    let mut lang = Lang::resolve(&cfg.language);
    let oem = OemInfo::detect();
    // Auto-reset events: settings changed / quit requested.
    let Ok(config_event) = (unsafe { CreateEventW(None, false, false, CONFIG_EVENT) }) else { return };
    let Ok(quit_event) = (unsafe { CreateEventW(None, false, false, QUIT_EVENT) }) else { return };

    let worker = worker::spawn(kind, &oem, Duration::from_secs(1), 4, false, || {});
    let mut guard = FanGuard { kind, profile: worker::device_profile(&oem), restore: None };
    // Hands the fans back at sign-out / shutdown too.
    let session_end = crate::session_end::SessionEnd::new(kind, worker::device_profile(&oem));
    // Saved firmware settings are sent whenever a connection is made (first
    // connect, reconnect) and after resume; see the timer tick below.
    let mut applied_connects = 0u32;
    let mut last_tick = std::time::Instant::now();
    if cfg.apply_on_start {
        let key = crate::modes::current_key(&cfg);
        let (p, base) = (crate::modes::profile(&cfg, &key), crate::modes::base_of(&cfg, &key));
        std::thread::spawn(move || crate::modes::apply_windows(&p, base));
    }

    let has_modes = oem.has_operation_modes();
    let (menu, mut actions) = build_menu(&cfg, lang, None, has_modes);
    let Some(icon) = tray_icon(cfg.bw_icon) else { return };
    // Left click opens the main window; the menu is on the right button.
    let tray = match TrayIconBuilder::new()
        .with_tooltip("OpenSense")
        .with_icon(icon)
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()
    {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("tray icon unavailable: {e}");
            return;
        }
    };
    // What the current menu was built from; rebuilt when it changes.
    let mut menu_state: MenuState = (
        crate::modes::current_key(&cfg),
        crate::modes::ordered(&cfg),
        None,
        (cfg.bw_icon, cfg.overlay, cfg.overlay_game_only),
        lang,
        has_modes,
    );
    let mut overlay: Option<crate::overlay::Overlay> = None;
    sync_overlay(&mut overlay, &cfg);
    let mut game_only = cfg.overlay_game_only;
    let mut fan_loop = crate::modes::FanLoop::default();
    let mut restart_after: Option<std::time::Instant> = None;
    // Answer of the GPU mode question, asked on its own thread: a modal box
    // on this thread would stop the fan curve while it is open.
    let (gpu_tx, gpu_rx) = std::sync::mpsc::channel::<(GpuMode, bool, bool)>();
    let mut gpu_asking = false;
    // Mode and curves the fan loop last ran with.
    let mut fan_source: Option<FanSource> = None;
    // Firmware mode seen on consecutive ticks, and when OpenSense last
    // switched modes itself (the firmware may still report the old one).
    let mut seen_mode: (Option<opensense_proto::mode::OperationMode>, u32) = (None, 0);
    let mut last_switch = std::time::Instant::now();
    let mut auto = crate::automation::Automation::new();
    auto.set_hotkeys(cfg.hotkeys);
    auto.apply_tweaks(&cfg);

    // 1 s timer: fan-curve loop, charge logic, tooltip. Between ticks the
    // loop sleeps until a window message or one of the events arrives.
    let timer = unsafe { SetTimer(None, 0, 1000, None) };
    let mut msg = MSG::default();
    'outer: loop {
        let woke =
            unsafe { MsgWaitForMultipleObjects(Some(&[config_event, quit_event]), false, INFINITE, QS_ALLINPUT) };
        if woke.0 == WAIT_OBJECT_0.0 + 1 {
            break;
        }
        if woke == WAIT_OBJECT_0 {
            last_switch = std::time::Instant::now();
            cfg = AppConfig::load();
            lang = Lang::resolve(&cfg.language);
            let _ = tray.set_icon(tray_icon(cfg.bw_icon));
            auto.set_hotkeys(cfg.hotkeys);
            auto.apply_tweaks(&cfg);
            if cfg.overlay_game_only != game_only {
                // Restart so visibility starts over.
                game_only = cfg.overlay_game_only;
                overlay = None;
            }
            sync_overlay(&mut overlay, &cfg);
        }
        let mut tick = false;
        let mut hotkeys = Vec::new();
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            if msg.message == WM_QUIT {
                break 'outer;
            }
            tick |= msg.message == WM_TIMER;
            if msg.message == WM_HOTKEY {
                hotkeys.extend(crate::automation::Hotkey::from_id(msg.wParam.0));
            }
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        for h in hotkeys {
            if run_automation(&mut cfg, &worker, |c| auto.hotkey(h, c, &worker, lang, mock)) {
                last_switch = std::time::Instant::now();
                fan_loop.reset();
            }
            sync_overlay(&mut overlay, &cfg);
        }
        if tick {
            run_automation(&mut cfg, &worker, |c| auto.tick(c, &worker, lang));
            if let Some(o) = overlay.as_mut() {
                if o.tick(&mut cfg, &crate::overlay::metrics(&worker)) {
                    let (mode, scale, pos) = (cfg.overlay_mode, cfg.overlay_scale, cfg.overlay_pos);
                    save_choice(&mut cfg, |c| {
                        c.overlay_mode = mode;
                        c.overlay_scale = scale;
                        c.overlay_pos = pos;
                    });
                }
            }
            let now = read_tick(&worker, &cfg, lang);
            let _ = tray.set_tooltip(Some(now.tip.clone()));
            close_idle_main_window();

            // Back from sleep, or connected (again) to the firmware: it may
            // have reset mode, lighting and the charge cap, so send them.
            let resumed = last_tick.elapsed() > RESUME_GAP;
            last_tick = std::time::Instant::now();
            if resumed {
                tracing::info!("resumed from sleep: re-applying settings");
            }
            if resumed || now.connects != applied_connects {
                applied_connects = now.connects;
                if cfg.apply_on_start && now.connects > 0 {
                    for c in crate::apply::saved_state(&cfg, &oem) {
                        let _ = worker.tx.send(c);
                    }
                }
                fan_loop.reset();
                // The firmware may report a stale mode for a while.
                last_switch = std::time::Instant::now();
            }

            // Software fan curve of the current mode.
            let key = crate::modes::current_key(&cfg);
            let profile = crate::modes::profile(&cfg, &key);
            // Another mode or an edited curve: start over, otherwise the
            // hysteresis would hold the old (higher) speed until the
            // temperature drops.
            let source = (key.clone(), profile.cpu_curve.clone(), profile.gpu_curve.clone());
            if fan_source.as_ref() != Some(&source) {
                if fan_source.is_some() && profile.apply_curve {
                    fan_loop.restart();
                }
                fan_source = Some(source);
            }
            if profile.apply_curve {
                let c = crate::modes::from_stored(&profile.cpu_curve);
                let g = crate::modes::from_stored(&profile.gpu_curve);
                if let (Some(c), Some(g)) = (c, g) {
                    match fan_loop.step([&c, &g], now.temps, std::time::Instant::now()) {
                        Some(crate::modes::FanStep::Custom(f)) => {
                            let _ = worker.tx.send(Command::FansCustom(f));
                        }
                        Some(crate::modes::FanStep::Auto) => {
                            tracing::warn!("no temperature readings: fans handed back to the firmware");
                            let _ = worker.tx.send(Command::FansAuto);
                        }
                        None => {}
                    }
                }
            } else if fan_loop.active() {
                fan_loop.reset();
                let _ = worker.tx.send(crate::apply::fan(&cfg.fan));
            }
            guard.restore = fan_loop.active().then(|| cfg.fan.clone());
            session_end.set_restore(guard.restore.clone());

            charge_tick(&mut cfg, &worker, now.cap, now.percent);

            // The mode changed outside OpenSense (hardware key, other app):
            // follow it so every window shows the real mode.
            seen_mode = if seen_mode.0 == now.mode { (now.mode, seen_mode.1 + 1) } else { (now.mode, 1) };
            if let Some(m) = now.mode.filter(|_| seen_mode.1 >= 3) {
                let builtin =
                    crate::modes::base_mode(&key).is_some() && crate::modes::BUILTIN.iter().any(|(_, b)| *b == m);
                let settled = last_switch.elapsed() > Duration::from_secs(10);
                if builtin && settled && crate::modes::base_of(&cfg, &key) != m {
                    let name = m.label().to_ascii_lowercase();
                    save_choice(&mut cfg, |c| {
                        c.current_mode = Some(name.clone());
                        c.operation_mode = Some(name.clone());
                    });
                }
            }

            // Restart after a confirmed GPU mode write.
            if let Some(since) = restart_after {
                match crate::apply::gpu_write_state(&worker.shared, since) {
                    crate::apply::WriteState::Pending => {}
                    crate::apply::WriteState::Done => {
                        restart_after = None;
                        let _ = opensense_sys::power::restart_now();
                    }
                    crate::apply::WriteState::Failed(e) => {
                        restart_after = None;
                        tracing::warn!("GPU mode: {e:?}");
                    }
                }
            }

            // Rebuild the menu when what it shows changed.
            let has_modes = now.caps.map_or(has_modes, |c| c.operation_modes);
            let state: MenuState = (
                key,
                crate::modes::ordered(&cfg),
                now.gpu,
                (cfg.bw_icon, cfg.overlay, cfg.overlay_game_only),
                lang,
                has_modes,
            );
            if state != menu_state {
                let (menu, a) = build_menu(&cfg, lang, now.gpu, has_modes);
                tray.set_menu(Some(Box::new(menu)));
                actions = a;
                menu_state = state;
            }
        }
        while let Ok((m, switch, restart)) = gpu_rx.try_recv() {
            gpu_asking = false;
            if switch {
                crate::apply::begin_gpu_write(&worker, m);
                if restart {
                    restart_after = Some(std::time::Instant::now());
                }
            }
        }
        while let Ok(e) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                toggle_window(mock);
            }
        }
        while let Ok(e) = MenuEvent::receiver().try_recv() {
            let Some((_, action)) = actions.iter().find(|(id, _)| *id == e.id) else { continue };
            match action {
                Action::Mode(key) => {
                    let key = key.clone();
                    save_choice(&mut cfg, |c| crate::modes::select(c, &key));
                    crate::modes::activate(&cfg, &key, &worker);
                    last_switch = std::time::Instant::now();
                    fan_loop.reset();
                    menu_state.0.clear();
                }
                Action::Gpu(m) => {
                    let m = *m;
                    if !gpu_asking {
                        gpu_asking = true;
                        let tx = gpu_tx.clone();
                        std::thread::spawn(move || {
                            let (switch, restart) = confirm_gpu(lang, m);
                            let _ = tx.send((m, switch, restart));
                        });
                    }
                    menu_state.0.clear();
                }
                Action::BwIcon => {
                    let on = !cfg.bw_icon;
                    save_choice(&mut cfg, |c| c.bw_icon = on);
                    let _ = tray.set_icon(tray_icon(cfg.bw_icon));
                    menu_state.0.clear();
                }
                Action::Overlay => {
                    let on = !cfg.overlay;
                    save_choice(&mut cfg, |c| c.overlay = on);
                    sync_overlay(&mut overlay, &cfg);
                    menu_state.0.clear();
                }
                Action::OverlayGameOnly => {
                    let on = !cfg.overlay_game_only;
                    save_choice(&mut cfg, |c| c.overlay_game_only = on);
                    game_only = cfg.overlay_game_only;
                    overlay = None;
                    sync_overlay(&mut overlay, &cfg);
                    menu_state.0.clear();
                }
                Action::Quit => break 'outer,
            }
        }
    }
    drop(overlay);
    auto.stop(&cfg);
    crate::nvidia::release_clock_limit();
    // The guard restores the fans synchronously when it is dropped below.
    guard.restore = fan_loop.active().then(|| cfg.fan.clone());
    unsafe {
        let _ = KillTimer(None, timer);
        let _ = CloseHandle(config_event);
        let _ = CloseHandle(quit_event);
    }
    drop(tray);
    drop(guard);
}
