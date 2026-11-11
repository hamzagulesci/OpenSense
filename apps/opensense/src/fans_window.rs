//! "Fans and Power" window, opened next to the main window from its
//! "Fans + Power" tile. Left: CPU tab (Windows power mode, CPU boost) and
//! GPU tab (NVIDIA clock limit). Right: per-mode fan curves for the CPU and
//! GPU fans with a mode selector, "Factory Defaults", "Calibrate" and
//! "Apply Fan Curve".
//!
//! Everything here is stored per mode. The curves are applied by the tray
//! agent's software loop, so they keep working when this window is closed.

use std::time::{Duration, Instant};

use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use opensense_proto::mode::OperationMode;
use opensense_sys::config::AppConfig;
use opensense_sys::power::{PowerMode, BOOST_MODES};

use crate::gh::icons::Icon;
use crate::gh::theme::{self, pal};
use crate::gh::ui;
use crate::i18n::{tr, Lang};
use crate::modes::{self, Curve};
use crate::worker::{Command, WorkerHandle};

/// Window property marking the Fans and Power window.
pub const MARK_PROP: &str = "OpenSense.Fans";
pub const WIDTH: f32 = 675.0;
pub const MIN_HEIGHT: f32 = 560.0;
const LEFT: f32 = 265.0;
/// Seconds without a new maximum that end a calibration.
const CALIBRATE_PLATEAU: u32 = 15;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Cpu,
    Gpu,
}

/// NVIDIA name and max clock; outer None = not read yet.
type NvidiaSlot = std::sync::Arc<std::sync::Mutex<Option<Option<(String, u32)>>>>;

struct Calibration {
    max: [u16; 2],
    same: u32,
    last_sample: Instant,
}

pub struct FansWindow {
    pub title: String,
    tab: Tab,
    /// Mode the cached curves belong to.
    key: String,
    curves: [Curve; 2],
    drag: Option<(usize, usize)>,
    /// Y axis in percent instead of RPM.
    percent_axis: bool,
    renaming: bool,
    confirm_remove: bool,
    result: String,
    calibration: Option<Calibration>,
    /// NVIDIA name and max clock, read once in the background.
    nvidia: NvidiaSlot,
    power_mode: Option<PowerMode>,
    /// Plans without power modes, with their names.
    plans: Vec<(PowerMode, String)>,
    boost: Option<u8>,
    windows_read: Option<Instant>,
}

impl FansWindow {
    pub fn new() -> Self {
        let brand = opensense_sys::cpu::CpuMonitor::new().brand();
        let nvidia = std::sync::Arc::new(std::sync::Mutex::new(None));
        let slot = nvidia.clone();
        std::thread::spawn(move || {
            *slot.lock().unwrap() = Some(crate::nvidia::info());
        });
        Self {
            title: {
                let name = tr(crate::i18n::ui_lang(), "Fans and Power");
                if brand.is_empty() {
                    name.into()
                } else {
                    format!("{name} - {brand}")
                }
            },
            tab: Tab::Cpu,
            key: String::new(),
            curves: [modes::default_curve(OperationMode::Default, false); 2],
            drag: None,
            percent_axis: false,
            renaming: false,
            confirm_remove: false,
            result: String::new(),
            calibration: None,
            nvidia,
            power_mode: None,
            plans: Vec::new(),
            boost: None,
            windows_read: None,
        }
    }

    fn reload(&mut self, cfg: &AppConfig) {
        let key = modes::current_key(cfg);
        let p = modes::profile(cfg, &key);
        let base = modes::base_of(cfg, &key);
        self.curves = [
            modes::from_stored(&p.cpu_curve).unwrap_or_else(|| modes::default_curve(base, false)),
            modes::from_stored(&p.gpu_curve).unwrap_or_else(|| modes::default_curve(base, true)),
        ];
        self.key = key;
        self.renaming = false;
        self.confirm_remove = false;
    }

