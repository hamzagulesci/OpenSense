//! Custom-drawn controls of the main window: tiles, row buttons, pills,
//! the slider, the colour button, check boxes and section titles.

use eframe::egui::{self, Align2, Color32, Mesh, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

use super::icons::{self, Icon};
use super::theme::{self, pal};

/// Corner radius of big tiles and of row buttons / pills.
pub const TILE_RADIUS: f32 = 3.0;
pub const ROW_RADIUS: f32 = 1.5;
/// Width of the accent outline of the selected tile.
const ACTIVE_STROKE: f32 = 1.5;
/// Gap around each grid cell.
pub const CELL_MARGIN: f32 = 2.0;

/// Splits a full-width row of `height` into `n` equal cells, each with a
/// [`CELL_MARGIN`] around it.
pub fn grid(ui: &mut Ui, n: usize, height: f32) -> Vec<Rect> {
    let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let w = row.width() / n as f32;
    (0..n)
        .map(|i| {
            Rect::from_min_size(Pos2::new(row.left() + i as f32 * w, row.top()), Vec2::new(w, height))
                .shrink(CELL_MARGIN)
        })
        .collect()
}

/// Background fill for a control of the given family in its current state.
fn fill(resp: &Response, secondary: bool, enabled: bool) -> Color32 {
    let p = pal();
    let base = if secondary { p.button2 } else { p.button };
    let c = if !enabled {
        base
    } else if resp.is_pointer_button_down_on() {
        theme::shift(base, 0.08)
    } else if resp.hovered() {
        theme::shift(base, 0.04)
    } else {
        base
    };
    if enabled {
        c
    } else {
        c.gamma_multiply(100.0 / 255.0)
    }
}

/// Resting outline, lighter along the top edge in the dark theme.
fn rest_border(painter: &egui::Painter, rect: Rect, radius: f32, secondary: bool) {
    let p = pal();
    let c = if secondary { p.border2 } else { p.border };
    painter.rect_stroke(rect.shrink(0.5), radius, Stroke::new(1.0_f32, c));
    if p.dark {
        let top = theme::lighten(c, 0.10);
        painter.line_segment(
            [Pos2::new(rect.left() + radius, rect.top() + 0.5), Pos2::new(rect.right() - radius, rect.top() + 0.5)],
            Stroke::new(1.0_f32, top),
        );
    }
}

/// Accent outline plus a soft tint along one edge: the selected look.
fn active_border(painter: &egui::Painter, rect: Rect, radius: f32, accent: Color32) {
    let p = pal();
    let inner = rect.shrink(ACTIVE_STROKE / 2.0);
    // Tint: accent at alpha 32 fading out over 20 % of the height.
    let h = inner.height() * 0.2;
    let (a, b) = if p.dark {
        (Rect::from_min_max(inner.min, Pos2::new(inner.right(), inner.top() + h)), true)
    } else {
        (Rect::from_min_max(Pos2::new(inner.left(), inner.bottom() - h), inner.max), false)
    };
    let strong = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 32);
    let clear = Color32::TRANSPARENT;
    let (top, bottom) = if b { (strong, clear) } else { (clear, strong) };
    let mut mesh = Mesh::default();
    mesh.colored_vertex(a.left_top(), top);
    mesh.colored_vertex(a.right_top(), top);
    mesh.colored_vertex(a.right_bottom(), bottom);
    mesh.colored_vertex(a.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
    painter.rect_stroke(inner, radius, Stroke::new(ACTIVE_STROKE, accent));
    // Lighter band along the top edge of the outline.
    painter.line_segment(
        [Pos2::new(inner.left() + radius, inner.top()), Pos2::new(inner.right() - radius, inner.top())],
        Stroke::new(ACTIVE_STROKE, theme::lighten(accent, 0.15)),
    );
}

pub struct TileStyle {
    pub icon: Option<Icon>,
    /// Secondary family (darker fill), e.g. "Fans + Power".
    pub secondary: bool,
    pub enabled: bool,
    /// Accent shown when selected.
    pub accent: Color32,
    pub selected: bool,
}

/// Big option tile: icon above the caption.
pub fn tile(ui: &mut Ui, rect: Rect, id: &str, label: &str, style: &TileStyle) -> Response {
    let p = pal();
    let sense = if style.enabled { Sense::click() } else { Sense::hover() };
    let resp = ui.interact(rect, ui.id().with(("tile", id)), sense);
    let painter = ui.painter();
    painter.rect_filled(rect, TILE_RADIUS, fill(&resp, style.secondary, style.enabled));
    if style.selected {
        active_border(painter, rect, TILE_RADIUS, style.accent);
    } else {
        rest_border(painter, rect, TILE_RADIUS, style.secondary);
    }
    let fg = if style.enabled { p.fore } else { Color32::from_gray(0x80) };
    match style.icon {
        Some(icon) => {
            icons::paint(painter, rect.center() - Vec2::new(0.0, 8.0), 24.0, icon, fg);
            painter.text(rect.center() + Vec2::new(0.0, 15.0), Align2::CENTER_CENTER, label, theme::body(), fg);
        }
        None => {
            painter.text(rect.center(), Align2::CENTER_CENTER, label, theme::body(), fg);
        }
    }
    if style.enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// Row button: small icon left of the text, both centred.
pub fn row_button(ui: &mut Ui, rect: Rect, id: &str, label: &str, icon: Option<Icon>, secondary: bool) -> Response {
    let p = pal();
    let resp = ui.interact(rect, ui.id().with(("row", id)), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, ROW_RADIUS, fill(&resp, secondary, true));
    rest_border(painter, rect, ROW_RADIUS, secondary);
    let w = text_width(ui, label, theme::body());
    let icon_w = if icon.is_some() { 20.0 } else { 0.0 };
    let x0 = rect.center().x - (w + icon_w) / 2.0;
    if let Some(icon) = icon {
        icons::paint(painter, Pos2::new(x0 + 8.0, rect.center().y), 16.0, icon, p.fore);
    }
    painter.text(Pos2::new(x0 + icon_w, rect.center().y), Align2::LEFT_CENTER, label, theme::body(), p.fore);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Small borderless toggle ("100%").
pub fn pill(ui: &mut Ui, rect: Rect, id: &str, label: &str, on: bool, on_color: Color32) -> Response {
    let p = pal();
    let resp = ui.interact(rect, ui.id().with(("pill", id)), Sense::click());
    let bg = if on { on_color } else { fill(&resp, true, true) };
    let fg = if on { Color32::WHITE } else { theme::MUTED };
    ui.painter().rect_filled(rect, ROW_RADIUS, bg);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, theme::bold(9.5), fg);
    let _ = p;
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// "Color ■" button with one or more swatches on its right.
pub fn color_button(ui: &mut Ui, rect: Rect, id: &str, label: &str, swatches: &[Color32]) -> Response {
    let p = pal();
    let resp = ui.interact(rect, ui.id().with(("color", id)), Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, ROW_RADIUS, fill(&resp, false, true));
    rest_border(painter, rect, ROW_RADIUS, false);
    let side = rect.height() * 0.45;
    let margin = (rect.height() - side) / 2.0;
    let mut x = rect.right() - margin - side;
    for &c in swatches.iter().rev() {
        let r = Rect::from_min_size(Pos2::new(x, rect.top() + margin), Vec2::splat(side));
        painter.rect_filled(r, ROW_RADIUS, c);
        painter.rect_stroke(r, ROW_RADIUS, Stroke::new(1.0_f32, p.border));
        x -= side + margin * 0.6;
    }
    let text_right = x + side;
    let w = text_width(ui, label, theme::body());
    let cx = rect.left() + (text_right - rect.left()) / 2.0;
    painter.text(Pos2::new(cx - w / 2.0, rect.center().y), Align2::LEFT_CENTER, label, theme::body(), p.fore);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub struct SliderOut {
    /// Value to show (follows the pointer while dragging).
    pub value: f32,
    /// Set once, when the user releases the slider on a new value.
    pub commit: Option<f32>,
}

/// Horizontal slider: thin track, accent fill up to the thumb, a ring
/// thumb whose inner dot grows on hover and shrinks while dragged. Values
/// snap to `supported` when given, else to `step`.
#[allow(clippy::too_many_arguments)]
pub fn slider(
    ui: &mut Ui,
    rect: Rect,
    id: &str,
    value: f32,
    min: f32,
    max: f32,
    step: f32,
    supported: &[f32],
    accent: Color32,
) -> SliderOut {
    let p = pal();
    let resp = ui.interact(rect, ui.id().with(("slider", id)), Sense::click_and_drag());
    let h = rect.height();
    let r = 0.4 * h;
    let (x0, x1) = (rect.left() + r, rect.right() - r);
    let snap = |v: f32| -> f32 {
        if supported.is_empty() {
            (min + ((v - min) / step).round() * step).clamp(min, max)
        } else {
            supported.iter().copied().min_by(|a, b| (a - v).abs().total_cmp(&(b - v).abs())).unwrap_or(v)
        }
    };
    let to_value = |x: f32| min + ((x - x0) / (x1 - x0)).clamp(0.0, 1.0) * (max - min);
    let mut shown = value;
    let mut thumb_x = x0 + ((value - min) / (max - min)).clamp(0.0, 1.0) * (x1 - x0);
    let active = resp.dragged() || resp.clicked() || resp.drag_stopped();
    if active {
        if let Some(pos) = resp.interact_pointer_pos() {
            shown = snap(to_value(pos.x));
            // While dragging the thumb follows the pointer exactly.
            thumb_x = if resp.dragged() { pos.x.clamp(x0, x1) } else { x0 + (shown - min) / (max - min) * (x1 - x0) };
        }
    }
    let commit = ((resp.clicked() || resp.drag_stopped()) && shown != value).then_some(shown);
    if commit.is_some() {
        thumb_x = x0 + (shown - min) / (max - min) * (x1 - x0);
    }
    let pressed = resp.dragged() || resp.is_pointer_button_down_on();
    let target = if pressed {
        10.0 / 22.0
    } else if resp.hovered() {
        16.0 / 22.0
    } else {
        12.0 / 22.0
    };
    let inner = ui.ctx().animate_value_with_time(resp.id.with("dot"), target, 0.12);
    let ticks = ui.ctx().animate_value_with_time(
        resp.id.with("ticks"),
        if resp.hovered() || pressed { 120.0 } else { 0.0 },
        0.15,
    );
    let painter = ui.painter();
    let cy = rect.center().y;
    let t = 0.15 * h;
    painter.rect_filled(Rect::from_min_max(Pos2::new(x0, cy - t / 2.0), Pos2::new(x1, cy + t / 2.0)), 0.0, p.grid);
    painter.rect_filled(Rect::from_min_max(Pos2::new(x0, cy - t / 2.0), Pos2::new(thumb_x, cy + t / 2.0)), 0.0, accent);
    if ticks > 1.0 {
        let c = Color32::from_rgba_unmultiplied(p.fore.r(), p.fore.g(), p.fore.b(), ticks as u8);
        for &s in supported {
            let x = x0 + (s - min) / (max - min) * (x1 - x0);
            let w = (t / 4.0).max(1.0);
            for dir in [-1.0, 1.0] {
                let y = cy + dir * (t / 2.0 + 0.5 * t + 0.375 * t);
                painter.rect_filled(Rect::from_center_size(Pos2::new(x, y), Vec2::new(w, 0.75 * t)), 0.0, c);
            }
        }
    }
    painter.circle_filled(Pos2::new(thumb_x, cy), r, p.button);
    painter.circle_filled(Pos2::new(thumb_x, cy), r * inner, accent);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    SliderOut { value: shown, commit }
}

/// Check box with its label; `rect` is the row. Returns true when toggled.
pub fn checkbox(ui: &mut Ui, rect: Rect, id: &str, label: &str, on: &mut bool) -> Response {
    let p = pal();
    let w = text_width(ui, label, theme::body());
    let hit = Rect::from_min_size(rect.min, Vec2::new(22.0 + w, rect.height()));
    let mut resp = ui.interact(hit, ui.id().with(("check", id)), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let b = Rect::from_center_size(Pos2::new(rect.left() + 7.5, rect.center().y), Vec2::splat(13.0));
    let painter = ui.painter();
    if *on {
        painter.rect_filled(b, 3.0, theme::STANDARD);
        let c = b.center();
        painter.add(egui::Shape::line(
            vec![c + Vec2::new(-3.5, 0.0), c + Vec2::new(-1.0, 2.5), c + Vec2::new(3.5, -2.5)],
            Stroke::new(1.6_f32, if p.dark { Color32::BLACK } else { Color32::WHITE }),
        ));
    } else {
        let edge = if resp.hovered() { p.fore } else { theme::MUTED };
        painter.rect_stroke(b, 3.0, Stroke::new(1.0_f32, edge));
    }
    painter.text(Pos2::new(rect.left() + 21.0, rect.center().y), Align2::LEFT_CENTER, label, theme::body(), p.fore);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Section title row: 16 pt icon, bold title, optional right-aligned text.
/// Returns the response of the right-hand text (clickable).
pub fn section_title(ui: &mut Ui, icon: Icon, title: &str, right: &str) -> Option<Response> {
    let p = pal();
    let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::hover());
    let painter = ui.painter();
    icons::paint(painter, Pos2::new(row.left() + 12.0, row.top() + 8.0), 16.0, icon, p.fore);
    painter.text(
        Pos2::new(row.left() + 24.0, row.top() + 8.0),
        Align2::LEFT_CENTER,
        title,
        theme::bold(theme::BODY),
        p.fore,
    );
    if right.is_empty() {
        return None;
    }
    let r =
        painter.text(Pos2::new(row.right() - 2.0, row.top() + 8.0), Align2::RIGHT_CENTER, right, theme::body(), p.fore);
    Some(
        ui.interact(r, ui.id().with(("title-right", title)), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand),
    )
}

/// Grey hint line under a tile row (empty lines keep their height).
pub fn hint_line(ui: &mut Ui, text: &str) {
    let (row, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
    if !text.is_empty() {
        ui.painter().text(
            Pos2::new(row.left() + 4.0, row.center().y),
            Align2::LEFT_CENTER,
            text,
            theme::body(),
            theme::TIP,
        );
    }
}

pub fn text_width(ui: &Ui, text: &str, font: egui::FontId) -> f32 {
    ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font, Color32::WHITE).size().x)
}

/// Underlined link-style text; returns its response.
pub fn link(ui: &mut Ui, pos: Pos2, id: &str, text: &str, color: Color32) -> Response {
    let r = ui.painter().text(pos, Align2::LEFT_CENTER, text, theme::body(), color);
    ui.painter().line_segment(
        [Pos2::new(r.left(), r.bottom() - 1.0), Pos2::new(r.right(), r.bottom() - 1.0)],
        Stroke::new(1.0_f32, color),
    );
    ui.interact(r, ui.id().with(("link", id)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand)
}
