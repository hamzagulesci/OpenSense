//! Main window (`opensense --panel`): Mode, GPU
//! Mode, Laptop Keyboard and Battery sections stacked in one fixed-width
//! column, then the startup row, version link and footer buttons.
//!
//! It is its own process, started by the tray agent, so the agent stays
//! free of any renderer. Closing the window ends the process. A tray click
//! brings it to the front, or closes it when it was already in front.

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use opensense_hw::TransportKind;
use opensense_proto::lighting::{Effect, Rgb};
use opensense_proto::mode::{GpuMode, OperationMode};
use opensense_proto::sensor::celsius_to_fahrenheit;
use opensense_sys::config::{AppConfig, LightingKind, TempUnit};
use opensense_sys::oem::OemInfo;

use crate::apply::WriteState;
use crate::gh::icons::Icon;
use crate::gh::theme::{self, pal};
use crate::gh::ui::{self, TileStyle};
use crate::i18n::{tr, Lang};
use crate::worker::{self, Command, WorkerHandle};

/// Window property that marks the main window, holding the tick count of
/// the moment it last lost focus (read by the tray agent).
pub const MARK_PROP: &str = "OpenSense.Main";
/// Logical width of the window's client area.
pub const WIDTH: f32 = 425.0;
/// First guess for the height; corrected before the window is shown.
pub const INITIAL_HEIGHT: f32 = 520.0;
/// Distance from the screen edge, in physical pixels.
const EDGE_GAP: f32 = 10.0;
/// Window padding and section side padding.
const PAD: f32 = 5.5;
const SIDE: f32 = 10.0;

/// Highest fan speed assumed before a higher one is seen (percent display).
const FAN_MAX_RPM: f32 = 7900.0;

/// Keyboard effects in the drop-down, in order.
const EFFECTS: [Effect; 6] =
    [Effect::Static, Effect::Breathing, Effect::Neon, Effect::Wave, Effect::Shifting, Effect::Zoom];

/// Preset colours of the colour picker: hues, then greys and pastels.
const PRESETS: [u32; 28] = [
    0xFF0000, 0xFF4000, 0xFF8000, 0xFFBF00, 0xFFFF00, 0x80FF00, 0x00FF00, 0x00FF80, 0x00FFFF, 0x0080FF, 0x0000FF,
    0x8000FF, 0xFF00FF, 0xFF0080, 0xFFFFFF, 0xE0E0E0, 0xC0C0C0, 0xA0A0A0, 0x808080, 0x606060, 0x404040, 0x000000,
    0xFFC0C0, 0xFFE0C0, 0xFFFFC0, 0xC0FFC0, 0xC0FFFF, 0xC0C0FF,
];

// ---- placement ------------------------------------------------------------------

/// Primary work area in physical pixels and whether the taskbar is at the
/// top. An auto-hidden taskbar does not shrink the work area but still
/// covers the window when it slides in, so its thickness is kept clear.
fn work_area(scale: f32) -> Option<(Rect, bool)> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::Shell::{SHAppBarMessage, ABE_BOTTOM, ABE_TOP, ABM_GETTASKBARPOS, APPBARDATA};
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };
    let mut r = RECT::default();
    unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(std::ptr::addr_of_mut!(r).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .ok()?;
    }
    let mut bar = APPBARDATA { cbSize: std::mem::size_of::<APPBARDATA>() as u32, ..Default::default() };
    let mut top = r.top > 0;
    if unsafe { SHAppBarMessage(ABM_GETTASKBARPOS, &mut bar) } != 0 {
        let min = (48.0 * scale) as i32;
        let tb = bar.rc;
        if bar.uEdge == ABE_TOP {
            r.top = r.top.max(tb.top + (tb.bottom - tb.top).max(min));
            top = true;
        } else if bar.uEdge == ABE_BOTTOM {
            r.bottom = r.bottom.min(tb.bottom - (tb.bottom - tb.top).max(min));
            top = false;
        }
    }
    Some((Rect::from_min_max(Pos2::new(r.left as f32, r.top as f32), Pos2::new(r.right as f32, r.bottom as f32)), top))
}

pub fn system_scale() -> f32 {
    let dpi = unsafe { windows::Win32::UI::HiDpi::GetDpiForSystem() };
    if dpi == 0 {
        1.0
    } else {
        dpi as f32 / 96.0
    }
}

/// Outer top-left corner (logical points) for a window whose outer size
/// is `size`: bottom-right of the work area (top-right with a top taskbar).
pub fn position(size: Vec2, scale: f32) -> Option<Pos2> {
    let (area, top) = work_area(scale)?;
    let gap = EDGE_GAP;
    let x = (area.right() - gap) / scale - size.x;
    let y = if top { (area.top() + gap) / scale } else { (area.bottom() - gap) / scale - size.y };
    Some(Pos2::new(x, y.max(area.top() / scale + 5.0)))
}

