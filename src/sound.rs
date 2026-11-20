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
//! The settings gate the alert: [`play`] returns early unless the switch is on
//! and the volume is above zero. A custom `sound_path` (a WAV/OGG the user
//! picked) wins over the bundled chime; a path that does not exist falls back
//! to it.
//!
//! The volume is the same on every platform: the player arguments carry no
//! volume (`PlaySoundW` has none at all), so the alert is a PCM WAV scaled to
//! `sound_volume` percent and written beside the bundled chime. A file this
//! build cannot rescale (an OGG, a float WAV) plays as recorded.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::paths::AppPaths;
use crate::settings::{SOUND_VOLUME_MAX, Settings};

/// The bundled bell: about three quarters of a second, two struck notes.
const BUNDLED: &[u8] = include_bytes!("../assets/audio/chime.wav");

/// Why a play attempt could not be made; the settings page shows the same
/// string when the Test button fails.
pub type SoundError = String;

/// Plays the configured alert on the calling thread's behalf. Never blocks on
/// the player (it is spawned) and never panics.
pub fn play(settings: &Settings) {
    if !settings.sound_enabled || settings.sound_volume == 0 {
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
    // the volume is the mute: at 0 there is nothing to hand a player
    if settings.sound_volume == 0 {
        return Ok(());
    }
    let file = resolve_file(settings)?;
    play_file(&alert_file(&file, settings.sound_volume))
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

/// The file the player opens: the source at full volume, or a scaled copy of a
/// PCM WAV for the configured volume. A format this build cannot rescale, or a
/// copy that cannot be written, plays the source as recorded — a bell at the
/// recorded level beats a silent one.
fn alert_file(source: &Path, percent: u8) -> PathBuf {
    if percent >= SOUND_VOLUME_MAX {
        return source.to_path_buf();
    }
    let scaled = std::fs::read(source)
        .ok()
        .and_then(|bytes| scale_wav(&bytes, percent));
    let Some(scaled) = scaled else {
        tracing::debug!(
            "{} is not a PCM WAV this build rescales; it plays at its own level",
            source.display()
        );
        return source.to_path_buf();
    };
    match write_volume_copy(percent, &scaled) {
        Ok(path) => path,
        Err(err) => {
            if warn_once() {
                tracing::warn!("could not write the volume copy: {err}");
            }
            source.to_path_buf()
        }
    }
}

/// Writes `bytes` to this volume's copy beside the bundled chime, reusing an
/// identical copy. The temp-then-rename replace means a player that still holds
/// the previous copy never opens a half-written file.
fn write_volume_copy(percent: u8, bytes: &[u8]) -> Result<PathBuf, SoundError> {
    let path = AppPaths::volume_copy_file(percent);
    if std::fs::read(&path).ok().as_deref() == Some(bytes) {
        return Ok(path);
    }
    if let Some(dir) = path.parent()
        && let Err(err) = std::fs::create_dir_all(dir)
    {
        return Err(format!("{}: {err}", dir.display()));
    }
    let tmp = path.with_extension("wav.tmp");
    std::fs::write(&tmp, bytes).map_err(|err| format!("{}: {err}", tmp.display()))?;
    if let Err(err) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{}: {err}", path.display()));
    }
    Ok(path)
}

/// `bytes` at `percent` of its recorded amplitude, with the header and every
/// non-sample byte untouched. `None` unless the file is a PCM WAV this build
/// knows how to rescale (8/16/24/32-bit integer samples).
fn scale_wav(bytes: &[u8], percent: u8) -> Option<Vec<u8>> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let (fmt_at, fmt_len) = chunk(bytes, b"fmt ")?;
    let fmt = bytes.get(fmt_at..fmt_at.checked_add(fmt_len)?)?;
    if fmt.len() < 16 {
        return None;
    }
    let format = u16::from_le_bytes([fmt[0], fmt[1]]);
    let bits = u16::from_le_bytes([fmt[14], fmt[15]]);
    // `WAVE_FORMAT_EXTENSIBLE` is plain PCM when its SubFormat says so
    let pcm = format == 1 || (format == 0xfffe && fmt.get(24..26) == Some([1, 0].as_slice()));
    if !pcm {
        return None;
    }
    let (data_at, data_len) = chunk(bytes, b"data")?;
    let end = data_at.saturating_add(data_len).min(bytes.len());
    let data = data_at.min(end)..end;

    let mut out = bytes.to_vec();
    let gain = f64::from(percent.min(SOUND_VOLUME_MAX)) / 100.;
    match bits {
        // 8-bit WAV samples are unsigned, centred on 128
        8 => {
            for sample in &mut out[data] {
                let scaled = (f64::from(*sample) - 128.) * gain + 128.;
                *sample = scaled.round().clamp(0., 255.) as u8;
            }
        }
        16 => {
            for sample in out[data].as_chunks_mut::<2>().0 {
                let value = i16::from_le_bytes(*sample);
                let scaled = (f64::from(value) * gain).round().clamp(-32768., 32767.) as i16;
                sample.copy_from_slice(&scaled.to_le_bytes());
            }
        }
        24 => {
            for sample in out[data].as_chunks_mut::<3>().0 {
                let value = i32::from(sample[0])
                    | i32::from(sample[1]) << 8
                    | i32::from(sample[2] as i8) << 16;
                let scaled =
                    ((f64::from(value) * gain).round() as i32).clamp(-8_388_608, 8_388_607);
                let bits = scaled as u32;
                sample[0] = bits as u8;
                sample[1] = (bits >> 8) as u8;
                sample[2] = (bits >> 16) as u8;
            }
        }
        32 => {
            for sample in out[data].as_chunks_mut::<4>().0 {
                let value = i32::from_le_bytes(*sample);
                let scaled = (f64::from(value) * gain)
                    .round()
                    .clamp(f64::from(i32::MIN), f64::from(i32::MAX))
                    as i32;
                sample.copy_from_slice(&scaled.to_le_bytes());
            }
        }
        _ => return None,
    }
    Some(out)
}

