//! In-game metrics overlay hosted by the tray agent: FPS, GPU / CPU
//! temperatures and fan speeds, load, memory and battery in two coloured
//! rows on a translucent box. A click-through layered window drawn with
//! GDI once a second, so games pay nothing for it.
//!
//! Hold Ctrl+Shift+Alt over it to grab it: drag to move, click to switch
//! the layout (Light / Default / Full), wheel to resize, middle click to
//! reset the size.

use std::sync::Mutex;

use opensense_sys::config::{AppConfig, TempUnit};
use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW,
    GetTextExtentPoint32W, MonitorFromPoint, ReleaseDC, SelectObject, SetBkMode, SetTextColor, TextOutW, AC_SRC_ALPHA,
    AC_SRC_OVER, ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DIB_RGB_COLORS, FF_DONTCARE, FW_BOLD, FW_NORMAL, HDC, HFONT, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, OUT_DEFAULT_PRECIS, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, ReleaseCapture, SetCapture, VK_CONTROL, VK_MENU, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetForegroundWindow, GetWindowLongW, GetWindowRect,
    GetWindowThreadProcessId, KillTimer, RegisterClassW, SetTimer, SetWindowLongW, SetWindowPos, ShowWindow,
    UpdateLayeredWindow, GWL_EXSTYLE, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_HIDE,
    SW_SHOWNOACTIVATE, ULW_ALPHA, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_TIMER,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::fps::FpsMonitor;
use crate::worker::WorkerHandle;

const GPU_COLOR: (u8, u8, u8) = (0x00, 0xFF, 0x50);
const CPU_COLOR: (u8, u8, u8) = (0x3C, 0xDC, 0xFF);
const TEXT_LIGHT: (u8, u8, u8) = (0xEB, 0xEB, 0xEB);
/// Layouts, cycled by clicking the grabbed overlay.
pub const MODES: [&str; 3] = ["Light", "Default", "Full"];
/// Drag-mode check while the overlay is visible.
const DRAG_TIMER: usize = 1;

/// Foreground apps that never count as games.
const DESKTOP_APPS: &[&str] = &[
    "chrome",
    "msedge",
    "firefox",
    "opera",
    "brave",
    "vivaldi",
    "iexplore",
    "chromium",
    "librewolf",
    "arc",
    "waterfox",
    "thorium",
    "windowsterminal",
    "conhost",
    "cmd",
    "powershell",
    "pwsh",
    "alacritty",
    "wezterm-gui",
    "mintty",
    "discord",
    "slack",
    "teams",
    "ms-teams",
    "spotify",
    "whatsapp",
    "signal",
    "telegram",
    "code",
    "notion",
    "obsidian",
    "zoom",
    "skype",
    "element",
    "viber",
    "line",
    "wechat",
    "weixin",
    "notepad",
    "notepad++",
    "sublime_text",
    "devenv",
    "rider64",
    "idea64",
    "pycharm64",
    "webstorm64",
    "steam",
    "steamwebhelper",
    "epicgameslauncher",
    "battle.net",
    "galaxyclient",
    "eadesktop",
    "ubisoftconnect",
    "vlc",
    "mpv",
    "mpc-hc64",
    "mpc-be64",
    "potplayermini64",
    "wmplayer",
    "smplayer",
    "foobar2000",
    "aimp",
    "winword",
    "excel",
    "powerpnt",
    "outlook",
    "acrobat",
    "acrord32",
    "sumatrapdf",
    "thunderbird",
    "onenote",
    "githubdesktop",
    "7zfm",
    "winrar",
    "snippingtool",
    "explorer",
    "shellexperiencehost",
    "shellhost",
    "searchhost",
    "startmenuexperiencehost",
    "applicationframehost",
    "systemsettings",
    "taskmgr",
    "opensense",
    "claude",
];

/// Mouse state shared with the window procedure.
#[derive(Default)]
struct Mouse {
    grab: bool,
    dragging: bool,
    start_cursor: (i32, i32),
    start_window: (i32, i32),
    moved: bool,
    /// Events for the next tick: click (cycle layout), moved, scale steps.
    clicked: bool,
    released_after_move: bool,
    wheel: i32,
    reset_scale: bool,
}

