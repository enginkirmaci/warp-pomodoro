//! The live snapshot Warp Station reads when the door is down:
//! `~/.warp/pomodoro/state.json`, rewritten at the edges the app decides —
//! start, pause, reset, skip, a phase end, a settings change — and once more
//! paused on a clean exit. A running countdown's second-by-second remainder is
//! never written: the live number is the door's job, and `updated_at` says when
//! the state last moved, not when the file was last touched.

use chrono::Local;
use serde_json::{Value, json};
use warpui_kit_platform::config::{CURRENT_VERSION, Store, VERSION_KEY};

use crate::history::History;
use crate::paths::AppPaths;
use crate::timer::{Snapshot, Timer};

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

/// Writes `state.json` and remembers what the file holds, so a call that would
/// write the same document again is a no-op.
pub struct Writer {
    last: Option<Snapshot>,
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

impl Writer {
    pub fn new() -> Self {
        Self { last: None }
    }

    /// Writes when the state moved in a way the file records. The running
    /// countdown's remainder is not one of those: it moves every second by
    /// design, and the file is the record of the timer's edges.
    pub fn maybe_write(&mut self, snapshot: &Snapshot) -> bool {
        if !should_write(self.last.as_ref(), snapshot) {
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
fn should_write(last: Option<&Snapshot>, snapshot: &Snapshot) -> bool {
    last.is_none_or(|last| !same_written_state(last, snapshot))
}

/// Whether two snapshots are the same *written* state: [`Snapshot::same_state`]
/// with a running countdown's remainder left out. That field moves every second
/// by design, so counting it as a change would put the file back on a
/// one-write-a-second beat; what the file records is the edge (the start, the
/// pause, the phase end), and the live number is what the door answers.
fn same_written_state(last: &Snapshot, snapshot: &Snapshot) -> bool {
    if !(last.running && snapshot.running) {
        return last.same_state(snapshot);
    }
    let mut last = last.clone();
    last.remaining_seconds = snapshot.remaining_seconds;
    last.same_state(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use crate::timer::Phase;

    #[test]
    fn the_first_snapshot_and_every_edge_lands() {
        let timer = Timer::new(&Settings::default());
        let snapshot = timer.snapshot(&[], Local::now());
        assert!(should_write(None, &snapshot), "the first snapshot lands");
        // a start, a pause, a phase end or a reset each move a recorded field
        let mut started = snapshot.clone();
        started.running = true;
        assert!(should_write(Some(&snapshot), &started));
        let mut paused = started.clone();
        paused.running = false;
        assert!(should_write(Some(&started), &paused));
        let mut next_phase = snapshot.clone();
        next_phase.phase = Phase::ShortBreak;
        next_phase.completed_in_cycle = 1;
        assert!(should_write(Some(&snapshot), &next_phase));
    }

    #[test]
    fn a_running_countdown_writes_nothing_between_its_edges() {
        let mut timer = Timer::new(&Settings::default());
        timer.start();
        let written = timer.snapshot(&[], Local::now());
        // the tick's next second: the remainder moved, nothing else did
        timer.advance(1);
        let ticking = timer.snapshot(&[], Local::now());
        assert_eq!(ticking.remaining_seconds + 1, written.remaining_seconds);
        assert!(
            !should_write(Some(&written), &ticking),
            "the countdown's ticking is not a write"
        );
        // the door's once-a-second `status` reads the same moment: no write
        assert!(!should_write(Some(&written), &written));
    }

    #[test]
    fn a_paused_state_leaves_the_file_alone() {
        let timer = Timer::new(&Settings::default());
        let snapshot = timer.snapshot(&[], Local::now());
        assert!(!should_write(Some(&snapshot), &snapshot));
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
