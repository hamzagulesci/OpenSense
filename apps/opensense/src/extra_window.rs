//! "Extra Settings" window (`opensense --extra`), opened from the keyboard
//! row's "Extra" button: keyboard shortcuts, keyboard backlight details,
//! Windows tweaks, appearance, hibernate timeout and vendor services.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Pos2, Rect, Sense, Vec2};
use opensense_proto::lighting::Rgb;
use opensense_sys::config::{AppConfig, LightingKind, TempUnit};
use opensense_sys::tweaks::{self, setting};

use crate::automation::HOTKEYS;
use crate::gh::icons::Icon;
use crate::gh::theme::{self, pal};
use crate::gh::ui;
use crate::i18n::{tr, Lang};
use crate::worker::WorkerHandle;

pub const MARK_PROP: &str = "OpenSense.Extra";
pub const WIDTH: f32 = 516.0;
/// Window title in the saved language.
pub fn title() -> &'static str {
    tr(crate::i18n::ui_lang(), "Extra Settings")
}
const ROW: f32 = 26.0;

pub struct ExtraApp {
    cfg: AppConfig,
    lang: Lang,
    worker: WorkerHandle,
    dark: bool,
    modified: Option<std::time::SystemTime>,
    checked: Instant,
    marked: bool,
    /// Closes the window when the user switches to another app.
    away: crate::tray::AwayWatch,
    /// Running vendor services, refreshed in the background (None = busy).
    services: Arc<Mutex<Option<Vec<&'static str>>>>,
    hibernate_min: Option<u32>,
    message: String,
}

impl ExtraApp {
    pub fn new(cc: &eframe::CreationContext<'_>, transport: opensense_hw::TransportKind) -> Self {
        let cfg = AppConfig::load();
        let dark = theme::is_dark(&cfg.ui_mode);
        theme::install_fonts(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx, dark);
        let oem = opensense_sys::oem::OemInfo::detect();
        let ctx = cc.egui_ctx.clone();
        let worker =
            crate::worker::spawn(transport, &oem, Duration::from_millis(2000), 4, false, move || ctx.request_repaint());
        let app = Self {
            lang: Lang::resolve(&cfg.language),
            cfg,
            worker,
            dark,
            modified: crate::fans_window::config_mtime(),
            checked: Instant::now(),
            marked: false,
            away: crate::tray::AwayWatch::default(),
            services: Arc::new(Mutex::new(None)),
            hibernate_min: tweaks::read(setting::HIBERNATE_AFTER).ok().map(|v| v.1 / 60),
            message: String::new(),
        };
        app.refresh_services();
        app
    }

    fn t(&self, s: &'static str) -> &'static str {
        tr(self.lang, s)
    }

    fn refresh_services(&self) {
        let slot = self.services.clone();
        *slot.lock().unwrap() = None;
        std::thread::spawn(move || {
            let running = tweaks::running_services();
            *slot.lock().unwrap() = Some(running);
        });
    }

    fn save(&mut self) {
        if let Err(e) = self.cfg.save() {
            self.message = e.to_string();
        }
        self.modified = crate::fans_window::config_mtime();
        crate::tray::notify_config_changed();
    }

    fn apply_lighting(&mut self) {
        let _ = self.worker.tx.send(crate::apply::lighting(&self.cfg.lighting));
        self.save();
    }
}

impl eframe::App for ExtraApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        raw_input.max_texture_side = Some(raw_input.max_texture_side.unwrap_or(2048).min(2048));
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        pal().form.to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.marked {
            if let Some(hwnd) = crate::tray::find_own_window(tr(self.lang, "Extra Settings")) {
                crate::panel::set_prop(hwnd, MARK_PROP, 1);
                crate::tray::tray_only(hwnd);
                theme::title_bar_of(hwnd, self.dark);
                self.marked = true;
            }
        }
        // Like a flyout: switching to another app closes the window.
        if self.away.gone(ctx) && !self.cfg.topmost {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.checked.elapsed() > Duration::from_millis(500) {
            self.checked = Instant::now();
            let m = crate::fans_window::config_mtime();
            if m != self.modified {
                self.modified = m;
                self.cfg = AppConfig::load();
                self.lang = Lang::resolve(&self.cfg.language);
            }
        }
        egui::CentralPanel::default().frame(egui::Frame::none().fill(pal().form).inner_margin(5.0)).show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(0.0, 2.0);
                self.key_bindings(ui);
                self.backlight(ui);
                self.other(ui);
                self.hibernate(ui);
                self.services_row(ui);
                if !self.message.is_empty() {
                    ui.colored_label(theme::TURBO, &self.message);
                }
            });
        });
        ctx.request_repaint_after(Duration::from_millis(500));
    }
}

