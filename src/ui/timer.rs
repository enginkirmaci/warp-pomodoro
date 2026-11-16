//! The Timer page: the countdown, the phase, the cycle dots, the day's counts
//! and the three verbs. One primary (Start/Pause), two secondaries (Reset and
//! Skip), and a hint naming the phase that comes next.

use chrono::Local;
use gpui::prelude::*;
use gpui::{AnyElement, Context, FontWeight, SharedString, div, px};
use warpui_kit_core::controls::{BtnKind, button};
use warpui_kit_core::shell;

use crate::theme;
use crate::ui::window::MainWindow;

impl MainWindow {
    pub(crate) fn render_timer(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let running = self.timer.running();
        let remaining = self.timer.remaining_seconds();
        let phase_seconds = self.timer.phase_seconds();
        let phase = self.timer.phase();
        let completed = self.timer.completed_in_cycle();
        let every = self.timer.long_break_every();
        let next = self.timer.next_phase();
        let next_minutes = self.timer.length_of(next) / 60;
        let counts = self.timer.snapshot(&self.history.sessions, Local::now());

        let primary_label = if running {
            "Pause"
        } else if remaining < phase_seconds {
            "Resume"
        } else {
            "Start"
        };
        let primary = button(
            "timer-primary-button",
            primary_label,
            BtnKind::Primary(theme::on_accent()),
            theme::accent(),
            theme::font_control(),
            true,
            cx.listener(|this, _, _, cx| this.toggle_timer(cx)),
        );
        let primary_slot = if running {
            control_slot("timer-pause", primary)
        } else {
            control_slot("timer-start", primary)
        };
        let reset = button(
            "timer-reset-button",
            "Reset",
            BtnKind::Chip,
            theme::accent(),
            theme::font_control(),
            true,
            cx.listener(|this, _, _, cx| this.reset_phase(cx)),
        );
        let skip = button(
            "timer-skip-button",
            "Skip",
            BtnKind::Chip,
            theme::accent(),
            theme::font_control(),
            true,
            cx.listener(|this, _, _, cx| this.skip_phase(cx)),
        );

        let mut dots = div()
            .debug_selector(|| "timer-cycle".into())
            .flex()
            .items_center()
            .gap(px(theme::scaled(theme::CYCLE_DOT_GAP)));
        for index in 0..every {
            let filled = index < completed;
            dots = dots.child(
                div()
                    .size(px(theme::scaled(theme::CYCLE_DOT)))
                    .rounded_full()
                    .when(filled, |dot| dot.bg(theme::accent()))
                    .when(!filled, |dot| {
                        dot.bg(theme::raised())
                            .border_1()
                            .border_color(theme::hairline())
                    }),
            );
        }

        let ring = self.ring_state(std::time::Instant::now());
        let ink = gpui::Hsla::from(theme::accent());
        let track = gpui::Hsla::from(theme::raised());
        let ring_size = theme::scaled(theme::RING_SIZE);

        shell::detail_pane()
            .debug_selector(|| "timer-page".into())
            .child(self.page_head(
                "Timer",
                format!(
                    "{} completed today · {} in the last 7 days",
                    counts.today_work, counts.week_work
                ),
            ))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_5()
                    .p(px(shell::DETAIL_PADDING))
                    .child(
                        // the ring is the page's clock face: the countdown,
                        // the phase and the cycle sit inside it, and the arc
                        // drains with the phase
                        div()
                            .relative()
                            .debug_selector(|| "timer-ring".into())
                            .size(px(ring_size))
                            .flex_shrink_0()
                            .child(crate::ui::ring::ring(
                                ring_size,
                                theme::scaled(theme::RING_THICKNESS),
                                ring,
                                ink,
                                track,
                            ))
                            .child(
                                // absolute without an inset sits at its
                                // static position (after the canvas), so pin
                                // it to the ring's own box
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .size_full()
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .debug_selector(|| "timer-phase".into())
                                            .text_size(px(theme::font_sub()))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(theme::accent_ink())
                                            .child(phase.label()),
                                    )
                                    .child(clock(remaining))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .items_center()
                                            .gap_1()
                                            .child(dots)
                                            .child(
                                                div()
                                                    .text_size(px(theme::font_micro()))
                                                    .text_color(theme::text_muted())
                                                    .child(format!("Cycle {completed}/{every}")),
                                            ),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .debug_selector(|| "timer-next".into())
                            .text_size(px(theme::font_meta()))
                            .text_color(theme::text_dim())
                            .child(format!(
                                "Next: {} · {next_minutes} min",
                                next.label().to_lowercase()
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(primary_slot)
                            .child(control_slot("timer-reset", reset))
                            .child(control_slot("timer-skip", skip)),
                    ),
            )
            .into_any_element()
    }
}

/// The countdown's face. The mono face keeps every digit the same width, so a
/// ticking second never makes the centred clock shuffle sideways.
fn clock(seconds: u32) -> AnyElement {
    div()
        .debug_selector(|| "timer-clock".into())
        .font_family(SharedString::from(theme::code_font()))
        .text_size(px(theme::scaled(theme::TIMER_DISPLAY)))
        .line_height(gpui::relative(1.1))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme::text())
        .child(SharedString::from(clock_text(seconds)))
        .into_any_element()
}

/// `mm:ss`, shared by the page's clock and the mini card. Durations run to a
/// day, so the minutes field is not capped at two digits.
pub(crate) fn clock_text(seconds: u32) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

/// A debug-selector slot around a shared control: `controls::button` takes no
/// debug selector of its own, and a probe clicks the slot's centre.
fn control_slot(selector: &'static str, element: AnyElement) -> AnyElement {
    div()
        .debug_selector(move || selector.into())
        .flex_shrink_0()
        .child(element)
        .into_any_element()
}
