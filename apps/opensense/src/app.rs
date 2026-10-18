//! egui front end: Monitor, Fans, Lighting, Settings and About.

use std::time::Duration;

use eframe::egui::{self, Color32, RichText, Sense};
use opensense_hw::TransportKind;
use opensense_proto::lighting::{self, Effect, Rgb};
use opensense_proto::mode::{BacklightTimeout, GpuMode, OperationMode};
use opensense_proto::sensor::celsius_to_fahrenheit;
use opensense_sys::config::{AppConfig, FanModeCfg, LightingKind, TempUnit};
use opensense_sys::oem::OemInfo;

use crate::apply::effect_from_name;
use crate::i18n::{tr, Lang};
use crate::widgets::{self, card, kv, pal, section, select_card, setting_row, Series, Theme};
use crate::worker::{self, Command, WorkerHandle};

pub const ACCENT: Color32 = Color32::from_rgb(0xE0, 0x1E, 0x2B);
const CPU_COLOR: Color32 = Color32::from_rgb(0xE6, 0x2B, 0x2B);
const GPU_COLOR: Color32 = Color32::from_rgb(0x2E, 0xA8, 0xFF);
const WARN: Color32 = Color32::from_rgb(0xFF, 0xB0, 0x40);
const ERROR: Color32 = Color32::from_rgb(255, 110, 110);

/// RPM shown as a full gauge.
const FAN_GAUGE_MAX: f32 = 7000.0;
/// Seconds shown in the history charts.
const CHART_WINDOW_S: f64 = 180.0;
/// CPU temperature at which the fan page suggests Max.
const HOT_CPU_C: u16 = 92;

const FAN_PRESETS: [(&str, u8); 4] = [("Silent", 0), ("Balanced", 20), ("Cool", 50), ("Full", 100)];

/// Measured Custom-mode fan speed per percentage. The firmware keeps the
/// fans at roughly 2,500 RPM even at 0 %.
const FAN_CURVE: [(u8, f32); 6] = [(0, 2500.0), (10, 2700.0), (20, 3750.0), (40, 4900.0), (60, 6000.0), (100, 7900.0)];

const PALETTE: [&str; 14] = [
    "#FF0000", "#FF4000", "#FF8000", "#FFC000", "#FFFF00", "#80FF00", "#00FF00", "#00FF80", "#00FFFF", "#0080FF",
    "#0000FF", "#8000FF", "#FF00FF", "#FFFFFF",
];

const THEMES: [(&str, [&str; 4]); 8] = [
    ("Nitro Red", ["#FF0000", "#FF0000", "#FF0000", "#FF0000"]),
    ("Ocean", ["#0040FF", "#0080FF", "#00C0FF", "#00FFFF"]),
    ("Sunset", ["#FF0040", "#FF4000", "#FF8000", "#FFC000"]),
    ("Rainbow", ["#FF0000", "#FFFF00", "#00FF00", "#0080FF"]),
    ("Forest", ["#00FF00", "#40FF00", "#00C040", "#80FF00"]),
    ("Ice", ["#FFFFFF", "#C0E0FF", "#80C0FF", "#FFFFFF"]),
    ("Purple", ["#8000FF", "#C000FF", "#FF00FF", "#8000FF"]),
    ("White", ["#FFFFFF", "#FFFFFF", "#FFFFFF", "#FFFFFF"]),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Monitor,
    Fans,
    Lighting,
    Settings,
    About,
}

pub struct OpenSenseApp {
    cfg: AppConfig,
    lang: Lang,
    oem: OemInfo,
    worker: WorkerHandle,
    page: Page,
    elevated: bool,
    mock: bool,
    autostart: bool,
    plans: Vec<opensense_sys::power::PowerPlan>,
    status: Option<String>,
    /// Keyboard zone picked in the lighting preview.
    selected_zone: usize,
    /// Palette clicks recolour every zone instead of the selected one.
    paint_all: bool,
    logo: egui::TextureHandle,
    /// When idle pages were last given back to Windows.
    last_trim: Option<std::time::Instant>,
}

impl OpenSenseApp {
    pub fn new(cc: &eframe::CreationContext<'_>, transport: TransportKind) -> Self {
        let mut cfg = AppConfig::load();
        if cfg.language != "tr" && cfg.language != "en" {
            cfg.language = Lang::resolve(&cfg.language).code().into();
            let _ = cfg.save();
        }
        widgets::install_fonts(&cc.egui_ctx);
        widgets::apply_theme(&cc.egui_ctx, Theme::from_name(&cfg.theme));
        let logo = cc.egui_ctx.load_texture(
            "logo",
            egui::ColorImage::from_rgba_unmultiplied([128, 128], &crate::icon::rgba(128)),
            egui::TextureOptions::LINEAR,
        );
        let lang = Lang::resolve(&cfg.language);
        let oem = OemInfo::detect();
        let ctx = cc.egui_ctx.clone();
        let worker =
            worker::spawn(transport, &oem, Duration::from_millis(cfg.poll_ms), cfg.history_len, true, move || {
                ctx.request_repaint()
            });
        let mock = transport == TransportKind::Mock;
        // The tray agent applies the saved settings when it starts; without
        // it the window does so itself.
        // A running tray agent applied the saved settings when it started;
        // re-sending them here would undo choices made from its menu.
        let agent_running = cfg.close_to_tray && (crate::tray::is_running() || crate::tray::ensure_running(mock));
        let mut app = Self {
            elevated: opensense_sys::privilege::is_elevated(),
            mock,
            cfg,
            lang,
            oem,
            worker,
            page: Page::Monitor,
            autostart: opensense_sys::autostart::is_enabled(),
            plans: opensense_sys::power::plans().unwrap_or_default(),
            status: None,
            selected_zone: 0,
            paint_all: false,
            logo,
            last_trim: None,
        };
        if app.cfg.apply_on_start && !agent_running {
            app.apply_saved();
        }
        app
    }

