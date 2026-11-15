//! The completed-session log at `~/.warp/pomodoro/history.json`: versioned
//! snake_case, newest last, capped at the last 500 completed work sessions.
//! Work sessions become history when they complete; a skip records nothing.
//! Completed breaks ride inside the same window, so the history page can show
//! what the day looked like next to its focus sessions.

use chrono::{DateTime, Duration, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use warpui_kit_platform::config::Store;

use crate::paths::AppPaths;
use crate::timer::Phase;

/// The file keeps the newest 500 completed work sessions.
pub const MAX_WORK_SESSIONS: usize = 500;

fn rfc3339(at: DateTime<Local>) -> String {
    // local time with its offset, seconds precision: the contract's shape
    at.to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

/// One completed session. The timestamps are RFC 3339 local strings, written
/// by this app; `seconds` is the phase's full length when it completed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub kind: Phase,
    pub started_at: String,
    pub ended_at: String,
    pub seconds: u32,
}

impl Session {
    /// A session that ended at `ended_at` and ran its full `seconds`.
    pub fn completed(kind: Phase, ended_at: DateTime<Local>, seconds: u32) -> Self {
        let started = ended_at - Duration::seconds(i64::from(seconds.min(86_400)));
        Self {
            kind,
            started_at: rfc3339(started),
            ended_at: rfc3339(ended_at),
            seconds,
        }
    }

    /// The local calendar day the session ended on. The RFC 3339 string we
    /// write starts with that date, so no timezone conversion is needed — and
    /// a hand-edited string in another offset still counts by the local date
    /// it claims.
    pub fn local_date(&self) -> Option<NaiveDate> {
        NaiveDate::parse_from_str(self.ended_at.get(..10)?, "%Y-%m-%d").ok()
    }

    /// `09:25–09:50` for the history page. The timestamps we write start with
    /// `YYYY-MM-DDTHH:MM:SS`, so the clock reads off the string.
    pub fn clock_range(&self) -> String {
        let start = self.started_at.get(11..16).unwrap_or("--:--");
        let end = self.ended_at.get(11..16).unwrap_or("--:--");
        format!("{start}–{end}")
    }
}

/// One day of the seven-day summary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DayCount {
    pub date: NaiveDate,
    pub work: u32,
    pub breaks: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct History {
    pub sessions: Vec<Session>,
}

impl History {
    /// Reads the log. A missing file is empty history; an unreadable or
    /// corrupt file is logged and treated as empty (the timer keeps working —
    /// only the counts lose their past).
    pub fn load() -> Self {
        let store = Store::<History>::new(AppPaths::history_file());
        match store.load_or_default() {
            Ok(mut history) => {
                history.trim();
                history
            }
            Err(err) => {
                tracing::warn!("could not read the history file: {err}");
                Self::default()
            }
        }
    }

    pub fn save(&self) {
        let store = Store::<History>::new(AppPaths::history_file());
        if let Err(err) = store.save(self) {
            tracing::warn!("could not save the history file: {err}");
        }
    }

    /// Appends one completed session and enforces the cap.
    pub fn record(&mut self, kind: Phase, ended_at: DateTime<Local>, seconds: u32) {
        self.sessions
            .push(Session::completed(kind, ended_at, seconds));
        self.trim();
    }

    /// Drops everything before the 500th-newest completed work session.
    pub fn trim(&mut self) {
        let work: Vec<usize> = self
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, session)| session.kind == Phase::Work)
            .map(|(index, _)| index)
            .collect();
        if work.len() > MAX_WORK_SESSIONS {
            let cut = work[work.len() - MAX_WORK_SESSIONS];
            self.sessions.drain(..cut);
        }
    }

    /// The sessions that ended on `day`, oldest first.
    pub fn sessions_on(&self, day: NaiveDate) -> Vec<&Session> {
        self.sessions
            .iter()
            .filter(|session| session.local_date() == Some(day))
            .collect()
    }

    /// The seven local days ending at `today`, oldest first, with the focus
    /// and break sessions each one holds.
    pub fn week(&self, today: NaiveDate) -> Vec<DayCount> {
        let mut days: Vec<DayCount> = (0..7)
            .rev()
            .map(|back| DayCount {
                date: today - Duration::days(back),
                work: 0,
                breaks: 0,
            })
            .collect();
        for session in &self.sessions {
            let Some(day) = session.local_date() else {
                continue;
            };
            let Some(entry) = days.iter_mut().find(|entry| entry.date == day) else {
                continue;
            };
            if session.kind == Phase::Work {
                entry.work += 1;
            } else {
                entry.breaks += 1;
            }
        }
        days
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(kind: Phase, day: &str, clock: &str) -> Session {
        Session {
            kind,
            started_at: format!("{day}T{clock}:00+03:00"),
            ended_at: format!("{day}T{clock}:00+03:00"),
            seconds: 60,
        }
    }

    #[test]
    fn the_cap_keeps_the_newest_work_sessions_and_the_breaks_beside_them() {
        let mut history = History::default();
        for index in 0..(MAX_WORK_SESSIONS + 5) {
            let day = format!("2026-11-{:02}", index % 28 + 1);
            history.sessions.push(session(Phase::Work, &day, "09:00"));
            history
                .sessions
                .push(session(Phase::ShortBreak, &day, "09:30"));
        }
        history.trim();
        let work = history
            .sessions
            .iter()
            .filter(|session| session.kind == Phase::Work)
            .count();
        assert_eq!(work, MAX_WORK_SESSIONS);
        // the five oldest work sessions and their breaks are gone
        assert_eq!(
            history.sessions.len(),
            MAX_WORK_SESSIONS * 2,
            "breaks inside the window stay"
        );
    }

    #[test]
    fn the_local_date_and_clock_read_off_the_written_strings() {
        let work = session(Phase::Work, "2026-11-15", "09:25");
        assert_eq!(work.local_date(), NaiveDate::from_ymd_opt(2026, 11, 15));
        assert_eq!(work.clock_range(), "09:25–09:25");
        let malformed = Session {
            kind: Phase::Work,
            started_at: "nope".into(),
            ended_at: "nope".into(),
            seconds: 1,
        };
        assert_eq!(malformed.local_date(), None);
        assert_eq!(malformed.clock_range(), "--:--–--:--");
    }

    #[test]
    fn the_week_counts_work_and_breaks_per_local_day() {
        let today = NaiveDate::from_ymd_opt(2026, 11, 15).unwrap();
        let history = History {
            sessions: vec![
                session(Phase::Work, "2026-11-15", "09:00"),
                session(Phase::Work, "2026-11-15", "10:00"),
                session(Phase::ShortBreak, "2026-11-15", "09:30"),
                session(Phase::Work, "2026-11-14", "11:00"),
                // one day outside the seven-day window
                session(Phase::Work, "2026-11-08", "11:00"),
            ],
        };
        let week = history.week(today);
        assert_eq!(week.len(), 7);
        assert_eq!(week[6].date, today);
        assert_eq!(week[6].work, 2);
        assert_eq!(week[6].breaks, 1);
        assert_eq!(week[5].work, 1);
        assert_eq!(week[0].date, NaiveDate::from_ymd_opt(2026, 11, 9).unwrap());
        assert_eq!(week.iter().map(|day| day.work).sum::<u32>(), 3);
        assert_eq!(history.sessions_on(today).len(), 3);
    }

    #[test]
    fn a_completed_session_spans_its_full_length() {
        let now = Local::now();
        let work = Session::completed(Phase::Work, now, 1500);
        assert_eq!(work.local_date(), Some(now.date_naive()));
        assert_eq!(work.seconds, 1500);
        assert_ne!(work.started_at, work.ended_at);
    }
}
