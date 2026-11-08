//! Frame rate of the foreground game from the graphics kernel's present
//! events, received through a real-time ETW session (needs administrator
//! rights). Windows 11 reports every presented frame with DxgKrnl event 184;
//! Windows 10 only has DXGI's Present event (id 42), used as a fallback.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use windows::core::{GUID, PCWSTR, PWSTR};
use windows::Win32::System::Diagnostics::Etw::{
    CloseTrace, ControlTraceW, EnableTraceEx2, OpenTraceW, ProcessTrace, StartTraceW, CONTROLTRACE_HANDLE,
    ENABLE_TRACE_PARAMETERS, ENABLE_TRACE_PARAMETERS_VERSION_2, EVENT_CONTROL_CODE_ENABLE_PROVIDER,
    EVENT_FILTER_DESCRIPTOR, EVENT_FILTER_EVENT_ID, EVENT_FILTER_TYPE_EVENT_ID, EVENT_RECORD,
    EVENT_TRACE_CONTROL_FLUSH, EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_LOGFILEW, EVENT_TRACE_PROPERTIES,
    EVENT_TRACE_REAL_TIME_MODE, PROCESSTRACE_HANDLE, PROCESS_TRACE_MODE_EVENT_RECORD, PROCESS_TRACE_MODE_RAW_TIMESTAMP,
    PROCESS_TRACE_MODE_REAL_TIME, WNODE_FLAG_TRACED_GUID,
};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

const SESSION: &str = "OpenSenseFpsSession";
const DXGKRNL: GUID = GUID::from_u128(0x802ec45a_1e99_4b83_9920_87c98277ba9d);
const DXGI: GUID = GUID::from_u128(0xca11c036_0102_4a2d_a6ad_f03cfed5d3c9);
const DXGKRNL_PRESENT_ID: u16 = 184;
const DXGKRNL_PRESENT_KEYWORD: u64 = 0x0000_0000_0800_0000;
const DXGI_PRESENT_ID: u16 = 42;
/// One second of frames at up to 360 fps.
const RING: usize = 360;

static TARGET: AtomicU32 = AtomicU32::new(0);
/// The target has produced a DxgKrnl present (then DXGI events are ignored
/// so frames are not counted twice).
static KERNEL_SEEN: AtomicBool = AtomicBool::new(false);
static FRAMES: Mutex<VecDeque<i64>> = Mutex::new(VecDeque::new());

unsafe extern "system" fn on_event(record: *mut EVENT_RECORD) {
    let Some(r) = (unsafe { record.as_ref() }) else { return };
    let h = &r.EventHeader;
    let target = TARGET.load(Ordering::Relaxed);
    if target == 0 || h.ProcessId != target {
        return;
    }
    let id = h.EventDescriptor.Id;
    let kernel = h.ProviderId == DXGKRNL && id == DXGKRNL_PRESENT_ID;
    let dxgi = h.ProviderId == DXGI && id == DXGI_PRESENT_ID;
    if kernel {
        KERNEL_SEEN.store(true, Ordering::Relaxed);
    } else if !(dxgi && !KERNEL_SEEN.load(Ordering::Relaxed)) {
        return;
    }
    if let Ok(mut f) = FRAMES.lock() {
        if f.len() == RING {
            f.pop_front();
        }
        f.push_back(h.TimeStamp);
    }
}

