//! `~/.warp/pomodoro/settings.json`: the durations, the long-break cycle, the
//! two auto-start switches and the desktop-notification switch. Versioned
//! snake_case through the platform store, strict (`deny_unknown_fields`), and
//! clamped on load so a hand-edited file can never make a zero-second phase.

use serde::{Deserialize, Serialize};
use warpui_kit_platform::config::Store;

use crate::paths::AppPaths;

/// The settled defaults.
pub const WORK_MINUTES: u32 = 25;
pub const SHORT_BREAK_MINUTES: u32 = 5;
pub const LONG_BREAK_MINUTES: u32 = 15;
pub const LONG_BREAK_EVERY: u32 = 4;

/// The bounds a hand-edited file is clamped into: a phase is at least one
/// minute and at most a day, and a cycle is 1..=12 focus sessions.
pub const MINUTES_MIN: u32 = 1;
pub const MINUTES_MAX: u32 = 24 * 60;
pub const LONG_BREAK_EVERY_MIN: u32 = 1;
pub const LONG_BREAK_EVERY_MAX: u32 = 12;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub work_minutes: u32,
    pub short_break_minutes: u32,
    pub long_break_minutes: u32,
    /// Focus sessions per cycle, 1..=12.
    pub long_break_every: u32,
    pub auto_start_breaks: bool,
    pub auto_start_work: bool,
    pub desktop_notifications: bool,
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
        }
    }
}

impl Settings {
    /// Reads the settings. A missing file is the defaults; a pre-versioning
    /// file is imported once and rewritten with the version, and a file this
    /// build cannot read (a newer schema, a corrupt or unknown key) leaves the
    /// file alone and runs on the defaults.
    pub fn load() -> Self {
        let path = AppPaths::settings_file();
        let store = Store::<Settings>::new(path.clone());
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
        let store = Store::<Settings>::new(AppPaths::settings_file());
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
    fn the_setters_keep_the_bounds() {
        let mut settings = Settings::default();
        settings.set_work_minutes(0);
        settings.set_long_break_every(40);
        assert_eq!(settings.work_minutes, MINUTES_MIN);
        assert_eq!(settings.long_break_every, LONG_BREAK_EVERY_MAX);
    }
}
