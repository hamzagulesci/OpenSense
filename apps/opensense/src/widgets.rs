//! Theme palette and reusable painted widgets: cards, gauges, the backlit
//! keyboard preview, swatches.

use std::f32::consts::PI;
use std::sync::RwLock;

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use crate::app::ACCENT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Oled,
    Light,
}

impl Theme {
    pub fn from_name(s: &str) -> Self {
        match s {
            "oled" => Theme::Oled,
            "light" => Theme::Light,
            _ => Theme::Dark,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Theme::Dark => "dark",
            Theme::Oled => "oled",
            Theme::Light => "light",
        }
    }
}

/// Colours that depend on the theme.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub panel: Color32,
    pub card: Color32,
    pub border: Color32,
    pub selected_card: Color32,
    pub track: Color32,
    pub text: Color32,
    pub weak: Color32,
}

const DARK: Palette = Palette {
    panel: Color32::from_rgb(0x15, 0x16, 0x1A),
    card: Color32::from_rgb(0x1E, 0x20, 0x26),
    border: Color32::from_rgb(0x2A, 0x2C, 0x33),
    selected_card: Color32::from_rgb(0x2A, 0x16, 0x19),
    track: Color32::from_rgb(0x30, 0x32, 0x3A),
    text: Color32::from_gray(225),
    weak: Color32::from_gray(140),
};

const OLED: Palette = Palette {
    panel: Color32::BLACK,
    card: Color32::from_rgb(0x0B, 0x0B, 0x0D),
    border: Color32::from_rgb(0x1E, 0x1F, 0x24),
    selected_card: Color32::from_rgb(0x1C, 0x05, 0x08),
    track: Color32::from_rgb(0x22, 0x23, 0x28),
    text: Color32::from_gray(230),
    weak: Color32::from_gray(130),
};

const LIGHT: Palette = Palette {
    panel: Color32::from_rgb(0xF3, 0xF4, 0xF6),
    card: Color32::WHITE,
    border: Color32::from_rgb(0xDA, 0xDC, 0xE0),
    selected_card: Color32::from_rgb(0xFD, 0xEC, 0xEE),
    track: Color32::from_rgb(0xE3, 0xE5, 0xE8),
    text: Color32::from_gray(25),
    weak: Color32::from_gray(110),
};

static PALETTE: RwLock<Palette> = RwLock::new(DARK);

pub fn pal() -> Palette {
    *PALETTE.read().unwrap()
}

/// Ubuntu for text with Hack as fallback (arrows, triangles). egui's emoji
/// fonts are dropped: they alone cost ~50 MB of memory, and every icon in
/// OpenSense is painted instead.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.retain(|name, _| name == "Ubuntu-Light" || name == "Hack");
    fonts.families.insert(egui::FontFamily::Proportional, vec!["Ubuntu-Light".into(), "Hack".into()]);
    fonts.families.insert(egui::FontFamily::Monospace, vec!["Hack".into(), "Ubuntu-Light".into()]);
    ctx.set_fonts(fonts);
}

/// Painted navigation icons.
#[derive(Clone, Copy)]
pub enum Icon {
    Monitor,
    Fan,
    Lighting,
    Settings,
    About,
}