// ---- sections -------------------------------------------------------------------

impl ExtraApp {
    fn key_bindings(&mut self, ui: &mut egui::Ui) {
        header(ui, Icon::Keyboard, self.t("Key Bindings"));
        let mut on = self.cfg.hotkeys;
        if check(ui, "hotkeys", self.t("Enable keyboard shortcuts"), &mut on) {
            self.cfg.hotkeys = on;
            self.save();
        }
        let p = pal();
        for (h, combo) in HOTKEYS {
            let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::hover());
            let fg = if self.cfg.hotkeys { p.fore } else { theme::MUTED };
            ui.painter().text(
                Pos2::new(row.left() + 10.0, row.center().y),
                Align2::LEFT_CENTER,
                tr(self.lang, h.label()),
                theme::body(),
                fg,
            );
            ui.painter().text(
                Pos2::new(row.right() - 10.0, row.center().y),
                Align2::RIGHT_CENTER,
                tr(self.lang, combo),
                theme::body(),
                theme::MUTED,
            );
        }
        ui.add_space(8.0);
    }

    fn backlight(&mut self, ui: &mut egui::Ui) {
        let rgb = self.worker.shared.lock().caps.is_none_or(|c| c.rgb_keyboard);
        // Header with the brightness slider on its right.
        let (row, _) = header_rect(ui);
        let p = pal();
        draw_header(ui, row, Icon::Keyboard, self.t("Laptop Backlight"));
        let level = self.cfg.lighting.brightness_level.clamp(1, 5);
        let slider = Rect::from_min_max(
            Pos2::new(row.right() - 230.0, row.top() + 5.0),
            Pos2::new(row.right() - 60.0, row.bottom() - 5.0),
        );
        let out = ui::slider(
            ui,
            slider,
            "kb-level",
            f32::from(level),
            1.0,
            5.0,
            1.0,
            &[1.0, 2.0, 3.0, 4.0, 5.0],
            theme::STANDARD,
        );
        let shown = out.value as u8;
        let label = if shown <= 1 {
            self.t("Off").to_owned()
        } else {
            format!("{}%", opensense_proto::lighting::level_to_percent(shown))
        };
        ui.painter().text(
            Pos2::new(row.right() - 10.0, row.center().y),
            Align2::RIGHT_CENTER,
            label,
            theme::bold(theme::BODY),
            p.fore,
        );
        if let Some(v) = out.commit {
            self.cfg.lighting.brightness_level = v as u8;
            self.apply_lighting();
        }
        if !rgb {
            ui.add_space(8.0);
            return;
        }
        // Animation speed and direction for effects.
        let dynamic = self.cfg.lighting.kind == LightingKind::Dynamic;
        let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::hover());
        label_left(ui, row, self.t("Animation Speed"), dynamic);
        let sl = Rect::from_min_max(
            Pos2::new(row.right() - 200.0, row.top() + 4.0),
            Pos2::new(row.right() - 10.0, row.bottom() - 4.0),
        );
        let out = ui::slider(
            ui,
            sl,
            "kb-speed",
            f32::from(self.cfg.lighting.speed.clamp(1, 9)),
            1.0,
            9.0,
            1.0,
            &[],
            theme::STANDARD,
        );
        if let Some(v) = out.commit {
            self.cfg.lighting.speed = v as u8;
            if dynamic {
                self.apply_lighting();
            } else {
                self.save();
            }
        }
        let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::hover());
        label_left(ui, row, self.t("Direction"), dynamic);
        let cells = [
            Rect::from_min_max(
                Pos2::new(row.right() - 200.0, row.top() + 2.0),
                Pos2::new(row.right() - 107.0, row.bottom() - 2.0),
            ),
            Rect::from_min_max(
                Pos2::new(row.right() - 103.0, row.top() + 2.0),
                Pos2::new(row.right() - 10.0, row.bottom() - 2.0),
            ),
        ];
        for (cell, (dir, text)) in cells.into_iter().zip([("left", "Left"), ("right", "Right")]) {
            let style = ui::TileStyle {
                icon: None,
                secondary: false,
                enabled: true,
                accent: theme::STANDARD,
                selected: self.cfg.lighting.direction == dir,
            };
            if ui::tile(ui, cell, dir, self.t(text), &style).clicked() && self.cfg.lighting.direction != dir {
                self.cfg.lighting.direction = dir.into();
                if dynamic {
                    self.apply_lighting();
                } else {
                    self.save();
                }
            }
        }
        // Zone colours (static lighting).
        let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW + 2.0), Sense::hover());
        label_left(ui, row, self.t("Zone colours"), true);
        let mut changed = false;
        for z in 0..4 {
            let x = row.right() - 10.0 - (4 - z) as f32 * 48.0 + 4.0;
            let r = Rect::from_min_size(Pos2::new(x, row.top() + 3.0), Vec2::new(44.0, row.height() - 6.0));
            let c = Rgb::from_hex(&self.cfg.lighting.zones[z]).unwrap_or_default();
            let mut arr = [c.r, c.g, c.b];
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(r), |ui| {
                ui.spacing_mut().interact_size = r.size();
                if egui::widgets::color_picker::color_edit_button_srgb(ui, &mut arr)
                    .on_hover_text(format!("{} {}", self.t("Zone"), z + 1))
                    .changed()
                {
                    self.cfg.lighting.zones[z] = Rgb::new(arr[0], arr[1], arr[2]).to_hex();
                    self.cfg.lighting.zone_enabled[z] = true;
                    changed = true;
                }
            });
        }
        if changed {
            self.cfg.lighting.kind = LightingKind::Static;
            self.apply_lighting();
        }
        // Backlight timeouts.
        let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::hover());
        label_left(ui, row, self.t("Backlight timeout plugged / on battery"), true);
        let (off, sec) = (self.t("Off"), self.t("sec"));
        let mut save = false;
        for (i, value) in [&mut self.cfg.kb_timeout_ac, &mut self.cfg.kb_timeout_battery].into_iter().enumerate() {
            let x = row.right() - 10.0 - (2 - i) as f32 * 82.0 + 4.0;
            let r = Rect::from_min_size(Pos2::new(x, row.top() + 3.0), Vec2::new(78.0, row.height() - 6.0));
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(r), |ui| {
                let resp = ui.add_sized(
                    r.size(),
                    egui::DragValue::new(value).range(0..=3600).speed(1.0).custom_formatter(move |v, _| {
                        if v == 0.0 {
                            off.to_owned()
                        } else {
                            format!("{v:.0} {sec}")
                        }
                    }),
                );
                save |= resp.changed();
            });
        }
        if save {
            self.save();
        }
        ui.add_space(8.0);
    }

    fn other(&mut self, ui: &mut egui::Ui) {
        let (row, _) = header_rect(ui);
        draw_header(ui, row, Icon::Gear, self.t("Other"));
        let scan = Rect::from_min_max(
            Pos2::new(row.right() - 90.0, row.top() + 4.0),
            Pos2::new(row.right() - 6.0, row.bottom() - 4.0),
        );
        if ui::row_button(ui, scan, "scan", self.t("Scan"), None, false)
            .on_hover_text(self.t("Save a report of every firmware value and open it"))
            .clicked()
        {
            scan_report();
        }
        let mut save = false;
        let mut v = self.cfg.aspm_off;
        if check(ui, "aspm", self.t("Disable PCIe Link State Management (plugged in)"), &mut v) {
            self.cfg.aspm_off = v;
            let _ = tweaks::write(setting::ASPM, Some(if v { 0 } else { 2 }), None);
            save = true;
        }
        let mut v = self.cfg.standby_network_off;
        if check(ui, "standby", self.t("Disable networking in Modern Standby"), &mut v) {
            self.cfg.standby_network_off = v;
            let (ac, dc) = if v { (0, 0) } else { (1, 2) };
            let _ = tweaks::write(setting::STANDBY_NETWORK, Some(ac), Some(dc));
            save = true;
        }
        let mut v = self.cfg.topmost;
        if check(ui, "topmost", self.t("Keep app window always on top"), &mut v) {
            self.cfg.topmost = v;
            save = true;
        }
        let mut v = self.cfg.clamshell;
        if check(ui, "clamshell", self.t("Auto Toggle Clamshell Mode"), &mut v) {
            self.cfg.clamshell = v;
            save = true;
        }
        let mut v = self.cfg.bw_icon;
        if check(ui, "bw", self.t("Black and white tray icon"), &mut v) {
            self.cfg.bw_icon = v;
            save = true;
        }
        let mut v = !self.cfg.disable_osd;
        if check(ui, "osd", self.t("Show on-screen notifications"), &mut v) {
            self.cfg.disable_osd = !v;
            save = true;
        }
        let mut v = self.cfg.apply_on_start;
        if check(ui, "restore", self.t("Restore settings at startup"), &mut v) {
            self.cfg.apply_on_start = v;
            save = true;
        }
        // Appearance.
        let lang = self.lang;
        let themes = [("windows", "Windows"), ("dark", "Dark"), ("light", "Light")];
        let cur = themes.iter().position(|(k, _)| *k == self.cfg.ui_mode).unwrap_or(0);
        if let Some(i) = combo_row(ui, "theme", tr(lang, "Theme"), cur, &themes.map(|(_, n)| tr(lang, n))) {
            self.cfg.ui_mode = themes[i].0.into();
            self.dark = theme::is_dark(&self.cfg.ui_mode);
            theme::apply(ui.ctx(), self.dark);
            if let Some((hwnd, _)) = crate::tray::find_marked_window(MARK_PROP) {
                theme::title_bar_of(hwnd, self.dark);
            }
            save = true;
        }
        let langs = [("en", "English"), ("tr", "Türkçe")];
        let cur = usize::from(lang == Lang::Tr);
        if let Some(i) = combo_row(ui, "language", tr(lang, "Language"), cur, &langs.map(|(_, n)| n)) {
            self.cfg.language = langs[i].0.into();
            self.lang = Lang::resolve(&self.cfg.language);
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Title(tr(self.lang, "Extra Settings").into()));
            save = true;
        }
        let cur = usize::from(self.cfg.temp_unit == TempUnit::Fahrenheit);
        if let Some(i) = combo_row(ui, "unit", tr(lang, "Temperature unit"), cur, &["°C", "°F"]) {
            self.cfg.temp_unit = if i == 1 { TempUnit::Fahrenheit } else { TempUnit::Celsius };
            save = true;
        }
        if save {
            self.save();
        }
        ui.add_space(8.0);
    }

    fn hibernate(&mut self, ui: &mut egui::Ui) {
        let Some(mut minutes) = self.hibernate_min else { return };
        let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW + 4.0), Sense::hover());
        crate::gh::icons::paint(
            ui.painter(),
            Pos2::new(row.left() + 12.0, row.center().y),
            16.0,
            Icon::Battery,
            pal().fore,
        );
        ui.painter().text(
            Pos2::new(row.left() + 26.0, row.center().y),
            Align2::LEFT_CENTER,
            self.t("Time to hibernate in sleep on battery"),
            theme::bold(theme::BODY),
            pal().fore,
        );
        let r = Rect::from_min_max(
            Pos2::new(row.right() - 96.0, row.top() + 4.0),
            Pos2::new(row.right() - 10.0, row.bottom() - 4.0),
        );
        let (off, min) = (self.t("Off"), self.t("min"));
        let mut changed = false;
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(r), |ui| {
            changed = ui
                .add_sized(
                    r.size(),
                    egui::DragValue::new(&mut minutes).range(0..=10_000).speed(10.0).custom_formatter(move |v, _| {
                        if v == 0.0 {
                            off.to_owned()
                        } else {
                            format!("{v:.0} {min}")
                        }
                    }),
                )
                .drag_stopped()
                || ui.input(|i| i.key_pressed(egui::Key::Enter));
        });
        self.hibernate_min = Some(minutes);
        if changed {
            if let Err(e) = tweaks::write(setting::HIBERNATE_AFTER, None, Some(minutes * 60)) {
                self.message = e.to_string();
            }
        }
    }

    fn services_row(&mut self, ui: &mut egui::Ui) {
        let running = self.services.lock().unwrap().clone();
        let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 38.0), Sense::hover());
        crate::gh::icons::paint(
            ui.painter(),
            Pos2::new(row.left() + 12.0, row.center().y),
            16.0,
            Icon::Gear,
            pal().fore,
        );
        let (text, color) = match &running {
            None => (format!("{}:  …", self.t("Acer Services Running")), pal().fore),
            Some(r) => (
                format!("{}:  {}", self.t("Acer Services Running"), r.len()),
                if r.is_empty() { theme::STANDARD } else { theme::TURBO },
            ),
        };
        let r = ui.painter().text(
            Pos2::new(row.left() + 26.0, row.center().y),
            Align2::LEFT_CENTER,
            text,
            theme::bold(theme::BODY),
            color,
        );
        if let Some(list) = running.as_ref().filter(|l| !l.is_empty()) {
            ui.interact(r, ui.id().with("services-list"), Sense::hover()).on_hover_text(list.join("\n"));
        }
        let button = Rect::from_min_max(
            Pos2::new(row.right() - 128.0, row.top() + 6.0),
            Pos2::new(row.right() - 6.0, row.bottom() - 6.0),
        );
        if let Some(list) = running {
            let stop = !list.is_empty();
            let label = if stop { self.t("Stop") } else { self.t("Start") };
            if ui::row_button(ui, button, "services", label, None, false).clicked() {
                let slot = self.services.clone();
                *slot.lock().unwrap() = None;
                std::thread::spawn(move || {
                    tweaks::set_services(!stop);
                    *slot.lock().unwrap() = Some(tweaks::running_services());
                });
            }
        }
    }
}