    /// Draws the window contents.
    pub fn show(&mut self, ctx: &egui::Context, cfg: &mut AppConfig, lang: Lang, worker: &WorkerHandle) -> bool {
        if self.key != modes::current_key(cfg) {
            self.reload(cfg);
        }
        if self.windows_read.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
            self.power_mode = opensense_sys::power::power_mode();
            self.plans = opensense_sys::power::plans_without_modes();
            self.boost = opensense_sys::power::cpu_boost();
            self.windows_read = Some(Instant::now());
        }
        let mut changed = false;
        egui::CentralPanel::default().frame(egui::Frame::none().fill(pal().form).inner_margin(5.0)).show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    Vec2::new(LEFT, ui.available_height()),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(LEFT);
                        changed |= self.left_column(ui, cfg, lang);
                    },
                );
                ui.add_space(6.0);
                ui.allocate_ui_with_layout(
                    Vec2::new(ui.available_width(), ui.available_height()),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        changed |= self.fan_column(ui, cfg, lang, worker);
                    },
                );
            });
        });
        self.calibrate_tick(ctx, cfg, lang, worker, &mut changed);
        changed
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Fans are at Max for a calibration run.
    pub fn calibrating(&self) -> bool {
        self.calibration.is_some()
    }

    // ---- left column ------------------------------------------------------------

    fn left_column(&mut self, ui: &mut egui::Ui, cfg: &mut AppConfig, lang: Lang) -> bool {
        let t = |s| tr(lang, s);
        let nvidia = self.nvidia.lock().unwrap().clone().flatten();
        let cells = ui::grid(ui, if nvidia.is_some() { 2 } else { 1 }, 31.0);
        for (i, (tab, label, accent)) in [(Tab::Cpu, "CPU", theme::STANDARD), (Tab::Gpu, "GPU", theme::TURBO)]
            .into_iter()
            .take(cells.len())
            .enumerate()
        {
            let style = ui::TileStyle { icon: None, secondary: true, enabled: true, accent, selected: self.tab == tab };
            if ui::tile(ui, cells[i], label, label, &style).clicked() {
                self.tab = tab;
            }
        }
        if nvidia.is_none() {
            self.tab = Tab::Cpu;
        }
        ui.add_space(6.0);
        match self.tab {
            Tab::Cpu => self.cpu_tab(ui, cfg, lang),
            Tab::Gpu => {
                let (name, max) = nvidia.unwrap_or_default();
                let _ = t;
                self.gpu_tab(ui, cfg, lang, &name, max)
            }
        }
    }

    fn cpu_tab(&mut self, ui: &mut egui::Ui, cfg: &mut AppConfig, lang: Lang) -> bool {
        let t = |s| tr(lang, s);
        let key = self.key.clone();
        let base = modes::base_of(cfg, &key);
        let mut changed = false;
        title_row(ui, Icon::Gauge, t("Windows Power Mode"));
        let saver = opensense_sys::power::energy_saver();
        let current = if saver { PowerMode::BestEfficiency } else { self.power_mode.unwrap_or(PowerMode::Balanced) };
        let mut picked = current;
        let name = |m: PowerMode| match m {
            PowerMode::Plan(_) => {
                self.plans.iter().find(|(p, _)| *p == m).map_or_else(|| t("Power plan").to_owned(), |(_, n)| n.clone())
            }
            _ => t(m.label()).to_owned(),
        };
        ui.add_enabled_ui(!saver, |ui| {
            combo(ui, "power-mode", &name(current), |ui| {
                for m in PowerMode::OVERLAYS {
                    ui.selectable_value(&mut picked, m, name(m));
                }
                if !self.plans.is_empty() {
                    ui.separator();
                }
                for (m, n) in &self.plans {
                    ui.selectable_value(&mut picked, *m, n.as_str());
                }
            });
        });
        if picked != current {
            match opensense_sys::power::set_power_mode(picked) {
                Ok(()) => {
                    self.power_mode = Some(picked);
                    let mode = (picked != modes::default_power_mode(base)).then(|| picked.key().to_owned());
                    edit(cfg, |c| modes::edit_profile(c, &key, |p| p.power_mode.clone_from(&mode)));
                    changed = true;
                }
                Err(e) => self.result = e.to_string(),
            }
        }
        if saver {
            hint(ui, t("Energy saver is on: Windows controls the power mode."));
        } else if matches!(current, PowerMode::Plan(_)) {
            hint(ui, t("This power plan has no power modes. Pick a mode above to switch back to Balanced."));
        }
        ui.add_space(8.0);
        title_row(ui, Icon::Rocket, t("CPU Boost"));
        let boost = self.boost.unwrap_or(1);
        let mut picked = boost;
        combo(ui, "boost", t(BOOST_MODES[usize::from(boost.min(6))]), |ui| {
            for (i, name) in BOOST_MODES.iter().enumerate() {
                ui.selectable_value(&mut picked, i as u8, t(name));
            }
        });
        if picked != boost {
            match opensense_sys::power::set_cpu_boost(picked) {
                Ok(()) => {
                    self.boost = Some(picked);
                    edit(cfg, |c| modes::edit_profile(c, &key, |p| p.cpu_boost = Some(picked)));
                    changed = true;
                }
                Err(e) => self.result = e.to_string(),
            }
        }
        hint(ui, t("Disabling boost lowers temperatures and fan noise at some cost in speed."));
        changed
    }

    fn gpu_tab(&mut self, ui: &mut egui::Ui, cfg: &mut AppConfig, lang: Lang, name: &str, max: u32) -> bool {
        let t = |s| tr(lang, s);
        let key = self.key.clone();
        title_row(ui, Icon::VideoCard, if name.is_empty() { t("GPU Settings") } else { name });
        let limit = modes::profile(cfg, &key).gpu_clock_limit.unwrap_or(crate::nvidia::CLOCK_MAX);
        let value = if limit >= crate::nvidia::CLOCK_MAX { t("Default").to_owned() } else { format!("{limit} MHz") };
        slider_label(ui, t("Core Clock Limit"), &value);
        let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::hover());
        let out = ui::slider(
            ui,
            row.shrink2(Vec2::new(3.0, 0.0)),
            "gpu-clock",
            limit as f32,
            crate::nvidia::CLOCK_MIN as f32,
            crate::nvidia::CLOCK_MAX as f32,
            10.0,
            &[],
            theme::TURBO,
        );
        let mut changed = false;
        if let Some(v) = out.commit {
            let v = v as u32;
            let new = (v < crate::nvidia::CLOCK_MAX).then_some(v);
            edit(cfg, |c| modes::edit_profile(c, &key, |p| p.gpu_clock_limit = new));
            // Forced: the tray agent may have set the lock this window knows
            // nothing about.
            std::thread::spawn(move || crate::nvidia::set_clock_limit(new, true));
            changed = true;
        }
        if max > 0 {
            hint(ui, &format!("{}: {max} MHz", t("Highest supported clock")));
        }
        hint(ui, t("Lower limits reduce GPU heat and fan noise. Needs administrator rights."));
        changed
    }

    // ---- fan column ------------------------------------------------------------

    fn fan_column(&mut self, ui: &mut egui::Ui, cfg: &mut AppConfig, lang: Lang, worker: &WorkerHandle) -> bool {
        let t = |s| tr(lang, s);
        let p = pal();
        let mut changed = false;
        // Header: icon, title, mode selector.
        let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 31.0), Sense::hover());
        crate::gh::icons::paint(ui.painter(), Pos2::new(row.left() + 12.0, row.center().y), 16.0, Icon::Fan, p.fore);
        ui.painter().text(
            Pos2::new(row.left() + 24.0, row.center().y),
            Align2::LEFT_CENTER,
            t("Fan Curves"),
            theme::bold(theme::BODY),
            p.fore,
        );
        let custom = modes::is_custom(&self.key);
        // Before the name field runs: clicking "Done" also takes the focus
        // from it, which already ends the rename this frame.
        let was_renaming = self.renaming;
        // Mode selector (or the name field while renaming) and "+".
        let sel = Rect::from_min_max(Pos2::new(row.right() - 210.0, row.top() + 2.0), row.max - Vec2::new(0.0, 4.0));
        let w = sel.width();
        let cell = |x0: f32, x1: f32| {
            Rect::from_min_max(Pos2::new(sel.left() + x0, sel.top()), Pos2::new(sel.left() + x1, sel.bottom()))
        };
        let combo_rect = cell(0.0, w - 33.0);
        let current_name = modes::display_name(cfg, lang, &self.key);
        let mut picked = self.key.clone();
        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(combo_rect), |ui| {
            if self.renaming && custom {
                let mut name = modes::profile(cfg, &self.key).name;
                let r = ui.add_sized(
                    combo_rect.size(),
                    egui::TextEdit::singleline(&mut name).vertical_align(egui::Align::Center),
                );
                if !r.has_focus() && !r.lost_focus() {
                    r.request_focus();
                }
                if r.changed() {
                    edit(cfg, |c| modes::edit_profile(c, &self.key, |p| p.name.clone_from(&name)));
                    changed = true;
                }
                if r.lost_focus() {
                    self.renaming = false;
                }
            } else {
                egui::ComboBox::from_id_salt("fan-mode")
                    .width(combo_rect.width() - 8.0)
                    .selected_text(current_name)
                    .show_ui(ui, |ui| {
                        for k in modes::ordered(cfg) {
                            let name = modes::display_name(cfg, lang, &k);
                            ui.selectable_value(&mut picked, k, name);
                        }
                    });
            }
        });
        let add_rect = cell(w - 30.0, w - 3.0);
        let mut added = None;
        if modes::can_add(cfg)
            && ui::row_button(ui, add_rect, "add-mode", "+", None, true).on_hover_text(t("Add mode")).clicked()
        {
            // Added and selected in one save, under a key still free in the file.
            edit(cfg, |c| {
                added = modes::add_custom(c, lang);
                if let Some(k) = &added {
                    modes::select(c, k);
                }
            });
        }
        if let Some(k) = added {
            modes::activate(cfg, &k, worker);
            self.reload(cfg);
            changed = true;
        } else if picked != self.key {
            edit(cfg, |c| modes::select(c, &picked));
            modes::activate(cfg, &picked, worker);
            self.reload(cfg);
            changed = true;
        }
        // "Clamp to Grid" shares the Rename / Delete row when there is one.
        let mut clamp_row = None;
        // Custom modes: Rename / Delete under the selector, or the delete
        // question in their place.
        if custom && !self.confirm_remove {
            let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::hover());
            let del = Rect::from_min_max(
                Pos2::new(row.right() - 84.0, row.top() + 2.0),
                Pos2::new(row.right() - 3.0, row.bottom() - 2.0),
            );
            let rename = Rect::from_min_max(
                Pos2::new(row.right() - 210.0, row.top() + 2.0),
                Pos2::new(row.right() - 88.0, row.bottom() - 2.0),
            );
            let label = if self.renaming { t("Done") } else { t("Rename") };
            if ui::row_button(ui, rename, "rename-mode", label, None, true).clicked() {
                self.renaming = !was_renaming;
            }
            if ui::row_button(ui, del, "remove-mode", t("Delete"), None, true).clicked() {
                self.renaming = false;
                self.confirm_remove = true;
            }
            clamp_row = Some(Rect::from_min_max(row.min, Pos2::new(rename.left() - 4.0, row.max.y)));
        }
        if self.confirm_remove && custom {
            let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::hover());
            ui.painter().text(
                Pos2::new(row.left() + 6.0, row.center().y),
                Align2::LEFT_CENTER,
                format!("{} \"{}\"?", t("Delete"), modes::display_name(cfg, lang, &self.key)),
                theme::body(),
                p.fore,
            );
            let del = Rect::from_min_max(
                Pos2::new(row.right() - 210.0, row.top() + 2.0),
                Pos2::new(row.right() - 88.0, row.bottom() - 2.0),
            );
            let cancel = Rect::from_min_max(
                Pos2::new(row.right() - 84.0, row.top() + 2.0),
                Pos2::new(row.right() - 3.0, row.bottom() - 2.0),
            );
            if ui::row_button(ui, del, "confirm-delete", t("Delete"), None, false).clicked() {
                let key = self.key.clone();
                edit(cfg, |c| {
                    modes::remove_custom(c, &key);
                    modes::select(c, "default");
                });
                modes::activate(cfg, "default", worker);
                self.reload(cfg);
                changed = true;
            }
            if ui::row_button(ui, cancel, "cancel-delete", t("Cancel"), None, true).clicked() {
                self.confirm_remove = false;
            }
        }

        // Clamp to Grid.
        let row = clamp_row
            .unwrap_or_else(|| ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover()).0);
        let mut clamp = cfg.fan_clamp;
        if ui::checkbox(ui, row.shrink2(Vec2::new(4.0, 0.0)), "clamp", t("Clamp to Grid"), &mut clamp).changed() {
            edit(cfg, |c| c.fan_clamp = clamp);
            changed = true;
        }

        // Charts.
        let profile = modes::profile(cfg, &self.key);
        let applied = profile.apply_curve;
        let (readings, max_rpm) = {
            let s = worker.shared.lock();
            (s.readings, cfg.fan_max_rpm)
        };
        let bottom_h = 58.0;
        let avail = ui.available_height() - bottom_h;
        let chart_h = (avail / 2.0).max(150.0);
        let mut released = false;
        for (i, (title, accent, temp, rpm)) in [
            (t("CPU Fan Profile"), theme::STANDARD, readings.cpu_temp, readings.cpu_fan_rpm),
            (t("GPU Fan Profile"), theme::TURBO, readings.gpu_temp, readings.gpu_fan_rpm),
        ]
        .into_iter()
        .enumerate()
        {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), chart_h), Sense::hover());
            let color = if applied { accent } else { Color32::from_gray(0x80) };
            let unit = if cfg.temp_unit == opensense_sys::config::TempUnit::Fahrenheit { "°F" } else { "°C" };
            let full_title = format!("{title}, {}/{unit}", if self.percent_axis { "%" } else { "RPM" });
            let r = self.chart(ui, rect.shrink2(Vec2::new(1.0, 5.0)), i, &full_title, color, cfg, temp, rpm, max_rpm);
            released |= r;
        }
        if released {
            // Only this mode's curves are written: a mode switched meanwhile
            // (shortcut, tray menu; reloading pauses during the drag) stays.
            let (cpu, gpu) = (modes::to_stored(&self.curves[0]), modes::to_stored(&self.curves[1]));
            edit(cfg, |c| {
                modes::edit_profile(c, &self.key, |p| {
                    p.cpu_curve.clone_from(&cpu);
                    p.gpu_curve.clone_from(&gpu);
                });
            });
            changed = true;
        }

        // Bottom row.
        let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), bottom_h), Sense::hover());
        let msg = if self.drag.is_some() {
            t("Hold SHIFT to drag all points together").to_owned()
        } else {
            self.result.clone()
        };
        if !msg.is_empty() {
            let color = if self.drag.is_some() { theme::MUTED } else { theme::TURBO };
            ui.painter().text(
                Pos2::new(row.left() + 6.0, row.top() + 9.0),
                Align2::LEFT_CENTER,
                msg,
                theme::body(),
                color,
            );
        }
        let y0 = row.top() + 20.0;
        let factory = Rect::from_min_size(Pos2::new(row.left() + 4.0, y0), Vec2::new(126.0, 27.0));
        let calibrate = Rect::from_min_size(Pos2::new(row.left() + 134.0, y0), Vec2::new(80.0, 27.0));
        if ui::row_button(ui, factory, "factory", t("Factory Defaults"), None, true).clicked() {
            let key = self.key.clone();
            edit(cfg, |c| {
                let base = modes::base_of(c, &key);
                modes::edit_profile(c, &key, |p| {
                    p.cpu_curve = modes::to_stored(&modes::default_curve(base, false));
                    p.gpu_curve = modes::to_stored(&modes::default_curve(base, true));
                    p.apply_curve = false;
                    p.power_mode = None;
                    p.cpu_boost = None;
                    p.gpu_clock_limit = None;
                });
            });
            let (p, base) = (modes::profile(cfg, &key), modes::base_of(cfg, &key));
            let _ = worker.tx.send(Command::FansAuto);
            std::thread::spawn(move || modes::apply_windows(&p, base));
            self.windows_read = None;
            self.result.clear();
            self.reload(cfg);
            changed = true;
        }
        let calibrating = self.calibration.is_some();
        if !calibrating && ui::row_button(ui, calibrate, "calibrate", t("Calibrate"), None, true).clicked() {
            let _ = worker.tx.send(Command::FansMax);
            self.calibration = Some(Calibration { max: [0, 0], same: 0, last_sample: Instant::now() });
        }
        let apply_rect =
            Rect::from_min_max(Pos2::new(row.right() - 180.0, y0 + 1.0), Pos2::new(row.right() - 4.0, y0 + 26.0));
        ui.painter().rect_filled(apply_rect, ui::ROW_RADIUS, p.button2);
        let mut apply = applied;
        if ui::checkbox(ui, apply_rect.shrink2(Vec2::new(6.0, 0.0)), "apply-curve", t("Apply Fan Curve"), &mut apply)
            .changed()
        {
            let key = self.key.clone();
            edit(cfg, |c| modes::edit_profile(c, &key, |p| p.apply_curve = apply));
            if !apply {
                let _ = worker.tx.send(crate::apply::fan(&cfg.fan));
            }
            changed = true;
        }
        changed
    }

    /// One fan-curve chart. Returns true when a drag ended (save time).
    #[allow(clippy::too_many_arguments)]
    fn chart(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        index: usize,
        title: &str,
        color: Color32,
        cfg: &AppConfig,
        temp: Option<u16>,
        rpm: Option<u16>,
        max_rpm: Option<[u16; 2]>,
    ) -> bool {
        let p = pal();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, p.chart);
        let plot = Rect::from_min_max(
            Pos2::new(rect.left() + 46.0, rect.top() + 20.0),
            Pos2::new(rect.right() - 10.0, rect.bottom() - 18.0),
        );
        painter.text(Pos2::new(plot.center().x, rect.top() + 9.0), Align2::CENTER_CENTER, title, theme::body(), p.fore);
        let to_screen = |x: f32, y: f32| {
            Pos2::new(plot.left() + (x - 20.0) / 90.0 * plot.width(), plot.bottom() - y / 100.0 * plot.height())
        };
        let to_data = |pos: Pos2| {
            ((pos.x - plot.left()) / plot.width() * 90.0 + 20.0, (plot.bottom() - pos.y) / plot.height() * 100.0)
        };
        let small = egui::FontId::proportional(9.5);
        let fahrenheit = cfg.temp_unit == opensense_sys::config::TempUnit::Fahrenheit;
        for i in 0..=9 {
            let x = 20.0 + 10.0 * i as f32;
            let s = to_screen(x, 0.0);
            painter.line_segment(
                [Pos2::new(s.x, plot.top()), Pos2::new(s.x, plot.bottom())],
                Stroke::new(1.0_f32, p.grid),
            );
            let label = if fahrenheit { format!("{:.0}", x * 9.0 / 5.0 + 32.0) } else { format!("{x:.0}") };
            painter.text(Pos2::new(s.x, plot.bottom() + 8.0), Align2::CENTER_CENTER, label, small.clone(), p.fore);
        }
        let max = max_rpm.map(|m| m[index]).filter(|&m| m > 3000);
        for i in 0..=10 {
            let y = 10.0 * i as f32;
            let s = to_screen(20.0, y);
            painter.line_segment(
                [Pos2::new(plot.left(), s.y), Pos2::new(plot.right(), s.y)],
                Stroke::new(1.0_f32, p.grid),
            );
            let label = self.y_label(y as u8, max);
            painter.text(Pos2::new(plot.left() - 4.0, s.y), Align2::RIGHT_CENTER, label, small.clone(), p.fore);
        }
        // Click on the Y labels toggles RPM / percent.
        let axis = Rect::from_min_max(Pos2::new(rect.left(), plot.top()), Pos2::new(plot.left(), plot.bottom()));
        if ui
            .interact(axis, ui.id().with(("axis", index)), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            self.percent_axis = !self.percent_axis;
        }
        // Firmware floor (~2500 RPM, i.e. 0 %): a faint band along the bottom.
        let floor = Rect::from_min_max(to_screen(20.0, 3.0), to_screen(110.0, 0.0));
        painter.rect_filled(floor, 0.0, color.gamma_multiply(0.25));
        // Live position.
        if let Some(t) = temp.map(f32::from).filter(|t| (20.0..=110.0).contains(t)) {
            let x = to_screen(t, 0.0).x;
            painter.line_segment(
                [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
                Stroke::new(1.0_f32, color.gamma_multiply(0.6)),
            );
            let d = modes::duty(&self.curves[index], t);
            painter.circle_filled(to_screen(t, d), 3.0, p.fore);
            let live = match rpm {
                Some(r) => format!("{t:.0}°  {r} RPM"),
                None => format!("{t:.0}°"),
            };
            // In the title row, clear of the curve's top points.
            painter.text(Pos2::new(plot.right(), rect.top() + 9.0), Align2::RIGHT_CENTER, live, small.clone(), color);
        }
        // Curve.
        let pts: Vec<Pos2> = self.curves[index].iter().map(|c| to_screen(c[0], c[1])).collect();
        painter.add(Shape::line(pts.clone(), Stroke::new(2.0_f32, color)));
        for &pt in &pts {
            painter.circle_filled(pt, 5.0, color);
        }
        // Interaction.
        let resp = ui.interact(plot.expand(6.0), ui.id().with(("chart", index)), Sense::click_and_drag());
        let pointer = resp.interact_pointer_pos().or(resp.hover_pos());
        let near = pointer.and_then(|pos| pts.iter().position(|pt| pt.distance(pos) <= 8.0));
        if resp.drag_started() {
            if let Some(k) = near {
                self.drag = Some((index, k));
            }
        }
        let mut released = false;
        if let (Some((ci, k)), Some(pos)) = (self.drag, resp.interact_pointer_pos()) {
            if ci == index && resp.dragged() {
                let (x, y) = to_data(pos);
                if ui.input(|i| i.modifiers.shift) {
                    let dy = y - self.curves[index][k][1];
                    for pt in self.curves[index].iter_mut() {
                        pt[1] = (pt[1] + dy).clamp(0.0, 100.0);
                    }
                    let c = &mut self.curves[index];
                    let lo = 30.0 + 10.0 * k as f32;
                    c[k][0] = if cfg.fan_clamp { x.clamp(lo, lo + 9.0) } else { x.clamp(20.0, 110.0) };
                } else {
                    modes::move_point(&mut self.curves[index], k, x, y, cfg.fan_clamp);
                }
                let pt = self.curves[index][k];
                let tip =
                    format!("{:.0}{}, {}", pt[0], if fahrenheit { "°F" } else { "°C" }, self.y_label(pt[1] as u8, max));
                let at = to_screen(pt[0], pt[1]);
                let r = painter.text(at + Vec2::new(8.0, -10.0), Align2::LEFT_BOTTOM, &tip, theme::body(), p.fore);
                painter.rect_filled(r.expand(2.0), 2.0, p.button);
                painter.text(at + Vec2::new(8.0, -10.0), Align2::LEFT_BOTTOM, tip, theme::body(), p.fore);
            }
        }
        if resp.drag_stopped() && self.drag.is_some_and(|(ci, _)| ci == index) {
            self.drag = None;
            released = true;
        }
        if near.is_some() || self.drag.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        released
    }

    fn y_label(&self, p: u8, max: Option<u16>) -> String {
        if self.percent_axis {
            return format!("{p}%");
        }
        let rpm = match max {
            // Calibrated: scale the measured curve to the measured maximum.
            Some(m) => modes::approx_rpm(p) as f32 * f32::from(m) / modes::approx_rpm(100) as f32,
            None => modes::approx_rpm(p) as f32,
        };
        format!("{}", (rpm / 100.0).round() as u32 * 100)
    }

    /// Calibration: fans at Max, sample every second, stop after 15 s with
    /// no new maximum and store the result.
    fn calibrate_tick(
        &mut self,
        ctx: &egui::Context,
        cfg: &mut AppConfig,
        lang: Lang,
        worker: &WorkerHandle,
        changed: &mut bool,
    ) {
        let Some(c) = self.calibration.as_mut() else { return };
        ctx.request_repaint_after(Duration::from_millis(250));
        if c.last_sample.elapsed() < Duration::from_secs(1) {
            return;
        }
        c.last_sample = Instant::now();
        let r = worker.shared.lock().readings;
        let mut grew = false;
        for (i, v) in [r.cpu_fan_rpm, r.gpu_fan_rpm].into_iter().enumerate() {
            if let Some(v) = v.filter(|&v| v > c.max[i]) {
                c.max[i] = v;
                grew = true;
            }
        }
        c.same = if grew { 0 } else { c.same + 1 };
        self.result = format!(
            "{} - CPU: {}, GPU: {} ({}s)",
            tr(lang, "Measuring Max Speed"),
            c.max[0],
            c.max[1],
            CALIBRATE_PLATEAU.saturating_sub(c.same)
        );
        if c.same >= CALIBRATE_PLATEAU {
            let max = c.max;
            self.calibration = None;
            if max.iter().all(|&m| (3000..=10400).contains(&m)) {
                edit(cfg, |c| c.fan_max_rpm = Some(max));
                *changed = true;
            }
            self.result = format!("{} - CPU: {}, GPU: {}", tr(lang, "Measured"), max[0], max[1]);
            let key = modes::current_key(cfg);
            for cmd in modes::commands(cfg, &key) {
                let _ = worker.tx.send(cmd);
            }
        }
    }
}

