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

/// Settings changes recorded field by field, to be applied again to what
/// the settings file holds when they are saved.
#[derive(Default)]
pub struct Edits(Vec<Edit>);

/// One recorded settings change.
type Edit = Box<dyn Fn(&mut AppConfig)>;

impl Edits {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Applies every recorded change to `cfg`, in order.
    pub fn apply(&self, cfg: &mut AppConfig) {
        for edit in &self.0 {
            edit(cfg);
        }
    }
}

/// The tray agent's settings as the automation sees them. They can be read
/// freely (`Deref`), but the only way to change them is [`Tracked::edit`],
/// which applies the change at once and records it in [`Edits`]. The tray
/// then saves exactly those changes on top of the file, so nothing the
/// automation sets can be left out of the save (and nothing else is written).
pub struct Tracked<'a> {
    cfg: &'a mut AppConfig,
    edits: Edits,
}

impl<'a> Tracked<'a> {
    pub fn new(cfg: &'a mut AppConfig) -> Self {
        Self { cfg, edits: Edits::default() }
    }

    /// Changes the settings. `edit` should set the fields it changes to
    /// values (not toggle what is there), since it runs again on the file's
    /// settings when they are saved.
    pub fn edit(&mut self, edit: impl Fn(&mut AppConfig) + 'static) {
        edit(self.cfg);
        self.edits.0.push(Box::new(edit));
    }

    /// The changes made, for saving.
    pub fn into_edits(self) -> Edits {
        self.edits
    }
}

impl std::ops::Deref for Tracked<'_> {
    type Target = AppConfig;

    fn deref(&self) -> &AppConfig {
        self.cfg
    }
}