/// "nitro an515-58" -> "Nitro AN515-58".
pub fn model_title(model: &str) -> String {
    let model = model.rsplit_once('_').map_or(model, |(m, _)| m);
    model
        .split_whitespace()
        .enumerate()
        .map(|(i, w)| {
            if i == 0 {
                let mut c = w.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase()).unwrap_or_default()
            } else {
                w.to_uppercase()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Window title: em dash when elevated, hyphen otherwise.
pub fn window_title(oem: &OemInfo) -> String {
    let sep = if opensense_sys::privilege::is_elevated() { "—" } else { "-" };
    match &oem.model {
        Some(m) => format!("OpenSense {sep} {}", model_title(m)),
        None => "OpenSense".into(),
    }
}

pub fn set_prop(hwnd: windows::Win32::Foundation::HWND, name: &str, value: usize) {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::UI::WindowsAndMessaging::SetPropW;
    let _ = unsafe { SetPropW(hwnd, &HSTRING::from(name), HANDLE(value as *mut _)) };
}

fn post_close(prop: &str) -> bool {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
    match crate::tray::find_marked_window(prop) {
        Some((hwnd, _)) => {
            let _ = unsafe { PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)) };
            true
        }
        None => false,
    }
}

/// Child windows, by window property.
const CHILDREN: [&str; 4] = [
    crate::fans_window::MARK_PROP,
    crate::extra_window::MARK_PROP,
    crate::updates_window::MARK_PROP,
    crate::diag_window::MARK_PROP,
];

/// Opens a child window, or closes it when it is open. Only one child is
/// open at a time: opening one closes the others.
pub fn toggle_child(prop: &str, args: &[&str]) {
    // The Hardware Test window hides instead of closing (it keeps its
    // results); asking for it again brings it back.
    if let Some((hwnd, _)) = crate::tray::find_marked_window(prop) {
        use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;
        if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
            for other in CHILDREN.into_iter().filter(|p| *p != prop) {
                post_close(other);
            }
            crate::tray::raise_window(hwnd);
            return;
        }
    }
    if post_close(prop) {
        return;
    }
    for other in CHILDREN.into_iter().filter(|p| *p != prop) {
        post_close(other);
    }
    crate::tray::spawn_detached(args);
}

/// Opens the Fans and Power window, or closes it when it is open.
fn toggle_fans(mock: bool) {
    toggle_child(crate::fans_window::MARK_PROP, if mock { &["--fans", "--mock"] } else { &["--fans"] });
}

/// Opens the Updates window, or closes it when it is open.
fn toggle_updates() {
    toggle_child(crate::updates_window::MARK_PROP, &["--updates"]);
}

/// Closes the main window (and with it the child windows).
pub fn close_main_window() {
    post_close(MARK_PROP);
}

/// Opens the Extra Settings window, or closes it when it is open.
fn toggle_extra(mock: bool) {
    toggle_child(crate::extra_window::MARK_PROP, if mock { &["--extra", "--mock"] } else { &["--extra"] });
}

fn tick_count() -> usize {
    unsafe { windows::Win32::System::SystemInformation::GetTickCount() as usize }
}

/// Opens a URL in the default browser. Through the shell directly: `cmd /c
/// start` treats every `&` in a query string as a command separator and
/// would cut the URL there.
pub fn open_url(url: &str) {
    use windows::core::{w, HSTRING};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return;
    }
    let r = unsafe { ShellExecuteW(None, w!("open"), &HSTRING::from(url), None, None, SW_SHOWNORMAL) };
    if r.0 as usize <= 32 {
        tracing::warn!("cannot open {url}");
    }
}

