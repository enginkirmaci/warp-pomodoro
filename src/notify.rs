//! Desktop notifications for a phase end: one entry point, one branch per
//! platform. Failures are logged by [`phase_ended`], never turned into a panic
//! — a missing notification daemon must not take a timer down.
//!
//! - Linux (and the other Unix desktops): spawn `notify-send`, which every
//!   notification daemon ships (the same command-gated pattern the other apps
//!   use). A missing binary is a log line.
//! - Windows: a WinRT toast through `tauri-winrt-notification`, addressed by
//!   PowerShell's whitelisted AppUserModelID so Action Center accepts it
//!   without a Start-menu registration.
//! - macOS: a best-effort stub so shared code never blocks on Apple specifics.

use crate::timer::Phase;

/// The one-line title and body of a phase-end notice.
pub fn phase_summary(ended: Phase, next: Phase) -> (&'static str, String) {
    match ended {
        Phase::Work => (
            "Work session complete",
            match next {
                Phase::LongBreak => "A long break is next.".to_string(),
                _ => "A short break is next.".to_string(),
            },
        ),
        Phase::ShortBreak | Phase::LongBreak => ("Break over", "Back to work.".to_string()),
    }
}

/// Relays one phase end to the desktop; the settings gate is the caller's.
pub fn phase_ended(ended: Phase, next: Phase) {
    let (summary, body) = phase_summary(ended, next);
    if let Err(err) = publish(summary, &body) {
        tracing::warn!("desktop notification failed: {err}");
    }
}

/// Publishes one notice on the calling thread. Failures are returned, never
/// logged twice or turned into a panic.
pub fn publish(summary: &str, body: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let _ = (summary, body);
        Ok(())
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        crate::spawn::detached(
            std::process::Command::new("notify-send")
                .arg("--app-name=Warp Pomodoro")
                .arg(summary)
                .arg(body),
        )
        .map_err(|err| format!("notify-send: {err}"))
    }

    #[cfg(windows)]
    {
        use tauri_winrt_notification::Toast;

        Toast::new(Toast::POWERSHELL_APP_ID)
            .title(summary)
            .text1(body)
            .show()
            .map_err(|err| format!("toast: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finished_work_session_names_the_break_that_follows() {
        assert_eq!(
            phase_summary(Phase::Work, Phase::ShortBreak),
            ("Work session complete", "A short break is next.".into())
        );
        assert_eq!(
            phase_summary(Phase::Work, Phase::LongBreak),
            ("Work session complete", "A long break is next.".into())
        );
    }

    #[test]
    fn a_finished_break_sends_the_user_back_to_work() {
        assert_eq!(
            phase_summary(Phase::ShortBreak, Phase::Work),
            ("Break over", "Back to work.".into())
        );
        assert_eq!(
            phase_summary(Phase::LongBreak, Phase::Work),
            ("Break over", "Back to work.".into())
        );
    }
}
