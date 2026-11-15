//! The Pomodoro window: the family's custom titlebar over the rail and one
//! detail pane. A timer is a glance rather than a workspace, so the window
//! stays compact and there is no list pane: the rail picks the page and the
//! detail pane draws it. The 250 ms tick is the whole runtime — it runs the
//! countdown, drains the door, persists the snapshot and fires the phase-end
//! notifications.

use chrono::Local;
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, FocusHandle, FontWeight, MouseButton, SharedString, Window,
    WindowControlArea, div, px,
};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use warpui_kit_core::about;
use warpui_kit_core::shell;

use crate::history::History;
use crate::ipc::{self, CommandKind, DoorCommand};
use crate::settings::Settings;
use crate::state::{self, Writer};
use crate::theme;
use crate::timer::{Timer, Transition};
use crate::ui::{icon, window_button, window_button_glyph};

/// The countdown's tick: four times a second keeps a displayed second within a
/// quarter second of the clock without a per-frame wall-clock read.
const TICK: Duration = Duration::from_millis(250);

const SHOW_TOOLTIPS: bool = true;

/// The launch contract: `warp-pomodoro [--start] [--version|-V]`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Launch {
    /// `--start` starts (or resumes) a work session as soon as the window opens.
    pub start: bool,
}

/// The rail's sections, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Section {
    Timer,
    History,
    About,
    Settings,
}

impl Section {
    fn label(self) -> &'static str {
        match self {
            Section::Timer => "Timer",
            Section::History => "History",
            Section::About => "About",
            Section::Settings => "Settings",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::Timer => "icons/icon-timer.svg",
            Section::History => "icons/icon-history.svg",
            Section::About => "icons/icon-about.svg",
            Section::Settings => "icons/icon-cog.svg",
        }
    }

    /// The capture hook's values and the element-id stems.
    fn key(self) -> &'static str {
        match self {
            Section::Timer => "timer",
            Section::History => "history",
            Section::About => "about",
            Section::Settings => "settings",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        match key {
            "timer" => Some(Section::Timer),
            "history" => Some(Section::History),
            "about" => Some(Section::About),
            "settings" => Some(Section::Settings),
            _ => None,
        }
    }
}

pub struct MainWindow {
    pub(crate) settings: Settings,
    pub(crate) history: History,
    pub(crate) timer: Timer,
    pub(crate) section: Section,
    pub(crate) about_selected: about::AppId,
    writer: Writer,
    /// The door this window publishes; dropping it removes the endpoint files.
    door: Option<ipc::Door>,
    commands: Receiver<DoorCommand>,
    page_focus: FocusHandle,
    /// The tick's wall clock: whole seconds are handed to the timer so a busy
    /// or suspended window reports every phase it crossed.
    last_tick: Instant,
    fractional: Duration,
    /// The exit snapshot is writing: a last tick must not overwrite it.
    quitting: bool,
    /// A left press landed on the titlebar background; the first move turns it
    /// into a window drag (Linux/macOS — Windows uses the HTCAPTION path).
    titlebar_should_move: bool,
}

