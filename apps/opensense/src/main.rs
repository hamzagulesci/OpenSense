#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod apply;
mod i18n;
mod icon;
mod tray;
mod widgets;
mod worker;

use eframe::egui;
use opensense_hw::TransportKind;

struct Args {
    transport: TransportKind,
    /// Run only the tray agent (`--tray`; `--minimized` is accepted for
    /// older autostart entries).
    tray: bool,
    /// Started by "Restart as administrator": the previous window may still
    /// be shutting down.
    relaunched: bool,
}

fn parse_args() -> Args {
    let mut a = Args { transport: TransportKind::Wmi, tray: false, relaunched: false };
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--mock" => a.transport = TransportKind::Mock,
            "--tray" | "--minimized" => a.tray = true,
            "--relaunched" => a.relaunched = true,
            _ => {}
        }
    }
    a
}

/// One window per session. After a relaunch, waits up to 3 s for the
/// previous window process to exit.
fn single_instance(relaunched: bool) -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;
    let attempts = if relaunched { 30 } else { 1 };
    for _ in 0..attempts {
        unsafe {
            let h = CreateMutexW(None, true, w!("Local\\OpenSense.Window"));
            if GetLastError() != ERROR_ALREADY_EXISTS {
                // Keep the handle for the process lifetime.
                std::mem::forget(h);
                return true;
            }
            if let Ok(h) = h {
                let _ = CloseHandle(h);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    false
}

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_env("OPENSENSE_LOG").unwrap_or_else(|_| "info".into()))
        .init();
    let args = parse_args();
    if args.tray {
        tray::run(args.transport);
        return Ok(());
    }
    if !single_instance(args.relaunched) {
        tracing::info!("OpenSense window is already open");
        return Ok(());
    }
    let icon = egui::IconData { rgba: icon::rgba(256), width: 256, height: 256 };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("OpenSense")
            .with_inner_size([1040.0, 680.0])
            .with_min_inner_size([820.0, 560.0])
            .with_icon(icon),
        // No depth/stencil/MSAA buffers: egui only needs a colour buffer.
        depth_buffer: 0,
        stencil_buffer: 0,
        multisampling: 0,
        vsync: true,
        persist_window: false,
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "OpenSense",
        options,
        Box::new(move |cc| Ok(Box::new(app::OpenSenseApp::new(cc, args.transport)))),
    )
}
