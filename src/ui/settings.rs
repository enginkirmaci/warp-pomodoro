//! The Settings page: the durations, the long-break cycle, the two auto-start
//! switches, the notification switch, and the family's text-size and palette
//! controls. Everything applies at once — the design has no Save — and lands
//! in `~/.warp/pomodoro/settings.json`.

use gpui::prelude::*;
use gpui::{AnyElement, App, ClickEvent, Context, SharedString, Window, div, px};
use warpui_kit_core::settings as kit;
use warpui_kit_core::shell;

use crate::settings::{LONG_BREAK_EVERY_MAX, LONG_BREAK_EVERY_MIN, MINUTES_MAX, MINUTES_MIN};
use crate::theme;
use crate::ui::A11y;
use crate::ui::window::MainWindow;

/// Which duration a stepper edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StepField {
    Work,
    ShortBreak,
    LongBreak,
    LongBreakEvery,
}

impl MainWindow {
    pub(crate) fn render_settings(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = self.settings.clone();
        let mut rows: Vec<AnyElement> = Vec::new();
        rows.push(kit::subhead("Durations"));
        rows.push(kit::row_with_desc(
            "settings-row-Focus",
            "Focus session",
            "How long one work session runs",
            self.stepper(StepField::Work, settings.work_minutes, "min", cx),
            true,
        ));
        rows.push(kit::row_with_desc(
            "settings-row-Short break",
            "Short break",
            "The break between focus sessions",
            self.stepper(
                StepField::ShortBreak,
                settings.short_break_minutes,
                "min",
                cx,
            ),
            false,
        ));
        rows.push(kit::row_with_desc(
            "settings-row-Long break",
            "Long break",
            "The break that closes a cycle",
            self.stepper(StepField::LongBreak, settings.long_break_minutes, "min", cx),
            false,
        ));
        rows.push(kit::row_with_desc(
            "settings-row-Long break after",
            "Long break after",
            "Focus sessions per cycle before the long break",
            self.stepper(
                StepField::LongBreakEvery,
                settings.long_break_every,
                "sessions",
                cx,
            ),
            false,
        ));

        rows.push(kit::subhead("Behaviour"));
        rows.push(kit::switch_row_with_desc(
            "switch-auto-start-breaks",
            "Start breaks automatically",
            "settings-row-Start breaks automatically",
            "A completed focus session starts its break at once",
            settings.auto_start_breaks,
            true,
            cx.listener(|this, on: &bool, _, cx| {
                this.settings.auto_start_breaks = *on;
                this.settings_changed(cx);
            }),
        ));
        rows.push(kit::switch_row_with_desc(
            "switch-auto-start-work",
            "Start focus automatically",
            "settings-row-Start focus automatically",
            "A finished break starts the next focus session at once",
            settings.auto_start_work,
            false,
            cx.listener(|this, on: &bool, _, cx| {
                this.settings.auto_start_work = *on;
                this.settings_changed(cx);
            }),
        ));
        rows.push(kit::switch_row_with_desc(
            "switch-desktop-notifications",
            "Desktop notifications",
            "settings-row-Desktop notifications",
            "A notification when a phase ends",
            settings.desktop_notifications,
            false,
            cx.listener(|this, on: &bool, _, cx| {
                this.settings.desktop_notifications = *on;
                this.settings_changed(cx);
            }),
        ));

        rows.push(kit::subhead("Appearance"));
        rows.push(kit::row_with_desc(
            "settings-row-Text size",
            "Text size",
            "Applied right away; the other Warp apps follow on their next start",
            warpui_kit_core::text_size_control(),
            true,
        ));
        rows.push(kit::row_with_desc(
            "settings-row-Palette",
            "Palette",
            "Shared by every app in the suite",
            warpui_kit_core::palette_control(),
            false,
        ));

        shell::detail_pane()
            .debug_selector(|| "settings-page".into())
            .child(self.page_head("Settings", "Durations, the cycle and the auto-start rules"))
            .child(
                div()
                    .id("settings-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(shell::DETAIL_PADDING))
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .max_w(px(shell::READING_WIDTH))
                            .child(kit::rows(rows)),
                    ),
            )
            .into_any_element()
    }

    /// A duration stepper: the value between two small −/+ buttons.
    fn stepper(
        &self,
        field: StepField,
        value: u32,
        unit: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let prefix = match field {
            StepField::Work => "settings-step-work",
            StepField::ShortBreak => "settings-step-short-break",
            StepField::LongBreak => "settings-step-long-break",
            StepField::LongBreakEvery => "settings-step-every",
        };
        let less = step_button(
            SharedString::from(format!("{prefix}-minus")),
            "−",
            "Less",
            cx.listener(move |this, _, _, cx| this.bump(field, -1, cx)),
        );
        let more = step_button(
            SharedString::from(format!("{prefix}-plus")),
            "+",
            "More",
            cx.listener(move |this, _, _, cx| this.bump(field, 1, cx)),
        );
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .child(less)
            .child(
                div()
                    .min_w(px(theme::scaled(theme::STEP_VALUE_MIN_W)))
                    .text_center()
                    .text_size(px(theme::font_control()))
                    .text_color(theme::text())
                    .child(SharedString::from(format!("{value} {unit}"))),
            )
            .child(more)
            .into_any_element()
    }

    /// One step of a stepper: clamp, write the file and retune the live timer.
    fn bump(&mut self, field: StepField, delta: i32, cx: &mut Context<Self>) {
        match field {
            StepField::Work => {
                let next = step_u32(self.settings.work_minutes, delta, MINUTES_MIN, MINUTES_MAX);
                self.settings.set_work_minutes(next);
            }
            StepField::ShortBreak => {
                let next = step_u32(
                    self.settings.short_break_minutes,
                    delta,
                    MINUTES_MIN,
                    MINUTES_MAX,
                );
                self.settings.set_short_break_minutes(next);
            }
            StepField::LongBreak => {
                let next = step_u32(
                    self.settings.long_break_minutes,
                    delta,
                    MINUTES_MIN,
                    MINUTES_MAX,
                );
                self.settings.set_long_break_minutes(next);
            }
            StepField::LongBreakEvery => {
                let next = step_u32(
                    self.settings.long_break_every,
                    delta,
                    LONG_BREAK_EVERY_MIN,
                    LONG_BREAK_EVERY_MAX,
                );
                self.settings.set_long_break_every(next);
            }
        }
        self.settings_changed(cx);
    }
}

/// One small square −/+ button. It is a hand-rolled control, so it carries its
/// role, its label and a debug selector a probe can click.
fn step_button(
    id: SharedString,
    glyph: &'static str,
    label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let selector = id.to_string();
    div()
        .id(id)
        .debug_selector(move || selector.clone())
        .a11y_button(label)
        .cursor_pointer()
        .size(px(theme::scaled(theme::STEP_BUTTON)))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .border_1()
        .border_color(theme::line_strong())
        .bg(theme::panel())
        .hover(|button| button.bg(theme::raised()))
        .text_size(px(theme::font_meta()))
        .text_color(theme::text())
        .on_click(on_click)
        .child(glyph)
        .into_any_element()
}

fn step_u32(value: u32, delta: i32, min: u32, max: u32) -> u32 {
    let next = i64::from(value) + i64::from(delta);
    next.clamp(i64::from(min), i64::from(max)) as u32
}