impl MainWindow {
    pub fn new(launch: Launch, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = Settings::load();
        let history = History::load();
        let mut timer = Timer::new(&settings);
        if launch.start {
            timer.start();
        }
        // E2E hook: WARP_POMODORO_PAGE=timer|history|about|settings opens that
        // rail section on launch (a hidden window takes no pointer, so a
        // capture needs the page to open itself). Read once, here.
        let section = std::env::var("WARP_POMODORO_PAGE")
            .ok()
            .and_then(|value| Section::from_key(value.trim()))
            .unwrap_or(Section::Timer);
        let (sender, commands) = std::sync::mpsc::channel();
        let door = match ipc::Door::start(sender) {
            Ok(door) => Some(door),
            Err(err) => {
                tracing::warn!("the door is unavailable, so station cannot drive the timer: {err}");
                None
            }
        };
        let snapshot = timer.snapshot(&history.sessions, Local::now());
        let mut this = Self {
            settings,
            history,
            timer,
            section,
            about_selected: about::AppId::Pomodoro,
            writer: Writer::new(),
            door,
            commands,
            page_focus: cx.focus_handle(),
            last_tick: Instant::now(),
            fractional: Duration::ZERO,
            quitting: false,
            titlebar_should_move: false,
        };
        // the live snapshot lands with the first frame
        this.writer.write_now(&snapshot);
        // a clean quit leaves a paused snapshot and removes the endpoint
        cx.on_app_quit(|this, _cx| {
            this.flush_exit();
            async {}
        })
        .detach();
        // the compositor's close is a save point too: the paused snapshot and
        // the endpoint cleanup land before the window goes
        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |_, cx| {
            let _ = weak.update(cx, |this, _cx| this.flush_exit());
            true
        });
        window.focus(&this.page_focus, cx);
        // the tick parks on a background timer; the entity's update fails once
        // the window is gone, which ends the loop
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                if this
                    .update_in(cx, |this, window, cx| this.tick(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        this
    }

    /// The rail's open section.
    pub(crate) fn set_section(&mut self, section: Section, cx: &mut Context<Self>) {
        if self.section != section {
            self.section = section;
            cx.notify();
        }
    }

    /// One user action: persist the moment and repaint.
    fn commit(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.timer.snapshot(&self.history.sessions, Local::now());
        self.writer.write_now(&snapshot);
        cx.notify();
    }

    pub(crate) fn toggle_timer(&mut self, cx: &mut Context<Self>) {
        self.timer.toggle();
        self.commit(cx);
    }

    pub(crate) fn reset_phase(&mut self, cx: &mut Context<Self>) {
        self.timer.reset();
        self.commit(cx);
    }

    pub(crate) fn skip_phase(&mut self, cx: &mut Context<Self>) {
        self.timer.skip();
        self.commit(cx);
    }

    /// Every settings page change applies at once: the file, the timer's
    /// lengths and the published snapshot move together.
    pub(crate) fn settings_changed(&mut self, cx: &mut Context<Self>) {
        self.settings.save();
        self.timer.apply_settings(&self.settings);
        self.commit(cx);
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last_tick);
        self.last_tick = now;
        let mut mutations = false;

        // the door's commands first: a Start from Warp Station applies to the
        // state the user is looking at, then its answer goes back on the wire
        while let Ok(command) = self.commands.try_recv() {
            let result = self.apply_command(command.kind, window);
            mutations = true;
            let _ = command.reply.send(result);
        }

        if self.timer.running() {
            self.fractional += elapsed;
            let whole = self.fractional.as_secs();
            if whole > 0 {
                self.fractional -= Duration::from_secs(whole);
                let transitions = self.timer.advance(whole);
                if !transitions.is_empty() {
                    self.record_transitions(&transitions);
                    mutations = true;
                }
            }
        } else {
            self.fractional = Duration::ZERO;
        }

        let snapshot = self.timer.snapshot(&self.history.sessions, Local::now());
        self.writer.maybe_write(&snapshot, mutations);
        if mutations || self.timer.running() {
            cx.notify();
        }
    }

    /// Applies one door command to the live state and returns its answer
    /// payload. `focus` only raises the window and answers `{}`.
    fn apply_command(
        &mut self,
        kind: CommandKind,
        window: &mut Window,
    ) -> Result<serde_json::Value, String> {
        match kind {
            CommandKind::Status => {}
            CommandKind::Start => {
                if !self.timer.running() {
                    self.timer.start();
                }
            }
            CommandKind::Pause => self.timer.pause(),
            CommandKind::Reset => self.timer.reset(),
            CommandKind::Skip => self.timer.skip(),
            CommandKind::Focus => {
                window.activate_window();
                return Ok(serde_json::json!({}));
            }
        }
        Ok(state::snapshot_value(
            &self.timer.snapshot(&self.history.sessions, Local::now()),
        ))
    }