pub fn paint_icon(p: &egui::Painter, c: Pos2, icon: Icon, color: Color32) {
    let s = Stroke::new(1.6_f32, color);
    match icon {
        Icon::Monitor => {
            // Gauge: 3/4 arc with a needle.
            let pts: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let a = 0.75 * PI + 1.5 * PI * i as f32 / 20.0;
                    c + 7.0 * Vec2::new(a.cos(), a.sin())
                })
                .collect();
            p.add(egui::Shape::line(pts, s));
            p.line_segment([c, c + Vec2::new(3.5, -4.0)], s);
            p.circle_filled(c, 1.6, color);
        }
        Icon::Fan => {
            for k in 0..3 {
                let a = k as f32 * std::f32::consts::TAU / 3.0;
                let d = Vec2::new(a.cos(), a.sin());
                let n = Vec2::new(-d.y, d.x);
                let tip = c + d * 7.0;
                p.add(egui::Shape::convex_polygon(
                    vec![c + n * 1.2, tip + n * 2.6, tip - n * 0.6, c - n * 1.0],
                    color,
                    Stroke::NONE,
                ));
            }
            p.circle_filled(c, 2.0, color);
        }
        Icon::Lighting => {
            // Keyboard: outlined body with key dots.
            let r = Rect::from_center_size(c, Vec2::new(16.0, 10.0));
            p.rect_stroke(r, 2.0, s);
            for row in 0..2 {
                for col in 0..4 {
                    let k = Pos2::new(r.left() + 3.5 + col as f32 * 3.0, r.top() + 3.2 + row as f32 * 3.4);
                    p.circle_filled(k, 0.9, color);
                }
            }
        }
        Icon::Settings => {
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::TAU / 8.0;
                let d = Vec2::new(a.cos(), a.sin());
                p.line_segment([c + d * 5.0, c + d * 7.5], Stroke::new(2.4_f32, color));
            }
            p.circle_stroke(c, 5.0, s);
            p.circle_stroke(c, 2.0, s);
        }
        Icon::About => {
            p.circle_stroke(c, 7.0, s);
            p.circle_filled(c + Vec2::new(0.0, -3.4), 1.1, color);
            p.line_segment([c + Vec2::new(0.0, -0.8), c + Vec2::new(0.0, 4.0)], s);
        }
    }
}

pub fn apply_theme(ctx: &egui::Context, theme: Theme) {
    let p = match theme {
        Theme::Dark => DARK,
        Theme::Oled => OLED,
        Theme::Light => LIGHT,
    };
    *PALETTE.write().unwrap() = p;
    let mut v = if theme == Theme::Light { egui::Visuals::light() } else { egui::Visuals::dark() };
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.panel_fill = p.panel;
    v.window_fill = p.card;
    v.extreme_bg_color = if theme == Theme::Light { Color32::WHITE } else { p.panel };
    v.faint_bg_color = p.card;
    if theme == Theme::Oled {
        v.widgets.inactive.bg_fill = Color32::from_rgb(0x16, 0x16, 0x19);
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x16, 0x16, 0x19);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    }
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.slider_width = 260.0;
    });
}

pub fn card(ui: &mut egui::Ui, title: &str, color: Color32, body: impl FnOnce(&mut egui::Ui)) {
    let p = pal();
    egui::Frame::none().fill(p.card).rounding(10.0).stroke(Stroke::new(1.0_f32, p.border)).inner_margin(12.0).show(
        ui,
        |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).strong().color(color));
            body(ui);
        },
    );
}

/// A clickable option card with a title and a short description.
pub fn select_card(ui: &mut egui::Ui, selected: bool, enabled: bool, title: &str, desc: &str) -> bool {
    let p = pal();
    let stroke = if selected { Stroke::new(2.0_f32, ACCENT) } else { Stroke::new(1.0_f32, p.border) };
    let fill = if selected { p.selected_card } else { p.card };
    let alpha = if enabled { 1.0 } else { 0.4 };
    let inner = egui::Frame::none().fill(fill).rounding(10.0).stroke(stroke).inner_margin(10.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(72.0);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
            ui.painter().circle_stroke(r.center(), 5.5, Stroke::new(1.5_f32, if selected { ACCENT } else { p.weak }));
            if selected {
                ui.painter().circle_filled(r.center(), 3.0, ACCENT);
            }
            ui.label(RichText::new(title).size(15.0).strong().color(p.text.gamma_multiply(alpha)));
        });
        ui.label(RichText::new(desc).small().color(p.weak.gamma_multiply(alpha)));
    });
    let resp = inner.response.interact(if enabled { Sense::click() } else { Sense::hover() });
    if enabled && resp.hovered() && !selected {
        ui.painter().rect_stroke(resp.rect, 10.0, Stroke::new(1.0_f32, ACCENT.gamma_multiply(0.6)));
    }
    enabled && resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

pub fn notice(ui: &mut egui::Ui, color: Color32, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(color.gamma_multiply(0.12))
        .stroke(Stroke::new(1.0_f32, color.gamma_multiply(0.6)))
        .rounding(8.0)
        .inner_margin(8.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                // Painted warning triangle.
                let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                let c = r.center();
                ui.painter().add(egui::Shape::convex_polygon(
                    vec![c + Vec2::new(0.0, -6.5), c + Vec2::new(7.0, 5.5), c + Vec2::new(-7.0, 5.5)],
                    color,
                    Stroke::NONE,
                ));
                ui.painter().line_segment(
                    [c + Vec2::new(0.0, -2.5), c + Vec2::new(0.0, 1.5)],
                    Stroke::new(1.6_f32, Color32::BLACK),
                );
                ui.painter().circle_filled(c + Vec2::new(0.0, 3.6), 0.9, Color32::BLACK);
                body(ui);
            });
        });
}

