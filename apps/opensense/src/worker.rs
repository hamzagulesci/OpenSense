//! Hardware worker thread. Owns the `NitroDevice` and the OS monitors,
//! executes UI commands and polls sensors, publishing a [`Snapshot`].
//!
//! One worker with a command queue keeps hardware writes ordered.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use opensense_hw::device::{Capabilities, CustomFan, DeviceProfile, DynamicLighting, SensorReadings, StaticLighting};
use opensense_hw::{open_transport, HwError, NitroDevice, TransportKind};
use opensense_proto::mode::{BacklightTimeout, GpuMode, OperationMode};
use opensense_sys::cpu::{CpuMonitor, CpuSample};
use opensense_sys::oem::OemInfo;
use opensense_sys::power::PowerStatus;
use parking_lot::Mutex;

#[derive(Debug, Clone)]
pub enum Command {
    FansAuto,
    FansMax,
    FansCustom(CustomFan),
    CoolBoost(bool),
    Mode(OperationMode),
    StaticLighting(StaticLighting),
    DynamicLighting(DynamicLighting),
    GpuMode(GpuMode),
    Overdrive(bool),
    KbTimeout(BacklightTimeout),
    ChargeLimit(bool),
    SetPollInterval(Duration),
    RefreshState,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Sample {
    /// Seconds since the worker started.
    pub t: f64,
    pub cpu_temp: Option<f32>,
    pub gpu_temp: Option<f32>,
    pub cpu_fan: Option<f32>,
    pub gpu_fan: Option<f32>,
}

/// Everything the UI renders. Cheap to clone the scalar part; history is
/// read under the lock.
#[derive(Debug, Default)]
pub struct Snapshot {
    pub connected: bool,
    pub transport: String,
    /// Probed once after connecting.
    pub caps: Option<Capabilities>,
    pub hw_error: Option<String>,
    /// (translatable label, error text if the call failed)
    pub last_action: Option<(&'static str, Option<String>)>,
    pub readings: SensorReadings,
    pub cpu: CpuSample,
    pub cpu_name: String,
    pub gpu_usage: Option<f32>,
    pub gpu_name: Option<String>,
    pub power: PowerStatus,
    pub mode: Option<OperationMode>,
    pub coolboost: Option<bool>,
    pub battery_boost: Option<bool>,
    pub gpu_mode: Option<GpuMode>,
    pub gpu_mux_supported: bool,
    pub overdrive: Option<bool>,
    pub kb_timeout: Option<BacklightTimeout>,
    pub battery: Option<opensense_proto::battery::BatteryHealth>,
    pub history: VecDeque<Sample>,
    pub cpu_temp_minmax: Option<(f32, f32)>,
    pub gpu_temp_minmax: Option<(f32, f32)>,
}

pub type Shared = Arc<Mutex<Snapshot>>;

fn ok<T>(r: Result<Option<T>, HwError>) -> Option<T> {
    r.ok().flatten()
}

pub struct WorkerHandle {
    pub tx: Sender<Command>,
    pub shared: Shared,
}

/// `monitors = false` skips the OS monitors (CPU, GPU load) for the
/// lightweight tray agent, which only needs firmware sensors.
pub fn spawn(
    kind: TransportKind,
    oem: &OemInfo,
    poll: Duration,
    history_len: usize,
    monitors: bool,
    repaint: impl Fn() + Send + 'static,
) -> WorkerHandle {
    let (tx, rx) = crossbeam_channel::unbounded();
    let shared: Shared = Arc::new(Mutex::new(Snapshot::default()));
    let profile = DeviceProfile {
        backlight_hotkey: oem.backlight_hotkey,
        zone_color_adjust: oem.zone_color_adjust,
        pd_support: oem.pd_support,
    };
    let s = shared.clone();
    std::thread::Builder::new()
        .name("opensense-hw".into())
        .spawn(move || Worker::new(kind, profile, s, history_len, monitors, Box::new(repaint)).run(rx, poll))
        .expect("spawn worker");
    WorkerHandle { tx, shared }
}

struct Worker {
    dev: Option<NitroDevice>,
    shared: Shared,
    history_len: usize,
    repaint: Box<dyn Fn() + Send>,
    cpu: Option<CpuMonitor>,
    #[cfg(windows)]
    gpu_load: Option<opensense_sys::gpu_load::GpuLoadMonitor>,
    started: Instant,
    tick: u64,
    /// For reconnecting after repeated read failures.
    kind: TransportKind,
    profile: DeviceProfile,
    failures: u32,
}

impl Worker {
    fn new(
        kind: TransportKind,
        profile: DeviceProfile,
        shared: Shared,
        history_len: usize,
        monitors: bool,
        repaint: Box<dyn Fn() + Send>,
    ) -> Self {
        let dev = match open_transport(kind) {
            Ok(t) => Some(NitroDevice::new(t, profile.clone())),
            Err(e) => {
                tracing::warn!("hardware unavailable: {e}");
                shared.lock().hw_error = Some(e.to_string());
                None
            }
        };
        let cpu = monitors.then(CpuMonitor::new);
        {
            let mut s = shared.lock();
            s.connected = dev.is_some();
            s.transport = dev.as_ref().map(|d| d.transport_name().to_owned()).unwrap_or_else(|| "none".into());
            s.cpu_name = cpu.as_ref().map(|c| c.brand()).unwrap_or_default();
            if monitors {
                s.gpu_name = opensense_sys::gpu_name::discrete_gpu_name();
            }
        }
        Self {
            dev,
            shared,
            history_len: history_len.max(10),
            repaint,
            cpu,
            #[cfg(windows)]
            gpu_load: if monitors { opensense_sys::gpu_load::GpuLoadMonitor::new().ok() } else { None },
            started: Instant::now(),
            tick: 0,
            kind,
            profile,
            failures: 0,
        }
    }

