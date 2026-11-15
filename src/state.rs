//! The live snapshot Warp Station reads when the door is down:
//! `~/.warp/pomodoro/state.json`, rewritten at most once a second and on every
//! mutation, and once more paused on a clean exit.

use std::time::{Duration, Instant};

use chrono::Local;
use serde_json::{Value, json};
use warpui_kit_platform::config::{CURRENT_VERSION, Store, VERSION_KEY};

use crate::history::History;
use crate::paths::AppPaths;
use crate::timer::{Snapshot, Timer};

/// The snapshot's refresh beat: Warp Station shows "last seen" from it, so a
/// running timer publishes at least once a second.
pub const WRITE_INTERVAL: Duration = Duration::from_secs(1);

/// The snapshot a status answer carries: the contract fields plus the store's
/// schema version, flat in the door's answer frame.
pub fn snapshot_value(snapshot: &Snapshot) -> Value {
    let mut value = serde_json::to_value(snapshot).unwrap_or_else(|_| json!({}));
    if let Some(object) = value.as_object_mut() {
        object.insert(VERSION_KEY.to_string(), Value::from(CURRENT_VERSION));
    }
    value
}

/// The snapshot the app leaves behind on a clean exit: the same state, paused.
pub fn exit_snapshot(timer: &Timer, history: &History) -> Snapshot {
    let mut snapshot = timer.snapshot(&history.sessions, Local::now());
    snapshot.running = false;
    snapshot
}

/// Writes `state.json` and remembers what the file holds, so the tick can skip
/// a frame and still meet the once-a-second refresh.
pub struct Writer {
    last: Option<Snapshot>,
    written: Instant,
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

impl Writer {
    pub fn new() -> Self {
        Self {
            last: None,
            written: Instant::now(),
        }
    }

    /// Writes when the state moved, when `force` asks (a mutation or the exit
    /// stamp), or when a second has passed.
    pub fn maybe_write(&mut self, snapshot: &Snapshot, force: bool) -> bool {
        if !should_write(self.last.as_ref(), snapshot, force, self.written.elapsed()) {
            return false;
        }
        self.write_now(snapshot)
    }

    /// Writes unconditionally and remembers the document.
    pub fn write_now(&mut self, snapshot: &Snapshot) -> bool {
        let store = Store::<Snapshot>::new(AppPaths::state_file());
        match store.save(snapshot) {
            Ok(()) => {
                self.last = Some(snapshot.clone());
                self.written = Instant::now();
                true
            }
            Err(err) => {
                tracing::warn!("could not write the state snapshot: {err}");
                false
            }
        }
    }
}

/// The write decision, pure so it can be tested without touching the disk.
fn should_write(
    last: Option<&Snapshot>,
    snapshot: &Snapshot,
    force: bool,
    since: Duration,
) -> bool {
    force || last.is_none_or(|last| !last.same_state(snapshot)) || since >= WRITE_INTERVAL
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    #[test]
    fn a_changed_state_or_a_forced_write_always_lands() {
        let timer = Timer::new(&Settings::default());
        let snapshot = timer.snapshot(&[], Local::now());
        assert!(should_write(None, &snapshot, false, Duration::ZERO));
        assert!(should_write(
            Some(&snapshot),
            &snapshot,
            true,
            Duration::ZERO
        ));
        let mut changed = snapshot.clone();
        changed.running = true;
        assert!(should_write(
            Some(&snapshot),
            &changed,
            false,
            Duration::from_millis(10)
        ));
    }

    #[test]
    fn an_unchanged_state_waits_out_the_second() {
        let timer = Timer::new(&Settings::default());
        let snapshot = timer.snapshot(&[], Local::now());
        assert!(!should_write(
            Some(&snapshot),
            &snapshot,
            false,
            Duration::from_millis(400)
        ));
        assert!(should_write(
            Some(&snapshot),
            &snapshot,
            false,
            WRITE_INTERVAL
        ));
    }

    #[test]
    fn a_status_answer_carries_the_schema_version_flat() {
        let timer = Timer::new(&Settings::default());
        let value = snapshot_value(&timer.snapshot(&[], Local::now()));
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["phase"], "work");
    }

    #[test]
    fn the_exit_snapshot_is_paused() {
        let mut timer = Timer::new(&Settings::default());
        timer.start();
        let exit = exit_snapshot(&timer, &History::default());
        assert!(!exit.running);
        assert_eq!(exit.remaining_seconds, timer.remaining_seconds());
    }
}