/// The first `id` chunk's payload as `(offset, length)`; the walk stops at the
/// end of the file and every chunk is word-aligned.
fn chunk(bytes: &[u8], id: &[u8; 4]) -> Option<(usize, usize)> {
    let mut at: usize = 12;
    loop {
        let header = bytes.get(at..at.checked_add(8)?)?;
        let size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
        let start = at + 8;
        if &header[0..4] == id {
            let len = size.min(bytes.len().saturating_sub(start));
            return Some((start, len));
        }
        at = start.checked_add(size)?.checked_add(size & 1)?;
    }
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

/// True the first time it is called: the alert path's failure log line (a
/// missing player, or a volume copy that could not be written).
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

    #[test]
    fn a_scaled_chime_keeps_its_header_and_scales_every_sample() {
        let scaled = scale_wav(BUNDLED, 50).expect("the bundled chime is a PCM WAV");
        assert_eq!(scaled.len(), BUNDLED.len(), "the file keeps its shape");
        // the bundled file's data chunk follows the 44-byte RIFF/fmt header
        assert_eq!(&scaled[..44], &BUNDLED[..44], "the header is untouched");
        let mut nonzero = false;
        for (before, after) in BUNDLED[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .zip(scaled[44..].as_chunks::<2>().0.iter())
        {
            let before = i16::from_le_bytes(*before);
            let after = i16::from_le_bytes(*after);
            let expected = (f64::from(before) * 0.5).round() as i16;
            assert_eq!(after, expected, "sample {before} scaled to {after}");
            nonzero |= before != 0;
        }
        assert!(nonzero, "the fixture has audio to scale");
    }

    #[test]
    fn a_silent_volume_zeroes_the_samples() {
        let silent = scale_wav(BUNDLED, 0).expect("the bundled chime is a PCM WAV");
        assert!(
            silent[44..]
                .as_chunks::<2>()
                .0
                .iter()
                .all(|sample| *sample == [0, 0]),
            "16-bit PCM silence is a zero sample"
        );
    }

    #[test]
    fn only_a_pcm_wav_is_rescaled() {
        assert!(scale_wav(b"not a wave file at all", 50).is_none());
        assert!(
            scale_wav(&BUNDLED[..20], 50).is_none(),
            "a truncated header"
        );
        // a float WAV (format 3) is not integer PCM: it plays as recorded
        let mut float = BUNDLED[..44].to_vec();
        float[20] = 3;
        assert!(scale_wav(&float, 50).is_none());
    }

    /// A minimal mono PCM WAV: a 44-byte header around `samples`.
    fn pcm_wav(bits: u16, samples: &[u8]) -> Vec<u8> {
        let block = bits / 8;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + samples.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&(8000u32 * u32::from(block)).to_le_bytes());
        wav.extend_from_slice(&block.to_le_bytes());
        wav.extend_from_slice(&bits.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(samples.len() as u32).to_le_bytes());
        wav.extend_from_slice(samples);
        wav
    }

    #[test]
    fn eight_and_twenty_four_bit_pcm_scale_around_their_centres() {
        // 8-bit samples are unsigned with 128 as silence
        let eight = pcm_wav(8, &[128, 255, 0]);
        let scaled = scale_wav(&eight, 50).expect("8-bit PCM scales");
        assert_eq!(&scaled[44..], &[128, 192, 64]);

        let mut samples = Vec::new();
        samples.extend_from_slice(&[0xff, 0xff, 0x7f]); // +8_388_607
        samples.extend_from_slice(&[0x00, 0x00, 0x80]); // -8_388_608
        samples.extend_from_slice(&[0x10, 0x00, 0x00]); // 16
        let wide = pcm_wav(24, &samples);
        let scaled = scale_wav(&wide, 50).expect("24-bit PCM scales");
        let sample = |i: usize| {
            let at = 44 + 3 * i;
            i32::from(scaled[at])
                | i32::from(scaled[at + 1]) << 8
                | i32::from(scaled[at + 2] as i8) << 16
        };
        assert_eq!(sample(0), 4_194_304);
        assert_eq!(sample(1), -4_194_304);
        assert_eq!(sample(2), 8);
    }

    #[test]
    fn the_volume_is_the_mute_and_asks_for_no_player() {
        let settings = Settings {
            sound_volume: 0,
            ..Settings::default()
        };
        assert_eq!(play_now(&settings), Ok(()));
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
