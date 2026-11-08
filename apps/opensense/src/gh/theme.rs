//! Colours, fonts and theme selection of the main window and its child
//! windows (Fans + Power, Extra). Dark and light palettes; the theme
//! follows the Windows app theme unless the user picked one.

use std::sync::RwLock;

use eframe::egui::{self, Color32, FontFamily, FontId, Stroke};

/// Fixed accent colours, the same in both themes.
pub const ECO: Color32 = Color32::from_rgb(0x06, 0xB4, 0x8A);
pub const STANDARD: Color32 = Color32::from_rgb(0x3A, 0xAE, 0xEF);
pub const TURBO: Color32 = Color32::from_rgb(0xFF, 0x20, 0x20);
pub const CUSTOM: Color32 = Color32::from_rgb(0xFF, 0x80, 0x00);
/// Hint lines under tile rows.
pub const TIP: Color32 = Color32::from_rgb(0x6D, 0x6D, 0x6D);
/// Muted status text and the text of "off" pills.
pub const MUTED: Color32 = Color32::from_rgb(0xA0, 0xA0, 0xA0);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub form: Color32,
    pub fore: Color32,
    pub button: Color32,
    pub button2: Color32,
    pub border: Color32,
    pub border2: Color32,
    pub chart: Color32,
    pub grid: Color32,
}

pub const DARK: Palette = Palette {
    dark: true,
    form: Color32::from_rgb(0x1C, 0x1C, 0x1C),
    fore: Color32::from_rgb(0xF0, 0xF0, 0xF0),
    button: Color32::from_rgb(0x2E, 0x2E, 0x2E),
    button2: Color32::from_rgb(0x24, 0x24, 0x24),
    border: Color32::from_rgb(0x37, 0x37, 0x37),
    border2: Color32::from_rgb(0x2A, 0x2A, 0x2A),
    chart: Color32::from_rgb(0x23, 0x23, 0x23),
    grid: Color32::from_rgb(0x46, 0x46, 0x46),
};

pub const LIGHT: Palette = Palette {
    dark: false,
    form: Color32::from_rgb(0xF0, 0xF0, 0xF0),
    fore: Color32::BLACK,
    button: Color32::WHITE,
    button2: Color32::from_rgb(0xE3, 0xE3, 0xE3),
    border: Color32::from_rgb(0xDC, 0xDC, 0xDC),
    border2: Color32::from_rgb(0xD7, 0xD7, 0xD7),
    chart: Color32::WHITE,
    grid: Color32::from_rgb(0xD3, 0xD3, 0xD3),
};

static CURRENT: RwLock<Palette> = RwLock::new(DARK);

pub fn pal() -> Palette {
    *CURRENT.read().unwrap()
}

/// Body text size (Segoe UI 9 pt).
pub const BODY: f32 = 12.0;

pub fn body() -> FontId {
    FontId::proportional(BODY)
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

/// Windows "apps use light theme" setting.
#[cfg(windows)]
pub fn windows_dark() -> bool {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let mut value = 1u32;
    let mut size = 4u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(std::ptr::addr_of_mut!(value).cast()),
            Some(&mut size),
        )
    };
    r.is_ok() && value == 0
}

#[cfg(not(windows))]
pub fn windows_dark() -> bool {
    true
}

/// Resolves the `ui_mode` setting.
pub fn is_dark(ui_mode: &str) -> bool {
    match ui_mode {
        "dark" => true,
        "light" => false,
        _ => windows_dark(),
    }
}

/// Segoe UI regular and bold from the Windows fonts folder, with egui's
/// own font as fallback (also for glyphs Segoe UI lacks).
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.retain(|name, _| name == "Ubuntu-Light" || name == "Hack");
    let dir = std::env::var("WINDIR").map(|w| std::path::PathBuf::from(w).join("Fonts")).ok();
    let load = |file: &str| dir.as_ref().and_then(|d| std::fs::read(d.join(file)).ok());
    let mut regular = vec![];
    let mut bold = vec![];
    if let Some(data) = load("segoeui.ttf") {
        fonts.font_data.insert("segoe".into(), egui::FontData::from_owned(data));
        regular.push("segoe".to_owned());
    }
    if let Some(data) = load("segoeuib.ttf") {
        fonts.font_data.insert("segoe-bold".into(), egui::FontData::from_owned(data));
        bold.push("segoe-bold".to_owned());
    }
    regular.extend(["Ubuntu-Light".to_owned(), "Hack".to_owned()]);
    bold.extend(regular.iter().cloned());
    fonts.families.insert(FontFamily::Proportional, regular.clone());
    fonts.families.insert(FontFamily::Name("bold".into()), bold);
    fonts.families.insert(FontFamily::Monospace, vec!["Hack".into(), "Ubuntu-Light".into()]);
    ctx.set_fonts(fonts);
}

