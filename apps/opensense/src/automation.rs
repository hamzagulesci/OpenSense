//! Background behaviour of the tray agent: global keyboard shortcuts, the
//! keyboard-backlight idle timeout, remembering a mode per power source,
//! clamshell mode and the Windows power tweaks from the Extra window.

use opensense_proto::mode::OperationMode;
use opensense_sys::config::AppConfig;
use opensense_sys::tweaks::{self, setting};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, VK_F12, VK_F5,
    VK_VOLUME_DOWN, VK_VOLUME_UP,
};

use crate::i18n::{tr, Lang};
use crate::osd::Osd;
use crate::worker::WorkerHandle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hotkey {
    CycleMode,
    CycleModeBack,
    ToggleWindow,
    Quiet,
    Default,
    Extreme,
    BacklightDown,
    BacklightUp,
    Overlay,
}

/// Shortcut table: (action, modifiers, key, label for the Extra window).
pub const HOTKEYS: [(Hotkey, &str); 9] = [
    (Hotkey::CycleMode, "Ctrl + Shift + F5"),
    (Hotkey::CycleModeBack, "Ctrl + Shift + Alt + F5"),
    (Hotkey::ToggleWindow, "Ctrl + Shift + F12"),
    (Hotkey::Quiet, "Ctrl + Shift + Alt + 1"),
    (Hotkey::Default, "Ctrl + Shift + Alt + 2"),
    (Hotkey::Extreme, "Ctrl + Shift + Alt + 3"),
    (Hotkey::BacklightDown, "Shift + Volume Down"),
    (Hotkey::BacklightUp, "Shift + Volume Up"),
    (Hotkey::Overlay, "Ctrl + Shift + Alt + O"),
];

impl Hotkey {
    pub fn label(self) -> &'static str {
        match self {
            Hotkey::CycleMode => "Next performance mode",
            Hotkey::CycleModeBack => "Previous performance mode",
            Hotkey::ToggleWindow => "Show / hide OpenSense",
            Hotkey::Quiet => "Quiet mode",
            Hotkey::Default => "Default mode",
            Hotkey::Extreme => "Extreme mode",
            Hotkey::BacklightDown => "Keyboard backlight down",
            Hotkey::BacklightUp => "Keyboard backlight up",
            Hotkey::Overlay => "In-game overlay on / off",
        }
    }

    fn binding(self) -> (HOT_KEY_MODIFIERS, u32) {
        let cs = MOD_CONTROL | MOD_SHIFT;
        let csa = cs | MOD_ALT;
        match self {
            Hotkey::CycleMode => (cs, u32::from(VK_F5.0)),
            Hotkey::CycleModeBack => (csa, u32::from(VK_F5.0)),
            Hotkey::ToggleWindow => (cs, u32::from(VK_F12.0)),
            Hotkey::Quiet => (csa, u32::from(b'1')),
            Hotkey::Default => (csa, u32::from(b'2')),
            Hotkey::Extreme => (csa, u32::from(b'3')),
            Hotkey::BacklightDown => (MOD_SHIFT, u32::from(VK_VOLUME_DOWN.0)),
            Hotkey::BacklightUp => (MOD_SHIFT, u32::from(VK_VOLUME_UP.0)),
            Hotkey::Overlay => (csa, u32::from(b'O')),
        }
    }

    pub fn from_id(id: usize) -> Option<Self> {
        HOTKEYS.get(id.checked_sub(1)?).map(|(h, _)| *h)
    }
}

pub struct Automation {
    pub osd: Osd,
    registered: bool,
    /// Backlight switched off by the idle timeout.
    kb_dimmed: bool,
    last_ac: Option<bool>,
    last_key: Option<String>,
    /// Lid action currently forced to "do nothing".
    clamshell_active: bool,
    ticks: u32,
}

impl Automation {
    pub fn new() -> Self {
        Self {
            osd: Osd::new(),
            registered: false,
            kb_dimmed: false,
            last_ac: None,
            last_key: None,
            clamshell_active: false,
            ticks: 0,
        }
    }

    /// Registers or removes the global shortcuts (on this thread's queue).
    pub fn set_hotkeys(&mut self, enabled: bool) {
        if enabled == self.registered {
            return;
        }
        for (i, (h, label)) in HOTKEYS.iter().enumerate() {
            let id = i as i32 + 1;
            unsafe {
                if enabled {
                    let (mods, vk) = h.binding();
                    if RegisterHotKey(None, id, mods | MOD_NOREPEAT, vk).is_err() {
                        tracing::warn!("cannot register shortcut {label}");
                    }
                } else {
                    let _ = UnregisterHotKey(None, id);
                }
            }
        }
        self.registered = enabled;
    }

    fn toast(&mut self, cfg: &AppConfig, text: &str) {
        if !cfg.disable_osd {
            self.osd.show(text);
        }
    }