// ---- small helpers ------------------------------------------------------------------

/// Saves the fields `f` sets and refreshes `cfg` from the file (see
/// [`crate::tray::edit_config`]).
fn edit(cfg: &mut AppConfig, f: impl FnMut(&mut AppConfig)) {
    if let Err(e) = crate::tray::edit_config(cfg, f) {
        tracing::warn!("cannot save settings: {e}");
    }
}

fn title_row(ui: &mut egui::Ui, icon: Icon, text: &str) {
    let p = pal();
    let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), Sense::hover());
    crate::gh::icons::paint(ui.painter(), Pos2::new(row.left() + 13.0, row.center().y + 2.0), 16.0, icon, p.fore);
    ui.painter().text(
        Pos2::new(row.left() + 25.0, row.center().y + 2.0),
        Align2::LEFT_CENTER,
        text,
        theme::bold(theme::BODY),
        p.fore,
    );
}

fn slider_label(ui: &mut egui::Ui, left: &str, right: &str) {
    let p = pal();
    let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::hover());
    ui.painter().text(Pos2::new(row.left() + 5.0, row.center().y), Align2::LEFT_CENTER, left, theme::body(), p.fore);
    ui.painter().text(
        Pos2::new(row.right() - 5.0, row.center().y),
        Align2::RIGHT_CENTER,
        right,
        theme::bold(theme::BODY),
        p.fore,
    );
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.horizontal_wrapped(|ui| {
        ui.add_space(5.0);
        ui.set_max_width(LEFT - 10.0);
        ui.label(egui::RichText::new(text).size(11.0).color(theme::TIP));
    });
}

