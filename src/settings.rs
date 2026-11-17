//! `~/.warp/pomodoro/settings.json`: the durations, the long-break cycle, the
//! two auto-start switches, the notification and alert-sound switches, the
//! reduced-motion choice, and the window the app opens in (with each mode's
//! saved box). Versioned snake_case through the platform store, tolerant of
//! keys a newer build wrote (they are carried through the next save, so
//! switching builds never drops a section), and clamped on load so a
//! hand-edited file can never make a zero-second phase.

use serde::{Deserialize, Serialize};
use warpui_kit_platform::config::Store;

use crate::paths::AppPaths;

/// The settled defaults.
pub const WORK_MINUTES: u32 = 25;
pub const SHORT_BREAK_MINUTES: u32 = 5;
pub const LONG_BREAK_MINUTES: u32 = 15;
pub const LONG_BREAK_EVERY: u32 = 4;

/// The bounds a hand-edited file is clamped into: a phase runs 1..=1440
/// minutes (a whole day is legal), and a cycle is 1..=12 focus sessions.
pub const MINUTES_MIN: u32 = 1;
pub const MINUTES_MAX: u32 = 24 * 60;
pub const LONG_BREAK_EVERY_MIN: u32 = 1;
pub const LONG_BREAK_EVERY_MAX: u32 = 12;

/// The window the app opens in: the full dashboard or the compact card.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMode {
    /// The rail, the pages and the window controls.
    #[default]
    Full,
    /// The small always-on-top card with the countdown and four controls.
    Mini,
}

impl WindowMode {
    /// The settings page's word for this mode.
    pub fn label(self) -> &'static str {
        match self {
            WindowMode::Full => "Full screen",
            WindowMode::Mini => "Mini screen",
        }
    }
}

/// A saved window box in window pixels. Integers keep the settings document
/// `Eq` and stable in the file; the UI converts at the window boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowBox {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl WindowBox {
    /// A box a hand-edited file cannot turn into an unusable window: at least
    /// the mini card, at most a very wide screen.
    pub fn sane(self) -> Option<Self> {
        const MIN_W: i32 = 260;
        const MIN_H: i32 = 88;
        const MAX: i32 = 16_384;
        let sane = (MIN_W..=MAX).contains(&self.w) && (MIN_H..=MAX).contains(&self.h);
        sane.then_some(self)
    }

    /// Clamps a box for the mini card: the card is small by design, so a
    /// remembered tiled size (a compositor can report one) must not become
    /// the card's box.
    pub fn sane_card(self) -> Option<Self> {
        const CARD_MAX_W: i32 = 720;
        const CARD_MAX_H: i32 = 420;
        let sane = self.sane()?;
        (sane.w <= CARD_MAX_W && sane.h <= CARD_MAX_H).then_some(sane)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub work_minutes: u32,
    pub short_break_minutes: u32,
    pub long_break_minutes: u32,
    /// Focus sessions per cycle, 1..=12.
    pub long_break_every: u32,
    pub auto_start_breaks: bool,
    pub auto_start_work: bool,
    pub desktop_notifications: bool,
    /// The short chime at a phase end.
    pub sound_enabled: bool,
    /// A custom WAV/OGG the sound plays instead of the bundled chime; empty
    /// means the bundled one.
    pub sound_path: String,
    /// The family's motion switch: keeps the countdown ring still, with no
    /// phase-end bloom and no in-between frames.
    #[serde(default)]
    pub reduce_motion: bool,
    /// The window the app opens in and lives in until the user switches.
    pub window_mode: WindowMode,
    /// Keep the mini card above other windows where the platform allows it.
    pub always_on_top: bool,
    /// Each mode's last box, restored the next time that mode opens.
    pub bounds_full: Option<WindowBox>,
    pub bounds_mini: Option<WindowBox>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            work_minutes: WORK_MINUTES,
            short_break_minutes: SHORT_BREAK_MINUTES,
            long_break_minutes: LONG_BREAK_MINUTES,
            long_break_every: LONG_BREAK_EVERY,
            auto_start_breaks: true,
            auto_start_work: false,
            desktop_notifications: true,
            sound_enabled: true,
            sound_path: String::new(),
            reduce_motion: false,
            window_mode: WindowMode::Full,
            always_on_top: true,
            bounds_full: None,
            bounds_mini: None,
        }
    }
}