    /// Runs a shortcut. Returns true when the config changed (save it).
    pub fn hotkey(&mut self, h: Hotkey, cfg: &mut AppConfig, worker: &WorkerHandle, lang: Lang, mock: bool) -> bool {
        match h {
            Hotkey::ToggleWindow => {
                crate::tray::toggle_window(mock);
                false
            }
            Hotkey::CycleMode | Hotkey::CycleModeBack => {
                let list = crate::modes::ordered(cfg);
                let cur = crate::modes::current_key(cfg);
                let full_power = full_power(worker);
                let n = list.len();
                let start = list.iter().position(|k| *k == cur).unwrap_or(0);
                let step = if h == Hotkey::CycleMode { 1 } else { n - 1 };
                // Skip modes based on Extreme while it cannot run.
                let next = (1..=n)
                    .map(|i| list[(start + step * i) % n].clone())
                    .find(|k| full_power || crate::modes::base_of(cfg, k) != OperationMode::Extreme);
                match next {
                    Some(k) => self.switch(cfg, &k, worker, lang),
                    None => false,
                }
            }
            Hotkey::Quiet | Hotkey::Default | Hotkey::Extreme => {
                let m = match h {
                    Hotkey::Quiet => OperationMode::Quiet,
                    Hotkey::Default => OperationMode::Default,
                    _ => OperationMode::Extreme,
                };
                if m.needs_full_power() && !full_power(worker) {
                    self.toast(cfg, tr(lang, "Extreme needs AC power"));
                    return false;
                }
                self.switch(cfg, &m.label().to_ascii_lowercase(), worker, lang)
            }
            Hotkey::Overlay => {
                cfg.overlay = !cfg.overlay;
                let text = if cfg.overlay { "Overlay On" } else { "Overlay Off" };
                self.toast(cfg, tr(lang, text));
                true
            }
            Hotkey::BacklightDown | Hotkey::BacklightUp => {
                let l = &mut cfg.lighting;
                let level = if h == Hotkey::BacklightUp {
                    (l.brightness_level + 1).min(5)
                } else {
                    l.brightness_level.saturating_sub(1).max(1)
                };
                l.brightness_level = level;
                self.kb_dimmed = false;
                let _ = worker.tx.send(crate::apply::lighting(&cfg.lighting));
                let text = if level <= 1 {
                    tr(lang, "Off").to_owned()
                } else {
                    format!("{}%", opensense_proto::lighting::level_to_percent(level))
                };
                self.toast(cfg, &text);
                true
            }
        }
    }

    /// Switches mode with a toast. Returns true (config changed).
    pub fn switch(&mut self, cfg: &mut AppConfig, key: &str, worker: &WorkerHandle, lang: Lang) -> bool {
        crate::modes::switch(cfg, key, worker);
        let name = crate::modes::display_name(cfg, lang, key);
        self.toast(cfg, &name);
        true
    }

    /// Applies the power tweaks chosen in the Extra window (idempotent).
    pub fn apply_tweaks(&mut self, cfg: &AppConfig) {
        let cfg = cfg.clone();
        std::thread::spawn(move || {
            if cfg.aspm_off {
                let _ = tweaks::write(setting::ASPM, Some(0), None);
            }
            if cfg.standby_network_off {
                let _ = tweaks::write(setting::STANDBY_NETWORK, Some(0), Some(0));
            }
        });
    }

    /// Once a second. Returns true when the config changed (save it).
    pub fn tick(&mut self, cfg: &mut AppConfig, worker: &WorkerHandle, lang: Lang) -> bool {
        self.osd.tick();
        self.ticks += 1;
        let mut changed = false;
        let on_ac = worker.shared.lock().power.on_ac;
        let key = crate::modes::current_key(cfg);

        // Remember the mode per power source; restore it when the source
        // changes.
        match self.last_ac {
            Some(prev) if prev != on_ac => {
                let wanted = if on_ac { cfg.mode_ac.clone() } else { cfg.mode_dc.clone() };
                if let Some(k) = wanted.filter(|k| *k != key && crate::modes::ordered(cfg).contains(k)) {
                    let ok = on_ac || crate::modes::base_of(cfg, &k) != OperationMode::Extreme;
                    if ok {
                        changed |= self.switch(cfg, &k, worker, lang);
                    }
                }
                self.kb_dimmed = false;
                let _ = worker.tx.send(crate::apply::lighting(&cfg.lighting));
            }
            _ => {
                if self.last_key.as_deref() != Some(key.as_str()) {
                    let slot = if on_ac { &mut cfg.mode_ac } else { &mut cfg.mode_dc };
                    if slot.as_deref() != Some(key.as_str()) {
                        *slot = Some(key.clone());
                        changed = true;
                    }
                }
            }
        }
        self.last_ac = Some(on_ac);
        self.last_key = Some(crate::modes::current_key(cfg));

        // Keyboard backlight idle timeout.
        let timeout = if on_ac { cfg.kb_timeout_ac } else { cfg.kb_timeout_battery };
        if timeout > 0 && cfg.lighting.brightness_level > 1 {
            let idle = tweaks::idle_seconds();
            if !self.kb_dimmed && idle >= timeout {
                let mut off = cfg.lighting.clone();
                off.brightness_level = 1;
                let _ = worker.tx.send(crate::apply::lighting(&off));
                self.kb_dimmed = true;
            } else if self.kb_dimmed && idle < timeout {
                let _ = worker.tx.send(crate::apply::lighting(&cfg.lighting));
                self.kb_dimmed = false;
            }
        } else if self.kb_dimmed {
            let _ = worker.tx.send(crate::apply::lighting(&cfg.lighting));
            self.kb_dimmed = false;
        }

        // Clamshell: lid closes without sleep while plugged in with an
        // external display (checked every 2 s).
        if self.ticks.is_multiple_of(2) {
            let want = cfg.clamshell && on_ac && tweaks::monitor_count() > 1;
            if want != self.clamshell_active {
                if want {
                    if cfg.clamshell_default_lid.is_none() {
                        let current = tweaks::read(setting::LID_ACTION).map(|v| v.0).unwrap_or(1);
                        cfg.clamshell_default_lid = Some(if current == 0 { 1 } else { current });
                        changed = true;
                    }
                    let _ = tweaks::write(setting::LID_ACTION, Some(0), None);
                } else {
                    let _ = tweaks::write(setting::LID_ACTION, Some(cfg.clamshell_default_lid.unwrap_or(1)), None);
                }
                self.clamshell_active = want;
            }
        }
        changed
    }
}

fn full_power(worker: &WorkerHandle) -> bool {
    let s = worker.shared.lock();
    s.power.on_ac && s.battery_boost != Some(false)
}
