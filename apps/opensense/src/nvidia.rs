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
///
/// The lock is held across the `nvidia-smi` run so two mode switches in
/// quick succession apply in order, and the state is recorded only when
/// the tool succeeded (a failure is retried next time). `force` runs the
/// tool even when this process believes nothing changed: a lock set by
/// another OpenSense process (the tray agent) is not visible here.
pub fn set_clock_limit(limit: Option<u32>, force: bool) {
    let limit = limit.filter(|&l| l < CLOCK_MAX);
    let mut applied = APPLIED.lock().unwrap_or_else(|e| e.into_inner());
    if !force {
        if *applied == Some(limit) {
            return;
        }
        // At start with no limit wanted: nothing of ours to undo.
        if applied.is_none() && limit.is_none() {
            *applied = Some(None);
            return;
        }
    }
    let ok = match limit {
        Some(l) => smi(&["-lgc", &format!("0,{l}")]).is_some(),
        None => smi(&["-rgc"]).is_some(),
    };
    if ok {
        *applied = Some(limit);
    } else {
        tracing::warn!("nvidia-smi could not set the clock limit {limit:?}");
    }
}

/// Applies a mode's clock limit (skipped when already applied).
pub fn apply_clock_limit(limit: Option<u32>) {
    set_clock_limit(limit, false);
}

/// On exit: removes a clock lock this process set.
pub fn release_clock_limit() {
    let set = matches!(*APPLIED.lock().unwrap_or_else(|e| e.into_inner()), Some(Some(_)));
    if set {
        set_clock_limit(None, true);
    }
}