impl Settings {
    /// Reads the settings. A missing file is the defaults; a pre-versioning
    /// file is imported once and rewritten with the version, and a file this
    /// build cannot read (a newer schema, corrupt JSON) leaves the file alone
    /// and runs on the defaults. A key this build does not know is not an
    /// error: it is carried through the next save.
    pub fn load() -> Self {
        let path = AppPaths::settings_file();
        let store = Store::<Settings>::new(path.clone()).preserving_unknown();
        match store.load() {
            Ok(settings) => settings.normalized(),
            Err(err) if err.needs_import() => {
                let imported = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
                    .unwrap_or_default();
                let settings = imported.normalized();
                if let Err(err) = store.save(&settings) {
                    tracing::warn!("could not write the imported settings: {err}");
                }
                settings
            }
            Err(err) => {
                tracing::warn!("could not read {}: {err}; using defaults", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self) {
        let store = Store::<Settings>::new(AppPaths::settings_file()).preserving_unknown();
        if let Err(err) = store.save(self) {
            tracing::warn!("could not save the settings: {err}");
        }
    }

    /// The values a hand-edited file may hold that the rest of the app would
    /// otherwise re-check on every use.
    pub fn normalized(mut self) -> Self {
        self.work_minutes = clamp_minutes(self.work_minutes);
        self.short_break_minutes = clamp_minutes(self.short_break_minutes);
        self.long_break_minutes = clamp_minutes(self.long_break_minutes);
        self.long_break_every = self
            .long_break_every
            .clamp(LONG_BREAK_EVERY_MIN, LONG_BREAK_EVERY_MAX);
        self.sound_path = self.sound_path.trim().to_string();
        self.bounds_full = self.bounds_full.and_then(WindowBox::sane);
        self.bounds_mini = self.bounds_mini.and_then(WindowBox::sane_card);
        self
    }

    pub fn set_work_minutes(&mut self, value: u32) {
        self.work_minutes = clamp_minutes(value);
    }

    pub fn set_short_break_minutes(&mut self, value: u32) {
        self.short_break_minutes = clamp_minutes(value);
    }

    pub fn set_long_break_minutes(&mut self, value: u32) {
        self.long_break_minutes = clamp_minutes(value);
    }

    pub fn set_long_break_every(&mut self, value: u32) {
        self.long_break_every = value.clamp(LONG_BREAK_EVERY_MIN, LONG_BREAK_EVERY_MAX);
    }

    pub fn work_seconds(&self) -> u32 {
        clamp_minutes(self.work_minutes) * 60
    }

    pub fn short_break_seconds(&self) -> u32 {
        clamp_minutes(self.short_break_minutes) * 60
    }

    pub fn long_break_seconds(&self) -> u32 {
        clamp_minutes(self.long_break_minutes) * 60
    }
}

fn clamp_minutes(minutes: u32) -> u32 {
    minutes.clamp(MINUTES_MIN, MINUTES_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_settled_contract() {
        let settings = Settings::default();
        assert_eq!(settings.work_minutes, 25);
        assert_eq!(settings.short_break_minutes, 5);
        assert_eq!(settings.long_break_minutes, 15);
        assert_eq!(settings.long_break_every, 4);
        assert!(settings.auto_start_breaks);
        assert!(!settings.auto_start_work);
        assert!(settings.desktop_notifications);
        assert_eq!(settings.work_seconds(), 1500);
    }

    #[test]
    fn a_hand_edited_file_is_clamped_into_the_contract_bounds() {
        let settings = Settings {
            work_minutes: 0,
            short_break_minutes: 5_000,
            long_break_minutes: 15,
            long_break_every: 0,
            ..Settings::default()
        }
        .normalized();
        assert_eq!(settings.work_minutes, MINUTES_MIN);
        assert_eq!(settings.short_break_minutes, MINUTES_MAX);
        assert_eq!(settings.long_break_every, LONG_BREAK_EVERY_MIN);

        let wide = Settings {
            long_break_every: 99,
            ..Settings::default()
        }
        .normalized();
        assert_eq!(wide.long_break_every, LONG_BREAK_EVERY_MAX);
    }

    #[test]
    fn a_newer_settings_file_loads_and_keeps_its_unknown_keys() {
        // the store pair `load`/`save` use: a file a newer build wrote reads
        // with the keys this build knows, and the next save keeps the rest
        let dir = std::env::temp_dir().join(format!(
            "warp-pomodoro-settings-newer-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            r#"{"schema_version":1,"work_minutes":50,"later_flag":true,"window_mode":"mini"}"#,
        )
        .unwrap();

        let store = Store::<Settings>::new(path.clone()).preserving_unknown();
        let mut settings = store.load().expect("a newer file still loads");
        assert_eq!(settings.work_minutes, 50);
        settings.work_minutes = 45;
        store.save(&settings).unwrap();

        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["later_flag"], true, "the newer key survives");
        assert_eq!(saved["work_minutes"], 45);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_setters_keep_the_bounds() {
        let mut settings = Settings::default();
        settings.set_work_minutes(0);
        settings.set_long_break_every(40);
        assert_eq!(settings.work_minutes, MINUTES_MIN);
        assert_eq!(settings.long_break_every, LONG_BREAK_EVERY_MAX);
    }

    #[test]
    fn the_window_and_sound_defaults_are_settled() {
        let settings = Settings::default();
        assert!(settings.sound_enabled);
        assert!(settings.sound_path.is_empty());
        assert_eq!(settings.window_mode, WindowMode::Full);
        assert!(settings.always_on_top);
        assert_eq!(settings.bounds_full, None);
        assert_eq!(settings.bounds_mini, None);
    }

    #[test]
    fn a_file_without_the_new_keys_reads_the_defaults() {
        let text = r#"{
            "work_minutes": 30,
            "short_break_minutes": 6,
            "long_break_minutes": 20,
            "long_break_every": 3,
            "auto_start_breaks": false,
            "auto_start_work": true,
            "desktop_notifications": false
        }"#;
        let settings: Settings = serde_json::from_str(text).unwrap();
        assert_eq!(settings.work_minutes, 30);
        assert!(settings.sound_enabled, "the new keys take their defaults");
        assert!(!settings.reduce_motion, "the motion switch defaults off");
        assert_eq!(settings.window_mode, WindowMode::Full);
        assert!(settings.always_on_top);
    }

    #[test]
    fn a_hand_edited_box_is_clamped_or_dropped() {
        let sane = WindowBox {
            x: 40,
            y: 40,
            w: 340,
            h: 104,
        };
        assert_eq!(sane.sane(), Some(sane));
        assert_eq!(
            WindowBox {
                w: 40,
                h: 30,
                ..sane
            }
            .sane(),
            None
        );
        let settings = Settings {
            bounds_mini: Some(WindowBox {
                x: 0,
                y: 0,
                w: 1,
                h: 1,
            }),
            sound_path: "  /tmp/ding.wav  ".into(),
            ..Settings::default()
        }
        .normalized();
        assert_eq!(settings.bounds_mini, None);
        assert_eq!(settings.sound_path, "/tmp/ding.wav");
    }

    #[test]
    fn a_legacy_unknown_key_reads_as_defaults_instead_of_failing() {
        // a pre-versioning file is the importer's input: the keys this build
        // knows are read, the rest are dropped rather than failing the read
        let text = r#"{"window": {"mode": "mini"}}"#;
        let settings: Settings = serde_json::from_str(text).expect("a tolerant read");
        assert_eq!(settings, Settings::default());
    }
}
