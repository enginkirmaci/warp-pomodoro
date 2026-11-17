//! The Pomodoro window: the family's custom titlebar over the rail and one
//! detail pane, or the mini card when the window is collapsed. A timer is a
//! glance rather than a workspace, so the window stays compact and there is no
//! list pane: the rail picks the page and the detail pane draws it. The 250 ms
//! tick is the runtime — it runs the countdown, drains the door, persists the
//! snapshot and fires the phase-end notifications and chime. The ring asks for
//! animation frames only while a phase-end bloom plays: the countdown itself
//! steps once a second with the tick, so a running timer draws a frame a second
//! instead of one per display refresh.

use chrono::Local;
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Entity, FocusHandle, FontWeight, MouseButton, SharedString, Window,
    WindowControlArea, div, px,
};
use gpui_kit::component::input::{InputEvent, InputState};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use warpui_kit_core::about;
use warpui_kit_core::motion;
use warpui_kit_core::shell;

use crate::history::History;
use crate::ipc::{self, CommandKind, DoorCommand};
use crate::settings::{Settings, WindowBox, WindowMode};
use crate::state::{self, Writer};
use crate::theme;
use crate::timer::{Phase, Timer, Transition};
use crate::ui::ring::RingState;
use crate::ui::{icon, window_button, window_button_glyph};

/// The countdown's tick: four times a second keeps a displayed second within a
/// quarter second of the clock without a per-frame wall-clock read.
const TICK: Duration = Duration::from_millis(250);

pub(crate) const SHOW_TOOLTIPS: bool = true;

/// The window's minimum size, asked for when the window is created: the
/// platform fixes it then and cannot be told a smaller one later, so it is the
/// card's floor rather than the full window's — otherwise a window opened in
/// full mode can never shrink into the card.
pub fn min_window_size() -> (f32, f32) {
    (theme::scaled(300.), theme::scaled(92.))
}

/// The mode this launch opens in: the `WARP_POMODORO_MODE=full|mini` capture
/// hook when set, else the mode the last session left in the settings.
pub fn launch_mode() -> WindowMode {
    match std::env::var("WARP_POMODORO_MODE").ok().as_deref() {
        Some("mini") => WindowMode::Mini,
        Some("full") => WindowMode::Full,
        _ => Settings::load().window_mode,
    }
}

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
    /// The installed members the About list renders: the running app from the
    /// build facts, then the siblings the one background scan finds.
    pub(crate) about_members: Vec<about::Member>,
    /// The one background sibling scan has been started; a second About open
    /// keeps the list it found.
    pub(crate) about_scan_started: bool,
    /// The GPL card is up over the About section.
    pub(crate) about_license_open: bool,
    /// The license document, built the first time the card opens: read-only
    /// markdown, so the GPL reads as prose and its text is selectable.
    pub(crate) about_license: Option<Entity<warpui_kit_markdown::LiveEditorState>>,
    /// The window's layout: the full dashboard or the mini card.
    pub(crate) mode: WindowMode,
    /// The keyboard sheet is up and owns the keys.
    shortcut_open: bool,
    shortcut_focus: FocusHandle,
    /// The alert-sound path field, shown on the Settings page.
    pub(crate) sound_path: Entity<InputState>,
    /// The Test button's last failure, cleared by a successful play.
    pub(crate) sound_test: Option<String>,
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
    /// When the last phase end started its ring bloom.
    bloom: Option<Instant>,
    /// Whether the platform accepted the always-on-top intent; `None` until
    /// the first application, `Some(false)` when the compositor has no such
    /// control (the Settings page shows the hint then).
    pub(crate) topmost_taken: Option<bool>,
    /// The startup intent still has to be applied once the window is mapped
    /// (the compositor may not list it yet at construction).
    pending_topmost: bool,
}