/// While `busy`, refuses a request to close the window (title-bar X, or
/// another window asking) and remembers it in `pending`; closes once the
/// work is done. Keeps a hardware test, calibration or update from being
/// cut off half-way.
pub fn hold_close(ctx: &egui::Context, busy: bool, pending: &mut bool) {
    if busy {
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            *pending = true;
        }
    } else if *pending {
        *pending = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

/// `powercfg /batteryreport` into the temp folder, then opens it.
fn battery_report() {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::thread::spawn(|| {
        let out = std::env::temp_dir().join("opensense-battery-report.html");
        let ok = std::process::Command::new("powercfg.exe")
            .arg("/batteryreport")
            .arg("/output")
            .arg(&out)
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .is_ok_and(|s| s.success());
        if ok {
            open_url(&out.display().to_string());
        }
    });
}

// ---- state ----------------------------------------------------------------------

/// Inline colour picker under the keyboard row.
struct Picker {
    color: Color32,
    /// Lighting when the picker opened, restored by Cancel if a colour was
    /// applied meanwhile (all zones, not just the first one's colour).
    original: opensense_sys::config::LightingConfig,
    applied: bool,
}

pub struct QuickPanel {
    cfg: AppConfig,
    lang: Lang,
    oem: OemInfo,
    worker: WorkerHandle,
    mock: bool,
    title: String,
    dark: bool,
    theme_checked: Instant,
    frames: u32,
    started: Instant,
    shown: bool,
    marked: bool,
    focused: bool,
    /// Hides the window when another app takes the foreground.
    away: crate::tray::AwayWatch,
    /// Last check whether the tray agent runs.
    tray_checked: Option<(Instant, bool)>,
    height: f32,
    autostart: bool,
    fan_max_seen: f32,
    gpu_hint: &'static str,
    gpu_confirm: Option<GpuMode>,
    /// GPU mode written; restart once the firmware confirmed.
    restart_after_write: Option<Instant>,
    /// Requested GPU mode that applies after the next restart.
    gpu_pending: Option<GpuMode>,
    picker: Option<Picker>,
    hover_full: bool,
    error: Option<String>,
    fans_requested: bool,
    /// Background update check result (new version), once it finished.
    update_check: Option<std::sync::Arc<std::sync::Mutex<Option<String>>>>,
    modified: Option<std::time::SystemTime>,
    config_checked: Instant,
}

impl QuickPanel {
    pub fn new(cc: &eframe::CreationContext<'_>, transport: TransportKind, title: String) -> Self {
        let cfg = AppConfig::load();
        let dark = theme::is_dark(&cfg.ui_mode);
        theme::install_fonts(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx, dark);
        let oem = OemInfo::detect();
        let ctx = cc.egui_ctx.clone();
        let worker =
            worker::spawn(transport, &oem, Duration::from_millis(1000), 4, false, move || ctx.request_repaint());
        Self {
            lang: Lang::resolve(&cfg.language),
            cfg,
            oem,
            worker,
            mock: transport == TransportKind::Mock,
            title,
            dark,
            theme_checked: Instant::now(),
            frames: 0,
            started: Instant::now(),
            shown: false,
            marked: false,
            focused: false,
            away: crate::tray::AwayWatch::default(),
            tray_checked: None,
            height: INITIAL_HEIGHT,
            autostart: opensense_sys::autostart::is_enabled(),
            fan_max_seen: FAN_MAX_RPM,
            gpu_hint: "",
            gpu_confirm: None,
            restart_after_write: None,
            gpu_pending: None,
            picker: None,
            hover_full: false,
            error: None,
            fans_requested: false,
            update_check: None,
            modified: crate::fans_window::config_mtime(),
            config_checked: Instant::now(),
        }
    }

    fn t(&self, s: &'static str) -> &'static str {
        tr(self.lang, s)
    }

    fn send(&self, c: Command) {
        let _ = self.worker.tx.send(c);
    }

    fn save(&mut self) {
        if let Err(e) = self.cfg.save() {
            self.error = Some(format!("{}: {e}", self.t("Could not save settings")));
        }
        self.modified = crate::fans_window::config_mtime();
        crate::tray::notify_config_changed();
    }

    fn set_mode(&mut self, key: &str) {
        crate::modes::switch(&mut self.cfg, key, &self.worker);
        self.save();
    }

    fn temp(&self, c: Option<u16>) -> Option<String> {
        c.map(|c| match self.cfg.temp_unit {
            TempUnit::Celsius => format!("{c}°C"),
            TempUnit::Fahrenheit => format!("{:.0}°F", celsius_to_fahrenheit(f32::from(c))),
        })
    }

    fn fan(&self, rpm: Option<u16>) -> Option<String> {
        rpm.map(|r| {
            if self.cfg.fan_rpm {
                format!("{r}RPM")
            } else {
                format!("{:.0}%", (f32::from(r) / self.fan_max_seen * 100.0).min(100.0))
            }
        })
    }

    /// "CPU: 44°C  Fan: 2900RPM"
    fn sensor_text(&self, name: &str, temp: Option<u16>, rpm: Option<u16>) -> String {
        let mut s = name.to_owned();
        if let Some(t) = self.temp(temp) {
            s.push_str(&format!(": {t}"));
        }
        if let Some(f) = self.fan(rpm) {
            s.push_str(&format!("  {}: {f}", self.t("Fan")));
        }
        s
    }

    /// Follows the settings file (theme, language, choices made in other
    /// windows) and the Windows theme without a noticeable delay.
    fn housekeeping(&mut self, ctx: &egui::Context) {
        crate::tray::keep_tray_only();
        let mut theme_due = self.theme_checked.elapsed() > Duration::from_secs(1);
        if self.config_checked.elapsed() > Duration::from_millis(200) {
            self.config_checked = Instant::now();
            let m = crate::fans_window::config_mtime();
            if m != self.modified {
                self.modified = m;
                self.cfg = AppConfig::load();
                self.lang = Lang::resolve(&self.cfg.language);
                theme_due = true;
            }
        }
        if theme_due {
            self.theme_checked = Instant::now();
            let dark = theme::is_dark(&self.cfg.ui_mode);
            if dark != self.dark {
                self.dark = dark;
                theme::apply(ctx, dark);
                if let Some((hwnd, _)) = crate::tray::find_marked_window(MARK_PROP) {
                    theme::title_bar_of(hwnd, dark);
                }
            }
        }
        ctx.request_repaint_after(Duration::from_millis(200));
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(false));
        if self.focused && !focused {
            // Tell the tray agent when focus was lost: a tray click right
            // after that should close the window rather than raise it.
            if let Some(hwnd) = crate::tray::find_own_window(&self.title) {
                set_prop(hwnd, MARK_PROP, tick_count().max(1));
            }
        }
        self.focused = focused;
        self.auto_hide(ctx);
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.picker.is_none() && self.gpu_confirm.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let r = self.worker.shared.lock().readings;
        if let Some(r) = r.cpu_fan_rpm.max(r.gpu_fan_rpm) {
            self.fan_max_seen = self.fan_max_seen.max(f32::from(r));
        }
    }

    /// Hides the window once another app takes the focus. OpenSense's own
    /// windows (Fans and Power, Extra, Updates) keep it open, and so does
    /// "always on top". The tray agent closes it after a while hidden.
    fn auto_hide(&mut self, ctx: &egui::Context) {
        // The real foreground decides, not egui's focus flag: when a child
        // window closes, Windows can make this window active in the
        // background while another app keeps the foreground.
        let busy = self.picker.is_some() || self.gpu_confirm.is_some();
        // Hidden without a tray agent, the window could not be reopened.
        if !self.shown || self.cfg.topmost || busy || !self.away_allowed() {
            self.away.reset();
            return;
        }
        if self.away.gone(ctx) {
            // Hidden through Win32 rather than egui: egui would stop updating
            // a window it believes hidden, also after the tray shows it again.
            if let Some((hwnd, _)) = crate::tray::find_marked_window(MARK_PROP) {
                use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
                let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
            }
        }
    }

    /// Auto-hide needs the tray agent (its icon reopens the window); checked
    /// at most every 2 s.
    fn away_allowed(&mut self) -> bool {
        if self.tray_checked.is_none_or(|(t, _)| t.elapsed() > Duration::from_secs(2)) {
            self.tray_checked = Some((Instant::now(), crate::tray::is_running()));
        }
        self.tray_checked.is_some_and(|(_, running)| running)
    }

    /// Resizes to the content, keeps the window in its corner and shows it
    /// once the first firmware reading is in.
    fn fit(&mut self, ctx: &egui::Context, content_height: f32) {
        let h = content_height.ceil();
        let resized = (h - self.height).abs() > 0.5;
        if resized {
            self.height = h;
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(WIDTH, h)));
        }
        if resized || !self.shown {
            let (scale, outer, inner) = ctx.input(|i| {
                let v = i.viewport();
                (v.native_pixels_per_point.unwrap_or_else(system_scale), v.outer_rect, v.inner_rect)
            });
            // Title bar and borders, from the current frame.
            let chrome = match (outer, inner) {
                (Some(o), Some(i)) => Vec2::new(o.width() - i.width(), o.height() - i.height()),
                _ => Vec2::new(2.0, 32.0),
            };
            if let Some(p) = position(Vec2::new(WIDTH, h) + chrome, scale) {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(p));
            }
        }
        self.frames += 1;
        let data = !self.worker.shared.lock().history.is_empty() || self.started.elapsed() > Duration::from_secs(2);
        if !self.shown && data && !resized && self.frames >= 3 {
            self.shown = true;
            // Tray-only (no taskbar button) only while the tray agent runs;
            // without it the taskbar button is the way back to the window.
            if crate::tray::is_running() {
                if let Some(hwnd) = crate::tray::find_own_window(&self.title) {
                    crate::tray::tray_only(hwnd);
                }
            } else {
                tracing::warn!("tray agent not running: main window keeps its taskbar button");
            }
            theme::title_bar(&self.title, self.dark);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        if self.shown && !self.marked {
            if let Some(hwnd) = crate::tray::find_own_window(&self.title) {
                set_prop(hwnd, MARK_PROP, 1);
                self.marked = true;
            }
        }
        if !self.shown {
            ctx.request_repaint_after(Duration::from_millis(15));
        }
    }

    /// Checks GitHub for a new version at most every 12 hours.
    fn update_tick(&mut self) {
        if !self.shown || self.cfg.skip_updates {
            return;
        }
        match &self.update_check {
            None => {
                let due = self
                    .cfg
                    .update_checked
                    .is_none_or(|t| crate::updates::now_secs().saturating_sub(t) >= crate::updates::CHECK_EVERY);
                if !due {
                    return;
                }
                let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
                let s = slot.clone();
                std::thread::spawn(move || {
                    let v = crate::updates::latest().map(|r| r.version).unwrap_or_default();
                    *s.lock().unwrap() = Some(v);
                });
                self.update_check = Some(slot);
            }
            Some(slot) => {
                let Some(v) = slot.lock().unwrap().take() else { return };
                self.cfg.update_checked = Some(crate::updates::now_secs());
                if !v.is_empty() {
                    self.cfg.latest_version = Some(v);
                }
                self.save();
            }
        }
    }

    fn poll_restart(&mut self, ctx: &egui::Context) {
        let Some(since) = self.restart_after_write else { return };
        match crate::apply::gpu_write_state(&self.worker.shared, since) {
            WriteState::Pending => ctx.request_repaint_after(Duration::from_millis(100)),
            WriteState::Done => {
                self.restart_after_write = None;
                if let Err(e) = opensense_sys::power::restart_now() {
                    self.error = Some(e.to_string());
                }
            }
            WriteState::Failed(e) => {
                self.restart_after_write = None;
                let e = e.unwrap_or_else(|| self.t("The firmware did not answer.").into());
                self.error = Some(format!("{}: {e}", self.t("GPU Mode")));
            }
        }
    }
}