    fn t(&self, s: &'static str) -> &'static str {
        tr(self.lang, s)
    }

    fn send(&self, c: Command) {
        let _ = self.worker.tx.send(c);
    }

    fn save(&mut self) {
        if let Err(e) = self.cfg.save() {
            self.status = Some(format!("{}: {e}", self.t("Could not save settings")));
        }
    }

    /// Re-send the persisted state.
    fn apply_saved(&mut self) {
        for c in crate::apply::saved_state(&self.cfg, &self.oem) {
            self.send(c);
        }
    }

    fn send_fan(&self) {
        self.send(crate::apply::fan(&self.cfg.fan));
    }

    fn send_lighting(&self) {
        self.send(crate::apply::lighting(&self.cfg.lighting));
    }

    fn set_mode(&mut self, m: OperationMode) {
        self.send(Command::Mode(m));
        if m.forces_auto_fan() {
            self.cfg.fan.mode = FanModeCfg::Auto;
        }
        self.cfg.operation_mode = Some(m.label().to_ascii_lowercase());
        self.save();
    }

    /// Firmware-probed capabilities, falling back to the optional OEM data
    /// until the probe has run.
    fn has_modes(&self) -> bool {
        self.worker.shared.lock().caps.map_or(self.oem.has_operation_modes(), |c| c.operation_modes)
    }

    fn has_rgb(&self) -> bool {
        self.worker.shared.lock().caps.map_or(self.oem.rgb_keyboard, |c| c.rgb_keyboard)
    }

    fn temp_value(&self, c: f32) -> f64 {
        match self.cfg.temp_unit {
            TempUnit::Celsius => f64::from(c),
            TempUnit::Fahrenheit => f64::from(celsius_to_fahrenheit(c)),
        }
    }

    fn temp_unit_suffix(&self) -> &'static str {
        match self.cfg.temp_unit {
            TempUnit::Celsius => "°C",
            TempUnit::Fahrenheit => "°F",
        }
    }

    fn temp(&self, c: Option<f32>) -> String {
        match c {
            None => "--".into(),
            Some(c) => match self.cfg.temp_unit {
                TempUnit::Celsius => format!("{c:.0} °C"),
                TempUnit::Fahrenheit => format!("{:.0} °F", celsius_to_fahrenheit(c)),
            },
        }
    }
}

impl eframe::App for OpenSenseApp {
    /// Caps the texture size egui plans its font atlas for; the driver's
    /// limit (8192 on NVIDIA) makes the atlas needlessly wide.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        raw_input.max_texture_side = Some(raw_input.max_texture_side.unwrap_or(2048).min(2048));
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // While the window is minimised or in the background, give pages it
        // is not using back to Windows (as Windows itself does under memory
        // pressure); they fault back in cheaply when needed.
        let (minimized, focused) =
            ctx.input(|i| (i.viewport().minimized.unwrap_or(false), i.viewport().focused.unwrap_or(true)));
        if minimized || !focused {
            let due = self.last_trim.is_none_or(|t| t.elapsed() >= Duration::from_secs(30));
            if due {
                trim_working_set();
                self.last_trim = Some(std::time::Instant::now());
            }
        } else {
            self.last_trim = None;
        }

        egui::SidePanel::left("nav").resizable(false).exact_width(190.0).show(ctx, |ui| {
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                ui.add(egui::Image::new(&self.logo).fit_to_exact_size(egui::vec2(40.0, 40.0)));
                ui.vertical(|ui| {
                    ui.add_space(1.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(RichText::new("Open").size(21.0).color(pal().text));
                        ui.label(RichText::new("Sense").size(21.0).strong().color(ACCENT));
                    });
                    let model = self.oem.model.clone().unwrap_or_else(|| "Acer Nitro".into()).to_uppercase();
                    ui.label(RichText::new(model).size(10.5).color(pal().weak));
                });
            });
            ui.add_space(10.0);
            let (line, _) = ui.allocate_exact_size(egui::vec2(176.0, 1.0), Sense::hover());
            ui.painter().rect_filled(line, 0.0, pal().border);
            ui.add_space(12.0);
            for (p, icon, label) in [
                (Page::Monitor, widgets::Icon::Monitor, "Monitor"),
                (Page::Fans, widgets::Icon::Fan, "Fan control"),
                (Page::Lighting, widgets::Icon::Lighting, "Lighting"),
                (Page::Settings, widgets::Icon::Settings, "Settings"),
                (Page::About, widgets::Icon::About, "About"),
            ] {
                let sel = self.page == p;
                let color = if sel { Color32::WHITE } else { pal().text };
                let text = RichText::new(format!("         {}", self.t(label))).size(15.0).color(color);
                let b = egui::Button::new(text)
                    .fill(if sel { ACCENT } else { Color32::TRANSPARENT })
                    .min_size(egui::vec2(176.0, 34.0));
                let resp = ui.add(b);
                widgets::paint_icon(
                    ui.painter(),
                    egui::pos2(resp.rect.left() + 22.0, resp.rect.center().y),
                    icon,
                    color,
                );
                if resp.clicked() {
                    self.page = p;
                }
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.add_space(8.0);
                let connected = self.worker.shared.lock().connected;
                let (txt, tip, col) = if connected && self.mock {
                    (self.t("Demo mode"), self.t("Simulated sensors; nothing is sent to the laptop."), WARN)
                } else if connected {
                    (
                        self.t("Hardware connected"),
                        self.t("Talking to the laptop firmware directly (ACPI-WMI)."),
                        Color32::from_rgb(80, 200, 120),
                    )
                } else {
                    (
                        self.t("Hardware not connected"),
                        self.t("Run OpenSense as administrator to control the laptop."),
                        WARN,
                    )
                };
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
                    ui.painter().circle_filled(rect.center(), 4.0, col);
                    ui.label(RichText::new(txt).small().color(col)).on_hover_text(tip);
                });
            });
        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            let snap = self.worker.shared.lock();
            ui.horizontal(|ui| {
                if let Some((label, err)) = &snap.last_action {
                    match err {
                        None => ui.label(RichText::new(self.t(label)).small().color(Color32::GRAY)),
                        Some(e) => ui.label(RichText::new(format!("{}: {e}", self.t(label))).small().color(ERROR)),
                    };
                }
                if let Some(s) = &self.status {
                    ui.label(RichText::new(s).small().color(ERROR));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let p = snap.power;
                    let batt = p.battery_percent.map(|b| format!("{b}%")).unwrap_or_else(|| "--".into());
                    let src = if p.on_ac { self.t("AC power") } else { self.t("Battery") };
                    ui.label(RichText::new(format!("{src}  {batt}")).small());
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.elevation_banner(ui);
            egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| match self.page {
                Page::Monitor => self.monitor(ui),
                Page::Fans => self.fans(ui),
                Page::Lighting => self.lighting(ui),
                Page::Settings => self.settings(ui),
                Page::About => self.about(ui),
            });
        });
    }
}

