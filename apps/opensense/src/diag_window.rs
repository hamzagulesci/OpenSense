//! "Hardware Test" window (`opensense --diagnostics`): detects the device,
//! runs the read-only compatibility tests automatically, offers the tests
//! that change hardware (one by one or all at once) after a confirmation,
//! and builds a report the user can read before saving, copying or
//! sharing it. Like the other windows it lives in the tray only; when the
//! user switches to another app it hides rather than closes, so the results
//! stay until it is reopened (the tray closes it after 5 minutes hidden).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Vec2};
use opensense_hw::diag::{self, Level, Outcome, TestResult};
use opensense_hw::{NitroDevice, TransportKind};
use opensense_sys::config::AppConfig;

use crate::diag_report::{self, Device, Os, Report};
use crate::gh::icons::Icon;
use crate::gh::theme::{self, pal};
use crate::gh::ui;
use crate::i18n::{tr, Lang};

pub const MARK_PROP: &str = "OpenSense.Diag";
pub const WIDTH: f32 = 580.0;

pub fn title() -> &'static str {
    tr(crate::i18n::ui_lang(), "Hardware Test")
}

/// Test results by id; `None` while a test runs.
type Results = Arc<Mutex<Vec<TestResult>>>;

pub struct DiagApp {
    lang: Lang,
    dark: bool,
    marked: bool,
    kind: TransportKind,
    results: Results,
    /// Test running in the background, if any.
    running: Arc<Mutex<Option<&'static str>>>,
    /// Test waiting for the user's yes ("all" = every confirm-level test).
    confirm: Option<&'static str>,
    away: crate::tray::AwayWatch,
    close_pending: bool,
    device: Arc<Mutex<Option<(Os, Device)>>>,
    show_report: bool,
    message: String,
}

fn open(kind: TransportKind) -> Result<NitroDevice, opensense_hw::HwError> {
    let oem = opensense_sys::oem::OemInfo::detect();
    opensense_hw::open_transport(kind).map(|t| NitroDevice::new(t, crate::worker::device_profile(&oem)))
}

impl DiagApp {
    pub fn new(cc: &eframe::CreationContext<'_>, kind: TransportKind) -> Self {
        let cfg = AppConfig::load();
        let dark = theme::is_dark(&cfg.ui_mode);
        theme::install_fonts(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx, dark);
        let app = Self {
            lang: Lang::resolve(&cfg.language),
            dark,
            marked: false,
            kind,
            results: Arc::new(Mutex::new(Vec::new())),
            running: Arc::new(Mutex::new(Some("connection"))),
            confirm: None,
            away: crate::tray::AwayWatch::default(),
            close_pending: false,
            device: Arc::new(Mutex::new(None)),
            show_report: false,
            message: String::new(),
        };
        // Device facts and the read-only tests, off the UI thread.
        let (results, running, device, ctx) =
            (app.results.clone(), app.running.clone(), app.device.clone(), cc.egui_ctx.clone());
        std::thread::spawn(move || {
            let cpu = opensense_sys::cpu::CpuMonitor::new().brand();
            let gpu = opensense_sys::gpu_name::discrete_gpu_name().unwrap_or_default();
            *device.lock().unwrap() = Some(diag_report::collect(cpu, gpu));
            let list = match open(kind) {
                Ok(dev) => diag::read_only(&dev),
                Err(e) => {
                    tracing::warn!("hardware test: no firmware connection: {e}");
                    diag::TESTS
                        .iter()
                        .map(|(id, level)| TestResult {
                            id,
                            level: *level,
                            outcome: if *id == "connection" { Outcome::Failed } else { Outcome::NotRun },
                            detail: if *id == "connection" { e.to_string() } else { String::new() },
                        })
                        .collect()
                }
            };
            for r in list.iter().filter(|r| r.outcome == Outcome::Failed) {
                tracing::warn!("hardware test {}: {}", r.id, r.detail);
            }
            *results.lock().unwrap() = list;
            *running.lock().unwrap() = None;
            ctx.request_repaint();
        });
        app
    }