impl eframe::App for QuickPanel {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        raw_input.max_texture_side = Some(raw_input.max_texture_side.unwrap_or(2048).min(2048));
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        pal().form.to_normalized_gamma_f32()
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Child windows close with the main window.
        post_close(crate::fans_window::MARK_PROP);
        post_close(crate::extra_window::MARK_PROP);
        post_close(crate::updates_window::MARK_PROP);
        post_close(crate::diag_window::MARK_PROP);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.housekeeping(ctx);
        self.poll_restart(ctx);
        let frame = egui::Frame::none().fill(pal().form).inner_margin(PAD);
        let mut bottom = 0.0;
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            bottom = ui.vertical(|ui| self.content(ui)).response.rect.bottom();
        });
        self.fit(ctx, bottom + PAD);
        self.update_tick();
        // "--fans" / "cpu": also open Fans and Power once the window is up.
        if self.shown && self.marked && !self.fans_requested {
            self.fans_requested = true;
            if std::env::args().any(|a| a == "--fans" || a == "cpu") {
                toggle_fans(self.mock);
            }
        }
        // Sensor text refreshes with the worker (1 s); keep a slow tick for
        // the theme check when nothing else repaints.
        ctx.request_repaint_after(Duration::from_secs(2));
    }
}

// ---- sections -------------------------------------------------------------------

/// Runs `body` inside a full-width block with the section padding.
fn section(ui: &mut egui::Ui, top: f32, bottom: f32, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none().inner_margin(egui::Margin { left: SIDE, right: SIDE, top, bottom }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing = Vec2::new(0.0, 2.0);
        body(ui);
    });
}

