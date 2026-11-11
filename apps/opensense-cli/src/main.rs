use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use opensense_hw::device::{CustomFan, DeviceProfile, DynamicLighting, StaticLighting};
use opensense_hw::{open_transport, NitroDevice, TransportKind};
use opensense_proto::lighting::{self, Direction, Effect, Rgb};
use opensense_proto::mode::{self, BacklightTimeout, GpuMode, OperationMode};
use opensense_proto::sensor::Sensor;
use opensense_proto::GamingMethod as M;
use opensense_sys::oem::OemInfo;

#[derive(Parser)]
#[command(name = "opensense-cli", version, about = "OpenSense command-line tool for Acer Nitro laptops")]
struct Cli {
    /// Hardware transport: wmi (real hardware, needs admin) or mock.
    #[arg(long, global = true, default_value = "wmi")]
    transport: TransportKind,
    /// Verbose logging (-v debug, -vv trace).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show temperatures, fan speeds and current modes.
    Status,
    /// Read-only dump of every firmware getter with raw values.
    Probe,
    /// Fan control.
    Fan {
        #[arg(value_enum)]
        mode: FanArg,
        /// CPU fan percent for custom mode (omit = auto).
        #[arg(long)]
        cpu: Option<u8>,
        /// GPU fan percent for custom mode (omit = auto).
        #[arg(long)]
        gpu: Option<u8>,
    },
    /// CoolBoost on/off.
    Coolboost {
        #[arg(value_enum)]
        state: OnOff,
    },
    /// Operation mode (Quiet / Default / Extreme).
    Mode {
        #[arg(value_enum)]
        mode: ModeArg,
    },
    /// Keyboard lighting.
    #[command(subcommand)]
    Kb(KbCmd),
    /// GPU MUX mode (applies after reboot).
    GpuMode {
        #[arg(value_enum)]
        mode: GpuArg,
    },
    /// LCD overdrive.
    Overdrive {
        #[arg(value_enum)]
        state: OnOff,
    },
    /// Keyboard backlight auto-off after 30 s idle.
    BacklightTimeout {
        #[arg(value_enum)]
        state: OnOff,
    },
    /// Windows power plans.
    #[command(subcommand)]
    Power(PowerCmd),
    /// Battery health: 80 % charge limit.
    Battery {
        /// status | on | off
        #[arg(default_value = "status")]
        action: String,
    },
    /// Diagnostics: call one WMI method with a raw value (hex).
    Raw {
        /// get | set | set-bytes
        op: String,
        /// e.g. SetGamingRgbKb or APGeAction.GetFunction
        method: String,
        /// Input value in hex (0x optional).
        value: String,
    },
}

#[derive(Subcommand)]
enum KbCmd {
    /// Static colours. Pass 1 colour (all zones) or 4.
    Static {
        /// Comma separated #RRGGBB values.
        #[arg(long, default_value = "#FF0000")]
        colors: String,
        /// Brightness level 1..5 (1 = off).
        #[arg(long, default_value_t = 5)]
        level: u8,
        /// Zones to light, e.g. 1,2,3,4.
        #[arg(long, default_value = "1,2,3,4")]
        zones: String,
    },
    /// Dynamic effect.
    Effect {
        #[arg(value_enum)]
        effect: EffectArg,
        #[arg(long, default_value_t = 5)]
        speed: u8,
        #[arg(long, default_value_t = 5)]
        level: u8,
        #[arg(long, value_enum, default_value = "left")]
        direction: DirArg,
        #[arg(long, default_value = "#FF0000")]
        color: String,
    },
}

#[derive(Subcommand)]
enum PowerCmd {
    /// List power plans.
    List,
    /// Activate a plan by GUID.
    Set { guid: String },
}

