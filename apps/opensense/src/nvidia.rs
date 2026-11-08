//! NVIDIA GPU clock limit through `nvidia-smi`, which ships with the
//! driver. Locking the graphics clock range needs administrator rights.

use std::process::Command;
use std::sync::Mutex;

/// Upper end of the slider; at this value the limit is removed.
pub const CLOCK_MAX: u32 = 3000;
pub const CLOCK_MIN: u32 = 400;

/// Last limit applied by this process, to avoid re-running the tool.
static APPLIED: Mutex<Option<Option<u32>>> = Mutex::new(None);

fn smi(args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("nvidia-smi.exe");
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Name and highest supported graphics clock (MHz) of the first NVIDIA GPU.
pub fn info() -> Option<(String, u32)> {
    let line = smi(&["--query-gpu=name,clocks.max.graphics", "--format=csv,noheader,nounits"])?;
    let mut parts = line.lines().next()?.split(',').map(str::trim);
    let name = parts.next()?.to_owned();
    let max = parts.next().and_then(|v| v.parse().ok()).unwrap_or(CLOCK_MAX);
    Some((name, max))
}

/// Locks the graphics clock to at most `limit` MHz, or removes the lock.
pub fn apply_clock_limit(limit: Option<u32>) {
    let limit = limit.filter(|&l| l < CLOCK_MAX);
    {
        let mut applied = APPLIED.lock().unwrap();
        if *applied == Some(limit) {
            return;
        }
        // Nothing to undo when no limit was ever set by us.
        if applied.is_none() && limit.is_none() {
            *applied = Some(None);
            return;
        }
        *applied = Some(limit);
    }
    let ok = match limit {
        Some(l) => smi(&["-lgc", &format!("0,{l}")]).is_some(),
        None => smi(&["-rgc"]).is_some(),
    };
    if !ok {
        tracing::warn!("nvidia-smi could not set the clock limit {limit:?}");
    }
}
