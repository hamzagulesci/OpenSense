//! Discrete GPU name from the display-adapter class in the registry, which
//! costs nothing compared with loading a vendor library.

const DISPLAY_CLASS: &str = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";

/// Prefers an NVIDIA/AMD adapter over the integrated one.
#[cfg(windows)]
pub fn discrete_gpu_name() -> Option<String> {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let mut names = Vec::new();
    for i in 0..16 {
        let key = HSTRING::from(format!("{DISPLAY_CLASS}\\{i:04}"));
        let value = HSTRING::from("DriverDesc");
        let mut buf = [0u16; 256];
        let mut len = (buf.len() * 2) as u32;
        let rc = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                &key,
                &value,
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut len),
            )
        };
        if rc.is_ok() {
            let n = (len as usize / 2).saturating_sub(1);
            names.push(String::from_utf16_lossy(&buf[..n]));
        }
    }
    // AMD-CPU laptops list their integrated "AMD Radeon(TM) Graphics" too,
    // so NVIDIA wins first, then a Radeon RX part.
    let has = |n: &String, s: &str| n.to_ascii_lowercase().contains(s);
    names
        .iter()
        .find(|n| has(n, "nvidia"))
        .or_else(|| names.iter().find(|n| has(n, "radeon rx")))
        .or(names.first())
        .cloned()
}

#[cfg(not(windows))]
pub fn discrete_gpu_name() -> Option<String> {
    None
}