static MOUSE: Mutex<Mouse> = Mutex::new(Mouse {
    grab: false,
    dragging: false,
    start_cursor: (0, 0),
    start_window: (0, 0),
    moved: false,
    clicked: false,
    released_after_move: false,
    wheel: 0,
    reset_scale: false,
});

fn keys_held() -> bool {
    let down = |vk: u16| unsafe { GetAsyncKeyState(i32::from(vk)) } as u16 & 0x8000 != 0;
    down(VK_CONTROL.0) && down(VK_SHIFT.0) && down(VK_MENU.0)
}

fn cursor() -> (i32, i32) {
    let mut p = POINT::default();
    let _ = unsafe { GetCursorPos(&mut p) };
    (p.x, p.y)
}

fn window_rect(hwnd: HWND) -> RECT {
    let mut r = RECT::default();
    let _ = unsafe { GetWindowRect(hwnd, &mut r) };
    r
}

fn set_click_through(hwnd: HWND, on: bool) {
    unsafe {
        let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        let new = if on { style | WS_EX_TRANSPARENT.0 } else { style & !WS_EX_TRANSPARENT.0 };
        if new != style {
            SetWindowLongW(hwnd, GWL_EXSTYLE, new as i32);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let Ok(mut m) = MOUSE.lock() else { return unsafe { DefWindowProcW(hwnd, msg, wp, lp) } };
    match msg {
        WM_TIMER if wp.0 == DRAG_TIMER => {
            if !m.dragging {
                let r = window_rect(hwnd);
                let (x, y) = cursor();
                let over = x >= r.left && x < r.right && y >= r.top && y < r.bottom;
                let grab = over && keys_held();
                if grab != m.grab {
                    m.grab = grab;
                    set_click_through(hwnd, !grab);
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN if m.grab => {
            let r = window_rect(hwnd);
            m.dragging = true;
            m.moved = false;
            m.start_cursor = cursor();
            m.start_window = (r.left, r.top);
            unsafe { SetCapture(hwnd) };
            LRESULT(0)
        }
        WM_MOUSEMOVE if m.dragging => {
            let (x, y) = cursor();
            let (dx, dy) = (x - m.start_cursor.0, y - m.start_cursor.1);
            if dx.abs() > 5 || dy.abs() > 5 {
                m.moved = true;
            }
            if m.moved {
                let r = window_rect(hwnd);
                let (w, h) = (r.right - r.left, r.bottom - r.top);
                let area = monitor_area(x, y);
                let nx = (m.start_window.0 + dx).clamp(area.left + 5, (area.right - w - 5).max(area.left + 5));
                let ny = (m.start_window.1 + dy).clamp(area.top + 5, (area.bottom - h - 5).max(area.top + 5));
                let _ = unsafe { SetWindowPos(hwnd, None, nx, ny, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) };
            }
            LRESULT(0)
        }
        WM_LBUTTONUP if m.dragging => {
            let _ = unsafe { ReleaseCapture() };
            m.dragging = false;
            if m.moved {
                m.released_after_move = true;
            } else {
                m.clicked = true;
            }
            if !keys_held() {
                m.grab = false;
                set_click_through(hwnd, true);
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL if m.grab => {
            let delta = ((wp.0 >> 16) & 0xFFFF) as u16 as i16;
            m.wheel += if delta > 0 { 1 } else { -1 };
            LRESULT(0)
        }
        WM_MBUTTONDOWN if m.grab => {
            m.reset_scale = true;
            LRESULT(0)
        }
        _ => {
            drop(m);
            unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
        }
    }
}

/// Bounds of the monitor containing a point.
fn monitor_area(x: i32, y: i32) -> RECT {
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe {
        let mon = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let _ = GetMonitorInfoW(mon, &mut info);
    }
    info.rcMonitor
}

fn process_name(pid: u32) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buf = [0u16; 512];
    let mut len = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) };
    let _ = unsafe { CloseHandle(process) };
    ok.ok()?;
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    let file = path.rsplit(['\\', '/']).next()?.to_ascii_lowercase();
    Some(file.trim_end_matches(".exe").to_owned())
}

fn is_desktop_app(pid: u32) -> bool {
    process_name(pid).is_some_and(|n| DESKTOP_APPS.contains(&n.as_str()))
}

/// Values shown in one frame.
#[derive(Default, Clone)]
pub struct Metrics {
    pub cpu_temp: Option<u16>,
    pub gpu_temp: Option<u16>,
    pub cpu_rpm: Option<u16>,
    pub gpu_rpm: Option<u16>,
    pub battery: Option<u8>,
    pub rate_w: Option<f32>,
    pub on_ac: bool,
}

pub struct Overlay {
    hwnd: HWND,
    fps: Option<FpsMonitor>,
    cpu: opensense_sys::cpu::CpuMonitor,
    gpu: Option<opensense_sys::gpu_load::GpuLoadMonitor>,
    fg_pid: u32,
    game_count: u32,
    latched: u32,
    shown: bool,
    fps_value: f32,
}

impl Overlay {
    pub fn start(cfg: &AppConfig) -> Option<Self> {
        let hwnd = unsafe {
            let instance = GetModuleHandleW(None).ok()?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: instance.into(),
                lpszClassName: w!("OpenSenseOverlay"),
                ..Default::default()
            };
            RegisterClassW(&class);
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
                w!("OpenSenseOverlay"),
                w!("OpenSense Overlay"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                instance,
                None,
            )
            .ok()?
        };
        unsafe {
            SetTimer(hwnd, DRAG_TIMER, 100, None);
        }
        let mut o = Self {
            hwnd,
            fps: FpsMonitor::start(),
            cpu: opensense_sys::cpu::CpuMonitor::new(),
            gpu: opensense_sys::gpu_load::GpuLoadMonitor::new().ok(),
            fg_pid: 0,
            game_count: 0,
            latched: 0,
            shown: false,
            fps_value: 0.0,
        };
        // Initial position: saved, else top-left of the primary monitor.
        let (x, y) = cfg.overlay_pos.map_or((10, 10), |p| (p[0], p[1]));
        unsafe {
            let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
        }
        if !cfg.overlay_game_only {
            o.show(true);
        }
        Some(o)
    }

    fn show(&mut self, on: bool) {
        if on != self.shown {
            self.shown = on;
            unsafe {
                let _ = ShowWindow(self.hwnd, if on { SW_SHOWNOACTIVATE } else { SW_HIDE });
            }
        }
    }

    /// Once a second. Returns true when the config changed (save it).
    pub fn tick(&mut self, cfg: &mut AppConfig, m: &Metrics) -> bool {
        let mut changed = false;
        // Mouse events from the window procedure.
        {
            let mut mouse = MOUSE.lock().unwrap_or_else(|e| e.into_inner());
            if std::mem::take(&mut mouse.clicked) {
                cfg.overlay_mode = (cfg.overlay_mode + 1) % MODES.len() as u8;
                changed = true;
            }
            let wheel = std::mem::take(&mut mouse.wheel);
            if wheel != 0 {
                cfg.overlay_scale = (i32::from(cfg.overlay_scale) + wheel * 10).clamp(35, 300) as u16;
                changed = true;
            }
            if std::mem::take(&mut mouse.reset_scale) {
                cfg.overlay_scale = 100;
                changed = true;
            }
            if std::mem::take(&mut mouse.released_after_move) {
                let r = window_rect(self.hwnd);
                cfg.overlay_pos = Some([r.left, r.top]);
                changed = true;
            }
        }

        // Foreground process: FPS target and game detection.
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid)) };
        let own = pid == 0 || pid == std::process::id();
        if !own && pid != self.fg_pid {
            self.fg_pid = pid;
            self.fps_value = 0.0;
            let target = if is_desktop_app(pid) { 0 } else { pid };
            if let Some(f) = &self.fps {
                f.set_target(target);
            }
        } else if let Some(f) = &self.fps {
            self.fps_value = f.sample();
        }
        if cfg.overlay_game_only {
            let game = self.fps_value >= 6.0 && !is_desktop_app(self.fg_pid);
            self.game_count = if game { self.game_count + 1 } else { 0 };
            if self.game_count >= 2 {
                self.latched = self.fg_pid;
            }
            self.show(self.latched != 0 && self.fg_pid == self.latched);
        } else {
            self.show(true);
        }
        if !self.shown {
            return changed;
        }

        let cpu_load = self.cpu.sample().usage;
        let gpu_load = self.gpu.as_mut().and_then(|g| g.sample().ok().flatten());
        let ram = memory_used();
        self.paint(cfg, m, cpu_load, gpu_load, ram);
        // Stay above windows that took the top of the z-order.
        unsafe {
            let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
        changed
    }

    fn paint(&self, cfg: &AppConfig, m: &Metrics, cpu_load: f32, gpu_load: Option<f32>, ram: Option<(f32, u32)>) {
        // Base scale 2.0 at 100 %, independent of the display scale.
        let sc = 2.0 * f32::from(cfg.overlay_scale) / 100.0;
        let px = |v: f32| (v * sc).floor() as i32;
        let (line_h, gap, pad_y, pad_x, col_gap) = (px(18.0), px(1.0), px(4.0), px(8.0), px(8.0));
        let inner_h = 2 * line_h + gap;
        let height = 2 * pad_y + inner_h;
        let rows = [pad_y, pad_y + line_h + gap];
        let mode = cfg.overlay_mode.min(2);
        let show_fan = mode >= 1;
        let show_load = mode >= 1;
        let show_ram = mode >= 2;
        let show_battery = mode >= 2 && !m.on_ac;

        let unit = |t: Option<u16>| -> String {
            match (t, cfg.temp_unit) {
                (None, _) => " -- ".into(),
                (Some(c), TempUnit::Celsius) => format!("{c:>3}°"),
                (Some(c), TempUnit::Fahrenheit) => {
                    format!("{:>3.0}°", opensense_proto::sensor::celsius_to_fahrenheit(f32::from(c)))
                }
            }
        };
        // Column x positions.
        let mut x = pad_x;
        let fps_x = x;
        x += px(52.0) + col_gap;
        let temp_x = x;
        x += if show_fan { px(128.0) } else { px(64.0) } + col_gap;
        let load_x = x;
        if show_load {
            x += px(30.0) + px(4.0) + px(5.0) + col_gap;
        }
        let ram_x = x;
        if show_ram {
            x += px(54.0) + px(4.0) + px(5.0) + col_gap;
        }
        let bat_x = x;
        if show_battery {
            x += px(70.0) + col_gap;
        }
        let width = x - col_gap + pad_x;

        let mut canvas = Canvas::new(width, height);
        let main = font(px(13.0), false);
        let small = font((8.5 * sc) as i32, false);
        let big = font((inner_h as f32 / 1.15) as i32, true);
        // FPS.
        let fps = if self.fps_value >= 1.0 { format!("{:.0}", self.fps_value) } else { "--".into() };
        let fw = canvas.text_width(big, &fps);
        canvas.text(big, fps_x + (px(52.0) - fw) / 2, pad_y - px(2.0), &fps, GPU_COLOR);
        // Temperatures and fans.
        for (row, (name, temp, rpm, color)) in
            [("GPU:", m.gpu_temp, m.gpu_rpm, GPU_COLOR), ("CPU:", m.cpu_temp, m.cpu_rpm, CPU_COLOR)]
                .into_iter()
                .enumerate()
        {
            let y = rows[row];
            let t = format!("{name}{} ", unit(temp));
            canvas.text(main, temp_x, y, &t, color);
            if show_fan {
                if let Some(r) = rpm {
                    let tx = temp_x + canvas.text_width(main, &t);
                    let digits = r.to_string();
                    canvas.text(main, tx, y, &digits, color);
                    let dx = tx + canvas.text_width(main, &digits) + px(2.0);
                    canvas.text(small, dx, y + px(1.0), "RPM", color);
                }
            }
        }
        // Load with segmented bars.
        if show_load {
            for (row, (load, color)) in [(gpu_load, GPU_COLOR), (Some(cpu_load), CPU_COLOR)].into_iter().enumerate() {
                let y = rows[row];
                let text = load.map(|l| format!("{l:.0}%")).unwrap_or_default();
                let w = canvas.text_width(main, &text);
                canvas.text(main, load_x + px(30.0) - w, y, &text, color);
                canvas.bar(load_x + px(34.0), y, px(5.0), line_h, sc, load.unwrap_or(0.0), color);
            }
        }
        // System memory (row 2).
        if show_ram {
            if let Some((gb, pct)) = ram {
                let text = format!("{gb:.1}GB");
                let w = canvas.text_width(main, &text);
                canvas.text(main, ram_x + px(54.0) - w, rows[1], &text, CPU_COLOR);
                canvas.bar(ram_x + px(58.0), rows[1], px(5.0), line_h, sc, pct as f32, CPU_COLOR);
            }
        }
        // Battery while unplugged.
        if show_battery {
            let level = m.battery.map(|b| format!("{b}%")).unwrap_or_else(|| "--".into());
            canvas.text(main, bat_x, rows[0], &level, TEXT_LIGHT);
            if let Some(r) = m.rate_w.filter(|r| r.abs() >= 0.1) {
                canvas.text(main, bat_x, rows[1], &format!("{r:+.1}W"), TEXT_LIGHT);
            }
        }
        unsafe {
            for f in [main, small, big] {
                let _ = DeleteObject(f);
            }
        }
        let grab = MOUSE.lock().map(|m| m.grab).unwrap_or(false);
        let alpha = if grab { cfg.overlay_alpha.max(110) } else { cfg.overlay_alpha };
        let pixels = canvas.finish(alpha, px(3.0) as f32);
        let r = window_rect(self.hwnd);
        push(self.hwnd, &pixels, r.left, r.top, width, height);
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        unsafe {
            let _ = KillTimer(self.hwnd, DRAG_TIMER);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// Used system memory in GB and percent.
fn memory_used() -> Option<(f32, u32)> {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut s = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    unsafe { GlobalMemoryStatusEx(&mut s) }.ok()?;
    let used = (s.ullTotalPhys - s.ullAvailPhys) as f32 / (1024.0 * 1024.0 * 1024.0);
    Some((used, s.dwMemoryLoad))
}

fn font(height: i32, bold: bool) -> HFONT {
    unsafe {
        CreateFontW(
            -height.max(6),
            0,
            0,
            0,
            if bold { FW_BOLD.0 } else { FW_NORMAL.0 } as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET.0.into(),
            OUT_DEFAULT_PRECIS.0.into(),
            CLIP_DEFAULT_PRECIS.0.into(),
            ANTIALIASED_QUALITY.0.into(),
            FF_DONTCARE.0.into(),
            w!("Consolas"),
        )
    }
}

/// Solid rectangle drawn after the text: x, y, width, height, colour, alpha.
type Shape = (i32, i32, i32, i32, (u8, u8, u8), u8);

/// A 32-bit DIB that text is drawn into with GDI (coloured on black), then
/// turned into premultiplied pixels over the translucent background.
struct Canvas {
    dc: HDC,
    screen: HDC,
    bitmap: windows::Win32::Graphics::Gdi::HBITMAP,
    old: windows::Win32::Graphics::Gdi::HGDIOBJ,
    bits: *mut u32,
    w: i32,
    h: i32,
    /// Solid shapes drawn after the text: (x, y, w, h, colour).
    shapes: Vec<Shape>,
}

impl Canvas {
    fn new(w: i32, h: i32) -> Self {
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
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0).unwrap_or_default();
            let old = SelectObject(dc, bitmap);
            SetBkMode(dc, TRANSPARENT);
            Self { dc, screen, bitmap, old, bits: bits.cast(), w, h, shapes: Vec::new() }
        }
    }

    fn text_width(&self, font: HFONT, text: &str) -> i32 {
        let wide: Vec<u16> = HSTRING::from(text).as_wide().to_vec();
        let mut size = SIZE::default();
        unsafe {
            let old = SelectObject(self.dc, font);
            let _ = GetTextExtentPoint32W(self.dc, &wide, &mut size);
            SelectObject(self.dc, old);
        }
        size.cx
    }

    fn text(&mut self, font: HFONT, x: i32, y: i32, text: &str, (r, g, b): (u8, u8, u8)) {
        let wide: Vec<u16> = HSTRING::from(text).as_wide().to_vec();
        unsafe {
            let old = SelectObject(self.dc, font);
            SetTextColor(self.dc, COLORREF(u32::from(r) | (u32::from(g) << 8) | (u32::from(b) << 16)));
            let _ = TextOutW(self.dc, x, y, &wide);
            SelectObject(self.dc, old);
        }
    }

    /// Vertical segmented bar, lit from the bottom.
    #[allow(clippy::too_many_arguments)]
    fn bar(&mut self, x: i32, row_y: i32, w: i32, line_h: i32, sc: f32, pct: f32, color: (u8, u8, u8)) {
        let cell = (sc.floor() as i32).max(1);
        let pitch = cell + 1;
        let cells = ((15.0 * sc).floor() as i32 + 1) / pitch;
        let cells = cells.max(2);
        let bar_h = cells * cell + (cells - 1);
        let top = row_y + (line_h - bar_h) / 2;
        let lit = ((pct.clamp(0.0, 100.0) * cells as f32 / 100.0).ceil() as i32).clamp(0, cells);
        for i in 0..cells {
            let y = top + bar_h - (i + 1) * cell - i;
            let a = if i < lit { 255 } else { 110 };
            self.shapes.push((x, y, w, cell, color, a));
        }
    }

    /// Premultiplied BGRA: text over the rounded background.
    fn finish(self, bg_alpha: u8, radius: f32) -> Vec<u32> {
        let n = (self.w * self.h) as usize;
        let mut out = vec![0u32; n];
        if self.bits.is_null() {
            return out;
        }
        let src = unsafe { std::slice::from_raw_parts(self.bits, n) };
        let (w, h) = (self.w as f32, self.h as f32);
        for y in 0..self.h {
            for x in 0..self.w {
                let i = (y * self.w + x) as usize;
                let p = src[i];
                let (r, g, b) = ((p >> 16) & 0xFF, (p >> 8) & 0xFF, p & 0xFF);
                let a = r.max(g).max(b);
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let cx = fx.clamp(radius, w - radius);
                let cy = fy.clamp(radius, h - radius);
                let inside = (radius - ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt() + 0.5).clamp(0.0, 1.0);
                let bg = (f32::from(bg_alpha) * inside) as u32;
                let alpha = a + bg * (255 - a) / 255;
                out[i] = (alpha << 24) | (r << 16) | (g << 8) | b;
            }
        }
        for (x0, y0, sw, sh, (r, g, b), a) in &self.shapes {
            for y in (*y0).max(0)..(y0 + sh).min(self.h) {
                for x in (*x0).max(0)..(x0 + sw).min(self.w) {
                    let i = (y * self.w + x) as usize;
                    let a = u32::from(*a);
                    let pm = |c: u8| u32::from(c) * a / 255;
                    let under = out[i] >> 24;
                    let alpha = a + under * (255 - a) / 255;
                    out[i] = (alpha << 24) | (pm(*r) << 16) | (pm(*g) << 8) | pm(*b);
                }
            }
        }
        out
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap);
            let _ = DeleteDC(self.dc);
            ReleaseDC(None, self.screen);
        }
    }
}

/// Pushes premultiplied pixels to the layered window at (x, y).
fn push(hwnd: HWND, pixels: &[u32], x: i32, y: i32, w: i32, h: i32) {
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
            std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits.cast::<u32>(), pixels.len());
            let old = SelectObject(dc, bitmap);
            let pos = POINT { x, y };
            let size = SIZE { cx: w, cy: h };
            let src = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
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
        }
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
    }
}

/// Reads the overlay metrics from the worker snapshot.
pub fn metrics(worker: &WorkerHandle) -> Metrics {
    let s = worker.shared.lock();
    Metrics {
        cpu_temp: s.readings.cpu_temp,
        gpu_temp: s.readings.gpu_temp,
        cpu_rpm: s.readings.cpu_fan_rpm,
        gpu_rpm: s.readings.gpu_fan_rpm,
        battery: s.power.battery_percent,
        rate_w: s.battery_info.and_then(|b| b.rate_watts()),
        on_ac: s.power.on_ac,
    }
}