// ---- pages -----------------------------------------------------------------

impl OpenSenseApp {
    fn elevation_banner(&mut self, ui: &mut egui::Ui) {
        let (connected, err) = {
            let s = self.worker.shared.lock();
            (s.connected, s.hw_error.clone())
        };
        if connected || self.mock {
            return;
        }
        widgets::notice(ui, WARN, |ui| {
            if !self.elevated {
                ui.label(self.t("OpenSense needs administrator rights to talk to the Acer firmware."));
                if ui.button(self.t("Restart as administrator")).clicked() {
                    match opensense_sys::privilege::relaunch_elevated("--relaunched") {
                        Ok(()) => std::process::exit(0),
                        Err(e) => self.status = Some(e.to_string()),
                    }
                }
            } else {
                ui.label(format!("{}: {}", self.t("Hardware unavailable"), err.unwrap_or_default()));
            }
        });
        ui.add_space(8.0);
    }

    fn monitor(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let t = |s| tr(lang, s);
        let snap = self.worker.shared.lock();
        let r = snap.readings;
        ui.heading(t("System monitor"));
        ui.add_space(6.0);
        ui.columns(2, |cols| {
            card(&mut cols[0], "CPU", CPU_COLOR, |ui| {
                ui.label(RichText::new(snap.cpu_name.clone()).small().weak());
                widgets::big(ui, &self.temp(r.cpu_temp.map(f32::from)));
                kv(ui, t("Usage"), format!("{:.0} %", snap.cpu.usage));
                kv(
                    ui,
                    t("Clock"),
                    if snap.cpu.frequency_mhz > 0 { format!("{} MHz", snap.cpu.frequency_mhz) } else { "--".into() },
                );
                kv(ui, t("Fan"), rpm(r.cpu_fan_rpm));
                if let Some((lo, hi)) = snap.cpu_temp_minmax {
                    kv(ui, t("Min / Max"), format!("{} / {}", self.temp(Some(lo)), self.temp(Some(hi))));
                }
            });
            card(&mut cols[1], "GPU", GPU_COLOR, |ui| {
                ui.label(
                    RichText::new(snap.gpu_name.clone().unwrap_or_else(|| t("Discrete GPU").into())).small().weak(),
                );
                let gpu_t = r.gpu_temp.map(f32::from).or(snap.history.back().and_then(|s| s.gpu_temp));
                widgets::big(ui, &self.temp(gpu_t));
                kv(ui, t("Usage"), snap.gpu_usage.map(|u| format!("{u:.0} %")).unwrap_or_else(|| "--".into()));
                kv(ui, t("Fan"), rpm(r.gpu_fan_rpm));
                if let Some((lo, hi)) = snap.gpu_temp_minmax {
                    kv(ui, t("Min / Max"), format!("{} / {}", self.temp(Some(lo)), self.temp(Some(hi))));
                }
            });
        });

        ui.add_space(12.0);
        ui.label(RichText::new(t("Temperature, last 3 minutes")).strong());
        ui.add_space(2.0);
        let now = snap.history.back().map_or(0.0, |s| s.t);
        let pts = |f: &dyn Fn(&worker::Sample) -> Option<f32>| -> Vec<[f64; 2]> {
            snap.history
                .iter()
                .filter(|s| now - s.t <= CHART_WINDOW_S)
                .filter_map(|s| f(s).map(|v| [s.t - now, self.temp_value(v)]))
                .collect()
        };
        let series = vec![
            Series {
                name: "CPU".into(),
                color: CPU_COLOR,
                points: pts(&|s| s.cpu_temp),
                current: self.temp(r.cpu_temp.map(f32::from)),
            },
            Series {
                name: "GPU".into(),
                color: GPU_COLOR,
                points: pts(&|s| s.gpu_temp),
                current: self.temp(snap.history.back().and_then(|s| s.gpu_temp)),
            },
        ];
        let unit = self.temp_unit_suffix();
        let range = if self.cfg.temp_unit == TempUnit::Celsius { (30.0, 90.0) } else { (86.0, 194.0) };
        widgets::history_chart(ui, "temps", series, range, unit, CHART_WINDOW_S);

        let mode = snap.mode;
        let full_power = snap.power.on_ac && snap.battery_boost != Some(false);
        drop(snap);

        if self.has_modes() {
            ui.add_space(12.0);
            ui.heading(t("Operation mode"));
            ui.add_space(4.0);
            let mut clicked = None;
            ui.columns(3, |cols| {
                for (i, (m, desc)) in [
                    (OperationMode::Quiet, "Quiet: low noise, fans stay on Auto."),
                    (OperationMode::Default, "Default: balanced performance and noise."),
                    (OperationMode::Extreme, "Extreme: maximum performance, needs the power adapter."),
                ]
                .into_iter()
                .enumerate()
                {
                    let enabled = !m.needs_full_power() || full_power;
                    if select_card(&mut cols[i], mode == Some(m), enabled, t(m.label()), t(desc)) {
                        clicked = Some(m);
                    }
                }
            });
            if let Some(m) = clicked {
                self.set_mode(m);
            }
            if !full_power {
                ui.label(RichText::new(t("Extreme needs AC power with the full-power adapter.")).small().color(WARN));
            }
        }
    }

