//! The warp-pomodoro theme: the family palette, type and scale from
//! warpui-kit-core, re-exported under the app's `theme::` paths, plus the kit
//! mapping and the compact timer's layout constants.

// every shared font step and color const stays reachable as `theme::X`
pub use warpui_kit_core::fonts::*;
// the fixed semantic colors stay constants (they never follow the palette)
pub use warpui_kit_core::palette::{AccentColors, BEHIND, DANGER, SCRIM, current_ramp, tint};
// the module itself names `palette::…` when it maps the kit theme
use warpui_kit_core::palette;
pub use warpui_kit_core::shell::scaled;

// ---- shell metrics ----------------------------------------------------------

/// The title bar's height, from the shared shell.
pub const TITLEBAR_H: f32 = warpui_kit_core::shell::TITLE_BAR_HEIGHT;

// ---- the timer page ---------------------------------------------------------

/// The countdown's face. The family's type scale stops at the screen title
/// (22), which is a heading rather than a clock; a timer's face is the one
/// display step an app owns, and it rides the scale factor like every shell
/// metric so a text-size change still moves it.
pub const TIMER_DISPLAY: f32 = 64.;
/// The countdown's line height.
pub const TIMER_DISPLAY_LINE: f32 = 74.;
/// A cycle dot's diameter and the gap between dots.
pub const CYCLE_DOT: f32 = 9.;
pub const CYCLE_DOT_GAP: f32 = 7.;
/// The widest the timer page's centred column goes.
pub const TIMER_CONTENT_MAX_W: f32 = 320.;
/// The countdown ring around the clock, and the arc's thickness.
pub const RING_SIZE: f32 = 300.;
pub const RING_THICKNESS: f32 = 10.;
/// How long the phase-end bloom runs, in milliseconds.
pub const BLOOM_MS: u64 = 600;

// ---- the mini card ---------------------------------------------------------

/// The card's design size; it is also the window's size while collapsed.
pub const MINI_W: f32 = 360.;
pub const MINI_H: f32 = 108.;
/// The card's clock face, its ring and that ring's arc.
pub const MINI_CLOCK: f32 = 30.;
pub const MINI_RING: f32 = 52.;
pub const MINI_RING_THICKNESS: f32 = 5.;
/// One mini control's square.
pub const MINI_BUTTON: f32 = 26.;

/// The mini card's one window box: the card is a design rather than a
/// resizable panel, so its size never comes from a saved window box.
pub fn card_size() -> (f32, f32) {
    (scaled(MINI_W), scaled(MINI_H))
}

// ---- the full window -------------------------------------------------------

/// The full window's design size, used until a session saves its own box.
pub const FULL_W: f32 = 760.;
pub const FULL_H: f32 = 720.;

// ---- the settings page ------------------------------------------------------

/// The small square −/+ buttons of a duration stepper.
pub const STEP_BUTTON: f32 = 26.;
/// The value between a stepper's two buttons.
pub const STEP_VALUE_MIN_W: f32 = 88.;

// ---- the history page -------------------------------------------------------

/// The day label's column in the seven-day rows.
pub const HISTORY_DAY_LABEL_W: f32 = 92.;
/// A bar's height, its minimum when the day has a session, and its cap.
pub const HISTORY_BAR_H: f32 = 10.;
pub const HISTORY_BAR_MIN_W: f32 = 3.;
pub const HISTORY_BAR_MAX_W: f32 = 200.;

// ---- the about page ---------------------------------------------------------

/// The About list's width bounds in this compact window: the family's shared
/// screen wants a list beside a detail, and the detail's cards need about 320.
pub const ABOUT_LIST_MIN_W: f32 = 200.;
pub const ABOUT_LIST_MAX_W: f32 = 320.;

// ---- gpui-kit theme mapping ------------------------------------------------

/// Map the family palette onto gpui-kit's shadcn-style theme tokens. Call
/// after [`gpui_kit::init`], before opening windows.
pub fn apply_kit_theme(cx: &mut gpui::App) {
    // the accent rides the live ramp, read at every retheme so a palette
    // switch moves the kit's primary controls with the rest of the app
    let ramp = palette::current_ramp();
    warpui_kit_core::apply_kit_theme(ramp.accent_colors(), &active_tokens(), ramp.base, cx);
}

// ---- the live palette ------------------------------------------------------
// Every surface comes through here, so switching the palette in one settings
// page moves all of them. The names stay the app's own vocabulary — the
// readers themselves are the kit's, re-exported.

pub use warpui_kit_core::palette::{
    accent, accent_hover, accent_ink, accent_soft, border_light, hairline, line_strong, on_accent,
    panel, popup, raised, surface, text, text_dim, text_muted, well, window_chrome,
};

// the app's own spellings, one-liners over the readers above
pub fn background() -> gpui::Rgba {
    surface()
}
pub fn overlay() -> gpui::Rgba {
    popup()
}
pub fn text_faint() -> gpui::Rgba {
    text_muted()
}