    fn t(&self, s: &'static str) -> &'static str {
        tr(self.lang, s)
    }

    fn report(&self) -> Option<Report> {
        let (os, device) = self.device.lock().unwrap().clone()?;
        let results = self.results.lock().unwrap().clone();
        (!results.is_empty()).then(|| Report::new(os, device, &results, diag_report::today()))
    }

    /// Runs confirm-level tests one after another in the background; each
    /// restores what it changed. "all" first re-runs the read-only tests so
    /// the report is fresh.
    fn run(&mut self, ctx: &egui::Context, which: &'static str) {
        let ids: Vec<&'static str> = if which == "all" {
            diag::TESTS.iter().filter(|(_, l)| *l == Level::Confirm).map(|(id, _)| *id).collect()
        } else {
            vec![which]
        };
        let (results, running, ctx, kind) = (self.results.clone(), self.running.clone(), ctx.clone(), self.kind);
        *running.lock().unwrap() = Some(ids[0]);
        std::thread::spawn(move || {
            let cfg = AppConfig::load();
            let dev = open(kind);
            if which == "all" {
                if let Ok(d) = &dev {
                    let fresh = diag::read_only(d);
                    let mut list = results.lock().unwrap();
                    for r in fresh.into_iter().filter(|r| r.level == Level::ReadOnly) {
                        match list.iter_mut().find(|x| x.id == r.id) {
                            Some(slot) => *slot = r,
                            None => list.push(r),
                        }
                    }
                }
            }
            for id in ids {
                *running.lock().unwrap() = Some(id);
                ctx.request_repaint();
                let result = match &dev {
                    Ok(d) => {
                        let restore = |d: &NitroDevice| match crate::apply::lighting(&cfg.lighting) {
                            crate::worker::Command::StaticLighting(l) => d.apply_static_lighting(&l),
                            crate::worker::Command::DynamicLighting(l) => d.apply_dynamic_lighting(&l),
                            _ => Ok(()),
                        };
                        diag::run_confirmed(d, id, Some(&restore), &std::thread::sleep)
                    }
                    Err(e) => TestResult { id, level: Level::Confirm, outcome: Outcome::Failed, detail: e.to_string() },
                };
                tracing::info!("hardware test {id}: {:?} {}", result.outcome, result.detail);
                let mut list = results.lock().unwrap();
                match list.iter_mut().find(|r| r.id == id) {
                    Some(slot) => *slot = result,
                    None => list.push(result),
                }
            }
            // The tray agent re-applies its fan curve and mode on its own.
            crate::tray::notify_config_changed();
            *running.lock().unwrap() = None;
            ctx.request_repaint();
        });
    }

    /// Hides the window (keeping the results) and notes when, so the tray
    /// agent can close it after a while.
    fn hide(&self) {
        use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
        if let Some((hwnd, _)) = crate::tray::find_marked_window(MARK_PROP) {
            let now = unsafe { windows::Win32::System::SystemInformation::GetTickCount() } as usize;
            crate::panel::set_prop(hwnd, MARK_PROP, now.max(2));
            let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
        }
    }

    fn save_json(&mut self) {
        let Some(report) = self.report() else { return };
        let Some(dir) = AppConfig::path().and_then(|p| Some(p.parent()?.parent()?.join("reports"))) else { return };
        let path = dir.join(format!("opensense-report-{}.json", report.date));
        let written = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, report.to_json()));
        match written {
            Ok(()) => {
                self.message = self.t("Report saved.").into();
                let _ = std::process::Command::new("explorer.exe").arg(format!("/select,{}", path.display())).spawn();
            }
            Err(e) => self.message = e.to_string(),
        }
    }
}

fn label(id: &str) -> &'static str {
    match id {
        "connection" => "Firmware connection",
        "cpu_temp" => "CPU temperature",
        "gpu_temp" => "GPU temperature",
        "cpu_fan_rpm" => "CPU fan speed",
        "gpu_fan_rpm" => "GPU fan speed",
        "operation_mode" => "Performance modes",
        "rgb_keyboard" => "RGB keyboard",
        "charge_limit" => "Battery charge limit",
        "coolboost" => "CoolBoost",
        "lcd_overdrive" => "Screen overdrive",
        "backlight_timeout" => "Keyboard light timeout",
        "gpu_mux" => "GPU mode (MUX)",
        "raw_firmware" => "Firmware values",
        "fan_max" => "Fan control",
        "mode_switch" => "Switch performance mode",
        "rgb_write" => "Keyboard colours",
        "charge_limit_write" => "Change charge limit",
        "gpu_mux_switch" => "Switch GPU mode",
        "overdrive_write" => "Change screen overdrive",
        _ => "Test",
    }
}

/// What a confirm-level test does, shown before it runs.
fn effect(id: &str) -> &'static str {
    match id {
        "fan_max" => "The fans run at full speed for up to 10 seconds, then return to automatic.",
        "mode_switch" => "Switches to another performance mode for a moment, then back.",
        "rgb_write" => "The keyboard turns red for 2 seconds, then your colours return.",
        "charge_limit_write" => "Turns the 80% charge limit on or off for a moment, then back.",
        _ => "Needs a restart, so it is not run here.",
    }
}