pub struct FpsMonitor {
    control: CONTROLTRACE_HANDLE,
    trace: PROCESSTRACE_HANDLE,
    props: Vec<u64>,
    stop: std::sync::Arc<AtomicBool>,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// EVENT_TRACE_PROPERTIES followed by room for the session name, in
/// 8-byte aligned storage.
fn properties() -> Vec<u64> {
    let size = std::mem::size_of::<EVENT_TRACE_PROPERTIES>() + 2 * (SESSION.len() + 1);
    let mut buf = vec![0u64; size.div_ceil(8)];
    let p = buf.as_mut_ptr().cast::<EVENT_TRACE_PROPERTIES>();
    unsafe {
        (*p).Wnode.BufferSize = size as u32;
        (*p).Wnode.Flags = WNODE_FLAG_TRACED_GUID;
        // QPC timestamps.
        (*p).Wnode.ClientContext = 1;
        (*p).LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
        (*p).BufferSize = 8;
        (*p).MinimumBuffers = 8;
        (*p).MaximumBuffers = 16;
        (*p).LoggerNameOffset = std::mem::size_of::<EVENT_TRACE_PROPERTIES>() as u32;
    }
    buf
}

fn enable(control: CONTROLTRACE_HANDLE, provider: &GUID, keyword: u64, id: u16) {
    let mut filter =
        EVENT_FILTER_EVENT_ID { FilterIn: windows::Win32::Foundation::BOOLEAN(1), Reserved: 0, Count: 1, Events: [id] };
    let mut desc = EVENT_FILTER_DESCRIPTOR {
        Ptr: std::ptr::addr_of_mut!(filter) as u64,
        Size: std::mem::size_of::<EVENT_FILTER_EVENT_ID>() as u32,
        Type: EVENT_FILTER_TYPE_EVENT_ID,
    };
    let params = ENABLE_TRACE_PARAMETERS {
        Version: ENABLE_TRACE_PARAMETERS_VERSION_2,
        EnableFilterDesc: &mut desc,
        FilterDescCount: 1,
        ..Default::default()
    };
    let code = EVENT_CONTROL_CODE_ENABLE_PROVIDER.0;
    let rc = unsafe { EnableTraceEx2(control, provider, code, 4, keyword, 0, 0, Some(&params)) };
    if rc.is_err() {
        // Retry without the event filter; the callback filters anyway.
        let _ = unsafe { EnableTraceEx2(control, provider, code, 4, keyword, 0, 0, None) };
    }
}

impl FpsMonitor {
    /// Starts the session, or `None` when ETW refuses (not elevated).
    pub fn start() -> Option<Self> {
        let name = wide(SESSION);
        // A session left over from a crash would block the new one.
        let mut stale = properties();
        unsafe {
            let _ = ControlTraceW(
                CONTROLTRACE_HANDLE::default(),
                PCWSTR(name.as_ptr()),
                stale.as_mut_ptr().cast(),
                EVENT_TRACE_CONTROL_STOP,
            );
        }
        let mut props = properties();
        let mut control = CONTROLTRACE_HANDLE::default();
        let rc = unsafe { StartTraceW(&mut control, PCWSTR(name.as_ptr()), props.as_mut_ptr().cast()) };
        if rc.is_err() {
            tracing::warn!("FPS monitor: cannot start the ETW session ({rc:?})");
            return None;
        }
        enable(control, &DXGKRNL, DXGKRNL_PRESENT_KEYWORD, DXGKRNL_PRESENT_ID);
        enable(control, &DXGI, 0, DXGI_PRESENT_ID);

        let mut name_buf = name.clone();
        let mut log = EVENT_TRACE_LOGFILEW { LoggerName: PWSTR(name_buf.as_mut_ptr()), ..Default::default() };
        log.Anonymous1.ProcessTraceMode =
            PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD | PROCESS_TRACE_MODE_RAW_TIMESTAMP;
        log.Anonymous2.EventRecordCallback = Some(on_event);
        let trace = unsafe { OpenTraceW(&mut log) };
        if trace.Value == u64::MAX {
            tracing::warn!("FPS monitor: cannot open the ETW session");
            let _ =
                unsafe { ControlTraceW(control, PCWSTR::null(), props.as_mut_ptr().cast(), EVENT_TRACE_CONTROL_STOP) };
            return None;
        }
        // ProcessTrace blocks until the session is closed.
        std::thread::Builder::new()
            .name("opensense-fps".into())
            .spawn(move || {
                let _ = unsafe { ProcessTrace(&[trace], None, None) };
            })
            .ok()?;
        // Flush often so frames arrive promptly instead of once a second.
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let (flag, control_value) = (stop.clone(), control.Value);
        std::thread::spawn(move || {
            let mut p = properties();
            while !flag.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(250));
                let _ = unsafe {
                    ControlTraceW(
                        CONTROLTRACE_HANDLE { Value: control_value },
                        PCWSTR::null(),
                        p.as_mut_ptr().cast(),
                        EVENT_TRACE_CONTROL_FLUSH,
                    )
                };
            }
        });
        Some(Self { control, trace, props, stop })
    }

    /// Process whose frames are counted (0 = none).
    pub fn set_target(&self, pid: u32) {
        if TARGET.swap(pid, Ordering::Relaxed) != pid {
            KERNEL_SEEN.store(false, Ordering::Relaxed);
            if let Ok(mut f) = FRAMES.lock() {
                f.clear();
            }
        }
    }

    /// Frames per second over the last second of presents (0 when idle).
    pub fn sample(&self) -> f32 {
        let (mut now, mut freq) = (0i64, 0i64);
        unsafe {
            let _ = QueryPerformanceCounter(&mut now);
            let _ = QueryPerformanceFrequency(&mut freq);
        }
        let Ok(f) = FRAMES.lock() else { return 0.0 };
        fps_from(&f, now, freq)
    }
}

/// FPS from present timestamps (QPC ticks): frames within one second of
/// the newest one, 0 when fewer than two or the newest is over 4 s old.
pub fn fps_from(frames: &VecDeque<i64>, now: i64, freq: i64) -> f32 {
    let (Some(&newest), true) = (frames.back(), freq > 0 && frames.len() >= 2) else { return 0.0 };
    if now - newest > 4 * freq {
        return 0.0;
    }
    let mut oldest = newest;
    let mut count = 0;
    for &t in frames.iter().rev() {
        if newest - t > freq {
            break;
        }
        oldest = t;
        count += 1;
    }
    let span = (newest - oldest) as f32 / freq as f32;
    if count < 2 || span <= 0.0 {
        0.0
    } else {
        (count - 1) as f32 / span
    }
}

impl Drop for FpsMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        TARGET.store(0, Ordering::Relaxed);
        unsafe {
            let _ =
                ControlTraceW(self.control, PCWSTR::null(), self.props.as_mut_ptr().cast(), EVENT_TRACE_CONTROL_STOP);
            let _ = CloseTrace(self.trace);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fps_window() {
        let freq = 1000;
        // 60 frames over the last second.
        let frames: VecDeque<i64> = (0..61).map(|i| 10_000 + i * 1000 / 60).collect();
        let fps = fps_from(&frames, 11_000, freq);
        assert!((fps - 60.0).abs() < 1.5, "{fps}");
        // Stale: newest frame 5 s ago.
        assert_eq!(fps_from(&frames, 16_000, freq), 0.0);
        assert_eq!(fps_from(&VecDeque::new(), 0, freq), 0.0);
    }
}