impl QuickPanel {
    fn content(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        if let Some(e) = self.error.clone() {
            section(ui, 4.0, 0.0, |ui| {
                ui.colored_label(theme::TURBO, e);
            });
        }
        let connected = self.worker.shared.lock().connected;
        if !connected && !self.mock && self.started.elapsed() > Duration::from_secs(3) {
            section(ui, 4.0, 0.0, |ui| {
                ui.colored_label(
                    theme::TURBO,
                    self.t("Can't connect to the Acer firmware. Run OpenSense as administrator."),
                );
            });
        }
        self.mode_section(ui);
        self.gpu_section(ui);
        self.keyboard_section(ui);
        self.battery_section(ui);
        self.startup_row(ui);
        self.version_row(ui);
        self.footer(ui);
    }

    fn mode_section(&mut self, ui: &mut egui::Ui) {
        let (mode, full_power, r, caps) = {
            let s = self.worker.shared.lock();
            (s.mode, s.power.on_ac && s.battery_boost != Some(false), s.readings, s.caps)
        };
        let has_modes = caps.map_or(self.oem.has_operation_modes(), |c| c.operation_modes);
        let key = crate::modes::current_key(&self.cfg);
        let custom = crate::modes::is_custom(&key);
        let curve = crate::modes::profile(&self.cfg, &key).apply_curve;
        section(ui, 10.0, 10.0, |ui| {
            let name = match (custom, mode) {
                (true, _) => crate::modes::display_name(&self.cfg, self.lang, &key),
                (false, Some(m)) => tr(self.lang, m.label()).to_owned(),
                (false, None) => "--".into(),
            };
            let title = format!("{}: {name}{}", self.t("Mode"), if curve { "+" } else { "" });
            let right = self.sensor_text("CPU", r.cpu_temp, r.cpu_fan_rpm);
            if ui::section_title(ui, Icon::Gauge, &title, &right).is_some_and(|r| r.clicked()) {
                self.cfg.fan_rpm = !self.cfg.fan_rpm;
                self.save();
            }
            ui.add_space(4.0);
            let cells = ui::grid(ui, 4, 64.0);
            let mut picked = None;
            for (cell, (m, icon, accent)) in cells.iter().zip([
                (OperationMode::Quiet, Icon::Bicycle, theme::ECO),
                (OperationMode::Default, Icon::Car, theme::STANDARD),
                (OperationMode::Extreme, Icon::Rocket, theme::TURBO),
            ]) {
                let enabled = has_modes && (!m.needs_full_power() || full_power);
                let selected = !custom && mode == Some(m);
                let style = TileStyle { icon: Some(icon), secondary: false, enabled, accent, selected };
                let resp = ui::tile(ui, *cell, m.label(), tr(self.lang, m.label()), &style);
                let resp = if m.needs_full_power() && !full_power {
                    resp.on_hover_text(self.t("Extreme needs AC power with the full-power adapter."))
                } else {
                    resp
                };
                if resp.clicked() {
                    picked = Some(m);
                }
            }
            let fan_accent = match crate::modes::base_of(&self.cfg, &key) {
                OperationMode::Quiet => theme::ECO,
                OperationMode::Extreme => theme::TURBO,
                _ => theme::STANDARD,
            };
            let style = TileStyle {
                icon: Some(Icon::Fan),
                secondary: true,
                enabled: true,
                accent: if custom { fan_accent } else { theme::CUSTOM },
                selected: custom,
            };
            if ui::tile(ui, cells[3], "fans", self.t("Fans + Power"), &style).clicked() {
                toggle_fans(self.mock);
            }
            if let Some(m) = picked.filter(|&m| custom || mode != Some(m)) {
                self.set_mode(&m.label().to_ascii_lowercase());
            }
        });
    }

    fn gpu_section(&mut self, ui: &mut egui::Ui) {
        let (mux, gpu_mode, r) = {
            let s = self.worker.shared.lock();
            (s.gpu_mux_supported, s.gpu_mode, s.readings)
        };
        let right = self.sensor_text("GPU", r.gpu_temp, r.gpu_fan_rpm);
        if !mux {
            // No MUX: keep the GPU readings visible under the mode row.
            section(ui, 0.0, 6.0, |ui| {
                if ui::section_title(ui, Icon::VideoCard, self.t("GPU"), &right).is_some_and(|r| r.clicked()) {
                    self.cfg.fan_rpm = !self.cfg.fan_rpm;
                    self.save();
                }
            });
            return;
        }
        let current = gpu_mode.unwrap_or(GpuMode::Hybrid);
        let state = |m: GpuMode| if m == GpuMode::Discrete { "dGPU exclusive" } else { "iGPU + dGPU" };
        section(ui, 10.0, 0.0, |ui| {
            let mut title = format!("{}: {}", self.t("GPU Mode"), self.t(state(current)));
            if let Some(p) = self.gpu_pending.filter(|&p| p != current) {
                title = format!("{title} → {} ({})", self.t(state(p)), self.t("restart"));
            }
            if ui::section_title(ui, Icon::VideoCard, &title, &right).is_some_and(|r| r.clicked()) {
                self.cfg.fan_rpm = !self.cfg.fan_rpm;
                self.save();
            }
            ui.add_space(4.0);
            let cells = ui::grid(ui, 4, 64.0);
            let mut hint = "";
            for (cell, (m, icon, accent, label, tip)) in cells.iter().zip([
                (GpuMode::Hybrid, Icon::Flower, theme::STANDARD, "Standard", "Enables dGPU for standard use"),
                (
                    GpuMode::Discrete,
                    Icon::Gamepad,
                    theme::TURBO,
                    "Ultimate",
                    "Routes laptop screen to dGPU, maximizing FPS",
                ),
            ]) {
                let selected = self.gpu_confirm.or(self.gpu_pending).unwrap_or(current) == m;
                let style = TileStyle { icon: Some(icon), secondary: false, enabled: true, accent, selected };
                let resp = ui::tile(ui, *cell, label, self.t(label), &style);
                if resp.hovered() {
                    hint = tip;
                }
                if resp.clicked() && self.gpu_pending.unwrap_or(current) != m {
                    self.gpu_confirm = Some(m);
                }
            }
            self.gpu_hint = hint;
            match self.gpu_confirm {
                Some(m) => self.gpu_question(ui, m, current),
                None => ui::hint_line(ui, self.t(self.gpu_hint)),
            }
        });
    }