#[derive(Clone, Copy, ValueEnum)]
enum FanArg {
    Auto,
    Max,
    Custom,
}
#[derive(Clone, Copy, ValueEnum)]
enum OnOff {
    On,
    Off,
}
#[derive(Clone, Copy, ValueEnum)]
enum ModeArg {
    Quiet,
    Default,
    Extreme,
}
#[derive(Clone, Copy, ValueEnum)]
enum GpuArg {
    Hybrid,
    Discrete,
}
#[derive(Clone, Copy, ValueEnum)]
enum EffectArg {
    Breathing,
    Neon,
    Wave,
    Shifting,
    Zoom,
}
#[derive(Clone, Copy, ValueEnum)]
enum DirArg {
    Left,
    Right,
}

impl From<OnOff> for bool {
    fn from(v: OnOff) -> bool {
        matches!(v, OnOff::On)
    }
}

fn parse_rgb(s: &str) -> Result<Rgb> {
    Rgb::from_hex(s).with_context(|| format!("invalid colour '{s}', expected #RRGGBB"))
}

fn open_device(kind: TransportKind) -> Result<NitroDevice> {
    let oem = OemInfo::detect();
    let transport = open_transport(kind).map_err(|e| {
        if !opensense_sys::privilege::is_elevated() && kind == TransportKind::Wmi {
            anyhow::anyhow!("{e}\nhint: run this from an elevated (Administrator) terminal")
        } else {
            anyhow::anyhow!(e)
        }
    })?;
    let profile = DeviceProfile {
        backlight_hotkey: oem.backlight_hotkey,
        zone_color_adjust: oem.zone_color_adjust,
        pd_support: oem.pd_support,
    };
    Ok(NitroDevice::new(transport, profile))
}

