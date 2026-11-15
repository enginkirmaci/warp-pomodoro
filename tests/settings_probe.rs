//! The settings store: the settled defaults, the versioned snake_case file,
//! the clamps a hand-edited file is pulled into, and the strict schema. Runs
//! under a scratch HOME for the whole binary, since every path resolves from
//! it; the scenarios take turns behind a lock.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

use warp_pomodoro::paths::AppPaths;
use warp_pomodoro::settings::{LONG_BREAK_EVERY_MAX, MINUTES_MAX, MINUTES_MIN, Settings};

static SETTINGS_LOCK: Mutex<()> = Mutex::new(());

fn hold() -> MutexGuard<'static, ()> {
    SETTINGS_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A scratch HOME for the whole test binary: `~/.warp/pomodoro` lands under it
/// and the developer's own settings are never read or written.
fn scratch_home() -> &'static PathBuf {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let home = std::env::temp_dir().join(format!(
            "warp-pomodoro-settings-probe-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".warp")).unwrap();
        std::fs::create_dir_all(home.join(".warp/pomodoro")).unwrap();
        // SAFETY: set once, inside the OnceLock, before this binary reads HOME
        // or spawns a thread that reads it
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("USERPROFILE", &home);
        }
        home
    })
}

fn settings_file() -> PathBuf {
    scratch_home().join(".warp/pomodoro/settings.json")
}

fn reset_file() {
    let _ = std::fs::remove_file(settings_file());
}

fn write_raw(body: &str) {
    std::fs::write(settings_file(), body).unwrap();
}

fn read_json() -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(settings_file()).unwrap()).unwrap()
}

#[test]
fn the_defaults_match_the_settled_contract() {
    let _guard = hold();
    scratch_home();
    reset_file();
    let settings = Settings::load();
    assert_eq!(settings, Settings::default());
    assert_eq!(settings.work_minutes, 25);
    assert_eq!(settings.short_break_minutes, 5);
    assert_eq!(settings.long_break_minutes, 15);
    assert_eq!(settings.long_break_every, 4);
    assert!(settings.auto_start_breaks);
    assert!(!settings.auto_start_work);
    assert!(settings.desktop_notifications);
}

#[test]
fn saving_writes_the_versioned_snake_case_document() {
    let _guard = hold();
    scratch_home();
    reset_file();
    let mut settings = Settings::default();
    settings.set_work_minutes(30);
    settings.save();
    let saved = read_json();
    assert_eq!(saved["schema_version"], 1);
    assert_eq!(saved["work_minutes"], 30);
    assert_eq!(saved["short_break_minutes"], 5);
    assert_eq!(saved["long_break_minutes"], 15);
    assert_eq!(saved["long_break_every"], 4);
    assert_eq!(saved["auto_start_breaks"], true);
    assert_eq!(saved["auto_start_work"], false);
    assert_eq!(saved["desktop_notifications"], true);
    assert!(
        saved.get("WorkMinutes").is_none(),
        "the document is snake_case only"
    );
    // and it round-trips through the store
    assert_eq!(Settings::load(), settings);
}

#[test]
fn a_hand_edited_file_is_clamped_on_load() {
    let _guard = hold();
    scratch_home();
    reset_file();
    write_raw(
        r#"{"schema_version":1,"work_minutes":0,"short_break_minutes":5000,
            "long_break_minutes":15,"long_break_every":99,
            "auto_start_breaks":true,"auto_start_work":false,
            "desktop_notifications":false}"#,
    );
    let settings = Settings::load();
    assert_eq!(settings.work_minutes, MINUTES_MIN);
    assert_eq!(settings.short_break_minutes, MINUTES_MAX);
    assert_eq!(settings.long_break_every, LONG_BREAK_EVERY_MAX);
    assert!(!settings.desktop_notifications);
}

#[test]
fn the_schema_is_strict_and_a_file_it_cannot_read_falls_back_to_defaults() {
    let _guard = hold();
    scratch_home();
    reset_file();
    // an unknown key is refused by the typed document itself
    assert!(
        serde_json::from_str::<Settings>(r#"{"work_minutes":30,"unknown_key":1}"#).is_err(),
        "the settings document denies unknown fields"
    );
    // and the live store leaves such a file alone instead of rewriting it
    write_raw(r#"{"schema_version":1,"work_minutes":30,"unknown_key":1}"#);
    let settings = Settings::load();
    assert_eq!(settings, Settings::default());
    assert_eq!(
        read_json()["work_minutes"],
        30,
        "the unreadable file is not overwritten"
    );
}

#[test]
fn a_pre_versioning_file_is_imported_once() {
    let _guard = hold();
    scratch_home();
    reset_file();
    write_raw(r#"{"work_minutes":40,"long_break_every":3}"#);
    let settings = Settings::load();
    assert_eq!(settings.work_minutes, 40);
    assert_eq!(settings.long_break_every, 3);
    let saved = read_json();
    assert_eq!(saved["schema_version"], 1);
    assert_eq!(saved["work_minutes"], 40);
    // the second read is the live store, not the importer
    assert_eq!(Settings::load(), settings);
}

#[test]
fn the_paths_are_the_pomodoro_folder_under_the_scratch_home() {
    let _guard = hold();
    let home = scratch_home();
    assert_eq!(AppPaths::config_dir(), home.join(".warp/pomodoro"));
    assert_eq!(AppPaths::settings_file(), settings_file());
    assert_eq!(
        AppPaths::history_file(),
        home.join(".warp/pomodoro/history.json")
    );
    assert_eq!(
        AppPaths::state_file(),
        home.join(".warp/pomodoro/state.json")
    );
    assert_eq!(AppPaths::logs_dir(), home.join(".warp/logs"));
}
