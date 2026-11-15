//! The headless Pomodoro state machine: phases, remaining time, the cycle
//! counter, completion and skip rules, and the snapshot Warp Station reads.
//! Display-free and I/O-free; the UI tick owns the wall clock and the history
//! file, and the unit tests here drive it a second at a time.
//!
//! Rules the rest of the app relies on:
//! - a phase that reaches zero always advances (the tick hands it whole
//!   seconds, so a busy or sleeping UI reports several transitions at once);
//! - a completed work session is what increments the cycle, and the cycle's
//!   Nth completion enters the long break — a skipped work session does not;
//! - the long break closes the cycle: the next work phase starts a new count;
//! - the phase after a completion starts only when its auto-start switch says
//!   so, while a skip preserves the phase's running state.

use chrono::{DateTime, Duration, Local, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::history::Session;
use crate::settings::Settings;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Work,
    ShortBreak,
    LongBreak,
}

impl Phase {
    pub fn is_work(self) -> bool {
        matches!(self, Phase::Work)
    }

    /// The word the UI shows for this phase.
    pub fn label(self) -> &'static str {
        match self {
            Phase::Work => "Work",
            Phase::ShortBreak => "Short break",
            Phase::LongBreak => "Long break",
        }
    }
}

/// What a natural completion did. The UI records the history entry and fires
/// the desktop notification from these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transition {
    pub ended: Phase,
    pub next: Phase,
    /// The ended phase's full length in seconds (the history entry's `seconds`).
    pub seconds: u32,
}

/// The state file's and the door's payload, exactly the contract's fields.
/// The `schema_version` key is the store's, added around this document.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub phase: Phase,
    pub running: bool,
    pub remaining_seconds: u32,
    pub phase_seconds: u32,
    pub long_break_every: u32,
    pub completed_in_cycle: u32,
    pub today_work: u32,
    pub week_work: u32,
    pub updated_at: String,
    pub version: String,
}

