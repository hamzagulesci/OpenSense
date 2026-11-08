//! On-screen notification shown by the tray agent when a shortcut changes
//! something (mode, backlight): white bold text on a translucent dark
//! rounded box near the bottom of the screen, for two seconds. A layered,
//! click-through window drawn with GDI, so the agent needs no renderer.

use std::time::{Duration, Instant};

use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW, GetDC, ReleaseDC,
    SelectObject, SetBkMode, SetTextColor, AC_SRC_ALPHA, AC_SRC_OVER, ANTIALIASED_QUALITY, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DIB_RGB_COLORS, DT_CENTER,
    DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_BOLD, OUT_DEFAULT_PRECIS, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetSystemMetrics, RegisterClassW, ShowWindow, UpdateLayeredWindow, SM_CXSCREEN,
    SM_CYSCREEN, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const VISIBLE: Duration = Duration::from_millis(2000);

pub struct Osd {
    hwnd: Option<HWND>,
    hide_at: Option<Instant>,
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    msg: u32,
    wp: windows::Win32::Foundation::WPARAM,
    lp: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

impl Osd {
    pub fn new() -> Self {
        Self { hwnd: None, hide_at: None }
    }

    fn window(&mut self) -> Option<HWND> {
        if self.hwnd.is_none() {
            unsafe {
                let instance = GetModuleHandleW(None).ok()?;
                let class = WNDCLASSW {
                    lpfnWndProc: Some(wndproc),
                    hInstance: instance.into(),
                    lpszClassName: w!("OpenSenseOsd"),
                    ..Default::default()
                };
                RegisterClassW(&class);
                let hwnd = CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
                    w!("OpenSenseOsd"),
                    w!("OpenSense"),
                    WS_POPUP,
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    instance,
                    None,
                )
                .ok()?;
                self.hwnd = Some(hwnd);
            }
        }
        self.hwnd
    }

    /// Shows `text`, replacing any toast on screen and restarting its timer.
    pub fn show(&mut self, text: &str) {
        let Some(hwnd) = self.window() else { return };
        let scale = unsafe { windows::Win32::UI::HiDpi::GetDpiForSystem() } as f32 / 96.0;
        let chars = text.chars().count() as f32;
        let (w, h) = (((100.0 + 22.0 * chars).max(300.0) * scale) as i32, (100.0 * scale) as i32);
        let pixels = render(text, w, h, scale);
        unsafe {
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(screen);
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
            let Ok(bitmap) = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
                let _ = DeleteDC(dc);
                ReleaseDC(None, screen);
                return;
            };
            std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u32>(), pixels.len());
            let old = SelectObject(dc, bitmap);
            let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
            let pos = POINT { x: (sw - w) / 2, y: sh - (300.0 * scale) as i32 - h };
            let size = SIZE { cx: w, cy: h };
            let src = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 250,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = UpdateLayeredWindow(
                hwnd,
                screen,
                Some(&pos),
                Some(&size),
                dc,
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            SelectObject(dc, old);
            let _ = DeleteObject(bitmap);
            let _ = DeleteDC(dc);
            ReleaseDC(None, screen);
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        self.hide_at = Some(Instant::now() + VISIBLE);
    }

    /// Hides the toast once its time is up; call regularly.
    pub fn tick(&mut self) {
        if self.hide_at.is_some_and(|t| Instant::now() >= t) {
            self.hide_at = None;
            if let Some(hwnd) = self.hwnd {
                let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
            }
        }
    }
}

/// Premultiplied BGRA pixels: rounded translucent black box with the text.
fn render(text: &str, w: i32, h: i32, scale: f32) -> Vec<u32> {
    let coverage = text_coverage(text, w, h, scale);
    let radius = 10.0 * scale;
    let inside = |x: i32, y: i32| -> f32 {
        // Distance-based coverage of a rounded rectangle.
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        let cx = fx.clamp(radius, w as f32 - radius);
        let cy = fy.clamp(radius, h as f32 - radius);
        let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
        (radius - d + 0.5).clamp(0.0, 1.0)
    };
    let mut out = vec![0u32; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            let c = coverage[i];
            let bg = 150.0 / 255.0 * inside(x, y);
            let a = c + bg * (1.0 - c);
            let v = (255.0 * c) as u32;
            let a = (255.0 * a) as u32;
            out[i] = (a << 24) | (v << 16) | (v << 8) | v;
        }
    }
    out
}

/// Grey-scale coverage (0..1) of the text drawn with GDI, white on black.
fn text_coverage(text: &str, w: i32, h: i32, scale: f32) -> Vec<f32> {
    let mut cov = vec![0f32; (w * h) as usize];
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(screen);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        if let Ok(bitmap) = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            let old = SelectObject(dc, bitmap);
            let font = CreateFontW(
                -(36.0 * scale) as i32,
                0,
                0,
                0,
                FW_BOLD.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET.0.into(),
                OUT_DEFAULT_PRECIS.0.into(),
                CLIP_DEFAULT_PRECIS.0.into(),
                ANTIALIASED_QUALITY.0.into(),
                FF_DONTCARE.0.into(),
                w!("Segoe UI"),
            );
            let old_font = SelectObject(dc, font);
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, COLORREF(0x00FF_FFFF));
            let mut r = RECT { left: 0, top: 0, right: w, bottom: h };
            let mut wide: Vec<u16> = HSTRING::from(text).as_wide().to_vec();
            DrawTextW(dc, &mut wide, &mut r, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
            let px = std::slice::from_raw_parts(bits.cast::<u32>(), (w * h) as usize);
            for (c, p) in cov.iter_mut().zip(px) {
                let (r, g, b) = ((p >> 16) & 0xFF, (p >> 8) & 0xFF, p & 0xFF);
                *c = r.max(g).max(b) as f32 / 255.0;
            }
            SelectObject(dc, old_font);
            let _ = DeleteObject(font);
            SelectObject(dc, old);
            let _ = DeleteObject(bitmap);
        }
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
    }
    cov
}
