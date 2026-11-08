//! "Updates" window (`opensense --updates`): OpenSense's own version
//! against the latest GitHub release with a one-click update, plus the
//! BIOS and graphics driver versions with a link to Acer's support site.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui::{self, Align2, Pos2, Rect, Sense, Vec2};
use opensense_sys::config::AppConfig;

use crate::gh::icons::Icon;
use crate::gh::theme::{self, pal};
use crate::gh::ui;
use crate::i18n::{tr, Lang};
use crate::updates::{self, Release};

pub const MARK_PROP: &str = "OpenSense.Updates";
/// Window title in the saved language.
pub fn title() -> &'static str {
    tr(crate::i18n::ui_lang(), "Updates")
}
pub const WIDTH: f32 = 560.0;
const ACER_SUPPORT: &str = "https://www.acer.com/support";

#[derive(Clone)]
enum State {
    Checking,
    Offline,
    Latest(Release),
    Installing,
    Installed(String),
    Failed(String),
}

pub struct UpdatesApp {
    lang: Lang,
    dark: bool,
    marked: bool,
    /// Closes the window when the user switches to another app.
    away: crate::tray::AwayWatch,
    state: Arc<Mutex<State>>,
    bios: Option<(String, String)>,
    nvidia: Arc<Mutex<Option<String>>>,
    model: String,
}

impl UpdatesApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let cfg = AppConfig::load();
        let dark = theme::is_dark(&cfg.ui_mode);
        theme::install_fonts(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx, dark);
        let state = Arc::new(Mutex::new(State::Checking));
        let nvidia = Arc::new(Mutex::new(None));
        let (s, n, ctx) = (state.clone(), nvidia.clone(), cc.egui_ctx.clone());
        std::thread::spawn(move || {
            let r = updates::latest();
            *s.lock().unwrap() = r.clone().map_or(State::Offline, State::Latest);
            if let Some(r) = r {
                let mut cfg = AppConfig::load();
                cfg.latest_version = Some(r.version);
                cfg.update_checked = Some(updates::now_secs());
                let _ = cfg.save();
            }
            *n.lock().unwrap() = updates::nvidia_driver();
            ctx.request_repaint();
        });
        Self {
            lang: Lang::resolve(&cfg.language),
            dark,
            marked: false,
            away: crate::tray::AwayWatch::default(),
            state,
            bios: updates::bios(),
            nvidia,
            model: opensense_sys::oem::OemInfo::detect()
                .model
                .map(|m| crate::panel::model_title(&m))
                .unwrap_or_default(),
        }
    }

    fn t(&self, s: &'static str) -> &'static str {
        tr(self.lang, s)
    }
}

impl eframe::App for UpdatesApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        pal().form.to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.marked {
            if let Some(hwnd) = crate::tray::find_own_window(tr(self.lang, "Updates")) {
                crate::panel::set_prop(hwnd, MARK_PROP, 1);
                crate::tray::tray_only(hwnd);
                theme::title_bar_of(hwnd, self.dark);
                self.marked = true;
            }
        }
        // Like a flyout: switching to another app closes the window.
        if self.away.gone(ctx) && !matches!(*self.state.lock().unwrap(), State::Installing) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        egui::CentralPanel::default().frame(egui::Frame::none().fill(pal().form).inner_margin(5.0)).show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(0.0, 2.0);
            self.app_section(ui);
            ui.add_space(10.0);
            self.drivers_section(ui);
        });
        ctx.request_repaint_after(Duration::from_millis(500));
    }
}

