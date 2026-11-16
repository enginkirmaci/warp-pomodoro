//! The Settings page: the durations, the long-break cycle, the two auto-start
//! switches, the notification switch, the reduce-motion switch, and the
//! family's text-size and palette controls. Everything applies at once — the
//! design has no Save — and lands in `~/.warp/pomodoro/settings.json`.

use gpui::Focusable as _;
use gpui::prelude::*;
use gpui::{AnyElement, App, ClickEvent, Context, SharedString, Window, div, px};
use warpui_kit_core::settings as kit;
use warpui_kit_core::shell;

use crate::settings::{
    LONG_BREAK_EVERY_MAX, LONG_BREAK_EVERY_MIN, MINUTES_MAX, MINUTES_MIN, WindowMode,
};
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
        window: &mut Window,
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

        rows.push(kit::subhead("Sound"));
        rows.push(kit::switch_row_with_desc(
            "switch-alert-sound",
            "Alert sound",
            "settings-row-Alert sound",
            "A short chime when a phase ends (independent of the notification)",
            settings.sound_enabled,
            true,
            cx.listener(|this, on: &bool, _, cx| {
                this.settings.sound_enabled = *on;
                this.settings_changed(cx);
            }),
        ));
        rows.push(kit::row_with_desc(
            "settings-row-Sound file",
            "Sound file",
            "A WAV or OGG to play instead; empty plays the bundled chime",
            self.sound_path_controls(window, cx),
            false,
        ));
        if let Some(error) = &self.sound_test {
            rows.push(sound_note(error.clone(), true));
        }

        rows.push(kit::subhead("Window"));
        rows.push(kit::switch_row_with_desc(
            "switch-always-on-top",
            "Always on top",
            "settings-row-Always on top",
            "Keep the mini screen above the other windows where the platform allows it",
            settings.always_on_top,
            true,
            cx.listener(|this, on: &bool, window, cx| {
                this.settings.always_on_top = *on;
                this.settings.save();
                this.reapply_topmost(window, cx);
            }),
        ));
        rows.push(kit::switch_row_with_desc(
            "switch-start-mini",
            "Start in the mini screen",
            "settings-row-Start in the mini screen",
            "Open the small always-on-top card next time the app starts",
            settings.window_mode == WindowMode::Mini,
            false,
            cx.listener(|this, on: &bool, _, cx| {
                this.settings.window_mode = if *on {
                    WindowMode::Mini
                } else {
                    WindowMode::Full
                };
                this.settings_changed(cx);
            }),
        ));
        if self.topmost_taken == Some(false) {
            rows.push(sound_note(
                "This compositor does not support always-on-top; the setting is saved.".to_string(),
                false,
            ));
        }

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
        rows.push(kit::switch_row_with_desc(
            "switch-reduce-motion",
            "Reduce motion",
            "settings-row-Reduce motion",
            "Keeps the ring still and drops the phase-end bloom; the countdown keeps its time",
            settings.reduce_motion,
            false,
            cx.listener(|this, on: &bool, _, cx| {
                this.set_reduce_motion(*on, cx);
            }),
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

    /// The sound file's field and its Test button, side by side.
    fn sound_path_controls(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let field = warpui_kit_core::field::boxed(
            "settings-sound-path",
            &self.sound_path.read(cx).focus_handle(cx),
            window,
            warpui_kit_core::field::title_height(),
        )
        .child(warpui_kit_core::field::mono_input(&self.sound_path));
        let test = warpui_kit_core::controls::button(
            "settings-sound-test",
            "Test",
            warpui_kit_core::controls::BtnKind::Chip,
            theme::accent(),
            theme::font_control(),
            true,
            cx.listener(|this, _, _, cx| {
                this.sound_test = crate::sound::play_now(&this.settings).err();
                cx.notify();
            }),
        );
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w(px(theme::scaled(360.)))
            .child(field)
            .child(
                div()
                    .debug_selector(|| "settings-sound-test-slot".into())
                    .flex_shrink_0()
                    .child(test),
            )
            .into_any_element()
    }

    /// The motion choice applies at once: the kit's global switch flips, every
    /// window repaints without the animation it would have played, and the
    /// settings file already holds it.
    fn set_reduce_motion(&mut self, on: bool, cx: &mut Context<Self>) {
        self.settings.reduce_motion = on;
        warpui_kit_core::motion::set_reduced(
            on || warpui_kit_core::motion::system_prefers_reduced(),
        );
        self.settings_changed(cx);
        cx.refresh_windows();
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

/// A quiet line under a settings section: the Test button's failure or the
/// compositor's always-on-top hint.
fn sound_note(text: String, is_error: bool) -> AnyElement {
    div()
        .debug_selector(move || {
            if is_error {
                "settings-sound-error".to_string()
            } else {
                "settings-topmost-hint".to_string()
            }
        })
        .w_full()
        .pb_2()
        .text_size(px(theme::font_micro()))
        .text_color(if is_error {
            theme::DANGER
        } else {
            theme::text_muted()
        })
        .child(SharedString::from(text))
        .into_any_element()
}