    fn fans(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let t = |s| tr(lang, s);
        let (r, coolboost, mode) = {
            let s = self.worker.shared.lock();
            (s.readings, s.coolboost, s.mode)
        };
        let quiet_lock = self.has_modes() && mode == Some(OperationMode::Quiet);
        ui.heading(t("Fan control"));
        ui.add_space(6.0);

        // Live gauges with the matching component temperature.
        ui.columns(2, |cols| {
            for (i, (name, color, fan, temp)) in [
                (t("CPU fan"), CPU_COLOR, r.cpu_fan_rpm, r.cpu_temp),
                (t("GPU fan"), GPU_COLOR, r.gpu_fan_rpm, r.gpu_temp),
            ]
            .into_iter()
            .enumerate()
            {
                card(&mut cols[i], name, color, |ui| {
                    ui.horizontal(|ui| {
                        let frac = fan.map(|v| f32::from(v) / FAN_GAUGE_MAX).unwrap_or(0.0);
                        widgets::gauge(
                            ui,
                            110.0,
                            frac,
                            color,
                            &fan.map(|v| v.to_string()).unwrap_or_else(|| "--".into()),
                            "RPM",
                        );
                        ui.vertical(|ui| {
                            ui.add_space(18.0);
                            ui.label(RichText::new(self.temp(temp.map(f32::from))).size(26.0).strong());
                            ui.label(RichText::new(if i == 0 { "CPU" } else { "GPU" }).weak());
                        });
                    });
                });
            }
        });

        // Context hint.
        if !quiet_lock && r.cpu_temp.is_some_and(|c| c >= HOT_CPU_C) && self.cfg.fan.mode != FanModeCfg::Max {
            ui.add_space(6.0);
            let mut go_max = false;
            widgets::notice(ui, WARN, |ui| {
                ui.label(t("CPU is very hot. Maximum fan speed is recommended."));
                go_max = ui.button(t("Use Maximum")).clicked();
            });
            if go_max {
                self.cfg.fan.mode = FanModeCfg::Max;
                self.send_fan();
                self.save();
            }
        }

        ui.add_space(10.0);
        ui.label(RichText::new(t("Fan mode")).strong());
        let mut changed = false;
        let mut go_silent = false;
        let modes = [
            (FanModeCfg::Auto, "Auto", "The laptop adjusts fan speed to temperature. Recommended for everyday use."),
            (FanModeCfg::Max, "Max", "Fans run at full speed. Best cooling, loudest."),
            (FanModeCfg::Custom, "Custom", "Choose the fan speed yourself."),
        ];
        let has_silent = self.has_modes();
        ui.columns(if has_silent { 4 } else { 3 }, |cols| {
            let mut i = 0;
            if has_silent {
                // Quiet operation mode: lowers CPU/GPU power so the firmware
                // can actually slow the fans (custom % cannot go below the
                // firmware's temperature-based minimum).
                if select_card(
                    &mut cols[0],
                    quiet_lock,
                    true,
                    t("Silent"),
                    t("Lowers CPU/GPU power so the fans can run slowest. Quietest option."),
                ) && !quiet_lock
                {
                    go_silent = true;
                }
                i = 1;
            }
            for (m, title, desc) in modes {
                let sel = !quiet_lock && self.cfg.fan.mode == m;
                if select_card(&mut cols[i], sel, true, t(title), t(desc)) && !sel {
                    self.cfg.fan.mode = m;
                    changed = true;
                }
                i += 1;
            }
        });
        if go_silent {
            self.set_mode(OperationMode::Quiet);
        } else if changed && quiet_lock {
            // Leaving Silent: Quiet mode would keep the fans on Auto.
            self.set_mode(OperationMode::Default);
        }

        if self.cfg.fan.mode == FanModeCfg::Custom && !quiet_lock && !go_silent {
            ui.add_space(8.0);
            changed |= self.custom_fan_panel(ui);
        }
        if changed {
            self.send_fan();
            self.save();
        }

        // CoolBoost is only offered when the firmware reports it (the
        // firmware answers 0xE1 when it is unsupported).
        if let Some(mut cb) = coolboost {
            ui.add_space(10.0);
            if ui.checkbox(&mut cb, t("CoolBoost (higher fan curve)")).changed() {
                self.cfg.fan.coolboost = cb;
                self.send(Command::CoolBoost(cb));
                self.save();
            }
        }

        ui.add_space(12.0);
        ui.label(RichText::new(t("Fan speed, last 3 minutes")).strong());
        ui.add_space(2.0);
        let snap = self.worker.shared.lock();
        let now = snap.history.back().map_or(0.0, |s| s.t);
        let pts = |f: &dyn Fn(&worker::Sample) -> Option<f32>| -> Vec<[f64; 2]> {
            snap.history
                .iter()
                .filter(|s| now - s.t <= CHART_WINDOW_S)
                .filter_map(|s| f(s).map(|v| [s.t - now, f64::from(v)]))
                .collect()
        };
        let series = vec![
            Series {
                name: t("CPU fan").into(),
                color: CPU_COLOR,
                points: pts(&|s| s.cpu_fan),
                current: rpm(r.cpu_fan_rpm),
            },
            Series {
                name: t("GPU fan").into(),
                color: GPU_COLOR,
                points: pts(&|s| s.gpu_fan),
                current: rpm(r.gpu_fan_rpm),
            },
        ];
        drop(snap);
        widgets::history_chart(ui, "fans", series, (0.0, 3000.0), "", CHART_WINDOW_S);
    }

