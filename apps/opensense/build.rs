// The procedural logo, shared with the app, rendered into the exe's icon
// resource so Explorer and Task Manager show it.
#[allow(dead_code)]
#[path = "src/icon.rs"]
mod icon;

/// Builds a .ico with 32-bit BMP images (bottom-up BGRA + AND mask).
fn write_ico(path: &std::path::Path) -> std::io::Result<()> {
    let sizes = [16u32, 24, 32, 48, 64, 256];
    let images: Vec<Vec<u8>> = sizes
        .iter()
        .map(|&s| {
            let rgba = icon::rgba(s);
            let mut img = Vec::new();
            // BITMAPINFOHEADER; height is doubled for the AND mask.
            img.extend_from_slice(&40u32.to_le_bytes());
            img.extend_from_slice(&(s as i32).to_le_bytes());
            img.extend_from_slice(&((s * 2) as i32).to_le_bytes());
            img.extend_from_slice(&1u16.to_le_bytes());
            img.extend_from_slice(&32u16.to_le_bytes());
            img.extend_from_slice(&[0u8; 24]);
            for y in (0..s).rev() {
                for x in 0..s {
                    let i = ((y * s + x) * 4) as usize;
                    img.extend_from_slice(&[rgba[i + 2], rgba[i + 1], rgba[i], rgba[i + 3]]);
                }
            }
            let mask_row = (s.div_ceil(32) * 4) as usize;
            img.extend(std::iter::repeat_n(0u8, mask_row * s as usize));
            img
        })
        .collect();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (&s, img) in sizes.iter().zip(&images) {
        let b = if s >= 256 { 0 } else { s as u8 };
        out.extend_from_slice(&[b, b, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(img.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for img in images {
        out.extend_from_slice(&img);
    }
    std::fs::write(path, out)
}

fn main() {
    println!("cargo:rerun-if-changed=src/icon.rs");
    // Embed version info and an asInvoker manifest that opts into per-monitor
    // DPI awareness. Elevation is requested at runtime instead of via the
    // manifest so `cargo run` keeps working from a normal terminal.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set("ProductName", "OpenSense");
        res.set("FileDescription", "OpenSense");
        let ico = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("opensense.ico");
        match write_ico(&ico) {
            Ok(()) => {
                res.set_icon(ico.to_str().unwrap());
            }
            Err(e) => println!("cargo:warning=icon not embedded: {e}"),
        }
        res.set_manifest(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security><requestedPrivileges>
      <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
    </requestedPrivileges></security>
  </trustInfo>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
  <dependency><dependentAssembly>
    <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
      processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
  </dependentAssembly></dependency>
</assembly>"#,
        );
        if let Err(e) = res.compile() {
            println!("cargo:warning=failed to embed Windows resources: {e}");
        }
    }
}
