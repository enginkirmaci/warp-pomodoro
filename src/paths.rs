//! Global per-user locations: settings, history and the live snapshot under
//! `~/.warp/pomodoro`, and the rolling logs in the suite's shared `~/.warp/logs`.

use std::path::PathBuf;
use warpui_kit_platform::paths;

pub struct AppPaths;

impl AppPaths {
    /// The app's own folder, `~/.warp/pomodoro`: settings, history, the live
    /// snapshot and the door's endpoint files.
    pub fn config_dir() -> PathBuf {
        paths::app_dir("pomodoro")
    }

    /// The suite's shared logs folder, `~/.warp/logs`.
    pub fn logs_dir() -> PathBuf {
        paths::logs_dir()
    }

    pub fn settings_file() -> PathBuf {
        Self::config_dir().join("settings.json")
    }

    pub fn history_file() -> PathBuf {
        Self::config_dir().join("history.json")
    }

    pub fn state_file() -> PathBuf {
        Self::config_dir().join("state.json")
    }

    /// The bundled alert chime, written out of the binary on first use so a
    /// system player has a file to open.
    pub fn chime_file() -> PathBuf {
        Self::config_dir().join("chime.wav")
    }

    /// A PCM WAV scaled to one volume, written beside the chime so every
    /// platform hands its own player a plain file. The name carries the
    /// percentage, so one volume's copy is never another's.
    pub fn volume_copy_file(percent: u8) -> PathBuf {
        Self::config_dir().join(format!("chime-{percent}.wav"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_folder_is_the_pomodoro_one_under_the_suite_root() {
        assert_eq!(AppPaths::config_dir(), paths::suite_root().join("pomodoro"));
        assert_eq!(AppPaths::logs_dir(), paths::suite_root().join("logs"));
        assert_eq!(
            AppPaths::settings_file(),
            AppPaths::config_dir().join("settings.json")
        );
        assert_eq!(
            AppPaths::history_file(),
            AppPaths::config_dir().join("history.json")
        );
        assert_eq!(
            AppPaths::state_file(),
            AppPaths::config_dir().join("state.json")
        );
        assert_eq!(
            AppPaths::chime_file(),
            AppPaths::config_dir().join("chime.wav")
        );
        assert_eq!(
            AppPaths::volume_copy_file(70),
            AppPaths::config_dir().join("chime-70.wav")
        );
    }
}
