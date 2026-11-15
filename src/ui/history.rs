//! The History page: today's sessions, then the seven-day summary as one bar
//! row per local day. Read-only — the numbers come from `history.json`, which
//! the timer writes as phases complete.

use chrono::Local;
use gpui::prelude::*;
use gpui::{AnyElement, Context, SharedString, div, px};
use warpui_kit_core::shell;

use crate::history::{DayCount, Session};
use crate::theme;
use crate::ui::window::MainWindow;

impl MainWindow {
    pub(crate) fn render_history(&mut self, _cx: &mut Context<Self>) -> AnyElement {
        let today = Local::now().date_naive();
        let sessions = self.history.sessions_on(today);
        let week = self.history.week(today);
        let today_work = sessions
            .iter()
            .filter(|session| session.kind.is_work())
            .count() as u32;
        let week_work: u32 = week.iter().map(|day| day.work).sum();
        let busiest = week.iter().map(|day| day.work).max().unwrap_or(0).max(1);

        let mut body = div()
            .id("history-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(px(shell::DETAIL_PADDING))
            .flex()
            .flex_col()
            .gap_3();
        body = body.child(section_head("Today", focus_sessions(today_work)));
        if sessions.is_empty() {
            body = body.child(empty_line("No sessions yet today."));
        } else {
            for session in sessions {
                body = body.child(session_row(session));
            }
        }
        body = body.child(
            div()
                .mt_4()
                .child(section_head("Last 7 days", focus_sessions(week_work))),
        );
        for (index, day) in week.iter().enumerate() {
            body = body.child(day_row(index, day, busiest, today));
        }

        shell::detail_pane()
            .debug_selector(|| "history-page".into())
            .child(self.page_head("History", "Today and the last seven days"))
            .child(body)
            .into_any_element()
    }
}

/// `1 focus session` / `3 focus sessions`: the head's count line reads as
/// prose, not as a template.
fn focus_sessions(count: u32) -> String {
    if count == 1 {
        "1 focus session".to_string()
    } else {
        format!("{count} focus sessions")
    }
}

fn section_head(title: &'static str, meta: String) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            div()
                .text_size(px(theme::font_heading()))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme::text())
                .child(title),
        )
        .child(
            div()
                .text_size(px(theme::font_meta()))
                .text_color(theme::text_muted())
                .child(meta),
        )
        .into_any_element()
}

fn empty_line(text: &'static str) -> AnyElement {
    div()
        .text_size(px(theme::font_meta()))
        .text_color(theme::text_muted())
        .child(text)
        .into_any_element()
}

/// One completed session: what it was, when it ran and how long it lasted.
fn session_row(session: &Session) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .px_3()
        .py_2()
        .rounded(px(6.))
        .bg(theme::panel())
        .border_1()
        .border_color(theme::hairline())
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(theme::font_body()))
                .text_color(theme::text())
                .child(session.kind.label()),
        )
        .child(
            div()
                .flex_shrink_0()
                .font_family(SharedString::from(theme::code_font()))
                .text_size(px(theme::font_mono_meta()))
                .text_color(theme::text_dim())
                .child(session.clock_range()),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_size(px(theme::font_meta()))
                .text_color(theme::text_muted())
                .child(format!("{} min", session.seconds / 60)),
        )
        .into_any_element()
}

/// One day of the seven-day summary: the day, its bar and its count.
fn day_row(index: usize, day: &DayCount, busiest: u32, today: chrono::NaiveDate) -> AnyElement {
    let label = if day.date == today {
        "Today".to_string()
    } else {
        day.date.format("%a %d").to_string()
    };
    let width = if day.work == 0 {
        0.
    } else {
        theme::HISTORY_BAR_MIN_W
            + (theme::HISTORY_BAR_MAX_W - theme::HISTORY_BAR_MIN_W)
                * (day.work as f32 / busiest as f32)
    };
    let selector = format!("history-day-{index}");
    div()
        .debug_selector(move || selector.clone())
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .child(
            div()
                .flex_shrink_0()
                .w(px(theme::scaled(theme::HISTORY_DAY_LABEL_W)))
                .text_size(px(theme::font_meta()))
                .text_color(theme::text_dim())
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h(px(theme::scaled(theme::HISTORY_BAR_H)))
                .rounded_full()
                .bg(theme::well())
                .child(
                    div()
                        .h_full()
                        .w(px(theme::scaled(width)))
                        .rounded_full()
                        .when(day.work > 0, |bar| bar.bg(theme::accent())),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .w(px(24.))
                .text_right()
                .text_size(px(theme::font_meta()))
                .text_color(theme::text_muted())
                .child(format!("{}", day.work)),
        )
        .into_any_element()
}