fn outcome_style(o: &Outcome) -> (&'static str, Color32) {
    match o {
        Outcome::Verified => ("Verified", theme::ECO),
        Outcome::Detected => ("Detected", theme::STANDARD),
        Outcome::NotSupported => ("Not supported", theme::MUTED),
        Outcome::Failed => ("Failed", theme::TURBO),
        Outcome::NotRun => ("Not run", theme::MUTED),
    }
}

impl eframe::App for DiagApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        pal().form.to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        crate::tray::keep_tray_only();
        if !self.marked {
            if let Some(hwnd) = crate::tray::find_own_window(tr(self.lang, "Hardware Test")) {
                crate::panel::set_prop(hwnd, MARK_PROP, 1);
                crate::tray::tray_only(hwnd);
                theme::title_bar_of(hwnd, self.dark);
                self.marked = true;
            }
        }
        let busy = self.running.lock().unwrap().is_some();
        // Another app took over: hide, but never during a test or while a
        // confirmation is open.
        // A running test restores what it changed at its end: no closing
        // before that.
        crate::panel::hold_close(ctx, busy, &mut self.close_pending);
        if busy || self.confirm.is_some() {
            self.away.reset();
        } else if self.away.gone(ctx) {
            self.hide();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !busy {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        egui::CentralPanel::default().frame(egui::Frame::none().fill(pal().form).inner_margin(5.0)).show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(0.0, 2.0);
                self.device_section(ui);
                ui.add_space(6.0);
                self.run_all_bar(ui);
                ui.add_space(6.0);
                self.tests_section(ui, Level::ReadOnly, Icon::Gauge, "Automatic tests (read only)");
                ui.add_space(8.0);
                self.tests_section(ui, Level::Confirm, Icon::Fan, "Tests that change hardware (ask first)");
                ui.add_space(8.0);
                self.tests_section(ui, Level::Restart, Icon::VideoCard, "Need a restart (not run)");
                ui.add_space(8.0);
                self.report_section(ui);
            });
        });
        ctx.request_repaint_after(Duration::from_millis(if busy { 100 } else { 300 }));
    }
}

impl DiagApp {
    fn device_section(&mut self, ui: &mut egui::Ui) {
        header(ui, Icon::Gear, self.t("Device"));
        let Some((os, d)) = self.device.lock().unwrap().clone() else {
            text_row(ui, self.t("Detecting…"), "", pal().fore);
            return;
        };
        let fore = pal().fore;
        text_row(ui, self.t("Model"), &format!("{} {}", d.manufacturer, d.model), fore);
        text_row(ui, "BIOS", &format!("{}  ({})", d.bios_version, d.bios_date), fore);
        text_row(ui, "CPU", &d.cpu, fore);
        if !d.gpu.is_empty() {
            text_row(ui, "GPU", &d.gpu, fore);
        }
        text_row(ui, "Windows", &format!("{} {} ({})", os.name, os.version, os.build), fore);
        text_row(ui, "OpenSense", env!("CARGO_PKG_VERSION"), fore);
    }