    /// Presets, linked/per-fan sliders. Returns true when the custom state
    /// should be sent.
    fn custom_fan_panel(&mut self, ui: &mut egui::Ui) -> bool {
        let lang = self.lang;
        let t = |s| tr(lang, s);
        let mut send = false;
        egui::Frame::none().fill(pal().card).rounding(10.0).stroke(egui::Stroke::new(1.0_f32, pal().border)).inner_margin(12.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let f = &mut self.cfg.fan;

            ui.label(
                RichText::new(t("In Custom mode the firmware keeps the fans at about 2,500 RPM or more, even at 0%. For the quietest laptop use Silent or Auto: in Auto the fans slow down further when the laptop is cool."))
                    .small()
                    .color(WARN),
            );
            ui.add_space(4.0);
            ui.label(RichText::new(t("Quick presets")).weak());
            ui.horizontal(|ui| {
                for (name, pct) in FAN_PRESETS {
                    let active = !f.cpu_auto && !f.gpu_auto && f.cpu_percent == pct && f.gpu_percent == pct;
                    let b = egui::Button::new(RichText::new(format!("{}  ~{} RPM", t(name), approx_rpm(pct))).size(14.0))
                        .fill(if active { ACCENT } else { ui.visuals().widgets.inactive.bg_fill })
                        .min_size(egui::vec2(110.0, 32.0));
                    if ui.add(b).clicked() {
                        f.cpu_percent = pct;
                        f.gpu_percent = pct;
                        f.cpu_auto = false;
                        f.gpu_auto = false;
                        send = true;
                    }
                }
            });
            ui.add_space(8.0);
            ui.checkbox(&mut f.linked, t("Adjust CPU and GPU together"));
            ui.add_space(4.0);

            // Send on release rather than while dragging.
            let released = |r: &egui::Response| r.drag_stopped() || (r.changed() && !r.dragged());
            if f.linked {
                let mut pct = f.cpu_percent;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t("Both fans")).strong());
                    let r = ui.add(fan_slider(&mut pct));
                    ui.label(RichText::new(fan_hint(lang, pct)).weak());
                    if r.changed() {
                        f.cpu_percent = pct;
                        f.gpu_percent = pct;
                        f.cpu_auto = false;
                        f.gpu_auto = false;
                    }
                    send |= released(&r);
                });
            } else {
                for (name, pct, auto) in [
                    ("CPU", &mut f.cpu_percent, &mut f.cpu_auto),
                    ("GPU", &mut f.gpu_percent, &mut f.gpu_auto),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(name).monospace().strong());
                        send |= ui.checkbox(auto, t("Auto")).changed();
                        let r = ui.add_enabled(!*auto, fan_slider(pct));
                        if !*auto {
                            ui.label(RichText::new(fan_hint(lang, *pct)).weak());
                        }
                        send |= released(&r);
                    });
                }
            }
        });
        send
    }

    fn lighting(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let t = |s| tr(lang, s);
        ui.heading(t("Keyboard lighting"));
        if !self.has_rgb() {
            ui.label(t("This model does not report an RGB keyboard."));
            return;
        }
        ui.add_space(6.0);
        let mut changed = false;

        // Static / effects tabs.
        ui.horizontal(|ui| {
            let l = &mut self.cfg.lighting;
            for (k, label) in [(LightingKind::Static, "Solid colours"), (LightingKind::Dynamic, "Effects")] {
                let sel = l.kind == k;
                let b = egui::Button::new(RichText::new(t(label)).size(15.0))
                    .fill(if sel { ACCENT } else { ui.visuals().widgets.inactive.bg_fill })
                    .min_size(egui::vec2(150.0, 34.0));
                if ui.add(b).clicked() && !sel {
                    l.kind = k;
                    changed = true;
                }
            }
        });
        ui.add_space(8.0);

        // Live preview.
        let preview = self.preview_colors(ui.ctx());
        let selected =
            (self.cfg.lighting.kind == LightingKind::Static && !self.paint_all).then_some(self.selected_zone);
        let labels = std::array::from_fn(|i| format!("{} {}", t("Zone"), i + 1));
        if let Some(z) = widgets::keyboard(ui, preview, selected, labels) {
            if self.cfg.lighting.kind == LightingKind::Static {
                self.selected_zone = z;
                self.paint_all = false;
            }
        }
        ui.add_space(8.0);

        // Brightness as segments.
        ui.horizontal(|ui| {
            ui.label(RichText::new(t("Brightness")).strong());
            for level in 1..=5u8 {
                let label =
                    if level == 1 { t("Off").to_owned() } else { format!("{}%", lighting::level_to_percent(level)) };
                let sel = self.cfg.lighting.brightness_level == level;
                let b = egui::Button::new(label)
                    .fill(if sel { ACCENT } else { ui.visuals().widgets.inactive.bg_fill })
                    .min_size(egui::vec2(58.0, 28.0));
                if ui.add(b).clicked() && !sel {
                    self.cfg.lighting.brightness_level = level;
                    changed = true;
                }
            }
        });
        ui.add_space(10.0);

        match self.cfg.lighting.kind {
            LightingKind::Static => changed |= self.static_panel(ui),
            LightingKind::Dynamic => changed |= self.effect_panel(ui),
        }
        if changed {
            self.send_lighting();
            self.save();
        }

        ui.add_space(14.0);
        let kb = self.worker.shared.lock().kb_timeout;
        let mut auto_off = kb.map(|k| k.auto_off).unwrap_or(false);
        if ui.checkbox(&mut auto_off, t("Turn backlight off after 30 s idle")).changed() {
            let brightness = kb.map(|k| k.brightness).unwrap_or(100);
            self.send(Command::KbTimeout(BacklightTimeout { auto_off, brightness }));
        }
    }

    fn static_panel(&mut self, ui: &mut egui::Ui) -> bool {
        let lang = self.lang;
        let t = |s| tr(lang, s);
        let mut changed = false;

        ui.label(RichText::new(t("Themes")).strong());
        ui.horizontal_wrapped(|ui| {
            for (name, colors) in THEMES {
                let rgb = colors.map(|h| to_color(Rgb::from_hex(h).unwrap_or_default()));
                if widgets::theme_tile(ui, t(name), rgb).clicked() {
                    let l = &mut self.cfg.lighting;
                    l.zones = colors.map(String::from);
                    l.zone_enabled = [true; 4];
                    if l.brightness_level == 1 {
                        l.brightness_level = 5;
                    }
                    changed = true;
                }
            }
        });
        ui.add_space(10.0);

        ui.horizontal(|ui| {
            ui.label(RichText::new(t("Colour")).strong());
            ui.label(RichText::new(t("Click a zone on the keyboard, then pick a colour.")).small().weak());
        });
        ui.checkbox(&mut self.paint_all, t("Apply to all zones"));
        let target: Vec<usize> = if self.paint_all { (0..4).collect() } else { vec![self.selected_zone] };
        let current = Rgb::from_hex(&self.cfg.lighting.zones[self.selected_zone]).unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            for hex in PALETTE {
                let c = Rgb::from_hex(hex).unwrap_or_default();
                if widgets::swatch(ui, to_color(c), c == current).clicked() {
                    for &z in &target {
                        self.cfg.lighting.zones[z] = c.to_hex();
                        self.cfg.lighting.zone_enabled[z] = true;
                    }
                    changed = true;
                }
            }
            let mut arr = [current.r, current.g, current.b];
            if ui.color_edit_button_srgb(&mut arr).on_hover_text(t("More colours…")).changed() {
                for &z in &target {
                    self.cfg.lighting.zones[z] = Rgb::new(arr[0], arr[1], arr[2]).to_hex();
                    self.cfg.lighting.zone_enabled[z] = true;
                }
                changed = true;
            }
        });
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            for z in 0..4 {
                let label = format!("{} {}", t("Zone"), z + 1);
                changed |= ui.checkbox(&mut self.cfg.lighting.zone_enabled[z], label).changed();
                ui.add_space(8.0);
            }
        });
        changed
    }

    fn effect_panel(&mut self, ui: &mut egui::Ui) -> bool {
        let lang = self.lang;
        let t = |s| tr(lang, s);
        let mut changed = false;
        let l = &mut self.cfg.lighting;
        let current = effect_from_name(&l.effect);

        let effects = [
            (Effect::Breathing, "Breathing", "Fades one colour in and out."),
            (Effect::Neon, "Neon", "Cycles through all colours."),
            (Effect::Wave, "Wave", "A rainbow sweeps across the keyboard."),
            (Effect::Shifting, "Shifting", "One colour slides across the zones."),
            (Effect::Zoom, "Zoom", "Colour pulses out from the centre."),
        ];
        ui.columns(5, |cols| {
            for (i, (e, name, desc)) in effects.into_iter().enumerate() {
                if select_card(&mut cols[i], current == e, true, t(name), t(desc)) && current != e {
                    l.effect = e.label().to_ascii_lowercase();
                    changed = true;
                }
            }
        });
        ui.add_space(10.0);
        let eff = effect_from_name(&l.effect);

        ui.horizontal(|ui| {
            ui.label(RichText::new(t("Speed")).strong());
            ui.label(RichText::new(t("Slow")).weak());
            let r = ui.add(egui::Slider::new(&mut l.speed, 1..=9).show_value(false));
            ui.label(RichText::new(t("Fast")).weak());
            changed |= r.drag_stopped() || (r.changed() && !r.dragged());
        });
        if eff.uses_direction() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(t("Direction")).strong());
                for (val, label) in [("left", "Left"), ("right", "Right")] {
                    let sel = l.direction == val;
                    let arrow = if val == "left" { "← " } else { "→ " };
                    let b = egui::Button::new(format!("{arrow}{}", t(label)))
                        .fill(if sel { ACCENT } else { ui.visuals().widgets.inactive.bg_fill })
                        .min_size(egui::vec2(90.0, 28.0));
                    if ui.add(b).clicked() && !sel {
                        l.direction = val.into();
                        changed = true;
                    }
                }
            });
        }
        if eff.uses_color() && eff != Effect::Wave {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(t("Colour")).strong());
                let current = Rgb::from_hex(&l.effect_color).unwrap_or_default();
                for hex in PALETTE {
                    let c = Rgb::from_hex(hex).unwrap_or_default();
                    if widgets::swatch(ui, to_color(c), c == current).clicked() {
                        l.effect_color = c.to_hex();
                        changed = true;
                    }
                }
                let mut arr = [current.r, current.g, current.b];
                if ui.color_edit_button_srgb(&mut arr).on_hover_text(t("More colours…")).changed() {
                    l.effect_color = Rgb::new(arr[0], arr[1], arr[2]).to_hex();
                    changed = true;
                }
            });
        }
        changed
    }

    /// Colours shown in the keyboard preview; dynamic effects are animated.
    fn preview_colors(&self, ctx: &egui::Context) -> [Option<Color32>; 4] {
        let l = &self.cfg.lighting;
        let level = f32::from(lighting::level_to_percent(l.brightness_level)) / 100.0;
        let dim = |c: Color32| -> Option<Color32> {
            if level == 0.0 || c == Color32::BLACK {
                return None;
            }
            let k = 0.35 + 0.65 * level;
            Some(Color32::from_rgb(
                (f32::from(c.r()) * k) as u8,
                (f32::from(c.g()) * k) as u8,
                (f32::from(c.b()) * k) as u8,
            ))
        };
        match l.kind {
            LightingKind::Static => std::array::from_fn(|i| {
                if l.zone_enabled[i] {
                    dim(to_color(Rgb::from_hex(&l.zones[i]).unwrap_or_default()))
                } else {
                    None
                }
            }),
            LightingKind::Dynamic => {
                ctx.request_repaint_after(Duration::from_millis(33));
                let time = ctx.input(|i| i.time) as f32 * (0.3 + f32::from(l.speed) * 0.15);
                let base = Rgb::from_hex(&l.effect_color).unwrap_or_default();
                let base = to_color(base);
                let dir = if l.direction == "right" { -1.0 } else { 1.0 };
                let hue = |h: f32| egui::ecolor::Hsva::new(h.rem_euclid(1.0), 1.0, 1.0, 1.0).into();
                let scale = |c: Color32, k: f32| {
                    Color32::from_rgb(
                        (f32::from(c.r()) * k) as u8,
                        (f32::from(c.g()) * k) as u8,
                        (f32::from(c.b()) * k) as u8,
                    )
                };
                let colors: [Color32; 4] = match effect_from_name(&l.effect) {
                    Effect::Breathing => [scale(base, 0.15 + 0.85 * (0.5 + 0.5 * (time * 2.0).sin())); 4],
                    Effect::Neon => [hue(time * 0.2); 4],
                    Effect::Wave => std::array::from_fn(|i| hue(time * 0.3 + dir * i as f32 * 0.18)),
                    Effect::Shifting => {
                        let pos = (time * dir).rem_euclid(4.0) as usize;
                        std::array::from_fn(|i| if i == pos { base } else { scale(base, 0.12) })
                    }
                    Effect::Zoom | Effect::Static => {
                        let phase = (time * 1.5).rem_euclid(2.0);
                        std::array::from_fn(|i| {
                            let dist = if i == 1 || i == 2 { 0.0 } else { 1.0 };
                            scale(base, (1.0 - (phase - dist).abs()).clamp(0.1, 1.0))
                        })
                    }
                };
                colors.map(dim)
            }
        }
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let t = |s| tr(lang, s);
        ui.heading(t("Settings"));
        ui.add_space(8.0);
        let mut dirty = false;

        section(ui, t("Appearance"), |ui| {
            setting_row(ui, t("Language"), None, |ui| {
                let cur = usize::from(self.cfg.language == "en");
                // Right-to-left layout: labels in reverse order.
                if let Some(i) = widgets::segmented(ui, 1 - cur, &["English", "Türkçe"]) {
                    let code = if i == 0 { "en" } else { "tr" };
                    self.cfg.language = code.into();
                    self.lang = Lang::resolve(code);
                    // Save now so the tray agent reads the new language.
                    if let Err(e) = self.cfg.save() {
                        self.status = Some(format!("{}: {e}", tr(self.lang, "Could not save settings")));
                    }
                    crate::tray::notify_config_changed();
                }
            });
            widgets::row_separator(ui);
            setting_row(ui, t("Theme"), None, |ui| {
                let themes = [Theme::Light, Theme::Oled, Theme::Dark];
                let cur = themes.iter().position(|th| th.name() == self.cfg.theme).unwrap_or(2);
                if let Some(i) = widgets::segmented(ui, cur, &[t("Light"), t("OLED black"), t("Dark")]) {
                    self.cfg.theme = themes[i].name().into();
                    widgets::apply_theme(ui.ctx(), themes[i]);
                    dirty = true;
                }
            });
            widgets::row_separator(ui);
            setting_row(ui, t("Temperature unit"), None, |ui| {
                let cur = usize::from(self.cfg.temp_unit == TempUnit::Celsius);
                if let Some(i) = widgets::segmented(ui, cur, &["°F", "°C"]) {
                    self.cfg.temp_unit = if i == 1 { TempUnit::Celsius } else { TempUnit::Fahrenheit };
                    dirty = true;
                }
            });
        });

        section(ui, t("Behaviour"), |ui| {
            setting_row(ui, t("Sensor refresh"), Some(t("How often temperatures and fan speeds are read.")), |ui| {
                let steps = [5000u64, 2000, 1000, 500];
                let cur = steps.iter().position(|&s| s == self.cfg.poll_ms).unwrap_or(2);
                if let Some(i) = widgets::segmented(ui, cur, &["5 s", "2 s", "1 s", "0.5 s"]) {
                    self.cfg.poll_ms = steps[i];
                    self.send(Command::SetPollInterval(Duration::from_millis(steps[i])));
                    dirty = true;
                }
            });
            widgets::row_separator(ui);
            setting_row(
                ui,
                t("Restore settings at startup"),
                Some(t("Re-applies operation mode, fans and lighting.")),
                |ui| {
                    dirty |= widgets::toggle(ui, &mut self.cfg.apply_on_start).changed();
                },
            );
            widgets::row_separator(ui);
            setting_row(
                ui,
                t("Keep running in the system tray"),
                Some(t("The window closes completely; a ~3 MB tray process stays.")),
                |ui| {
                    if widgets::toggle(ui, &mut self.cfg.close_to_tray).changed() {
                        dirty = true;
                        if self.cfg.close_to_tray {
                            crate::tray::ensure_running(self.mock);
                        }
                    }
                },
            );
            widgets::row_separator(ui);
            setting_row(
                ui,
                t("Start with Windows"),
                Some(t("Starts the tray process at sign-in, without a UAC prompt.")),
                |ui| {
                    let mut auto = self.autostart;
                    if widgets::toggle(ui, &mut auto).changed() {
                        let res =
                            if auto { opensense_sys::autostart::enable() } else { opensense_sys::autostart::disable() };
                        match res {
                            Ok(()) => self.autostart = auto,
                            Err(e) => self.status = Some(format!("{}: {e}", t("Start with Windows"))),
                        }
                    }
                },
            );
        });
        if dirty {
            self.save();
        }

        let (gpu_mode, mux, overdrive) = {
            let s = self.worker.shared.lock();
            (s.gpu_mode, s.gpu_mux_supported, s.overdrive)
        };
        section(ui, t("Hardware"), |ui| {
            if mux {
                setting_row(
                    ui,
                    t("GPU mode"),
                    Some(t("Discrete only gives more performance; takes effect after a restart.")),
                    |ui| {
                        let cur = usize::from(gpu_mode != Some(GpuMode::Discrete));
                        if let Some(i) = widgets::segmented(ui, cur, &[t("Discrete only"), t("Hybrid")]) {
                            self.send(Command::GpuMode(if i == 0 { GpuMode::Discrete } else { GpuMode::Hybrid }));
                        }
                    },
                );
                widgets::row_separator(ui);
            }
            if let Some(mut od) = overdrive {
                setting_row(ui, t("LCD overdrive"), Some(t("Faster pixel response, may add ghosting.")), |ui| {
                    if widgets::toggle(ui, &mut od).changed() {
                        self.send(Command::Overdrive(od));
                    }
                });
                widgets::row_separator(ui);
            }
            setting_row(ui, t("Firmware state"), Some(t("Read modes and settings from the firmware again")), |ui| {
                if ui.button(format!("↻ {}", t("Re-read"))).clicked() {
                    self.send(Command::RefreshState);
                }
            });
        });

        let battery = self.worker.shared.lock().battery;
        if let Some(b) = battery.filter(|b| b.limit_supported) {
            section(ui, t("Battery"), |ui| {
                setting_row(
                    ui,
                    t("Limit charging to 80%"),
                    Some(t("Extends battery life when the laptop is mostly plugged in. Stored in the firmware.")),
                    |ui| {
                        let mut on = b.limit_active;
                        if widgets::toggle(ui, &mut on).changed() {
                            self.send(Command::ChargeLimit(on));
                            self.cfg.charge_limit = Some(on);
                            self.save();
                        }
                    },
                );
            });
        }

        section(ui, t("Windows power plan"), |ui| {
            let plans = self.plans.clone();
            let mut seen = std::collections::HashMap::new();
            for p in &plans {
                *seen.entry(p.name.clone()).or_insert(0) += 1;
            }
            for (i, p) in plans.iter().enumerate() {
                if i > 0 {
                    widgets::row_separator(ui);
                }
                // Same-named plans get a short GUID suffix.
                let label = match (p.name.is_empty(), seen.get(&p.name).copied().unwrap_or(0) > 1) {
                    (true, _) => p.guid.clone(),
                    (false, true) => format!("{}  ({})", p.name, &p.guid[..8]),
                    (false, false) => p.name.clone(),
                };
                setting_row(ui, &label, None, |ui| {
                    let mut on = p.active;
                    if widgets::toggle(ui, &mut on).changed() && on {
                        if let Err(e) = opensense_sys::power::set_active(&p.guid) {
                            self.status = Some(format!("{}: {e}", t("Windows power plan")));
                        }
                        self.plans = opensense_sys::power::plans().unwrap_or_default();
                    }
                });
            }
        });

        if let Some(path) = AppConfig::path() {
            ui.label(RichText::new(format!("{}: {}", t("Settings file"), path.display())).small().color(pal().weak));
        }
    }

    fn about(&self, ui: &mut egui::Ui) {
        let t = |s| tr(self.lang, s);
        let p = pal();
        ui.horizontal(|ui| {
            ui.add(egui::Image::new(&self.logo).fit_to_exact_size(egui::vec2(72.0, 72.0)));
            ui.vertical(|ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(RichText::new("Open").size(30.0).color(p.text));
                    ui.label(RichText::new("Sense").size(30.0).strong().color(ACCENT));
                });
                ui.label(RichText::new(format!("{} {}", t("Version"), env!("CARGO_PKG_VERSION"))).color(p.weak));
            });
        });
        ui.add_space(10.0);
        ui.label(RichText::new(t("An open-source control center for Acer Nitro laptops: fans, operation modes, keyboard lighting and system monitoring.")).size(15.0));
        ui.add_space(12.0);

        section(ui, t("About the software"), |ui| {
            for line in [
                "No code has been copied from anywhere: OpenSense is written entirely from scratch.",
                "Written in Rust and optimised to run with minimal resources (about 3 MB in the background).",
                "Works without NitroSense or any Acer service installed: it talks to the laptop firmware directly through Windows' built-in ACPI-WMI driver.",
            ] {
                ui.horizontal_wrapped(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 2.5, ACCENT);
                    ui.label(t(line));
                });
            }
        });

        let (connected, transport, caps) = {
            let s = self.worker.shared.lock();
            (s.connected, s.transport.clone(), s.caps)
        };
        section(ui, t("This device"), |ui| {
            kv(ui, t("Model"), self.oem.model.clone().map(|m| m.to_uppercase()).unwrap_or_else(|| "--".into()));
            kv(
                ui,
                t("Firmware connection"),
                if self.mock {
                    t("Demo mode").into()
                } else if connected {
                    format!("{} ({transport})", t("Hardware connected"))
                } else {
                    t("Hardware not connected").into()
                },
            );
            let yes_no = |b: bool| if b { t("Supported") } else { t("Not supported") };
            if let Some(c) = caps {
                kv(ui, t("Operation modes"), yes_no(c.operation_modes).into());
                kv(ui, t("RGB keyboard"), yes_no(c.rgb_keyboard).into());
            }
        });

        section(ui, t("Technology"), |ui| {
            kv(ui, t("Language"), "Rust".into());
            kv(ui, t("Interface"), "egui / OpenGL".into());
            kv(ui, t("Hardware access"), "ACPI-WMI (AcerGamingFunction, APGeAction)".into());
            kv(ui, t("Licence"), "MIT".into());
        });

        section(ui, t("Disclaimer"), |ui| {
            ui.label(t("All settings changed in the application are your own responsibility. Fan, operation mode and power settings change how your laptop cools itself; the firmware still enforces its own safety limits."));
            ui.add_space(6.0);
            ui.label(
                RichText::new(t("OpenSense is not affiliated with or endorsed by Acer. Acer, Nitro and NitroSense are trademarks of Acer Inc."))
                    .small()
                    .color(p.weak),
            );
        });
    }
}