pub fn big(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(34.0).strong());
}

pub fn kv(ui: &mut egui::Ui, k: &str, v: String) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(k).weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(v));
    });
}

/// 270° arc gauge with a value and unit in the middle.
pub fn gauge(ui: &mut egui::Ui, size: f32, frac: f32, color: Color32, value: &str, unit: &str) {
    let pl = pal();
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    let r = size * 0.42;
    let arc = |from: f32, to: f32| -> Vec<Pos2> {
        let n = 48;
        (0..=n)
            .map(|i| {
                let a = from + (to - from) * i as f32 / n as f32;
                Pos2::new(c.x + r * a.cos(), c.y + r * a.sin())
            })
            .collect()
    };
    let start = 0.75 * PI;
    let end = 2.25 * PI;
    p.add(egui::Shape::line(arc(start, end), Stroke::new(8.0_f32, pl.track)));
    let frac = frac.clamp(0.0, 1.0);
    if frac > 0.0 {
        p.add(egui::Shape::line(arc(start, start + (end - start) * frac), Stroke::new(8.0_f32, color)));
    }
    p.text(
        c - Vec2::new(0.0, 6.0),
        egui::Align2::CENTER_CENTER,
        value,
        egui::FontId::proportional(size * 0.2),
        pl.text,
    );
    p.text(
        c + Vec2::new(0.0, size * 0.16),
        egui::Align2::CENTER_CENTER,
        unit,
        egui::FontId::proportional(size * 0.11),
        pl.weak,
    );
}

// ---- keyboard ------------------------------------------------------------------