impl UpdatesApp {
    fn app_section(&mut self, ui: &mut egui::Ui) {
        header(ui, Icon::Download, "OpenSense");
        columns(ui, self.t("Name"), self.t("Installed"), self.t("Available"));
        let current = env!("CARGO_PKG_VERSION");
        let state = self.state.lock().unwrap().clone();
        let (available, color) = match &state {
            State::Checking => ("…".to_owned(), pal().fore),
            State::Offline => (self.t("Can't reach GitHub").to_owned(), theme::MUTED),
            State::Latest(r) if updates::is_newer(&r.version, current) => (r.version.clone(), theme::TURBO),
            State::Latest(r) => (r.version.clone(), theme::ECO),
            State::Installing => (self.t("Installing…").to_owned(), pal().fore),
            State::Installed(v) => (v.clone(), theme::ECO),
            State::Failed(_) => ("—".into(), theme::TURBO),
        };
        row(ui, "OpenSense", current, &available, color);
        let (bar, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::hover());
        let button = Rect::from_min_max(
            Pos2::new(bar.right() - 200.0, bar.top() + 4.0),
            Pos2::new(bar.right() - 6.0, bar.bottom() - 4.0),
        );
        let status_pos = Pos2::new(bar.left() + 10.0, bar.center().y);
        match state {
            State::Latest(r) if updates::is_newer(&r.version, current) => {
                if let Some(zip) = r.zip.clone() {
                    if ui::row_button(
                        ui,
                        button,
                        "install",
                        self.t("Download and install"),
                        Some(Icon::Download),
                        false,
                    )
                    .clicked()
                    {
                        let s = self.state.clone();
                        let version = r.version.clone();
                        *s.lock().unwrap() = State::Installing;
                        std::thread::spawn(move || {
                            *s.lock().unwrap() = match updates::install(&zip) {
                                Ok(()) => State::Installed(version),
                                Err(e) => State::Failed(e),
                            };
                        });
                    }
                } else if ui::row_button(ui, button, "page", self.t("Open release page"), None, false).clicked() {
                    crate::panel::open_url(&r.page);
                }
            }
            State::Latest(_) => {
                ui.painter().text(
                    status_pos,
                    Align2::LEFT_CENTER,
                    self.t("OpenSense is up to date."),
                    theme::body(),
                    theme::ECO,
                );
            }
            State::Installed(_) => {
                ui.painter().text(
                    status_pos,
                    Align2::LEFT_CENTER,
                    self.t("Update installed."),
                    theme::body(),
                    theme::ECO,
                );
                if ui::row_button(ui, button, "restart", self.t("Restart OpenSense"), None, false).clicked() {
                    crate::tray::request_quit();
                    std::thread::sleep(Duration::from_millis(600));
                    crate::tray::spawn_detached(&[]);
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    crate::panel::close_main_window();
                }
            }
            State::Failed(e) => {
                ui.painter().text(
                    status_pos,
                    Align2::LEFT_CENTER,
                    format!("{}: {e}", self.t("Update failed")),
                    theme::body(),
                    theme::TURBO,
                );
            }
            _ => {}
        }
    }

    fn drivers_section(&mut self, ui: &mut egui::Ui) {
        header(ui, Icon::VideoCard, self.t("BIOS and Drivers"));
        columns(ui, self.t("Name"), self.t("Installed"), "");
        let fore = pal().fore;
        if let Some((v, date)) = self.bios.clone() {
            row(ui, "BIOS", &v, &date, theme::MUTED);
        }
        let nv = self.nvidia.lock().unwrap().clone();
        if let Some(v) = nv {
            row(ui, self.t("NVIDIA graphics driver"), &v, "", fore);
        }
        if !self.model.is_empty() {
            row(ui, self.t("Model"), &self.model, "", fore);
        }
        let (bar, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::hover());
        let button = Rect::from_min_max(
            Pos2::new(bar.right() - 200.0, bar.top() + 4.0),
            Pos2::new(bar.right() - 6.0, bar.bottom() - 4.0),
        );
        ui.painter().text(
            Pos2::new(bar.left() + 10.0, bar.center().y),
            Align2::LEFT_CENTER,
            self.t("Search for your model on Acer's site."),
            theme::body(),
            theme::MUTED,
        );
        if ui::row_button(ui, button, "acer", self.t("Acer Drivers and Manuals"), None, true).clicked() {
            crate::panel::open_url(ACER_SUPPORT);
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

fn columns(ui: &mut egui::Ui, a: &str, b: &str, c: &str) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
    let x = col_x(r);
    for (text, x) in [(a, x[0]), (b, x[1]), (c, x[2])] {
        ui.painter().text(
            Pos2::new(x, r.center().y),
            Align2::LEFT_CENTER,
            text,
            theme::bold(theme::BODY),
            theme::MUTED,
        );
    }
}

fn row(ui: &mut egui::Ui, name: &str, installed: &str, available: &str, color: egui::Color32) {
    let p = pal();
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
    let x = col_x(r);
    ui.painter().text(Pos2::new(x[0], r.center().y), Align2::LEFT_CENTER, name, theme::body(), p.fore);
    ui.painter().text(Pos2::new(x[1], r.center().y), Align2::LEFT_CENTER, installed, theme::body(), p.fore);
    ui.painter().text(Pos2::new(x[2], r.center().y), Align2::LEFT_CENTER, available, theme::body(), color);
}

fn col_x(r: Rect) -> [f32; 3] {
    [r.left() + 10.0, r.left() + r.width() * 0.45, r.left() + r.width() * 0.70]
}