fn combo(ui: &mut egui::Ui, id: &str, selected: &str, body: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        egui::ComboBox::from_id_salt(id).width(LEFT - 24.0).selected_text(selected).show_ui(ui, body);
    });
    ui.add_space(4.0);
}

// ---- process ------------------------------------------------------------------------

/// The window as its own process, started from the main window. Settings
/// are shared through the config file.
pub struct FansApp {
    window: FansWindow,
    cfg: AppConfig,
    lang: Lang,
    worker: WorkerHandle,
    dark: bool,
    modified: Option<std::time::SystemTime>,
    checked: Instant,
    marked: bool,
    close_pending: bool,
    /// Closes the window when the user switches to another app.
    away: crate::tray::AwayWatch,
}

pub fn config_mtime() -> Option<std::time::SystemTime> {
    AppConfig::path().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok())
}

impl FansApp {
    pub fn new(cc: &eframe::CreationContext<'_>, transport: opensense_hw::TransportKind) -> Self {
        let cfg = AppConfig::load();
        let dark = theme::is_dark(&cfg.ui_mode);
        theme::install_fonts(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx, dark);
        let oem = opensense_sys::oem::OemInfo::detect();
        let ctx = cc.egui_ctx.clone();
        let worker =
            crate::worker::spawn(transport, &oem, Duration::from_millis(1000), 4, false, move || ctx.request_repaint());
        Self {
            window: FansWindow::new(),
            lang: Lang::resolve(&cfg.language),
            cfg,
            worker,
            dark,
            modified: config_mtime(),
            checked: Instant::now(),
            marked: false,
            close_pending: false,
            away: crate::tray::AwayWatch::default(),
        }
    }
}

