//! The mini card: the same timer in a small always-on-top window. It carries
//! the countdown, the phase and cycle, and four controls — start/pause,
//! expand to the full window, pin (always on top) and close. The whole card
//! drags the window and a double click expands it.
//!
//! Nothing here owns state: it reads the window's timer and settings and calls
//! the same verbs the full pages call, so the door, the snapshot and the
//! history behave exactly as they do in the big window.

use std::time::Instant;

use gpui::prelude::*;
use gpui::{App, ClickEvent, Context, FontWeight, MouseButton, SharedString, Window, div, px};

use crate::settings::WindowMode;
use crate::theme;
use crate::ui::A11y;
use crate::ui::icon;
use crate::ui::timer::clock_text;
use crate::ui::window::{MainWindow, SHOW_TOOLTIPS};

impl MainWindow {
    /// The mini card as the window's whole content.
    pub(crate) fn render_mini(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let running = self.timer.running();
        let remaining = self.timer.remaining_seconds();
        let phase = self.timer.phase();
        let completed = self.timer.completed_in_cycle();
        let every = self.timer.long_break_every();
        let next = self.timer.next_phase();
        let ring = self.ring_state(Instant::now());
        let ink = gpui::Hsla::from(theme::accent());
        let track = gpui::Hsla::from(theme::raised());
        let pinned = self.settings.always_on_top;

        let phase_line = SharedString::from(format!("{} · {}/{}", phase.label(), completed, every));
        let next_minutes = self.timer.length_of(next) / 60;

        div()
            .id("mini-card")
            .debug_selector(|| "mini-card".into())
            .size_full()
            .flex()
            .items_center()
            .gap_3()
            .p_3()
            .rounded_lg()
            .border_1()
            .border_color(theme::hairline())
            .bg(theme::panel())
            // the card is the window: every press that reaches it drags the
            // card (the controls stop their own presses), and a double click
            // opens the full window
            .on_mouse_down(MouseButton::Left, |_, window, _| {
                window.start_window_move();
            })
            .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                if event.click_count() >= 2 {
                    this.set_mode(WindowMode::Full, window, cx);
                }
            }))
            .child(crate::ui::ring::ring(
                theme::scaled(theme::MINI_RING),
                theme::scaled(theme::MINI_RING_THICKNESS),
                ring,
                ink,
                track,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .debug_selector(|| "mini-clock".into())
                            .font_family(theme::code_font())
                            .text_size(px(theme::scaled(theme::MINI_CLOCK)))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme::text())
                            .child(SharedString::from(clock_text(remaining))),
                    )
                    .child(
                        div()
                            .debug_selector(|| "mini-phase".into())
                            .text_size(px(theme::font_micro()))
                            .text_color(theme::text_dim())
                            .whitespace_nowrap()
                            .child(phase_line),
                    )
                    .child(
                        div()
                            .text_size(px(theme::font_micro()))
                            .text_color(theme::text_muted())
                            .whitespace_nowrap()
                            .child(SharedString::from(format!("Next: {next_minutes} min"))),
                    ),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .child(self.mini_button(
                        "mini-toggle",
                        if running {
                            "icons/icon-pause.svg"
                        } else {
                            "icons/icon-play.svg"
                        },
                        if running { "Pause" } else { "Start" },
                        false,
                        cx.listener(|this, _, _, cx| this.toggle_timer(cx)),
                    ))
                    .child(self.mini_button(
                        "mini-expand",
                        "icons/icon-expand.svg",
                        "Full screen",
                        false,
                        cx.listener(|this, _, window, cx| {
                            this.set_mode(WindowMode::Full, window, cx)
                        }),
                    ))
                    .child(self.mini_button(
                        "mini-pin",
                        "icons/icon-pin.svg",
                        if pinned {
                            "Always on top on"
                        } else {
                            "Always on top off"
                        },
                        pinned,
                        cx.listener(|this, _, window, cx| this.toggle_topmost(window, cx)),
                    ))
                    .child(self.mini_button(
                        "mini-close",
                        "icons/icon-window-close.svg",
                        "Close",
                        false,
                        cx.listener(|this, _, window, _| {
                            this.flush_exit();
                            window.remove_window();
                        }),
                    )),
            )
            .into_any_element()
    }

    /// One mini control: a small square icon button that stops its own press
    /// from dragging the card. `active` fills it for the pin's on state.
    fn mini_button(
        &self,
        id: &'static str,
        icon_path: &'static str,
        tooltip: &'static str,
        active: bool,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> gpui::AnyElement {
        let color = if active {
            theme::accent_ink()
        } else {
            theme::text_muted()
        };
        div()
            .id(id)
            .debug_selector(move || id.into())
            .a11y_button(tooltip)
            .cursor_pointer()
            .occlude()
            .flex_shrink_0()
            .size(px(theme::scaled(theme::MINI_BUTTON)))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .when(active, |button| button.bg(theme::accent_soft()))
            .hover(|button| {
                if active {
                    button.bg(theme::accent_soft())
                } else {
                    button.bg(theme::raised())
                }
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(on_click)
            .when(SHOW_TOOLTIPS, |button| {
                button.tooltip(crate::ui::tooltip_text(tooltip))
            })
            .child(icon(icon_path, px(theme::icon_small()), color))
            .into_any_element()
    }
}