    fn record_transitions(&mut self, transitions: &[Transition]) {
        let now = Local::now();
        for transition in transitions {
            self.history
                .record(transition.ended, now, transition.seconds);
            if self.settings.desktop_notifications {
                crate::notify::phase_ended(transition.ended, transition.next);
            }
        }
        self.history.save();
    }

    /// The clean-exit half of the contract: a paused snapshot and no endpoint
    /// files. Runs on app quit and on the titlebar's close button.
    fn flush_exit(&mut self) {
        if self.quitting {
            return;
        }
        self.quitting = true;
        let snapshot = state::exit_snapshot(&self.timer, &self.history);
        self.writer.write_now(&snapshot);
        self.door.take();
    }

    fn render_rail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let mut rail = shell::rail().debug_selector(|| "pomodoro-rail".into());
        for item in [Section::Timer, Section::History] {
            rail = rail.child(self.rail_item(item, cx));
        }
        rail.child(div().flex_1())
            .child(self.rail_item(Section::About, cx))
            .child(self.rail_item(Section::Settings, cx))
            .into_any_element()
    }

    fn rail_item(&self, item: Section, cx: &mut Context<Self>) -> impl IntoElement {
        let id = SharedString::from(format!("rail-{}", item.key()));
        let selector = id.to_string();
        shell::rail_item(id, item.icon(), item.label(), self.section == item)
            .debug_selector(move || selector.clone())
            .on_click(cx.listener(move |this, _, _, cx| this.set_section(item, cx)))
    }

    fn render_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.section {
            Section::Timer => self.render_timer(cx),
            Section::History => self.render_history(cx),
            Section::About => self.render_about(window, cx),
            Section::Settings => self.render_settings(window, cx),
        }
    }

    /// The band every page opens with: the page's name and one line of
    /// context. `About` draws the kit's own head instead.
    pub(crate) fn page_head(
        &self,
        title: &'static str,
        subtitle: impl Into<SharedString>,
    ) -> gpui::Div {
        div()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_0p5()
            .px(px(shell::DETAIL_PADDING))
            .py_4()
            .border_b_1()
            .border_color(theme::hairline())
            .child(
                div()
                    .text_size(px(theme::font_screen_title()))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text())
                    .child(title),
            )
            .child(
                div()
                    .text_size(px(theme::font_meta()))
                    .text_color(theme::text_dim())
                    .child(subtitle.into()),
            )
    }

    fn render_titlebar(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        shell::title_bar()
            .px(px(14.))
            // the whole bar drags; the window buttons occlude it from the
            // Windows hit test and stop the press so they stay clickable
            .window_control_area(WindowControlArea::Drag)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    gpui_kit::base::GlobalState::suppress_text_selection(cx);
                    this.titlebar_should_move = true;
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.titlebar_should_move = false;
                }),
            )
            .on_mouse_move(
                cx.listener(|this, event: &gpui::MouseMoveEvent, window, _| {
                    if this.titlebar_should_move {
                        this.titlebar_should_move = false;
                        // a missed mouse-up (the OS move loop swallowed it)
                        // must not turn a later hover into a drag
                        if event.pressed_button == Some(MouseButton::Left) {
                            window.start_window_move();
                        }
                    }
                }),
            )
            .on_mouse_down_out(cx.listener(|this, _, _, _| {
                this.titlebar_should_move = false;
            }))
            // the left column: the app's mark and name, then the family's
            // links. One flexible column, balanced by the spacer before the
            // window controls, so the bar reads centred at any width.
            .child(
                div()
                    .debug_selector(|| "pomodoro-titlebar-left".into())
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(28.))
                            .rounded(px(6.))
                            .bg(theme::accent_soft())
                            .child(icon(
                                "icons/logo.svg",
                                px(theme::icon_small()),
                                theme::accent_ink(),
                            )),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(theme::font_brand()))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text())
                            .whitespace_nowrap()
                            .child("Warp Pomodoro"),
                    )
                    // the family: the sibling windows this one can open
                    .child(
                        shell::app_links()
                            .child(
                                shell::app_link(
                                    "pomodoro-launch-station",
                                    "icons/logo-station.svg",
                                    "Open Warp Station",
                                )
                                .on_click(|_, _, _| launch_sibling("warp-station"))
                                .when(SHOW_TOOLTIPS, |d| {
                                    d.tooltip(crate::ui::tooltip_text("Open Warp Station"))
                                }),
                            )
                            .child(
                                shell::app_link(
                                    "pomodoro-launch-notes",
                                    "icons/logo-notes.svg",
                                    "Open Warp Notes",
                                )
                                .on_click(|_, _, _| launch_sibling("warp-notes"))
                                .when(SHOW_TOOLTIPS, |d| {
                                    d.tooltip(crate::ui::tooltip_text("Open Warp Notes"))
                                }),
                            ),
                    ),
            )
            .child(div().flex_1())
            .child(
                div()
                    .debug_selector(|| "pomodoro-titlebar-right".into())
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_1()
                    .child(window_button(
                        "pomodoro-minimize",
                        "icons/icon-window-minimize.svg",
                        theme::raised().into(),
                        "Minimize",
                        SHOW_TOOLTIPS,
                        |_, window, _| window.minimize_window(),
                    ))
                    .child(window_button_glyph(
                        "pomodoro-zoom",
                        theme::raised().into(),
                        if window.is_maximized() {
                            "Restore"
                        } else {
                            "Maximize"
                        },
                        SHOW_TOOLTIPS,
                        cx.listener(|_, _, window, cx| {
                            window.zoom_window();
                            // the compositor toggles asynchronously — nudge a
                            // re-render so the maximize/restore glyph keeps up
                            cx.notify();
                        }),
                        shell::zoom_glyph(window.is_maximized(), theme::text_muted()),
                    ))
                    .child(window_button(
                        "pomodoro-close",
                        "icons/icon-window-close.svg",
                        theme::tint(theme::DANGER),
                        "Close",
                        SHOW_TOOLTIPS,
                        cx.listener(|this, _, window, cx| {
                            // closing is a save point: the paused snapshot and
                            // the endpoint cleanup land before the window goes
                            this.flush_exit();
                            let _ = cx;
                            window.remove_window();
                        }),
                    )),
            )
            .into_any_element()
    }
}

impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let page_focus = self.page_focus.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .bg(theme::background())
            .track_focus(&page_focus)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                let modifiers = event.keystroke.modifiers;
                let plain = !modifiers.control && !modifiers.alt && !modifiers.platform;
                if plain && event.keystroke.key == "space" {
                    this.toggle_timer(cx);
                    cx.stop_propagation();
                }
            }))
            .child(self.render_titlebar(window, cx))
            .child(
                div()
                    .id("pomodoro-body")
                    .debug_selector(|| "pomodoro-body".into())
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .flex()
                    .flex_row()
                    .child(self.render_rail(cx))
                    .child(self.render_body(window, cx)),
            )
    }
}

/// Opens a sibling suite app: the window outlives this one, so the child is
/// reaped on a thread instead of being left as a zombie. Windows gets
/// `CREATE_NO_WINDOW` so a debug-build GUI binary leaves no console behind.
pub(crate) fn launch_sibling(binary: &'static str) {
    let Some(path) = warpui_kit_platform::sibling::binary(binary) else {
        tracing::warn!("{binary} is not installed next to this app or on PATH");
        return;
    };
    let mut command = std::process::Command::new(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    match command.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(err) => tracing::warn!("could not launch {binary}: {err}"),
    }
}