impl eframe::App for FansApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        raw_input.max_texture_side = Some(raw_input.max_texture_side.unwrap_or(2048).min(2048));
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        pal().form.to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        crate::tray::keep_tray_only();
        if !self.marked {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.window.title.clone()));
            if let Some(hwnd) = crate::tray::find_own_window(&self.window.title) {
                crate::panel::set_prop(hwnd, MARK_PROP, 1);
                crate::tray::tray_only(hwnd);
                theme::title_bar(&self.window.title, self.dark);
                self.marked = true;
            }
        }
        // A calibration has the fans at Max: the window stays until it has
        // finished and handed them back.
        let busy = self.window.calibrating();
        crate::panel::hold_close(ctx, busy, &mut self.close_pending);
        // Like a flyout: switching to another app closes the window.
        if busy {
            self.away.reset();
        } else if self.away.gone(ctx) && !self.cfg.topmost {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !busy {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Follow changes made by the main window or the tray menu. Not while
        // a curve point is dragged: a mode switched meanwhile would swap the
        // curves being edited. The save on release writes only those curves,
        // so the switch is kept and shown right after.
        if self.checked.elapsed() > Duration::from_millis(200) && !self.window.dragging() {
            self.checked = Instant::now();
            let m = config_mtime();
            if m != self.modified {
                self.modified = m;
                self.cfg = AppConfig::load();
                self.lang = Lang::resolve(&self.cfg.language);
            }
            // Theme from the settings or, on "Windows", the system.
            theme::follow(ctx, &self.cfg.ui_mode, &mut self.dark, MARK_PROP);
        }
        // Changes are saved as they are made; `cfg` is then the file's
        // current settings.
        if self.window.show(ctx, &mut self.cfg, self.lang, &self.worker) {
            self.modified = config_mtime();
            self.lang = Lang::resolve(&self.cfg.language);
            crate::tray::notify_config_changed();
        }
        ctx.request_repaint_after(Duration::from_millis(200));
    }
}