fn show<T: std::fmt::Debug>(label: &str, r: Result<T, opensense_hw::HwError>) {
    match r {
        Ok(v) => println!("{label:<22} {v:?}"),
        Err(e) => println!("{label:<22} error: {e}"),
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let filter = match cli.verbose {
        0 => "warn",
        1 => "debug",
        _ => "trace",
    };
    tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).with_ansi(false).init();

    match cli.cmd {
        Cmd::Power(PowerCmd::List) => {
            for p in opensense_sys::power::plans()? {
                println!("{} {}  {}", if p.active { "*" } else { " " }, p.guid, p.name);
            }
            return Ok(());
        }
        Cmd::Power(PowerCmd::Set { guid }) => {
            opensense_sys::power::set_active(&guid)?;
            return Ok(());
        }
        _ => {}
    }

    let dev = open_device(cli.transport)?;
    match cli.cmd {
        Cmd::Status => {
            let oem = OemInfo::detect();
            println!("transport              {}", dev.transport_name());
            println!("model                  {}", oem.model.as_deref().unwrap_or("?"));
            println!("machine type           {:?}", oem.machine_type);
            let r = dev.readings()?;
            let t = |v: Option<u16>| v.map(|x| format!("{x} °C")).unwrap_or_else(|| "--".into());
            let f = |v: Option<u16>| v.map(|x| format!("{x} RPM")).unwrap_or_else(|| "--".into());
            println!("CPU temperature        {}", t(r.cpu_temp));
            println!("GPU temperature        {}", t(r.gpu_temp));
            println!("CPU fan                {}", f(r.cpu_fan_rpm));
            println!("GPU fan                {}", f(r.gpu_fan_rpm));
            show("operation mode", dev.operation_mode());
            show("coolboost", dev.coolboost());
            show("battery boost", dev.battery_boost());
            show("AC (firmware)", dev.ac_from_firmware());
            show("GPU mode", dev.gpu_mode());
            show("keyboard timeout", dev.backlight_timeout());
            if let Ok(mut g) = opensense_sys::gpu_load::GpuLoadMonitor::new() {
                std::thread::sleep(std::time::Duration::from_secs(1));
                if let Ok(Some(load)) = g.sample() {
                    println!("GPU load               {load:.0} %");
                }
            }
            if let Ok(p) = opensense_sys::power::status() {
                println!("power                  ac={} battery={:?}%", p.on_ac, p.battery_percent);
            }
            if let Ok(b) = opensense_sys::battery::info() {
                let w = b.rate_watts().map(|w| format!("{w:+.1} W")).unwrap_or_else(|| "--".into());
                let h = b.health_percent().map(|h| format!("{h:.1} %")).unwrap_or_else(|| "--".into());
                let c = b.cycles.map(|c| c.to_string()).unwrap_or_else(|| "--".into());
                println!("battery                rate {w}, health {h}, cycles {c}");
            }
        }
        Cmd::Probe => probe(&dev),
        Cmd::Raw { op, method, value } => {
            let m = M::from_name(&method).with_context(|| format!("unknown method '{method}'"))?;
            let v = u64::from_str_radix(value.trim_start_matches("0x"), 16).context("value must be hex")?;
            match op.as_str() {
                "get" => {
                    let input = u32::try_from(v).context("get takes a 32-bit value")?;
                    println!("{}({v:#x}) -> {:#018x}", m.qualified_name(), dev.raw_get(m, input)?)
                }
                "set" => println!("{}({v:#x}) -> status {:#x}", m.qualified_name(), dev.raw_set(m, v)?),
                "set-bytes" => {
                    println!("{}({v:#x} as 16 bytes) -> status {:#x}", m.qualified_name(), dev.raw_set_bytes(m, v)?)
                }
                other => bail!("unknown op '{other}' (get|set|set-bytes)"),
            }
        }
        Cmd::Fan { mode: FanArg::Auto | FanArg::Max, cpu, gpu } if cpu.is_some() || gpu.is_some() => {
            bail!("--cpu / --gpu only apply to custom mode")
        }
        Cmd::Fan { mode, cpu, gpu } => match mode {
            FanArg::Auto => dev.set_fans_auto()?,
            FanArg::Max => dev.set_fans_max()?,
            FanArg::Custom => {
                if cpu.is_none() && gpu.is_none() {
                    bail!("custom mode needs --cpu and/or --gpu (percent)");
                }
                dev.set_fans_custom(CustomFan { cpu_percent: cpu, gpu_percent: gpu })?
            }
        },
        Cmd::Coolboost { state } => dev.set_coolboost(state.into())?,
        Cmd::Mode { mode } => {
            let m = match mode {
                ModeArg::Quiet => OperationMode::Quiet,
                ModeArg::Default => OperationMode::Default,
                ModeArg::Extreme => OperationMode::Extreme,
            };
            if m.needs_full_power() && dev.battery_boost().ok().flatten() == Some(false) {
                eprintln!("warning: {} needs AC power with the full-power adapter", m.label());
            }
            dev.set_operation_mode(m)?;
        }
        Cmd::Kb(KbCmd::Static { colors, level, zones }) => {
            let list: Vec<Rgb> = colors.split(',').map(parse_rgb).collect::<Result<_>>()?;
            let zones_rgb = match list.len() {
                1 => [list[0]; 4],
                4 => [list[0], list[1], list[2], list[3]],
                n => bail!("expected 1 or 4 colours, got {n}"),
            };
            let mut enabled = [false; 4];
            for z in zones.split(',') {
                let i: usize = z.trim().parse().context("zones are 1..4")?;
                if !(1..=4).contains(&i) {
                    bail!("zones are 1..4");
                }
                enabled[i - 1] = true;
            }
            dev.apply_static_lighting(&StaticLighting {
                zones: zones_rgb,
                enabled,
                brightness_percent: lighting::level_to_percent(level),
            })?;
        }
        Cmd::Kb(KbCmd::Effect { effect, speed, level, direction, color }) => {
            let effect = match effect {
                EffectArg::Breathing => Effect::Breathing,
                EffectArg::Neon => Effect::Neon,
                EffectArg::Wave => Effect::Wave,
                EffectArg::Shifting => Effect::Shifting,
                EffectArg::Zoom => Effect::Zoom,
            };
            dev.apply_dynamic_lighting(&DynamicLighting {
                effect,
                speed,
                brightness_percent: lighting::level_to_percent(level),
                direction: match direction {
                    DirArg::Left => Direction::Left,
                    DirArg::Right => Direction::Right,
                },
                color: parse_rgb(&color)?,
            })?;
        }
        Cmd::GpuMode { mode } => {
            if !dev.gpu_mode_supported()? {
                bail!("this machine does not report a switchable GPU MUX");
            }
            dev.set_gpu_mode(match mode {
                GpuArg::Hybrid => GpuMode::Hybrid,
                GpuArg::Discrete => GpuMode::Discrete,
            })?;
            println!("GPU mode set; reboot to apply.");
        }
        Cmd::Overdrive { state } => dev.set_lcd_overdrive(state.into())?,
        Cmd::Battery { action } => {
            match action.as_str() {
                "status" => {}
                "on" => dev.set_charge_limit(true)?,
                "off" => dev.set_charge_limit(false)?,
                other => bail!("unknown action '{other}' (status|on|off)"),
            }
            let h = dev.battery_health()?;
            println!("80% charge limit       supported={} active={}", h.limit_supported, h.limit_active);
            println!("calibration            supported={} active={}", h.calibration_supported, h.calibration_active);
        }
        Cmd::BacklightTimeout { state } => {
            let current = dev.backlight_timeout()?.unwrap_or(BacklightTimeout { auto_off: false, brightness: 100 });
            dev.set_backlight_timeout(BacklightTimeout { auto_off: state.into(), ..current })?;
        }
        Cmd::Power(_) => unreachable!(),
    }
    Ok(())
}