/// Keyboard backlight level after a brightness shortcut (1 = off .. 5).
fn backlight_step(level: u8, up: bool) -> u8 {
    if up {
        (level + 1).min(5)
    } else {
        level.saturating_sub(1).max(1)
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
    clamshell_checked: bool,
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
            clamshell_checked: false,
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

    /// Runs a shortcut. Settings it changes are recorded in `cfg` (save them).
    pub fn hotkey(&mut self, h: Hotkey, cfg: &mut Tracked, worker: &WorkerHandle, lang: Lang, mock: bool) {
        match h {
            Hotkey::ToggleWindow => crate::tray::toggle_window(mock),
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
                if let Some(k) = next {
                    self.switch(cfg, &k, worker, lang);
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
                    return;
                }
                self.switch(cfg, &m.label().to_ascii_lowercase(), worker, lang);
            }
            Hotkey::Overlay => {
                let on = !cfg.overlay;
                cfg.edit(move |c| c.overlay = on);
                self.toast(cfg, tr(lang, if on { "Overlay On" } else { "Overlay Off" }));
            }
            Hotkey::BacklightDown | Hotkey::BacklightUp => {
                let level = backlight_step(cfg.lighting.brightness_level, h == Hotkey::BacklightUp);
                // Only the brightness: colours or an effect changed in a
                // window at the same moment are kept.
                cfg.edit(move |c| c.lighting.brightness_level = level);
                self.kb_dimmed = false;
                let _ = worker.tx.send(crate::apply::lighting(&cfg.lighting));
                let text = if level <= 1 {
                    tr(lang, "Off").to_owned()
                } else {
                    format!("{}%", opensense_proto::lighting::level_to_percent(level))
                };
                self.toast(cfg, &text);
            }
        }
    }

    /// Switches mode with a toast; the fields the switch sets are recorded.
    pub fn switch(&mut self, cfg: &mut Tracked, key: &str, worker: &WorkerHandle, lang: Lang) {
        let owned = key.to_owned();
        cfg.edit(move |c| crate::modes::select(c, &owned));
        crate::modes::activate(cfg, key, worker);
        let name = crate::modes::display_name(cfg, lang, key);
        self.toast(cfg, &name);
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

    /// Once a second. Settings it changes are recorded in `cfg` (save them).
    pub fn tick(&mut self, cfg: &mut Tracked, worker: &WorkerHandle, lang: Lang) {
        self.osd.tick();
        self.ticks += 1;
        // Asked from Windows directly: the worker's snapshot reads "on
        // battery" until its first poll, which would look like a power
        // source change (and store the AC mode as the battery mode).
        let Ok(on_ac) = opensense_sys::power::status().map(|p| p.on_ac) else { return };
        let key = crate::modes::current_key(cfg);

        // Remember the mode per power source; restore it when the source
        // changes.
        match self.last_ac {
            Some(prev) if prev != on_ac => {
                let wanted = if on_ac { cfg.mode_ac.clone() } else { cfg.mode_dc.clone() };
                if let Some(k) = wanted.filter(|k| *k != key && crate::modes::ordered(cfg).contains(k)) {
                    let ok = on_ac || crate::modes::base_of(cfg, &k) != OperationMode::Extreme;
                    if ok {
                        self.switch(cfg, &k, worker, lang);
                    }
                }
                self.kb_dimmed = false;
                let _ = worker.tx.send(crate::apply::lighting(&cfg.lighting));
            }
            _ => {
                if self.last_key.as_deref() != Some(key.as_str()) {
                    let slot = if on_ac { &cfg.mode_ac } else { &cfg.mode_dc };
                    if slot.as_deref() != Some(key.as_str()) {
                        let key = Some(key.clone());
                        if on_ac {
                            cfg.edit(move |c| c.mode_ac.clone_from(&key));
                        } else {
                            cfg.edit(move |c| c.mode_dc.clone_from(&key));
                        }
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
            // A previous run that was killed may have left the lid action
            // forced to "do nothing"; pick that up so it gets restored.
            if !self.clamshell_checked {
                self.clamshell_checked = true;
                self.clamshell_active =
                    cfg.clamshell_default_lid.is_some() && tweaks::read(setting::LID_ACTION).is_ok_and(|v| v.0 == 0);
            }
            let want = cfg.clamshell && on_ac && tweaks::monitor_count() > 1;
            if want != self.clamshell_active {
                if want {
                    if cfg.clamshell_default_lid.is_none() {
                        let current = tweaks::read(setting::LID_ACTION).map(|v| v.0).unwrap_or(1);
                        let lid = Some(if current == 0 { 1 } else { current });
                        cfg.edit(move |c| c.clamshell_default_lid = lid);
                    }
                    let _ = tweaks::write(setting::LID_ACTION, Some(0), None);
                } else {
                    let _ = tweaks::write(setting::LID_ACTION, Some(cfg.clamshell_default_lid.unwrap_or(1)), None);
                }
                self.clamshell_active = want;
            }
        }
    }

    /// On exit: gives the lid action back if clamshell mode forced it.
    pub fn stop(&mut self, cfg: &AppConfig) {
        if self.clamshell_active {
            let _ = tweaks::write(setting::LID_ACTION, Some(cfg.clamshell_default_lid.unwrap_or(1)), None);
            self.clamshell_active = false;
        }
    }
}

fn full_power(worker: &WorkerHandle) -> bool {
    let s = worker.shared.lock();
    s.power.on_ac && s.battery_boost != Some(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backlight_steps_stay_in_range() {
        assert_eq!(backlight_step(5, true), 5);
        assert_eq!(backlight_step(3, true), 4);
        assert_eq!(backlight_step(1, false), 1);
        assert_eq!(backlight_step(0, false), 1);
        assert_eq!(backlight_step(4, false), 3);
    }

    #[test]
    fn tracked_edits_apply_now_and_replay_field_by_field() {
        let mut mine = AppConfig::default();
        let mut t = Tracked::new(&mut mine);
        t.edit(|c| c.lighting.brightness_level = 2);
        t.edit(|c| c.overlay = true);
        assert_eq!(t.lighting.brightness_level, 2);
        let edits = t.into_edits();
        assert!(!edits.is_empty());
        assert!(mine.overlay);

        // Another process changed the colours and the mode meanwhile: the
        // replay keeps them and sets only what the automation changed.
        let mut file = AppConfig::default();
        file.lighting.zones[0] = "#00FF00".into();
        file.current_mode = Some("quiet".into());
        edits.apply(&mut file);
        assert_eq!(file.lighting.brightness_level, 2);
        assert!(file.overlay);
        assert_eq!(file.lighting.zones[0], "#00FF00");
        assert_eq!(file.current_mode.as_deref(), Some("quiet"));
    }

    #[test]
    fn no_edits_means_nothing_to_save() {
        let mut cfg = AppConfig::default();
        assert!(Tracked::new(&mut cfg).into_edits().is_empty());
    }
}
