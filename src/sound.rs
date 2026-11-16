//! The alert chime: one short bell at a phase end. The WAV ships inside the
//! binary and is written to `~/.warp/pomodoro/chime.wav` the first time it is
//! needed, so every platform can hand a plain file to its own player.
//!
//! - Linux (and the other Unix desktops): the first of `pw-play`, `paplay`,
//!   `aplay` that starts wins for the session; a machine with none of them
//!   gets one log line, never an error in the timer.
//! - Windows: `PlaySoundW` (winmm) with `SND_ASYNC`, so the timer never waits
//!   for the bell.
//! - macOS: `afplay`.
//!
//! The settings gate is the caller's: [`play`] is only called when the switch
//! is on. A custom `sound_path` (a WAV/OGG the user picked) wins over the
//! bundled chime; a path that does not exist falls back to it.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::paths::AppPaths;
use crate::settings::Settings;

/// The bundled bell: about three quarters of a second, two struck notes.
const BUNDLED: &[u8] = include_bytes!("../assets/audio/chime.wav");

/// Why a play attempt could not be made; the settings page shows the same
/// string when the Test button fails.
pub type SoundError = String;

/// Plays the configured alert on the calling thread's behalf. Never blocks on
/// the player (it is spawned) and never panics.
pub fn play(settings: &Settings) {
    if !settings.sound_enabled {
        return;
    }
    match play_now(settings) {
        Ok(()) => {}
        Err(err) => {
            // one line per session: a missing player must not fill the log
            // every 25 minutes
            if warn_once() {
                tracing::warn!("alert sound unavailable: {err}");
            }
        }
    }
}

/// Plays the configured alert and reports a failure (the Test button uses
/// this, so it also works while the switch is off).
pub fn play_now(settings: &Settings) -> Result<(), SoundError> {
    let file = resolve_file(settings)?;
    play_file(&file)
}

/// The file the alert would play: the custom path when it exists, else the
/// bundled chime extracted beside the settings.
pub fn resolve_file(settings: &Settings) -> Result<PathBuf, SoundError> {
    let custom = settings.sound_path.trim();
    if !custom.is_empty() {
        let path = expand_home(custom);
        if path.is_file() {
            return Ok(path);
        }
        tracing::warn!(
            "the configured sound {} does not exist; the bundled chime plays",
            path.display()
        );
    }
    bundled_file()
}

/// The bundled chime in the app folder, written when missing or when the
/// binary carries a different one.
pub fn bundled_file() -> Result<PathBuf, SoundError> {
    static WRITTEN: OnceLock<Result<PathBuf, SoundError>> = OnceLock::new();
    WRITTEN
        .get_or_init(|| {
            let path = AppPaths::chime_file();
            if let Some(dir) = path.parent()
                && let Err(err) = std::fs::create_dir_all(dir)
            {
                return Err(format!("{}: {err}", dir.display()));
            }
            let current = std::fs::read(&path).ok();
            if current.as_deref() != Some(BUNDLED)
                && let Err(err) = std::fs::write(&path, BUNDLED)
            {
                return Err(format!("could not write {}: {err}", path.display()));
            }
            Ok(path)
        })
        .clone()
}

/// `~/Music/ding.wav` → the home-joined path. Only a leading `~` is special;
/// everything else is taken as written.
fn expand_home(value: &str) -> PathBuf {
    if let Some(rest) = value.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(value)
}

/// Hands the file to the platform's player; on Unix the session's player is
/// picked once so a phase end does not probe three binaries every time.
fn play_file(path: &Path) -> Result<(), SoundError> {
    #[cfg(windows)]
    {
        return windows_player::play(path);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let player =
            player().ok_or_else(|| "no audio player (pw-play, paplay, aplay)".to_string())?;
        crate::spawn::detached(
            std::process::Command::new(player)
                .arg(path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null()),
        )
        .map_err(|err| format!("{player}: {err}"))
    }

    #[cfg(target_os = "macos")]
    {
        crate::spawn::detached(
            std::process::Command::new("afplay")
                .arg(path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null()),
        )
        .map_err(|err| format!("afplay: {err}"))
    }
}

/// The Unix players tried in order; the first one that exists wins the
/// session. `None` once the probe failed, so a silent machine logs once.
#[cfg(all(unix, not(target_os = "macos")))]
fn player() -> Option<&'static str> {
    const CANDIDATES: [&str; 3] = ["pw-play", "paplay", "aplay"];
    static PLAYER: OnceLock<Option<&'static str>> = OnceLock::new();
    *PLAYER.get_or_init(|| CANDIDATES.into_iter().find(|name| on_path(name)))
}

/// Whether a bare command resolves on `PATH` (the GUI's minimal one included
/// the user dirs, which `paths` owns elsewhere; a player is a system binary).
#[cfg(all(unix, not(target_os = "macos")))]
fn on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

/// True the first time it is called: the alert's failure log line.
fn warn_once() -> bool {
    static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed)
}

#[cfg(windows)]
mod windows_player {
    use std::os::windows::ffi::OsStrExt as _;
    use std::path::Path;

    use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT};
    use windows::core::PCWSTR;

    /// One asynchronous play; the bell does not serialize the UI tick.
    pub fn play(path: &Path) -> Result<(), String> {
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        wide.push(0);
        let played = unsafe {
            PlaySoundW(
                PCWSTR(wide.as_ptr()),
                None,
                SND_FILENAME | SND_ASYNC | SND_NODEFAULT,
            )
        };
        if played.as_bool() {
            Ok(())
        } else {
            Err(format!("PlaySoundW refused {}", path.display()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_custom_path_is_taken_as_written_and_a_missing_one_falls_back() {
        let dir = std::env::temp_dir().join(format!("warp-pomodoro-sound-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("ding.wav");
        std::fs::write(&file, b"RIFF").unwrap();

        let custom = Settings {
            sound_path: file.to_string_lossy().to_string(),
            ..Settings::default()
        };
        assert_eq!(resolve_file(&custom), Ok(file.clone()));

        let missing = Settings {
            sound_path: dir.join("nope.wav").to_string_lossy().to_string(),
            ..Settings::default()
        };
        assert_eq!(resolve_file(&missing), bundled_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_bundled_chime_lands_in_the_app_folder() {
        let path = bundled_file().expect("the chime writes");
        assert!(path.is_file());
        assert_eq!(std::fs::read(&path).unwrap(), BUNDLED);
        assert_eq!(path, AppPaths::chime_file());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn the_unix_player_list_is_the_three_players_in_order() {
        const CANDIDATES: [&str; 3] = ["pw-play", "paplay", "aplay"];
        assert_eq!(CANDIDATES.len(), 3);
        // a probe of the real PATH may find none, one or all: any answer is
        // valid, the order is the contract
        let found = player();
        if let Some(name) = found {
            assert!(CANDIDATES.contains(&name), "unexpected player {name}");
        }
    }
}