    /// "Restart now?" under the GPU tiles.
    fn gpu_question(&mut self, ui: &mut egui::Ui, m: GpuMode, current: GpuMode) {
        let p = pal();
        let text = if m == GpuMode::Discrete {
            self.t("Ultimate Mode requires restart")
        } else {
            self.t("Switching off Ultimate Mode requires restart")
        };
        ui.add_space(4.0);
        let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), egui::Sense::hover());
        ui.painter().text(
            Pos2::new(row.left() + 4.0, row.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            theme::bold(theme::BODY),
            p.fore,
        );
        let cells = ui::grid(ui, 3, 28.0);
        let mut choice = None;
        for (i, (cell, label)) in
            cells.iter().zip([self.t("Restart now"), self.t("Later"), self.t("Cancel")]).enumerate()
        {
            if ui::row_button(ui, *cell, label, label, None, i != 0).clicked() {
                choice = Some(i);
            }
        }
        match choice {
            Some(0) => {
                crate::apply::begin_gpu_write(&self.worker, m);
                self.restart_after_write = Some(Instant::now());
                self.gpu_pending = Some(m);
                self.gpu_confirm = None;
            }
            Some(1) => {
                self.send(Command::GpuMode(m));
                self.gpu_pending = (m != current).then_some(m);
                self.gpu_confirm = None;
            }
            Some(_) => self.gpu_confirm = None,
            None => {}
        }
        ui.add_space(4.0);
    }

    fn keyboard_section(&mut self, ui: &mut egui::Ui) {
        let rgb = self.worker.shared.lock().caps.map_or(self.oem.rgb_keyboard, |c| c.rgb_keyboard);
        if !rgb {
            return;
        }
        let lang = self.lang;
        section(ui, 10.0, 0.0, |ui| {
            ui::section_title(ui, Icon::Keyboard, self.t("Laptop Keyboard"), "");
            // "Overlay" pill at the right end of the title row.
            let title = ui.min_rect();
            let pill =
                Rect::from_min_size(Pos2::new(title.right() - 64.0, title.bottom() - 20.0), Vec2::new(62.0, 18.0));
            let on = self.cfg.overlay;
            if ui::pill(ui, pill, "overlay", self.t("Overlay"), on, theme::ECO)
                .on_hover_text(self.t("In-game overlay (Ctrl + Shift + Alt + O)"))
                .clicked()
            {
                self.cfg.overlay = !on;
                self.save();
            }
            ui.add_space(4.0);
            let cells = ui::grid(ui, 3, 26.0);
            let l = &self.cfg.lighting;
            let current = match l.kind {
                LightingKind::Static => Effect::Static,
                LightingKind::Dynamic => crate::apply::effect_from_name(&l.effect),
            };
            // Effect drop-down.
            let mut picked = current;
            let combo = cells[0].shrink2(Vec2::new(1.5, 1.0));
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(combo), |ui| {
                egui::ComboBox::from_id_salt("kb-effect")
                    .width(combo.width() - 8.0)
                    .selected_text(tr(lang, current.label()))
                    .show_ui(ui, |ui| {
                        for e in EFFECTS {
                            ui.selectable_value(&mut picked, e, tr(lang, e.label()));
                        }
                    });
            });
            // Colour button.
            let swatches: Vec<Color32> = match current {
                Effect::Static => {
                    let zones: Vec<Color32> = l.zones.iter().map(|z| hex_color(z)).collect();
                    if zones.iter().all(|&z| z == zones[0]) {
                        vec![zones[0]]
                    } else {
                        zones
                    }
                }
                e if e.uses_color() && e != Effect::Wave => vec![hex_color(&l.effect_color)],
                _ => vec![],
            };
            let can_color = !swatches.is_empty();
            if can_color {
                if ui::color_button(ui, cells[1], "kb", self.t("Color"), &swatches).clicked() {
                    self.picker = match self.picker {
                        Some(_) => None,
                        None => {
                            Some(Picker { color: swatches[0], original: self.cfg.lighting.clone(), applied: false })
                        }
                    };
                }
            } else {
                self.picker = None;
            }
            if ui::row_button(ui, cells[2], "extra", self.t("Extra"), Some(Icon::Gear), true).clicked() {
                toggle_extra(self.mock);
            }
            if picked != current {
                let l = &mut self.cfg.lighting;
                if picked == Effect::Static {
                    l.kind = LightingKind::Static;
                } else {
                    l.kind = LightingKind::Dynamic;
                    l.effect = picked.label().to_ascii_lowercase();
                }
                self.apply_lighting();
            }
            if self.picker.is_some() {
                self.color_picker(ui);
            }
            ui.add_space(8.0);
        });
    }

    fn apply_lighting(&mut self) {
        if self.cfg.lighting.brightness_level <= 1 {
            self.cfg.lighting.brightness_level = 5;
        }
        self.send(crate::apply::lighting(&self.cfg.lighting));
        self.save();
    }

    fn set_keyboard_color(&mut self, c: Color32) {
        let hex = Rgb::new(c.r(), c.g(), c.b()).to_hex();
        let l = &mut self.cfg.lighting;
        if l.kind == LightingKind::Static {
            l.zones = [hex.clone(), hex.clone(), hex.clone(), hex];
            l.zone_enabled = [true; 4];
        } else {
            l.effect_color = hex;
        }
        self.apply_lighting();
    }

    /// Preset swatches, a hue/saturation picker and OK / Cancel.
    fn color_picker(&mut self, ui: &mut egui::Ui) {
        let p = pal();
        let Some(mut picker) = self.picker.take() else { return };
        let mut close = false;
        ui.add_space(6.0);
        egui::Frame::none()
            .fill(p.button2)
            .stroke(egui::Stroke::new(1.0_f32, p.border))
            .rounding(ui::ROW_RADIUS)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let cols = 14;
                let (area, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 2.0 * 24.0), egui::Sense::hover());
                let step = area.width() / cols as f32;
                for (i, rgb) in PRESETS.into_iter().enumerate() {
                    let c = Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
                    let center = Pos2::new(
                        area.left() + step * ((i % cols) as f32 + 0.5),
                        area.top() + 12.0 + 24.0 * (i / cols) as f32,
                    );
                    let r = Rect::from_center_size(center, Vec2::splat(18.0));
                    let resp = ui.interact(r, ui.id().with(("preset", i)), egui::Sense::click());
                    ui.painter().rect_filled(r, 2.0, c);
                    let edge = if picker.color == c {
                        egui::Stroke::new(2.0_f32, theme::STANDARD)
                    } else {
                        egui::Stroke::new(1.0_f32, p.border)
                    };
                    ui.painter().rect_stroke(r, 2.0, edge);
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        picker.color = c;
                        picker.applied = true;
                        self.set_keyboard_color(c);
                    }
                }
                ui.add_space(6.0);
                ui.spacing_mut().slider_width = ui.available_width() - 40.0;
                ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                egui::widgets::color_picker::color_picker_color32(
                    ui,
                    &mut picker.color,
                    egui::widgets::color_picker::Alpha::Opaque,
                );
                ui.add_space(4.0);
                let cells = ui::grid(ui, 3, 26.0);
                ui.painter().rect_filled(cells[0].shrink(4.0), 2.0, picker.color);
                if ui::row_button(ui, cells[1], "picker-ok", self.t("OK"), None, false).clicked() {
                    let c = picker.color;
                    self.set_keyboard_color(c);
                    close = true;
                }
                if ui::row_button(ui, cells[2], "picker-cancel", self.t("Cancel"), None, true).clicked() {
                    if picker.applied {
                        self.cfg.lighting = picker.original.clone();
                        self.apply_lighting();
                    }
                    close = true;
                }
            });
        if !close {
            self.picker = Some(picker);
        }
    }

    fn battery_section(&mut self, ui: &mut egui::Ui) {
        let (limit, info, power) = {
            let s = self.worker.shared.lock();
            (s.battery.filter(|b| b.limit_supported), s.battery_info, s.power)
        };
        let rate = info.and_then(|i| i.rate_watts()).filter(|w| w.abs() >= 0.1);
        let right = match rate {
            Some(w) if w > 0.0 => format!("{}: {w:.1}W", self.t("Charging")),
            Some(w) => format!("{}: {:.1}W", self.t("Discharging"), -w),
            None => String::new(),
        };
        let Some(b) = limit else {
            // No charge limit: still show the battery state.
            section(ui, 7.5, 0.0, |ui| {
                ui::section_title(ui, Icon::Battery, self.t("Battery"), &right);
            });
            return;
        };
        let saved = self.cfg.charge_target().unwrap_or(if b.limit_active { 80 } else { 100 });
        let id = egui::Id::new("charge-drag");
        let dragging: Option<f32> = ui.ctx().data(|d| d.get_temp(id));
        let shown = dragging.map_or(saved, |v| v as u8);
        section(ui, 7.5, 0.0, |ui| {
            let title = format!("{}: {shown}%", self.t("Battery Charge Limit"));
            if ui::section_title(ui, Icon::Battery, &title, &right).is_some_and(|r| r.clicked()) {
                self.cfg.charge_watt = !self.cfg.charge_watt;
                self.save();
            }
            ui.add_space(4.0);
            let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), egui::Sense::hover());
            let pill = Rect::from_min_size(Pos2::new(row.right() - 36.5, row.center().y - 9.0), Vec2::new(36.5, 18.0));
            let track =
                Rect::from_min_max(row.min, Pos2::new(pill.left() - 12.0, row.max.y)).shrink2(Vec2::new(0.0, 1.0));
            let supported = [80.0, 85.0, 90.0, 95.0, 100.0];
            let out = ui::slider(ui, track, "charge", f32::from(saved), 80.0, 100.0, 5.0, &supported, theme::STANDARD);
            ui.ctx().data_mut(|d| {
                if out.value != f32::from(saved) && out.commit.is_none() {
                    d.insert_temp(id, out.value);
                } else {
                    d.remove::<f32>(id);
                }
            });
            if let Some(v) = out.commit {
                let t = v as u8;
                self.cfg.set_charge_target(t);
                self.cfg.charge_full = false;
                if let Some(on) = crate::apply::charge_cap_for(t, power.battery_percent, b.limit_active) {
                    self.send(Command::ChargeLimit(on));
                }
                self.save();
            }
            let full = self.cfg.charge_full;
            let resp = ui::pill(ui, pill, "full", "100%", full, theme::STANDARD);
            self.hover_full = resp.hovered();
            if resp.clicked() {
                self.cfg.charge_full = !full;
                if self.cfg.charge_full {
                    self.send(Command::ChargeLimit(false));
                } else if let Some(on) = crate::apply::charge_cap_for(saved, power.battery_percent, b.limit_active) {
                    self.send(Command::ChargeLimit(on));
                }
                self.save();
            }
        });
    }

    fn startup_row(&mut self, ui: &mut egui::Ui) {
        let (info, power) = {
            let s = self.worker.shared.lock();
            (s.battery_info, s.power)
        };
        section(ui, 0.0, 0.0, |ui| {
            let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 25.0), egui::Sense::hover());
            let mut auto = self.autostart;
            let cb = Rect::from_min_max(Pos2::new(row.left() + 5.0, row.top()), row.max);
            if ui::checkbox(ui, cb, "startup", self.t("Run on Startup"), &mut auto).changed() {
                let res = if auto { opensense_sys::autostart::enable() } else { opensense_sys::autostart::disable() };
                match res {
                    Ok(()) => self.autostart = auto,
                    Err(e) => self.error = Some(format!("{}: {e}", self.t("Run on Startup"))),
                }
            }
            let text = if self.hover_full {
                self.t("One time charge to 100%").to_owned()
            } else {
                let charge = if self.cfg.charge_watt {
                    info.and_then(|i| i.remaining_mwh)
                        .map(|m| format!("{}: {:.1}Wh", self.t("Charge"), m as f32 / 1000.0))
                        .unwrap_or_default()
                } else {
                    info.and_then(|i| i.charge_percent())
                        .map(|c| format!("{}: {c:.1}%", self.t("Charge")))
                        .or(power.battery_percent.map(|b| format!("{}: {b}%", self.t("Charge"))))
                        .unwrap_or_default()
                };
                // Charge and battery health side by side.
                match info.and_then(|i| i.health_percent()) {
                    Some(h) if !charge.is_empty() => format!("{charge}   {}: {h:.0}%", self.t("Health")),
                    _ => charge,
                }
            };
            let r = ui.painter().text(
                Pos2::new(row.right() - 2.0, row.center().y),
                egui::Align2::RIGHT_CENTER,
                &text,
                theme::body(),
                theme::MUTED,
            );
            let resp = ui
                .interact(r, ui.id().with("charge-text"), egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.on_hover_text(self.t("Open the Windows battery report")).clicked() {
                battery_report();
            }
        });
    }

    fn version_row(&mut self, ui: &mut egui::Ui) {
        section(ui, 2.5, 2.5, |ui| {
            let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), egui::Sense::hover());
            let current = env!("CARGO_PKG_VERSION");
            let newer = self.cfg.latest_version.clone().filter(|v| crate::updates::is_newer(v, current));
            let (text, color) = match &newer {
                Some(v) => (format!("{}: {current} → {v}", self.t("Download")), theme::TURBO),
                None => (
                    format!("{}: {current}", self.t("Version")),
                    Color32::from_rgba_unmultiplied(0x80, 0x80, 0x80, 160),
                ),
            };
            if ui::link(ui, Pos2::new(row.left(), row.center().y), "version", &text, color).clicked() {
                if newer.is_some() {
                    toggle_updates();
                } else {
                    open_url(concat!(env!("CARGO_PKG_REPOSITORY"), "/releases"));
                }
            }
        });
    }

    fn footer(&mut self, ui: &mut egui::Ui) {
        section(ui, 5.0, 10.0, |ui| {
            let cells = ui::grid(ui, 3, 28.0);
            if ui::row_button(ui, cells[0], "github", "GitHub", Some(Icon::Star), true).clicked() {
                open_url(env!("CARGO_PKG_REPOSITORY"));
            }
            let update = self
                .cfg
                .latest_version
                .as_deref()
                .is_some_and(|v| crate::updates::is_newer(v, env!("CARGO_PKG_VERSION")));
            if ui::row_button(ui, cells[1], "updates", self.t("Updates"), Some(Icon::Download), true).clicked() {
                toggle_updates();
            }
            if update {
                // Red badge: an update is waiting.
                let c = Pos2::new(cells[1].right() - cells[1].height() / 2.0, cells[1].center().y);
                ui.painter().circle_filled(c, 7.0, theme::TURBO);
                ui.painter().text(c, egui::Align2::CENTER_CENTER, "1", theme::bold(10.0), Color32::WHITE);
            }
            if ui::row_button(ui, cells[2], "quit", self.t("Quit"), Some(Icon::Quit), true).clicked() {
                crate::tray::request_quit();
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }
}

fn hex_color(hex: &str) -> Color32 {
    let c = Rgb::from_hex(hex).unwrap_or_default();
    Color32::from_rgb(c.r, c.g, c.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_title_format() {
        assert_eq!(model_title("nitro an515-58"), "Nitro AN515-58");
        assert_eq!(model_title("Predator PH315-55_1234"), "Predator PH315-55");
    }
}