// ---- helpers ----------------------------------------------------------------

fn fan_slider(pct: &mut u8) -> egui::Slider<'_> {
    egui::Slider::new(pct, 0..=100).step_by(10.0).suffix(" %")
}

/// Approximate Custom-mode fan speed for a percentage, rounded to 50 RPM.
fn approx_rpm(pct: u8) -> u32 {
    let pct = pct.min(100);
    let rpm = FAN_CURVE
        .windows(2)
        .find(|w| pct <= w[1].0)
        .map(|w| {
            let (p0, r0) = w[0];
            let (p1, r1) = w[1];
            r0 + (r1 - r0) * f32::from(pct - p0) / f32::from(p1 - p0)
        })
        .unwrap_or(FAN_CURVE[FAN_CURVE.len() - 1].1);
    (rpm / 50.0).round() as u32 * 50
}

/// "~3750 RPM · moderate"
fn fan_hint(lang: Lang, pct: u8) -> String {
    let noise = match pct {
        0..=10 => "quiet",
        11..=30 => "moderate",
        31..=60 => "loud",
        _ => "very loud",
    };
    format!("~{} RPM · {}", approx_rpm(pct), tr(lang, noise))
}

fn trim_working_set() {
    use windows::Win32::System::ProcessStatus::EmptyWorkingSet;
    use windows::Win32::System::Threading::GetCurrentProcess;
    unsafe {
        let _ = EmptyWorkingSet(GetCurrentProcess());
    }
}

fn to_color(c: Rgb) -> Color32 {
    Color32::from_rgb(c.r, c.g, c.b)
}

fn rpm(v: Option<u16>) -> String {
    v.map(|r| format!("{r} RPM")).unwrap_or_else(|| "--".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approx_rpm_follows_measured_curve() {
        assert_eq!(approx_rpm(0), 2500);
        assert_eq!(approx_rpm(20), 3750);
        assert_eq!(approx_rpm(30), 4350);
        assert_eq!(approx_rpm(100), 7900);
        assert_eq!(approx_rpm(255), 7900);
    }
}