    fn tests_section(&mut self, ui: &mut egui::Ui, level: Level, icon: Icon, title: &'static str) {
        header(ui, icon, self.t(title));
        let results = self.results.lock().unwrap().clone();
        let running = *self.running.lock().unwrap();
        let p = pal();
        for (id, _) in diag::TESTS.iter().filter(|(_, l)| *l == level) {
            let r = results.iter().find(|r| r.id == *id);
            let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::hover());
            ui.painter().text(
                Pos2::new(row.left() + 10.0, row.center().y),
                Align2::LEFT_CENTER,
                self.t(label(id)),
                theme::body(),
                p.fore,
            );
            let status_x = row.left() + row.width() * 0.42;
            let (state, color) = if running == Some(*id) || (results.is_empty() && running.is_some()) {
                (self.t("Running…").to_owned(), p.fore)
            } else {
                let o = r.map_or(&Outcome::NotRun, |r| &r.outcome);
                let (s, c) = outcome_style(o);
                (self.t(s).to_owned(), c)
            };
            ui.painter().circle_filled(Pos2::new(status_x, row.center().y), 4.0, color);
            ui.painter().text(
                Pos2::new(status_x + 10.0, row.center().y),
                Align2::LEFT_CENTER,
                &state,
                theme::body(),
                color,
            );
            let detail = r.map(|r| detail_text(self.lang, &r.detail)).unwrap_or_default();
            let detail = detail.as_str();
            if level == Level::Confirm {
                let button = Rect::from_min_max(
                    Pos2::new(row.right() - 96.0, row.top() + 2.0),
                    Pos2::new(row.right() - 6.0, row.bottom() - 2.0),
                );
                let enabled = running.is_none() && self.confirm.is_none();
                if enabled && ui::row_button(ui, button, id, self.t("Run"), None, true).clicked() {
                    self.confirm = Some(id);
                }
            }
            if !detail.is_empty() && *id != "raw_firmware" {
                let detail_x = status_x + 10.0 + ui::text_width(ui, &state, theme::body()) + 12.0;
                let max = if level == Level::Confirm { row.right() - 104.0 } else { row.right() - 8.0 };
                let shown = elide(ui, detail, max - detail_x);
                ui.painter().text(
                    Pos2::new(detail_x, row.center().y),
                    Align2::LEFT_CENTER,
                    shown,
                    theme::body(),
                    theme::MUTED,
                );
            }
            if self.confirm == Some(*id) {
                self.confirm_row(ui, id);
            }
        }
    }

    /// "Run all tests": one confirmation listing what each test does.
    fn run_all_bar(&mut self, ui: &mut egui::Ui) {
        let running = self.running.lock().unwrap().is_some();
        let (bar, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), Sense::hover());
        let button = bar.shrink2(Vec2::new(4.0, 2.0));
        let label = if running { self.t("Running…") } else { self.t("Run all tests") };
        if !running
            && self.confirm.is_none()
            && ui::row_button(ui, button, "run-all", label, Some(Icon::Gauge), false).clicked()
        {
            self.confirm = Some("all");
        } else if running || self.confirm.is_some() {
            // Drawn but inactive while something runs or awaits an answer.
            ui.painter().rect_filled(button, 4.0, pal().button2);
            ui.painter().text(button.center(), Align2::CENTER_CENTER, label, theme::body(), theme::MUTED);
        }
        if self.confirm != Some("all") {
            return;
        }
        let ids: Vec<&'static str> =
            diag::TESTS.iter().filter(|(_, l)| *l == Level::Confirm).map(|(id, _)| *id).collect();
        let lines = ids.len() as f32 + 1.0;
        let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0 * lines + 40.0), Sense::hover());
        ui.painter().rect_filled(row.shrink2(Vec2::new(4.0, 1.0)), 4.0, pal().button2);
        ui.painter().text(
            Pos2::new(row.left() + 14.0, row.top() + 14.0),
            Align2::LEFT_CENTER,
            self.t("All tests run one after another (about 25 seconds):"),
            theme::body(),
            pal().fore,
        );
        for (i, id) in ids.iter().enumerate() {
            ui.painter().text(
                Pos2::new(row.left() + 22.0, row.top() + 34.0 + 20.0 * i as f32),
                Align2::LEFT_CENTER,
                format!("• {}", self.t(effect(id))),
                theme::body(),
                theme::MUTED,
            );
        }
        let yes = Rect::from_min_max(
            Pos2::new(row.right() - 200.0, row.bottom() - 30.0),
            Pos2::new(row.right() - 108.0, row.bottom() - 4.0),
        );
        let no = Rect::from_min_max(
            Pos2::new(row.right() - 102.0, row.bottom() - 30.0),
            Pos2::new(row.right() - 10.0, row.bottom() - 4.0),
        );
        if ui::row_button(ui, yes, "all-yes", self.t("Start"), None, false).clicked() {
            self.confirm = None;
            let ctx = ui.ctx().clone();
            self.run(&ctx, "all");
        }
        if ui::row_button(ui, no, "all-no", self.t("Cancel"), None, true).clicked() {
            self.confirm = None;
        }
    }

    fn confirm_row(&mut self, ui: &mut egui::Ui, id: &'static str) {
        let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 52.0), Sense::hover());
        ui.painter().rect_filled(row.shrink2(Vec2::new(4.0, 1.0)), 4.0, pal().button2);
        ui.painter().text(
            Pos2::new(row.left() + 14.0, row.top() + 14.0),
            Align2::LEFT_CENTER,
            self.t(effect(id)),
            theme::body(),
            pal().fore,
        );
        let yes = Rect::from_min_max(
            Pos2::new(row.right() - 200.0, row.top() + 26.0),
            Pos2::new(row.right() - 108.0, row.bottom() - 4.0),
        );
        let no = Rect::from_min_max(
            Pos2::new(row.right() - 102.0, row.top() + 26.0),
            Pos2::new(row.right() - 10.0, row.bottom() - 4.0),
        );
        if ui::row_button(ui, yes, "confirm-yes", self.t("Run test"), None, false).clicked() {
            self.confirm = None;
            let ctx = ui.ctx().clone();
            self.run(&ctx, id);
        }
        if ui::row_button(ui, no, "confirm-no", self.t("Cancel"), None, true).clicked() {
            self.confirm = None;
        }
    }

    fn report_section(&mut self, ui: &mut egui::Ui) {
        header(ui, Icon::Download, self.t("Compatibility report"));
        let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::hover());
        ui.painter().text(
            Pos2::new(row.left() + 10.0, row.center().y),
            Align2::LEFT_CENTER,
            self.t("Only the values above. No serial numbers, user names or files."),
            theme::body(),
            theme::MUTED,
        );
        let ready = self.running.lock().unwrap().is_none() && self.report().is_some();
        let (bar, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::hover());
        let w = (bar.width() - 8.0) / 4.0;
        let cell = |i: f32| {
            Rect::from_min_size(
                Pos2::new(bar.left() + 4.0 + i * w, bar.top() + 2.0),
                Vec2::new(w - 4.0, bar.height() - 4.0),
            )
        };
        let show = if self.show_report { self.t("Hide report") } else { self.t("Show report") };
        if ui::row_button(ui, cell(0.0), "show", show, None, true).clicked() {
            self.show_report = !self.show_report;
        }
        if ready && ui::row_button(ui, cell(1.0), "save", self.t("Save JSON"), None, true).clicked() {
            self.save_json();
        }
        if ready && ui::row_button(ui, cell(2.0), "copy", self.t("Copy"), None, true).clicked() {
            if let Some(r) = self.report() {
                ui.ctx().copy_text(r.to_json());
                self.message = self.t("Report copied.").into();
            }
        }
        if ready
            && ui::row_button(ui, cell(3.0), "share", self.t("Share on GitHub"), Some(Icon::Star), false)
                .on_hover_text(
                    self.t("Opens a pre-filled issue in your browser. Nothing is sent until you submit it there."),
                )
                .clicked()
        {
            if let Some(r) = self.report() {
                crate::panel::open_url(&r.issue_url(crate::updates::REPO));
            }
        }
        if !self.message.is_empty() {
            text_row(ui, &self.message.clone(), "", theme::ECO);
        }
        if self.show_report {
            if let Some(r) = self.report() {
                let mut json = r.to_json();
                ui.add_space(4.0);
                ui.add(
                    egui::TextEdit::multiline(&mut json)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .interactive(false),
                );
            }
        }
    }
}

