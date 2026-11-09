#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod apply;
mod automation;
mod extra_window;
mod fans_window;
mod fps;
mod gh;
mod i18n;
mod icon;
mod modes;
mod nvidia;
mod osd;
mod overlay;
mod panel;
mod tray;
mod updates;
mod updates_window;
mod worker;

use eframe::egui;
use opensense_hw::TransportKind;

struct Args {
    transport: TransportKind,
    /// Run only the tray agent (`--tray`; `--minimized` is accepted for
    /// older autostart entries).
    tray: bool,
    /// Quick panel next to the notification area (`--panel`).
    panel: bool,
    /// Fans and Power window (`--fans`); with `--panel` it opens both.
    fans: bool,
    /// Extra Settings window (`--extra`).
    extra: bool,
    /// Updates window (`--updates`).
    updates: bool,
    /// Started by "Restart as administrator": the previous window may still
    /// be shutting down.
    relaunched: bool,
}

fn parse_args() -> Args {
    let mut a = Args {
        transport: TransportKind::Wmi,
        tray: false,
        panel: false,
        fans: false,
        extra: false,
        updates: false,
        relaunched: false,
    };
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--mock" => a.transport = TransportKind::Mock,
            "--tray" | "--minimized" => a.tray = true,
            "--panel" => a.panel = true,
            "--fans" => a.fans = true,
            "--extra" => a.extra = true,
            "--updates" => a.updates = true,
            "--relaunched" => a.relaunched = true,
            _ => {}
        }
    }
    a
}

/// One instance of a window per session. After a relaunch, waits up to
/// 3 s for the previous process to exit.
fn single_instance(name: windows::core::PCWSTR, relaunched: bool) -> bool {
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;
    let attempts = if relaunched { 30 } else { 1 };
    for _ in 0..attempts {
        unsafe {
            let h = CreateMutexW(None, true, name);
            if GetLastError() != ERROR_ALREADY_EXISTS {
                // Keep the handle for the process lifetime.
                std::mem::forget(h);
                return true;
            }
            if let Ok(h) = h {
                let _ = CloseHandle(h);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    false
}

/// Log file for this process: `tray.log` for the agent, `app.log` for the
/// windows, in `%APPDATA%\OpenSense\logs`. A file over 512 KB is moved to
/// `*.old.log` at start, so at most two files per role are kept.
fn log_file() -> Option<std::fs::File> {
    let dir = opensense_sys::config::AppConfig::path()?.parent()?.parent()?.join("logs");
    std::fs::create_dir_all(&dir).ok()?;
    let role = if std::env::args().any(|a| a == "--tray" || a == "--minimized") { "tray" } else { "app" };
    let path = dir.join(format!("{role}.log"));
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 512 * 1024) {
        let _ = std::fs::rename(&path, dir.join(format!("{role}.old.log")));
    }
    std::fs::OpenOptions::new().create(true).append(true).open(path).ok()
}

/// Logs to the file (and stderr in debug builds); panics are logged too,
/// so a crash leaves a trace instead of disappearing silently.
fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_env("OPENSENSE_LOG").unwrap_or_else(|_| "info".into());
    match log_file() {
        Some(f) => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(f))
            .init(),
        None => tracing_subscriber::fmt().with_env_filter(filter).init(),
    }
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("panic: {info}");
        default(info);
    }));
}