/// Read-only: every firmware getter, raw and decoded. Useful to
/// verify the inferred WMI mappings on a new model.
fn probe(dev: &NitroDevice) {
    println!("transport: {}\n", dev.transport_name());
    println!("GetGamingSysInfo:");
    for s in Sensor::ALL {
        match dev.sensor(s) {
            Ok(v) => println!("  {:#06x} {:<22} {:?}", s.request(), s.label(), v),
            Err(e) => println!("  {:#06x} {:<22} error: {e}", s.request(), s.label()),
        }
    }
    let hk = dev.profile().backlight_hotkey;
    let raw: [(&str, M, u32); 9] = [
        ("battery boost", M::GetGamingSysInfo, opensense_proto::sensor::SYSINFO_BATTERY),
        ("coolboost", M::ApgeGetFunction, opensense_proto::fan::COOLBOOST_GET),
        ("AC (PD)", M::ApgeGetFunction, mode::PROFILE_GET_AC),
        ("Type-C DP on dGPU", M::ApgeGetFunction, mode::PROFILE_GET_TYPEC_DP),
        ("kb backlight timeout", M::ApgeGetFunction, mode::backlight_get_word(hk)),
        ("operation mode", M::GetGamingMiscSetting, u32::from(mode::MISC_PLATFORM_PROFILE)),
        ("GPU mode", M::GetGamingMiscSetting, u32::from(mode::MISC_GPU_MODE)),
        ("GPU MUX support", M::GetGamingMiscSetting, u32::from(mode::MISC_GPU_MODE_SUPPORT)),
        ("gaming profile", M::GetGamingProfile, 0),
    ];
    println!("\nraw getters (method(input) -> output):");
    for (label, method, input) in raw {
        match dev.raw_get(method, input) {
            Ok(v) => println!("  {label:<22} {}({input:#x}) -> {v:#018x}", method.qualified_name()),
            Err(e) => println!("  {label:<22} {}({input:#x}) error: {e}", method.qualified_name()),
        }
    }
    println!("\ndecoded:");
    show("  operation mode", dev.operation_mode());
    show("  coolboost", dev.coolboost());
    show("  battery boost", dev.battery_boost());
    show("  GPU mode", dev.gpu_mode());
    show("  lcd overdrive", dev.lcd_overdrive());
    show("  kb timeout", dev.backlight_timeout());
}