// ---- helpers --------------------------------------------------------------------

fn header_rect(ui: &mut egui::Ui) -> (Rect, egui::Response) {
    ui.add_space(2.0);
    ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::hover())
}

fn draw_header(ui: &mut egui::Ui, row: Rect, icon: Icon, title: &str) {
    let p = pal();
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

fn header(ui: &mut egui::Ui, icon: Icon, title: &str) {
    let (row, _) = header_rect(ui);
    draw_header(ui, row, icon, title);
}

fn label_left(ui: &mut egui::Ui, row: Rect, text: &str, enabled: bool) {
    let c = if enabled { pal().fore } else { theme::MUTED };
    ui.painter().text(Pos2::new(row.left() + 10.0, row.center().y), Align2::LEFT_CENTER, text, theme::body(), c);
}

fn check(ui: &mut egui::Ui, id: &str, label: &str, on: &mut bool) -> bool {
    let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
    ui::checkbox(ui, row.shrink2(Vec2::new(10.0, 0.0)), id, label, on).changed()
}

fn combo_row(ui: &mut egui::Ui, id: &str, label: &str, current: usize, items: &[&str]) -> Option<usize> {
    let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW), Sense::hover());
    label_left(ui, row, label, true);
    let r = Rect::from_min_max(
        Pos2::new(row.right() - 160.0, row.top() + 2.0),
        Pos2::new(row.right() - 10.0, row.bottom() - 2.0),
    );
    let mut picked = current;
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(r), |ui| {
        egui::ComboBox::from_id_salt(id).width(r.width() - 8.0).selected_text(items[current]).show_ui(ui, |ui| {
            for (i, item) in items.iter().enumerate() {
                ui.selectable_value(&mut picked, i, *item);
            }
        });
    });
    (picked != current).then_some(picked)
}

/// Runs the command-line tool's firmware dump into a text file and opens it.
fn scan_report() {
    std::thread::spawn(|| {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let Some(cli) = std::env::current_exe().ok().map(|e| e.with_file_name("opensense-cli.exe")) else { return };
        let Ok(out) = std::process::Command::new(&cli).arg("probe").creation_flags(CREATE_NO_WINDOW).output() else {
            return;
        };
        let path = std::env::temp_dir().join("opensense-scan.txt");
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        if std::fs::write(&path, text).is_ok() {
            let _ = std::process::Command::new("notepad.exe").arg(&path).spawn();
        }
    });
}