fn main() -> eframe::Result<()> {
    init_logging();
    // Used by the installer before it replaces or removes the files: ask
    // the tray agent to quit (it restores the fans) and wait up to 5 s.
    if std::env::args().any(|a| a == "--quit") {
        tray::request_quit();
        for _ in 0..50 {
            if !tray::is_running() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        return Ok(());
    }
    // Used by the installer: the language picked in the setup, then exit.
    let argv: Vec<String> = std::env::args().collect();
    if let Some(i) = argv.iter().position(|a| a == "--set-language") {
        if let Some(code @ ("en" | "tr")) = argv.get(i + 1).map(String::as_str) {
            let mut cfg = opensense_sys::config::AppConfig::load();
            cfg.language = code.into();
            if let Err(e) = cfg.save() {
                tracing::warn!("cannot save settings: {e}");
            }
        }
        return Ok(());
    }
    // Used by the installer: start with Windows on / off, then exit.
    if let Some(on) = std::env::args().find_map(|a| match a.as_str() {
        "--autostart-on" => Some(true),
        "--autostart-off" => Some(false),
        _ => None,
    }) {
        let r = if on { opensense_sys::autostart::enable() } else { opensense_sys::autostart::disable() };
        if let Err(e) = r {
            tracing::warn!("autostart: {e}");
        }
        return Ok(());
    }
    let args = parse_args();
    updates::cleanup();
    if args.tray {
        tray::run(args.transport);
        return Ok(());
    }
    if args.panel {
        return run_panel(args.transport, args.relaunched);
    }
    if args.fans {
        return run_fans(args.transport);
    }
    if args.extra {
        return run_extra(args.transport);
    }
    if args.updates {
        return run_child(
            windows::core::w!("Local\\OpenSense.Updates"),
            updates_window::title(),
            updates_window::WIDTH,
            320.0,
            |cc| Box::new(updates_window::UpdatesApp::new(cc)),
        );
    }
    // Direct launch: like G-Helper, the main window plus the tray agent.
    let mock = args.transport == TransportKind::Mock;
    if !mock && !args.relaunched && !opensense_sys::privilege::is_elevated() {
        match opensense_sys::privilege::relaunch_elevated("--relaunched") {
            Ok(()) => return Ok(()),
            Err(e) => tracing::warn!("cannot restart elevated: {e}"),
        }
    }
    if let Some((hwnd, _)) = tray::find_marked_window(panel::MARK_PROP) {
        tray::raise_window(hwnd);
        return Ok(());
    }
    if opensense_sys::config::AppConfig::load().close_to_tray {
        tray::ensure_running(mock);
    }
    run_panel(args.transport, args.relaunched)
}

/// Starts the quick panel, hidden until it has sized and placed itself.
fn run_panel(transport: TransportKind, relaunched: bool) -> eframe::Result<()> {
    if !single_instance(windows::core::w!("Local\\OpenSense.Panel"), relaunched) {
        return Ok(());
    }
    let cfg = opensense_sys::config::AppConfig::load();
    let oem = opensense_sys::oem::OemInfo::detect();
    let title = panel::window_title(&oem);
    let size = egui::vec2(panel::WIDTH, panel::INITIAL_HEIGHT);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(title.clone())
        .with_inner_size(size)
        .with_resizable(false)
        .with_maximize_button(false)
        .with_minimize_button(false)
        .with_visible(false)
        .with_icon(egui::IconData { rgba: icon::rgba(64), width: 64, height: 64 });
    if cfg.topmost {
        viewport = viewport.with_always_on_top();
    }
    if let Some(p) = panel::position(size + egui::vec2(2.0, 32.0), panel::system_scale()) {
        viewport = viewport.with_position(p);
    }
    let options = eframe::NativeOptions {
        viewport,
        depth_buffer: 0,
        stencil_buffer: 0,
        multisampling: 0,
        vsync: true,
        persist_window: false,
        ..Default::default()
    };
    let app_title = title.clone();
    eframe::run_native(
        &app_title,
        options,
        Box::new(move |cc| Ok(Box::new(panel::QuickPanel::new(cc, transport, title)))),
    )
}

/// Outer rect of the main window in logical points.
fn main_window_rect(scale: f32) -> Option<egui::Rect> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;
    let (hwnd, _) = tray::find_marked_window(panel::MARK_PROP)?;
    let mut r = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut r) }.ok()?;
    Some(egui::Rect::from_min_max(
        egui::pos2(r.left as f32 / scale, r.top as f32 / scale),
        egui::pos2(r.right as f32 / scale, r.bottom as f32 / scale),
    ))
}

/// A child window (Fans and Power, Extra) in its own process, placed to the
/// left of the main window with its bottom edge level with the main
/// window's (to the right when there is no room on the left).
fn run_child(
    mutex: windows::core::PCWSTR,
    title: &str,
    width: f32,
    min_height: f32,
    make: impl FnOnce(&eframe::CreationContext<'_>) -> Box<dyn eframe::App> + 'static,
) -> eframe::Result<()> {
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;
    let h = unsafe { CreateMutexW(None, true, mutex) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return Ok(());
    }
    std::mem::forget(h);
    let main = main_window_rect(panel::system_scale());
    // Title bar and borders of a decorated window, in points.
    let chrome = egui::vec2(2.0, 32.0);
    let height = main.map_or(min_height, |m| (m.height() - chrome.y).max(min_height));
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(title)
        .with_inner_size([width, height])
        .with_resizable(false)
        .with_maximize_button(false)
        .with_minimize_button(false)
        .with_taskbar(false)
        .with_icon(egui::IconData { rgba: icon::rgba(64), width: 64, height: 64 });
    if opensense_sys::config::AppConfig::load().topmost {
        viewport = viewport.with_always_on_top();
    }
    if let Some(m) = main {
        let w = width + chrome.x;
        let x = if m.left() - w - 5.0 >= 0.0 { m.left() - w - 5.0 } else { m.right() + 5.0 };
        let y = (m.bottom() - height - chrome.y).max(0.0);
        viewport = viewport.with_position([x, y]);
    }
    let options = eframe::NativeOptions {
        viewport,
        depth_buffer: 0,
        stencil_buffer: 0,
        multisampling: 0,
        vsync: true,
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(title, options, Box::new(move |cc| Ok(make(cc))))
}

fn run_fans(transport: TransportKind) -> eframe::Result<()> {
    run_child(
        windows::core::w!("Local\\OpenSense.Fans"),
        i18n::tr(i18n::ui_lang(), "Fans and Power"),
        fans_window::WIDTH,
        fans_window::MIN_HEIGHT,
        move |cc| Box::new(fans_window::FansApp::new(cc, transport)),
    )
}

fn run_extra(transport: TransportKind) -> eframe::Result<()> {
    run_child(
        windows::core::w!("Local\\OpenSense.Extra"),
        extra_window::title(),
        extra_window::WIDTH,
        500.0,
        move |cc| Box::new(extra_window::ExtraApp::new(cc, transport)),
    )
}