/// One key: legend, width in key units and rows spanned (0 = empty slot
/// covered by a taller key from the row above).
type Key = (&'static str, f32, u8);

/// Main block of a Turkish-Q Acer Nitro keyboard, 15 units per row.
const MAIN: [&[Key]; 6] = [
    &[
        ("Esc", 0.9375, 1),
        ("F1", 0.9375, 1),
        ("F2", 0.9375, 1),
        ("F3", 0.9375, 1),
        ("F4", 0.9375, 1),
        ("F5", 0.9375, 1),
        ("F6", 0.9375, 1),
        ("F7", 0.9375, 1),
        ("F8", 0.9375, 1),
        ("F9", 0.9375, 1),
        ("F10", 0.9375, 1),
        ("F11", 0.9375, 1),
        ("F12", 0.9375, 1),
        ("PrtSc", 0.9375, 1),
        ("Ins", 0.9375, 1),
        ("Del", 0.9375, 1),
    ],
    &[
        ("\"", 1.0, 1),
        ("1", 1.0, 1),
        ("2", 1.0, 1),
        ("3", 1.0, 1),
        ("4", 1.0, 1),
        ("5", 1.0, 1),
        ("6", 1.0, 1),
        ("7", 1.0, 1),
        ("8", 1.0, 1),
        ("9", 1.0, 1),
        ("0", 1.0, 1),
        ("*", 1.0, 1),
        ("-", 1.0, 1),
        ("Bksp", 2.0, 1),
    ],
    &[
        ("Tab", 1.75, 1),
        ("Q", 1.0, 1),
        ("W", 1.0, 1),
        ("E", 1.0, 1),
        ("R", 1.0, 1),
        ("T", 1.0, 1),
        ("Y", 1.0, 1),
        ("U", 1.0, 1),
        ("I", 1.0, 1),
        ("O", 1.0, 1),
        ("P", 1.0, 1),
        ("Ğ", 1.0, 1),
        ("Ü", 1.0, 1),
        ("Enter", 1.25, 2),
    ],
    &[
        ("Caps Lock", 1.75, 1),
        ("A", 1.0, 1),
        ("S", 1.0, 1),
        ("D", 1.0, 1),
        ("F", 1.0, 1),
        ("G", 1.0, 1),
        ("H", 1.0, 1),
        ("J", 1.0, 1),
        ("K", 1.0, 1),
        ("L", 1.0, 1),
        ("Ş", 1.0, 1),
        ("İ", 1.0, 1),
        (",", 1.0, 1),
        ("", 1.25, 0),
    ],
    &[
        ("Shift", 1.25, 1),
        ("<", 1.0, 1),
        ("Z", 1.0, 1),
        ("X", 1.0, 1),
        ("C", 1.0, 1),
        ("V", 1.0, 1),
        ("B", 1.0, 1),
        ("N", 1.0, 1),
        ("M", 1.0, 1),
        ("Ö", 1.0, 1),
        ("Ç", 1.0, 1),
        (".", 1.0, 1),
        ("Shift", 1.75, 1),
        ("↑", 1.0, 1),
    ],
    &[
        ("Ctrl", 1.0, 1),
        ("Fn", 1.0, 1),
        ("Win", 1.0, 1),
        ("Alt", 1.0, 1),
        ("", 5.0, 1),
        ("Alt Gr", 1.0, 1),
        ("Menu", 1.0, 1),
        ("Ctrl", 2.0, 1),
        ("←", 1.0, 1),
        ("↓", 1.0, 1),
    ],
];

/// Numpad, 4 units wide; the right arrow sits in its bottom-left slot and
/// the Enter key spans two rows, as on the real keyboard.
const NUMPAD: [&[Key]; 6] = [
    &[("|◀◀", 1.0, 1), ("▶‖", 1.0, 1), ("▶▶|", 1.0, 1), ("Pwr", 1.0, 1)],
    &[("[N]", 1.0, 1), ("NumLk", 1.0, 1), ("/", 1.0, 1), ("*", 1.0, 1)],
    &[("7", 1.0, 1), ("8", 1.0, 1), ("9", 1.0, 1), ("-", 1.0, 1)],
    &[("4", 1.0, 1), ("5", 1.0, 1), ("6", 1.0, 1), ("+", 1.0, 1)],
    &[("1", 1.0, 1), ("2", 1.0, 1), ("3", 1.0, 1), ("Enter", 1.0, 2)],
    &[("→", 1.0, 1), ("0", 1.0, 1), (",", 1.0, 1), ("", 1.0, 0)],
];

/// Backlit keyboard preview split into the four Nitro lighting zones (three
/// across the main block, one on the numpad). `zones` are the lit colours
/// (`None` = zone off). Returns the zone that was clicked.
pub fn keyboard(
    ui: &mut egui::Ui,
    zones: [Option<Color32>; 4],
    selected: Option<usize>,
    labels: [String; 4],
) -> Option<usize> {
    let width = ui.available_width().min(820.0);
    let pad = 16.0;
    let gap_units = 0.45;
    let unit = (width - pad * 2.0) / (15.0 + gap_units + 4.0);
    let row_h = |row: usize| if row == 0 { unit * 0.72 } else { unit };
    let kb_h: f32 = (0..6).map(row_h).sum();
    let height = kb_h + pad * 2.0 + 16.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let p = ui.painter_at(rect);

    // Chassis.
    p.rect_filled(rect, 14.0, Color32::from_rgb(0x0E, 0x0F, 0x12));
    p.rect_stroke(rect, 14.0, Stroke::new(1.0_f32, Color32::from_rgb(0x2A, 0x2C, 0x33)));

    let origin = rect.min + Vec2::splat(pad);
    let mut zone_rects: [Option<Rect>; 4] = [None; 4];
    let mut draw_block = |rows: &[&[Key]; 6], x0: f32, numpad: bool| {
        let mut y = origin.y;
        for (ri, row) in rows.iter().enumerate() {
            let h = row_h(ri);
            let mut x = x0;
            for &(legend, w, span) in row.iter() {
                if span == 0 {
                    x += w * unit;
                    continue;
                }
                let key_h = if span == 2 { h + row_h(ri + 1) } else { h };
                let r = Rect::from_min_size(Pos2::new(x, y), Vec2::new(w * unit, key_h)).shrink(2.2);
                let center_units = (x - x0) / unit + w / 2.0;
                let zone = if numpad { 3 } else { ((center_units / 5.0) as usize).min(2) };
                draw_key(&p, r, legend, zones[zone], unit);
                if selected == Some(zone) {
                    p.rect_stroke(r.expand(1.0), 5.0, Stroke::new(1.5_f32, Color32::from_white_alpha(200)));
                }
                zone_rects[zone] = Some(zone_rects[zone].map_or(r, |acc| acc.union(r)));
                x += w * unit;
            }
            y += h;
        }
    };
    draw_block(&MAIN, origin.x, false);
    draw_block(&NUMPAD, origin.x + (15.0 + gap_units) * unit, true);

    for (i, zr) in zone_rects.iter().enumerate() {
        if let Some(zr) = zr {
            let color = if selected == Some(i) { Color32::WHITE } else { Color32::from_gray(140) };
            p.text(
                Pos2::new(zr.center().x, rect.max.y - pad * 0.5 - 6.0),
                egui::Align2::CENTER_CENTER,
                &labels[i],
                egui::FontId::proportional(12.0),
                color,
            );
        }
    }
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.clicked() {
        let pos = resp.interact_pointer_pos()?;
        return zone_rects.iter().position(|r| r.is_some_and(|r| r.expand(3.0).contains(pos)));
    }
    None
}

fn draw_key(p: &egui::Painter, r: Rect, legend: &str, light: Option<Color32>, unit: f32) {
    let radius = 4.0;
    // Backlight bleeding around the cap.
    if let Some(c) = light {
        p.rect_filled(r.expand(1.6), radius + 1.5, c.gamma_multiply(0.22));
        p.rect_filled(r.expand(0.6), radius + 0.5, c.gamma_multiply(0.35));
    }
    // Cap: dark skirt with a slightly lighter top face.
    p.rect_filled(r, radius, Color32::from_rgb(0x0A, 0x0A, 0x0D));
    let top = Rect::from_min_max(r.min, Pos2::new(r.max.x, r.max.y - 2.5));
    p.rect_filled(top, radius, Color32::from_rgb(0x1E, 0x1F, 0x25));
    // Translucent legend lit from below.
    if !legend.is_empty() {
        let n = legend.chars().count();
        let size = if n > 5 {
            unit * 0.18
        } else if n > 2 {
            unit * 0.22
        } else {
            unit * 0.34
        };
        let color = light.unwrap_or(Color32::from_rgb(0x40, 0x41, 0x48));
        p.text(top.center(), egui::Align2::CENTER_CENTER, legend, egui::FontId::proportional(size), color);
    }
}

/// Round colour button.
pub fn swatch(ui: &mut egui::Ui, color: Color32, selected: bool) -> egui::Response {
    let pl = pal();
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
    let p = ui.painter();
    p.circle_filled(rect.center(), 11.0, color);
    let ring = if selected {
        Stroke::new(2.5_f32, pl.text)
    } else if resp.hovered() {
        Stroke::new(1.5_f32, pl.weak)
    } else {
        Stroke::new(1.0_f32, pl.border)
    };
    p.circle_stroke(rect.center(), 13.5, ring);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Theme preview tile: four colour bars and a name.
pub fn theme_tile(ui: &mut egui::Ui, name: &str, colors: [Color32; 4]) -> egui::Response {
    let pl = pal();
    let size = Vec2::new(104.0, 52.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 8.0, pl.card);
    let bar_w = (rect.width() - 16.0) / 4.0;
    for (i, c) in colors.iter().enumerate() {
        let min = rect.min + Vec2::new(8.0 + i as f32 * bar_w, 8.0);
        let bar = Rect::from_min_size(min, Vec2::new(bar_w - 2.0, 14.0));
        p.rect_filled(bar, 3.0, *c);
        p.rect_stroke(bar, 3.0, Stroke::new(1.0_f32, pl.border));
    }
    p.text(
        Pos2::new(rect.center().x, rect.max.y - 13.0),
        egui::Align2::CENTER_CENTER,
        name,
        egui::FontId::proportional(12.0),
        pl.text,
    );
    let stroke = if resp.hovered() { Stroke::new(1.5_f32, ACCENT) } else { Stroke::new(1.0_f32, pl.border) };
    p.rect_stroke(rect, 8.0, stroke);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ---- settings building blocks -------------------------------------------------

/// iOS-style on/off switch.
pub fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = egui::vec2(40.0, 22.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let pl = pal();
    let bg = if *on { ACCENT } else { pl.track };
    let p = ui.painter();
    p.rect_filled(rect, rect.height() / 2.0, bg);
    let x = egui::lerp((rect.left() + 11.0)..=(rect.right() - 11.0), t);
    p.circle_filled(Pos2::new(x, rect.center().y), 8.0, Color32::WHITE);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Row of mutually exclusive buttons. Returns the newly picked index.
pub fn segmented(ui: &mut egui::Ui, selected: usize, labels: &[&str]) -> Option<usize> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (i, label) in labels.iter().enumerate() {
            let sel = i == selected;
            let text = RichText::new(*label).color(if sel { Color32::WHITE } else { pal().text });
            let b = egui::Button::new(text)
                .fill(if sel { ACCENT } else { ui.visuals().widgets.inactive.weak_bg_fill })
                .min_size(egui::vec2(72.0, 28.0));
            if ui.add(b).clicked() && !sel {
                picked = Some(i);
            }
        }
    });
    picked
}

/// Titled settings card.
pub fn section(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    let p = pal();
    ui.label(RichText::new(title).size(13.0).strong().color(p.weak));
    ui.add_space(2.0);
    egui::Frame::none()
        .fill(p.card)
        .rounding(10.0)
        .stroke(Stroke::new(1.0_f32, p.border))
        .inner_margin(egui::Margin::symmetric(14.0, 8.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            body(ui);
        });
    ui.add_space(14.0);
}

/// One settings line: title (+ optional hint) on the left, control on the right.
pub fn setting_row(ui: &mut egui::Ui, title: &str, hint: Option<&str>, control: impl FnOnce(&mut egui::Ui)) {
    let p = pal();
    ui.horizontal(|ui| {
        ui.set_min_height(40.0);
        ui.vertical(|ui| {
            ui.add_space(if hint.is_some() { 4.0 } else { 14.0 });
            ui.label(RichText::new(title).color(p.text));
            if let Some(h) = hint {
                ui.label(RichText::new(h).small().color(p.weak));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
}

pub fn row_separator(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(r, 0.0, pal().border);
}

// ---- charts ---------------------------------------------------------------------

/// One plotted series: name, colour, points as (seconds ago, value) and the
/// current value text for the legend.
pub struct Series {
    pub name: String,
    pub color: Color32,
    pub points: Vec<[f64; 2]>,
    pub current: String,
}

/// Minimal time-series chart: filled lines over the last `window_s`
/// seconds, horizontal guide lines only, legend chips with live values.
pub fn history_chart(
    ui: &mut egui::Ui,
    id: &str,
    series: Vec<Series>,
    y_range: (f64, f64),
    unit: &'static str,
    window_s: f64,
) {
    let p = pal();
    egui::Frame::none().fill(p.card).rounding(10.0).stroke(Stroke::new(1.0_f32, p.border)).inner_margin(12.0).show(
        ui,
        |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                for s in &series {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 4.5, s.color);
                    ui.label(RichText::new(&s.name).color(p.weak));
                    ui.label(RichText::new(&s.current).strong().color(p.text));
                    ui.add_space(14.0);
                }
            });
            ui.add_space(4.0);
            let lo = series.iter().flat_map(|s| s.points.iter().map(|p| p[1])).fold(y_range.0, f64::min);
            let hi = series.iter().flat_map(|s| s.points.iter().map(|p| p[1])).fold(y_range.1, f64::max);
            egui_plot::Plot::new(id)
                .height(150.0)
                .show_background(false)
                .show_axes([false, true])
                .show_grid([false, true])
                .allow_drag(false)
                .allow_zoom(false)
                .allow_scroll(false)
                .allow_boxed_zoom(false)
                .allow_double_click_reset(false)
                .include_x(-window_s)
                .include_x(0.0)
                .include_y(lo)
                .include_y(hi)
                .set_margin_fraction(egui::vec2(0.0, 0.08))
                .y_axis_min_width(40.0)
                .y_axis_formatter(move |mark, _| format!("{:.0}{unit}", mark.value))
                .label_formatter(
                    move |name, v| if name.is_empty() { String::new() } else { format!("{name}: {:.0}{unit}", v.y) },
                )
                .show(ui, |plot| {
                    for s in series {
                        plot.line(
                            egui_plot::Line::new(egui_plot::PlotPoints::from(s.points))
                                .name(s.name)
                                .color(s.color)
                                .width(2.2_f32),
                        );
                    }
                });
        },
    );
}