impl MainWindow {
    pub fn new(launch: Launch, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = Settings::load();
        let history = History::load();
        let mut timer = Timer::new(&settings);
        if launch.start {
            timer.start();
        }
        // the mode the window was opened in (the launcher read the same hook);
        // a saved mini opens the card, and the card always floats
        let mode = launch_mode();
        let sound_path = cx.new(|cx| {
            let mut state =
                InputState::new(window, cx).placeholder("Empty plays the bundled chime");
            state.set_value(settings.sound_path.clone(), window, cx);
            state
        });
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
            about_members: vec![about::current_member(
                about::AppId::Pomodoro,
                &warpui_kit_core::build_info!(),
            )],
            about_scan_started: false,
            about_license_open: false,
            about_license: None,
            mode,
            // E2E hook: WARP_POMODORO_SHORTCUTS=1 opens the keyboard sheet on
            // launch, the family's capture hook for that card.
            shortcut_open: matches!(std::env::var("WARP_POMODORO_SHORTCUTS").as_deref(), Ok("1")),
            shortcut_focus: cx.focus_handle(),
            sound_path: sound_path.clone(),
            sound_test: None,
            writer: Writer::new(),
            door,
            commands,
            page_focus: cx.focus_handle(),
            last_tick: Instant::now(),
            fractional: Duration::ZERO,
            quitting: false,
            titlebar_should_move: false,
            bloom: None,
            topmost_taken: None,
            pending_topmost: mode == WindowMode::Mini,
        };
        // typing in the sound path applies at once, like every other setting
        cx.subscribe(&sound_path, |this, field, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.settings.sound_path = field.read(cx).value().to_string();
                this.settings.save();
                this.sound_test = None;
                cx.notify();
            }
        })
        .detach();
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
        // the capture hook can open the About section on launch; its sibling
        // scan starts with it, so a capture shows the installed members
        if section == Section::About {
            this.refresh_about_members(cx);
        }
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
            if section == Section::About {
                // the first About open starts the one sibling scan; the list
                // already shows this app, and the scan fills in the rest
                self.refresh_about_members(cx);
            }
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

    /// Switches between the full dashboard and the mini card: save where the
    /// old layout sat, resize, and let the compositor know the new intent (the
    /// card floats; the pin follows the setting).
    pub(crate) fn set_mode(
        &mut self,
        mode: WindowMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.mode == mode {
            return;
        }
        self.track_bounds(window);
        self.mode = mode;
        self.settings.window_mode = mode;
        self.settings.save();
        // Linux: a tiled window ignores its own size, so float it before the
        // resize, and leaving the card drops the float so the layout can own
        // the full window again. The resize runs in both directions: the float
        // only decides who re-tiles, and a compositor without its own IPC would
        // otherwise keep the card's box around the dashboard.
        self.apply_topmost(window);
        let (width, height) = self.mode_size(mode);
        window.resize(gpui::size(px(width), px(height)));
        // Wayland treats the client's size request as a hint; where the
        // compositor has its own IPC (Hyprland), the box is asked for directly
        // so the window and the card cannot disagree
        crate::topmost::resize(window, width, height);
        cx.notify();
    }

    /// `M` and the titlebar's collapse button: the other mode.
    pub(crate) fn toggle_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = match self.mode {
            WindowMode::Full => WindowMode::Mini,
            WindowMode::Mini => WindowMode::Full,
        };
        self.set_mode(next, window, cx);
    }

    /// The pin in the mini card and the Settings switch: the intent is stored
    /// even where the compositor cannot honor it.
    pub(crate) fn toggle_topmost(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings.always_on_top = !self.settings.always_on_top;
        self.settings.save();
        self.apply_topmost(window);
        cx.notify();
    }

    /// The Settings switch wrote the setting; hand the intent to the platform
    /// and repaint the page's hint. Only the card has an intent: in the full
    /// window the layout owns the size and the stacking, so a toggle there
    /// just waits for the next collapse.
    pub(crate) fn reapply_topmost(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.mode == WindowMode::Mini {
            self.apply_topmost(window);
        } else {
            self.topmost_taken = Some(true);
        }
        cx.notify();
    }

    /// Hands the intent to the platform; the card floats in mini mode on
    /// Linux whether or not the pin is on.
    fn apply_topmost(&mut self, window: &Window) {
        let mini = self.mode == WindowMode::Mini;
        let pin = mini && self.settings.always_on_top;
        let taken = crate::topmost::apply(window, mini, pin);
        self.topmost_taken = Some(!pin || taken);
    }

    /// The saved size for a mode, or its design size when the session has
    /// never used it. The card has no size of its own: it is a fixed design, so
    /// a box remembered from a bigger window (or a compositor that tiled it)
    /// cannot stretch the layout over empty space.
    fn mode_size(&self, mode: WindowMode) -> (f32, f32) {
        match mode {
            WindowMode::Mini => theme::card_size(),
            WindowMode::Full => match self.settings.bounds_full {
                Some(boxed) => (boxed.w as f32, boxed.h as f32),
                None => (theme::scaled(theme::FULL_W), theme::scaled(theme::FULL_H)),
            },
        }
    }

    /// Remembers where the current mode sits, so the next launch reopens it
    /// there. The tick calls this every quarter second; a drag or a resize is
    /// therefore never lost, with no bounds-change hook to miss.
    fn track_bounds(&mut self, window: &Window) {
        if self.pending_topmost {
            // the window still has the compositor's first size; the claim
            // below is what gives the card its box
            return;
        }
        let bounds = window.bounds();
        let boxed = WindowBox {
            x: bounds.origin.x.to_i32(),
            y: bounds.origin.y.to_i32(),
            w: bounds.size.width.to_i32(),
            h: bounds.size.height.to_i32(),
        }
        .sane();
        match self.mode {
            WindowMode::Full => self.settings.bounds_full = boxed,
            // Only a box that *is* the card is worth remembering: a sample
            // taken before the mode's resize landed carries the full window's
            // size, and on Windows that sample used to be saved as the card's
            // box, so every later launch opened a card the size of the old
            // window.
            WindowMode::Mini => {
                let (w, h) = self.mode_size(WindowMode::Mini);
                if let Some(b) =
                    boxed.filter(|b| (b.w as f32 - w).abs() <= 2. && (b.h as f32 - h).abs() <= 2.)
                {
                    // a sample that is not the card leaves the remembered card
                    // alone rather than erasing where it sat
                    self.settings.bounds_mini = Some(WindowBox {
                        w: w as i32,
                        h: h as i32,
                        ..b
                    })
                    .and_then(WindowBox::sane_card);
                }
            }
        }
    }

    /// The ring's state: the share of the phase left and the bloom after a
    /// phase end.
    ///
    /// The countdown steps with the clock instead of interpolating between
    /// ticks. A 25-minute phase turns the arc 0.24° a second — no frame budget
    /// can draw that smoother than one step a second, and interpolating it
    /// would repaint the whole window at the display's refresh rate for the
    /// length of every session.
    pub(crate) fn ring_state(&self, now: Instant) -> RingState {
        let total = self.timer.phase_seconds() as f32;
        let remaining = self.timer.remaining_seconds() as f32;
        // reduced motion keeps the ring still: no phase-end bloom
        let bloom = if motion::reduced() {
            None
        } else {
            self.bloom.and_then(|started| {
                let t = now.saturating_duration_since(started).as_secs_f32()
                    / (theme::BLOOM_MS as f32 / 1000.);
                (t < 1.).then_some(t)
            })
        };
        RingState {
            progress: crate::ui::ring::progress(remaining, total),
            bloom,
        }
    }

    /// Whether the window needs a new frame right now: only the phase-end bloom
    /// moves faster than the tick. A running countdown is stepped by the tick
    /// itself, so it earns no extra frames, and neither a paused timer nor a
    /// reduced-motion ring asks for any.
    fn blooming(&self, now: Instant) -> bool {
        !motion::reduced()
            && self.bloom.is_some_and(|started| {
                now.saturating_duration_since(started) < Duration::from_millis(theme::BLOOM_MS)
            })
    }

    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last_tick);
        self.last_tick = now;
        let mut mutations = false;
        // a bloom that has run out draws nothing past its own length, so it is
        // dropped here rather than repainted away
        if self.bloom.is_some_and(|started| {
            now.saturating_duration_since(started) >= Duration::from_millis(theme::BLOOM_MS)
        }) {
            self.bloom = None;
        }
        // what the window is showing right now: only a change in here earns a
        // repaint, so a 250 ms tick that lands inside the same second stays
        // free
        let shown = self.shown_state();

        // the door's commands first: a Start from Warp Station applies to the
        // state the user is looking at, then its answer goes back on the wire
        while let Ok(command) = self.commands.try_recv() {
            let result = self.apply_command(command.kind, window);
            // a `status` poll answers and moves on; only the four verbs and the
            // phase ends below are edges worth a write or a repaint
            mutations |= command.kind.changes_state();
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

        // a drag or resize is remembered continuously; the close saves the file
        self.track_bounds(window);
        // the startup intent waits for the window to be mapped, so the
        // compositor lists it when the card asks to float and stay on top
        if self.pending_topmost && window.is_visible() {
            self.pending_topmost = false;
            self.apply_topmost(window);
            // a saved mini opens the card, and the compositor's first frame
            // may have given it the tiled or default size: claim the box
            if self.mode == WindowMode::Mini {
                let (width, height) = self.mode_size(self.mode);
                window.resize(gpui::size(px(width), px(height)));
                crate::topmost::resize(window, width, height);
            }
        }

        let shown_moved = self.shown_state() != shown;
        // the state file records edges, not the ticking: a mutation is a start,
        // a pause, a reset, a skip, a phase end or a settings change, and the
        // second-by-second countdown writes nothing between them
        if mutations {
            let snapshot = self.timer.snapshot(&self.history.sessions, Local::now());
            self.writer.maybe_write(&snapshot);
        }
        if mutations || shown_moved {
            cx.notify();
        }
    }

    /// The part of the timer the window draws: a repaint is needed exactly when
    /// this changes (the countdown's second, the phase, the cycle's count).
    fn shown_state(&self) -> (Phase, u32, u32) {
        (
            self.timer.phase(),
            self.timer.remaining_seconds(),
            self.timer.completed_in_cycle(),
        )
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
        // one bloom and one chime per tick, even when a suspended window
        // walked several phases at once; reduced motion drops the bloom and
        // keeps the chime
        if !motion::reduced() {
            self.bloom = Some(Instant::now());
        }
        if self.settings.sound_enabled {
            crate::sound::play(&self.settings);
        }
    }

    /// The clean-exit half of the contract: a paused snapshot, the window box
    /// and no endpoint files. Runs on app quit and on the close buttons.
    pub(crate) fn flush_exit(&mut self) {
        if self.quitting {
            return;
        }
        self.quitting = true;
        let snapshot = state::exit_snapshot(&self.timer, &self.history);
        self.writer.write_now(&snapshot);
        self.settings.save();
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
                    // the family link lives with the window controls, so the
                    // brand cluster carries only this app's own mark
                    .child(window_button(
                        "pomodoro-launch-station",
                        "icons/logo-station.svg",
                        theme::raised().into(),
                        "Open Warp Station",
                        SHOW_TOOLTIPS,
                        |_, _, _| launch_sibling("warp-station"),
                    ))
                    .child(window_button(
                        "pomodoro-collapse",
                        "icons/icon-collapse.svg",
                        theme::raised().into(),
                        "Mini screen",
                        SHOW_TOOLTIPS,
                        cx.listener(|this, _, window, cx| {
                            this.set_mode(WindowMode::Mini, window, cx)
                        }),
                    ))
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

/// The full window's keys: the countdown and the mode switch, then the sheet
/// itself.
const TIMER_SHORTCUTS: &[(&str, &str)] = &[
    ("Space", "Start or pause the countdown"),
    ("M", "Switch between the dashboard and the mini screen"),
    ("? / Ctrl+/", "This cheat sheet"),
];

/// The mini card answers two keys of its own on top of the window's.
const MINI_SHORTCUTS: &[(&str, &str)] = &[
    ("P", "Pin the card above other windows"),
    ("Esc", "Expand back to the dashboard"),
];

/// The shared card's content: the two groups the window's handler answers to.
fn shortcut_groups() -> Vec<warpui_kit_core::shortcuts::Group> {
    [("Timer", TIMER_SHORTCUTS), ("Mini screen", MINI_SHORTCUTS)]
        .into_iter()
        .map(|(title, rows)| {
            warpui_kit_core::shortcuts::Group::new(
                title,
                rows.iter()
                    .map(|(keys, label)| ((*keys).into(), (*label).into()))
                    .collect(),
            )
        })
        .collect()
}

impl MainWindow {
    /// The window's plain-key shortcuts: `Space` on any screen, the mini card's
    /// own set, and the two help chords. The key handler sits on the window
    /// root, so an input that owns the keys keeps them (typing a space in
    /// Settings stays typing).
    fn handle_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.keystroke.modifiers;
        let key = event.keystroke.key.as_str();
        // `?` and Ctrl+/ are the sheet. Ctrl+/ stays global (it is the chord
        // the family answers); a bare `?` would be typing in the sound-path
        // field, so it only opens when no text surface owns the keys.
        let in_field = window
            .context_stack()
            .iter()
            .any(|context| context.contains("Input"));
        let help_key = !modifiers.alt
            && !modifiers.platform
            && (event.keystroke.key_char.as_deref() == Some("?")
                || key == "?"
                || (key == "/" && (modifiers.shift || modifiers.control)));
        // the sheet owns the keyboard while it is up: the help chord closes
        // it and nothing behind it fires
        if self.shortcut_open {
            if help_key || key == "escape" {
                self.close_shortcuts(window, cx);
            }
            cx.stop_propagation();
            return;
        }
        // the license is the same kind of layer: Escape puts it away and
        // nothing behind it fires
        if self.about_license_open {
            if key == "escape" {
                self.about_license_open = false;
                cx.notify();
            }
            cx.stop_propagation();
            return;
        }
        if help_key && (modifiers.control || !in_field) {
            self.open_shortcuts(window, cx);
            cx.stop_propagation();
            return;
        }
        if modifiers.control || modifiers.alt || modifiers.platform {
            return;
        }
        match key {
            "space" => {
                self.toggle_timer(cx);
                cx.stop_propagation();
            }
            "m" => {
                self.toggle_mode(window, cx);
                cx.stop_propagation();
            }
            "p" if self.mode == WindowMode::Mini => {
                self.toggle_topmost(window, cx);
                cx.stop_propagation();
            }
            "escape" if self.mode == WindowMode::Mini => {
                self.set_mode(WindowMode::Full, window, cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    /// Opens the keyboard sheet and gives it the keys, so no shortcut fires
    /// behind the card.
    fn open_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shortcut_open {
            return;
        }
        self.shortcut_open = true;
        window.focus(&self.shortcut_focus, cx);
        cx.notify();
    }

    /// Closes the sheet and hands the keyboard back to the page root; a focused
    /// element that left the page would otherwise leave the keys dead until
    /// the next click.
    fn close_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.shortcut_open {
            return;
        }
        self.shortcut_open = false;
        window.focus(&self.page_focus, cx);
        cx.notify();
    }

    fn render_shortcuts(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let groups = shortcut_groups();
        let page = cx.entity();
        warpui_kit_core::shortcuts::dialog(
            "shortcut-sheet",
            "Keyboard shortcuts",
            &groups,
            &self.shortcut_focus,
            move |_, window, cx| {
                page.update(cx, |this, cx| this.close_shortcuts(window, cx));
            },
        )
    }
}

impl Render for MainWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // the ring asks for the next frame only while a bloom plays; the
        // countdown is stepped by the tick, a paused timer leaves the window
        // idle, and a hidden window never animates at all
        if self.blooming(Instant::now()) && window.is_visible() {
            window.request_animation_frame();
        }
        let page_focus = self.page_focus.clone();
        let root = div()
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .bg(theme::background())
            .track_focus(&page_focus)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                this.handle_key(event, window, cx);
            }));
        let mut page = if self.mode == WindowMode::Mini {
            root.child(self.render_mini(window, cx))
        } else {
            root.child(self.render_titlebar(window, cx)).child(
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
        };
        // the two layers sit over the page: the keyboard sheet, and the GPL
        // card when About's Legal row opened it
        if self.shortcut_open {
            page = page.child(self.render_shortcuts(cx));
        }
        if self.about_license_open {
            page = page.child(self.render_about_license(window, cx));
        }
        page.into_any_element()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_minimum_lets_the_card_shrink_to_its_design() {
        let (min_w, min_h) = min_window_size();
        let (card_w, card_h) = theme::card_size();
        assert!(
            min_w <= card_w && min_h <= card_h,
            "the platform's fixed floor ({min_w}x{min_h}) must not hold the card ({card_w}x{card_h}) open"
        );
    }
}