/// Applies the palette to egui's built-in widgets (combo boxes, popups,
/// text fields) and makes it current for the custom widgets.
pub fn apply(ctx: &egui::Context, dark: bool) {
    let p = if dark { DARK } else { LIGHT };
    *CURRENT.write().unwrap() = p;
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = p.form;
    v.window_fill = p.button;
    v.window_stroke = Stroke::new(1.0_f32, p.border);
    v.extreme_bg_color = p.button;
    v.faint_bg_color = p.button2;
    v.override_text_color = Some(p.fore);
    v.selection.bg_fill = STANDARD;
    v.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.rounding = 2.0.into();
        w.fg_stroke = Stroke::new(1.0_f32, p.fore);
    }
    v.widgets.inactive.weak_bg_fill = p.button;
    v.widgets.inactive.bg_fill = p.button;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);
    v.widgets.hovered.weak_bg_fill = shift(p.button, 0.04);
    v.widgets.hovered.bg_fill = shift(p.button, 0.04);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.border);
    v.widgets.active.weak_bg_fill = shift(p.button, 0.08);
    v.widgets.active.bg_fill = shift(p.button, 0.08);
    v.widgets.open.weak_bg_fill = p.button;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.fore);
    v.popup_shadow = egui::epaint::Shadow::NONE;
    v.window_shadow = egui::epaint::Shadow::NONE;
    v.menu_rounding = 2.0.into();
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(4.0, 4.0);
        s.spacing.button_padding = egui::vec2(6.0, 3.0);
        s.spacing.interact_size.y = 20.0;
        s.text_styles.insert(egui::TextStyle::Body, body());
        s.text_styles.insert(egui::TextStyle::Button, body());
        s.text_styles.insert(egui::TextStyle::Small, FontId::proportional(10.5));
    });
}

/// Moves a colour 4 % / 8 % toward black (light fills) or white (dark
/// fills), the hover and pressed look of buttons.
pub fn shift(c: Color32, amount: f32) -> Color32 {
    let lum = 0.30 * f32::from(c.r()) + 0.59 * f32::from(c.g()) + 0.11 * f32::from(c.b());
    let target = if lum > 128.0 { 0.0 } else { 255.0 };
    let mix = |v: u8| (f32::from(v) + (target - f32::from(v)) * amount).round() as u8;
    Color32::from_rgba_unmultiplied(mix(c.r()), mix(c.g()), mix(c.b()), c.a())
}

/// Moves a colour toward white.
pub fn lighten(c: Color32, amount: f32) -> Color32 {
    let mix = |v: u8| (f32::from(v) + (255.0 - f32::from(v)) * amount).round() as u8;
    Color32::from_rgb(mix(c.r()), mix(c.g()), mix(c.b()))
}

/// Dark or light native title bar for a window of this process.
#[cfg(windows)]
pub fn title_bar(title: &str, dark: bool) {
    if let Some(hwnd) = crate::tray::find_own_window(title) {
        title_bar_of(hwnd, dark);
    }
}

/// Dark or light title bar for a window handle.
pub fn title_bar_of(hwnd: windows::Win32::Foundation::HWND, dark: bool) {
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
    let on: i32 = dark.into();
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWINDOWATTRIBUTE(20),
            std::ptr::addr_of!(on).cast(),
            std::mem::size_of::<i32>() as u32,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_shift_direction() {
        // Dark fills get lighter, light fills darker.
        assert!(shift(DARK.button, 0.04).r() > DARK.button.r());
        assert!(shift(Color32::WHITE, 0.04).r() < 255);
        assert_eq!(shift(Color32::WHITE, 0.04).r(), 245);
    }
}