fn header(ui: &mut egui::Ui, icon: Icon, title: &str) {
    let p = pal();
    let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::hover());
    ui.painter().rect_filled(row, 0.0, p.button2);
    crate::gh::icons::paint(ui.painter(), Pos2::new(row.left() + 14.0, row.center().y), 16.0, icon, p.fore);
    ui.painter().text(
        Pos2::new(row.left() + 28.0, row.center().y),
        Align2::LEFT_CENTER,
        title,
        theme::bold(13.3),
        p.fore,
    );
}

fn text_row(ui: &mut egui::Ui, name: &str, value: &str, color: Color32) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
    ui.painter().text(Pos2::new(r.left() + 10.0, r.center().y), Align2::LEFT_CENTER, name, theme::body(), color);
    ui.painter().text(
        Pos2::new(r.left() + r.width() * 0.25, r.center().y),
        Align2::LEFT_CENTER,
        value,
        theme::body(),
        pal().fore,
    );
}

/// Detail text for display: the fixed phrases are translated, values
/// (temperatures, RPM, mode names) are shown as they are.
fn detail_text(lang: Lang, detail: &str) -> String {
    let fixed: [&'static str; 6] = [
        "supported; not run, needs a restart",
        "toggled and restored",
        "firmware accepted the colours",
        "80% limit on",
        "80% limit off",
        "not run: needs a restart",
    ];
    match fixed.iter().find(|f| **f == detail) {
        Some(f) => tr(lang, f).to_owned(),
        None => detail.to_owned(),
    }
}

/// Cuts text to fit `width`, ending with "…".
fn elide(ui: &egui::Ui, text: &str, width: f32) -> String {
    if width <= 10.0 {
        return String::new();
    }
    if ui::text_width(ui, text, theme::body()) <= width {
        return text.to_owned();
    }
    let mut s: String = text.to_owned();
    while !s.is_empty() && ui::text_width(ui, &format!("{s}…"), theme::body()) > width {
        s.pop();
    }
    format!("{s}…")
}