    fn run(mut self, rx: Receiver<Command>, poll: Duration) {
        // A hand-edited poll_ms of 0 must not turn into a busy loop.
        let mut poll = poll.max(Duration::from_millis(250));
        self.refresh_state();
        self.poll();
        let mut next = Instant::now() + poll;
        loop {
            let timeout = next.saturating_duration_since(Instant::now());
            match rx.recv_timeout(timeout) {
                Ok(Command::SetPollInterval(d)) => {
                    poll = d.max(Duration::from_millis(250));
                    next = Instant::now() + poll;
                }
                Ok(cmd) => {
                    self.execute(cmd);
                    (self.repaint)();
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.poll();
                    next = Instant::now() + poll;
                    (self.repaint)();
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    fn execute(&mut self, cmd: Command) {
        let Some(dev) = self.dev.as_ref() else {
            self.shared.lock().last_action = Some(("No hardware connection", None));
            return;
        };
        let (label, res): (&'static str, Result<(), HwError>) = match &cmd {
            Command::FansAuto => ("Fans: Auto", dev.set_fans_auto()),
            Command::FansMax => ("Fans: Max", dev.set_fans_max()),
            Command::FansCustom(c) => ("Fans: Custom", dev.set_fans_custom(*c)),
            Command::CoolBoost(on) => ("CoolBoost updated", dev.set_coolboost(*on)),
            Command::Mode(m) => ("Operation mode changed", dev.set_operation_mode(*m)),
            Command::StaticLighting(l) => ("Keyboard colours applied", dev.apply_static_lighting(l)),
            Command::DynamicLighting(d) => ("Lighting effect applied", dev.apply_dynamic_lighting(d)),
            Command::GpuMode(m) => ("GPU mode set, reboot to apply", dev.set_gpu_mode(*m)),
            Command::Overdrive(on) => ("LCD overdrive updated", dev.set_lcd_overdrive(*on)),
            Command::KbTimeout(t) => ("Keyboard timeout updated", dev.set_backlight_timeout(*t)),
            Command::ChargeLimit(on) => ("Battery charge limit updated", dev.set_charge_limit(*on)),
            Command::RefreshState => {
                self.refresh_state();
                return;
            }
            Command::SetPollInterval(_) => return,
        };
        if let Err(e) = &res {
            tracing::warn!("{label}: {e}");
        }
        self.shared.lock().last_action = Some((label, res.as_ref().err().map(|e| e.to_string())));
        if matches!(
            cmd,
            Command::Mode(_)
                | Command::CoolBoost(_)
                | Command::GpuMode(_)
                | Command::Overdrive(_)
                | Command::KbTimeout(_)
                | Command::ChargeLimit(_)
        ) {
            self.refresh_state();
        }
    }

    /// Slow-changing firmware state.
    fn refresh_state(&mut self) {
        let Some(dev) = self.dev.as_ref() else { return };
        let mode = ok(dev.operation_mode());
        let coolboost = ok(dev.coolboost());
        let battery_boost = ok(dev.battery_boost());
        let gpu_mode = ok(dev.gpu_mode());
        let gpu_mux_supported = dev.gpu_mode_supported().unwrap_or(false);
        let overdrive = ok(dev.lcd_overdrive());
        let kb_timeout = ok(dev.backlight_timeout());
        let battery = dev.battery_health().ok();
        // Probe once, without holding the UI lock during the WMI calls.
        let needs_probe = self.shared.lock().caps.is_none();
        let caps = needs_probe.then(|| dev.capabilities());
        let mut s = self.shared.lock();
        if caps.is_some() {
            s.caps = caps;
        }
        s.mode = mode;
        s.coolboost = coolboost;
        s.battery_boost = battery_boost;
        s.gpu_mode = gpu_mode;
        s.gpu_mux_supported = gpu_mux_supported;
        s.overdrive = overdrive;
        s.kb_timeout = kb_timeout;
        s.battery = battery;
    }

    fn poll(&mut self) {
        self.tick += 1;
        let readings = match self.dev.as_ref().map(|d| d.readings()) {
            Some(Ok(r)) => {
                self.failures = 0;
                Some(r)
            }
            Some(Err(e)) => {
                // Show "--" rather than freezing the last values, and reopen
                // the WMI connection if it keeps failing.
                self.failures += 1;
                {
                    let mut s = self.shared.lock();
                    s.hw_error = Some(e.to_string());
                    s.readings = SensorReadings::default();
                }
                if self.failures >= 3 {
                    self.failures = 0;
                    if let Ok(t) = open_transport(self.kind) {
                        tracing::info!("reconnected to the firmware");
                        self.dev = Some(NitroDevice::new(t, self.profile.clone()));
                    }
                }
                None
            }
            None => None,
        };
        let cpu = self.cpu.as_mut().map(|c| c.sample()).unwrap_or_default();
        #[cfg(windows)]
        let gpu_load = self.gpu_load.as_mut().and_then(|g| g.sample().ok().flatten());
        #[cfg(not(windows))]
        let gpu_load: Option<f32> = None;
        let power = opensense_sys::power::status().unwrap_or_default();
        if self.tick.is_multiple_of(30) {
            self.refresh_state();
        }

        let mut s = self.shared.lock();
        if let Some(r) = readings {
            s.readings = r;
            s.hw_error = None;
        }
        s.cpu = cpu;
        s.gpu_usage = gpu_load;
        s.power = power;

        let r = s.readings;
        let gpu_temp = r.gpu_temp.map(f32::from);
        let sample = Sample {
            t: self.started.elapsed().as_secs_f64(),
            cpu_temp: r.cpu_temp.map(f32::from),
            gpu_temp,
            cpu_fan: r.cpu_fan_rpm.map(f32::from),
            gpu_fan: r.gpu_fan_rpm.map(f32::from),
        };
        let mm = |cur: Option<(f32, f32)>, v: Option<f32>| match (cur, v) {
            (Some((lo, hi)), Some(v)) => Some((lo.min(v), hi.max(v))),
            (None, Some(v)) => Some((v, v)),
            (c, None) => c,
        };
        s.cpu_temp_minmax = mm(s.cpu_temp_minmax, sample.cpu_temp);
        s.gpu_temp_minmax = mm(s.gpu_temp_minmax, sample.gpu_temp);
        s.history.push_back(sample);
        while s.history.len() > self.history_len {
            s.history.pop_front();
        }
    }
}