impl Snapshot {
    /// Whether the live state matches, ignoring the stamp the writer refreshes
    /// at least once a second.
    pub fn same_state(&self, other: &Self) -> bool {
        self.phase == other.phase
            && self.running == other.running
            && self.remaining_seconds == other.remaining_seconds
            && self.phase_seconds == other.phase_seconds
            && self.long_break_every == other.long_break_every
            && self.completed_in_cycle == other.completed_in_cycle
            && self.today_work == other.today_work
            && self.week_work == other.week_work
            && self.version == other.version
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Timer {
    settings: Settings,
    phase: Phase,
    remaining: u32,
    phase_seconds: u32,
    running: bool,
    completed_in_cycle: u32,
}

impl Timer {
    /// A timer paused at the start of a full work phase.
    pub fn new(settings: &Settings) -> Self {
        let settings = settings.clone();
        let mut timer = Self {
            settings,
            phase: Phase::Work,
            remaining: 0,
            phase_seconds: 0,
            running: false,
            completed_in_cycle: 0,
        };
        timer.enter(Phase::Work);
        timer
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn remaining_seconds(&self) -> u32 {
        self.remaining
    }

    pub fn phase_seconds(&self) -> u32 {
        self.phase_seconds
    }

    pub fn running(&self) -> bool {
        self.running
    }

    pub fn completed_in_cycle(&self) -> u32 {
        self.completed_in_cycle
    }

    pub fn long_break_every(&self) -> u32 {
        self.settings.long_break_every.clamp(
            crate::settings::LONG_BREAK_EVERY_MIN,
            crate::settings::LONG_BREAK_EVERY_MAX,
        )
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// The phase a natural completion of the current one enters: the break
    /// after this work session (the long one when this completion fills the
    /// cycle), or work after either break.
    pub fn next_phase(&self) -> Phase {
        match self.phase {
            Phase::Work if self.completed_in_cycle + 1 >= self.long_break_every() => {
                Phase::LongBreak
            }
            Phase::Work => Phase::ShortBreak,
            Phase::ShortBreak | Phase::LongBreak => Phase::Work,
        }
    }

    /// The phase's length at the current settings.
    pub fn length_of(&self, phase: Phase) -> u32 {
        let minutes = match phase {
            Phase::Work => self.settings.work_minutes,
            Phase::ShortBreak => self.settings.short_break_minutes,
            Phase::LongBreak => self.settings.long_break_minutes,
        };
        minutes.clamp(1, 24 * 60) * 60
    }

    pub fn start(&mut self) {
        if self.remaining == 0 {
            self.remaining = self.phase_seconds;
        }
        self.running = true;
    }

    pub fn pause(&mut self) {
        self.running = false;
    }

    pub fn toggle(&mut self) {
        if self.running {
            self.pause();
        } else {
            self.start();
        }
    }

    /// Back to the phase's full length, paused.
    pub fn reset(&mut self) {
        self.remaining = self.phase_seconds;
        self.running = false;
    }

    /// Advances to the next phase without completing this one: a skipped work
    /// session never counts, and the phase's running state is preserved.
    pub fn skip(&mut self) {
        let next = match self.phase {
            // a skipped work session does not fill the cycle
            Phase::Work => Phase::ShortBreak,
            Phase::ShortBreak | Phase::LongBreak => Phase::Work,
        };
        if self.phase == Phase::LongBreak {
            self.completed_in_cycle = 0;
        }
        let running = self.running;
        self.enter(next);
        self.running = running;
    }

    /// Runs the countdown forward `seconds`, completing every phase boundary
    /// it crosses. Returns one transition per completed phase, in order; the
    /// caller records history and notifies from them.
    pub fn advance(&mut self, seconds: u64) -> Vec<Transition> {
        let mut transitions = Vec::new();
        let mut left = seconds;
        while left > 0 && self.running {
            if self.remaining > 0 && left < u64::from(self.remaining) {
                self.remaining -= left as u32;
                break;
            }
            left = left.saturating_sub(u64::from(self.remaining));
            self.remaining = 0;
            let ended = self.phase;
            let ended_seconds = self.phase_seconds;
            let next = self.finish_phase();
            transitions.push(Transition {
                ended,
                next,
                seconds: ended_seconds,
            });
        }
        transitions
    }

    /// Retunes a running or paused timer when the settings change. A phase
    /// that was at its full length takes the new length; a phase paused
    /// halfway keeps its remaining time, capped by the new length.
    pub fn apply_settings(&mut self, settings: &Settings) {
        let was_full = self.remaining == self.phase_seconds;
        self.settings = settings.clone();
        self.completed_in_cycle = self.completed_in_cycle.min(self.long_break_every());
        self.phase_seconds = self.length_of(self.phase);
        self.remaining = if was_full {
            self.phase_seconds
        } else {
            self.remaining.min(self.phase_seconds)
        };
    }

    /// The contract's snapshot, with the counts the history file implies for
    /// local day `now` and the six days before it.
    pub fn snapshot(&self, sessions: &[Session], now: DateTime<Local>) -> Snapshot {
        let counts = work_counts(sessions, now.date_naive());
        Snapshot {
            phase: self.phase,
            running: self.running,
            remaining_seconds: self.remaining,
            phase_seconds: self.phase_seconds,
            long_break_every: self.long_break_every(),
            completed_in_cycle: self.completed_in_cycle,
            today_work: counts.today,
            week_work: counts.week,
            updated_at: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
            version: crate::VERSION.to_string(),
        }
    }

    fn enter(&mut self, phase: Phase) {
        self.phase = phase;
        self.phase_seconds = self.length_of(phase);
        self.remaining = self.phase_seconds;
    }

    fn finish_phase(&mut self) -> Phase {
        let ended = self.phase;
        let next = match ended {
            Phase::Work => {
                self.completed_in_cycle += 1;
                if self.completed_in_cycle >= self.long_break_every() {
                    Phase::LongBreak
                } else {
                    Phase::ShortBreak
                }
            }
            Phase::ShortBreak | Phase::LongBreak => Phase::Work,
        };
        // the long break closes the cycle
        if ended == Phase::LongBreak {
            self.completed_in_cycle = 0;
        }
        self.enter(next);
        self.running = if next == Phase::Work {
            self.settings.auto_start_work
        } else {
            self.settings.auto_start_breaks
        };
        next
    }
}

/// The seven-day window's completed focus sessions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorkCounts {
    pub today: u32,
    pub week: u32,
}

/// Counts completed work sessions on the local calendar day `today` and in
/// the seven-day window ending there. Breaks are never counted.
pub fn work_counts(sessions: &[Session], today: NaiveDate) -> WorkCounts {
    let week_start = today - Duration::days(6);
    let mut counts = WorkCounts::default();
    for session in sessions {
        if session.kind != Phase::Work {
            continue;
        }
        let Some(day) = session.local_date() else {
            continue;
        };
        if day == today {
            counts.today += 1;
        }
        if day >= week_start && day <= today {
            counts.week += 1;
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(change: impl FnOnce(&mut Settings)) -> Settings {
        let mut settings = Settings::default();
        change(&mut settings);
        settings
    }

    /// Starts the phase if it is paused and runs it out, returning its one
    /// transition.
    fn finish(timer: &mut Timer) -> Transition {
        timer.start();
        let remaining = u64::from(timer.remaining_seconds());
        let mut transitions = timer.advance(remaining);
        assert_eq!(transitions.len(), 1, "one phase boundary per finish");
        transitions.remove(0)
    }

    fn session(kind: Phase, day: &str, clock: &str) -> Session {
        Session {
            kind,
            started_at: format!("{day}T{clock}:00+03:00"),
            ended_at: format!("{day}T{clock}:00+03:00"),
            seconds: 60,
        }
    }

    #[test]
    fn a_new_timer_starts_paused_at_a_full_work_phase() {
        let timer = Timer::new(&Settings::default());
        assert_eq!(timer.phase(), Phase::Work);
        assert!(!timer.running());
        assert_eq!(timer.remaining_seconds(), 1500);
        assert_eq!(timer.phase_seconds(), 1500);
        assert_eq!(timer.completed_in_cycle(), 0);
        assert_eq!(timer.long_break_every(), 4);
    }

    #[test]
    fn the_countdown_runs_down_and_a_pause_freezes_it() {
        let mut timer = Timer::new(&Settings::default());
        timer.start();
        assert!(timer.advance(1).is_empty());
        assert_eq!(timer.remaining_seconds(), 1499);
        timer.pause();
        assert!(timer.advance(10).is_empty());
        assert_eq!(timer.remaining_seconds(), 1499);
        timer.start();
        assert_eq!(timer.remaining_seconds(), 1499);
    }

    #[test]
    fn a_completed_work_session_counts_and_starts_the_short_break() {
        let mut timer = Timer::new(&Settings::default());
        timer.start();
        let transitions = timer.advance(1500);
        assert_eq!(
            transitions,
            vec![Transition {
                ended: Phase::Work,
                next: Phase::ShortBreak,
                seconds: 1500,
            }]
        );
        assert_eq!(timer.phase(), Phase::ShortBreak);
        assert_eq!(timer.remaining_seconds(), 300);
        assert_eq!(timer.completed_in_cycle(), 1);
        assert!(timer.running(), "auto_start_breaks defaults on");
    }

    #[test]
    fn the_next_phase_parks_paused_when_its_auto_start_is_off() {
        let mut timer = Timer::new(&settings(|settings| {
            settings.auto_start_breaks = false;
            settings.auto_start_work = true;
        }));
        timer.start();
        timer.advance(1500);
        assert_eq!(timer.phase(), Phase::ShortBreak);
        assert!(!timer.running());
        // auto_start_work is on: finishing the break starts work by itself
        let transition = finish(&mut timer);
        assert_eq!(transition.next, Phase::Work);
        assert!(timer.running());
    }

    #[test]
    fn every_completed_work_session_moves_the_cycle_and_the_fourth_is_long() {
        let mut timer = Timer::new(&Settings::default());
        for round in 1..=4 {
            let work = finish(&mut timer);
            assert_eq!(work.ended, Phase::Work);
            if round < 4 {
                assert_eq!(work.next, Phase::ShortBreak);
                assert_eq!(timer.completed_in_cycle(), round);
                let brk = finish(&mut timer);
                assert_eq!(brk.next, Phase::Work);
            } else {
                assert_eq!(work.next, Phase::LongBreak);
                assert_eq!(timer.phase(), Phase::LongBreak);
                assert_eq!(timer.remaining_seconds(), 900);
                assert_eq!(timer.completed_in_cycle(), 4);
                let brk = finish(&mut timer);
                assert_eq!(brk.next, Phase::Work);
                assert_eq!(
                    timer.completed_in_cycle(),
                    0,
                    "the long break closes the cycle"
                );
            }
        }
    }

    #[test]
    fn a_skipped_work_session_records_nothing_and_keeps_running() {
        let mut timer = Timer::new(&Settings::default());
        timer.start();
        timer.advance(10);
        timer.skip();
        assert_eq!(timer.phase(), Phase::ShortBreak);
        assert_eq!(timer.completed_in_cycle(), 0);
        assert!(timer.running(), "a skip preserves the running state");
        assert_eq!(timer.remaining_seconds(), 300);
        timer.skip();
        assert_eq!(timer.phase(), Phase::Work);
        assert!(timer.running());
    }

    #[test]
    fn skipping_a_long_break_starts_the_next_cycle() {
        let mut timer = Timer::new(&Settings::default());
        for _ in 0..4 {
            finish(&mut timer);
            if timer.phase() != Phase::LongBreak {
                finish(&mut timer);
            }
        }
        assert_eq!(timer.phase(), Phase::LongBreak);
        assert_eq!(timer.completed_in_cycle(), 4);
        timer.skip();
        assert_eq!(timer.phase(), Phase::Work);
        assert_eq!(timer.completed_in_cycle(), 0);
    }

    #[test]
    fn a_reset_returns_the_phase_to_full_and_pauses() {
        let mut timer = Timer::new(&Settings::default());
        timer.start();
        timer.advance(10);
        timer.reset();
        assert_eq!(timer.remaining_seconds(), 1500);
        assert!(!timer.running());
    }

    #[test]
    fn a_long_tick_reports_every_phase_it_crossed() {
        let mut timer = Timer::new(&settings(|settings| settings.auto_start_work = true));
        timer.start();
        let transitions = timer.advance(1800);
        assert_eq!(transitions.len(), 2);
        assert_eq!(transitions[0].ended, Phase::Work);
        assert_eq!(transitions[0].next, Phase::ShortBreak);
        assert_eq!(transitions[1].ended, Phase::ShortBreak);
        assert_eq!(transitions[1].next, Phase::Work);
        assert_eq!(timer.phase(), Phase::Work);
        assert_eq!(timer.remaining_seconds(), 1500);
    }

    #[test]
    fn changing_the_durations_retunes_a_waiting_phase() {
        let mut timer = Timer::new(&Settings::default());
        timer.start();
        timer.advance(10);
        timer.pause();
        timer.apply_settings(&settings(|settings| settings.work_minutes = 30));
        assert_eq!(timer.phase_seconds(), 1800);
        assert_eq!(
            timer.remaining_seconds(),
            1490,
            "a paused phase keeps its time"
        );
        timer.reset();
        assert_eq!(timer.remaining_seconds(), 1800);
        timer.apply_settings(&settings(|settings| settings.work_minutes = 5));
        assert_eq!(
            timer.remaining_seconds(),
            300,
            "a full phase takes the new full length"
        );
    }

    #[test]
    fn the_cycle_prediction_names_the_break_that_will_follow() {
        let mut timer = Timer::new(&Settings::default());
        assert_eq!(timer.next_phase(), Phase::ShortBreak);
        for _ in 0..3 {
            finish(&mut timer);
            finish(&mut timer);
        }
        assert_eq!(timer.phase(), Phase::Work);
        assert_eq!(timer.completed_in_cycle(), 3);
        assert_eq!(timer.next_phase(), Phase::LongBreak);
    }

    #[test]
    fn the_counts_read_local_days_and_ignore_breaks() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
        let sessions = vec![
            session(Phase::Work, "2026-11-15", "09:00"),
            session(Phase::Work, "2026-11-15", "10:00"),
            session(Phase::ShortBreak, "2026-11-15", "09:30"),
            session(Phase::Work, "2026-11-10", "11:00"),
            session(Phase::Work, "2026-11-08", "11:00"),
            session(Phase::LongBreak, "2026-11-09", "12:00"),
        ];
        let counts = work_counts(&sessions, today);
        assert_eq!(counts.today, 2);
        assert_eq!(counts.week, 3, "today plus the six days before it");
    }

    #[test]
    fn the_snapshot_is_the_contract_document() {
        let timer = Timer::new(&Settings::default());
        let now = Local::now();
        let snapshot = timer.snapshot(&[], now);
        let value = serde_json::to_value(&snapshot).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "completed_in_cycle",
                "long_break_every",
                "phase",
                "phase_seconds",
                "remaining_seconds",
                "running",
                "today_work",
                "updated_at",
                "version",
                "week_work",
            ]
        );
        assert_eq!(value["phase"], "work");
        assert_eq!(value["running"], false);
        assert_eq!(value["remaining_seconds"], 1500);
        assert_eq!(value["phase_seconds"], 1500);
        assert_eq!(value["long_break_every"], 4);
        assert_eq!(value["completed_in_cycle"], 0);
        assert_eq!(value["version"], crate::VERSION);
        let stamp = value["updated_at"].as_str().unwrap();
        assert!(stamp.contains('T') && stamp.len() >= 20, "{stamp}");
    }

    #[test]
    fn the_second_stamp_does_not_make_a_snapshot_different() {
        let timer = Timer::new(&Settings::default());
        let first = timer.snapshot(&[], Local::now());
        let mut second = first.clone();
        second.updated_at = "2026-11-15T20:45:14+03:00".into();
        assert!(first.same_state(&second));
        second.remaining_seconds -= 1;
        assert!(!first.same_state(&second));
    }
}
