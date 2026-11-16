//! The mini card, in its own process so its scratch HOME cannot race the full
//! window's: collapsing the window shows the card (no rail, the clock and the
//! four controls), the mode and the pin land in the versioned settings file,
//! and expanding returns to the dashboard.

use std::path::PathBuf;
use std::sync::OnceLock;

use gpui::{Modifiers, Point, TestAppContext, VisualTestContext, px, size};
use warp_pomodoro::settings::WindowMode;
use warp_pomodoro::ui::window::{Launch, MainWindow};

fn scratch_home() -> &'static PathBuf {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let home =
            std::env::temp_dir().join(format!("warp-pomodoro-mini-probe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".warp/pomodoro")).unwrap();
        // SAFETY: set once, before any window or door thread exists
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("USERPROFILE", &home);
        }
        home
    })
}

fn settings_value(home: &std::path::Path) -> serde_json::Value {
    serde_json::from_str(
        &std::fs::read_to_string(home.join(".warp/pomodoro/settings.json")).unwrap(),
    )
    .unwrap()
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} missing from the frame"));
    let center = Point {
        x: bounds.origin.x + bounds.size.width / 2.,
        y: bounds.origin.y + bounds.size.height / 2.,
    };
    cx.simulate_click(center, Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| drop(window.draw(cx)));
}

#[gpui::test]
fn the_mini_card_collapses_expands_and_remembers_the_mode(cx: &mut TestAppContext) {
    let home = scratch_home();
    cx.update(gpui_kit::init);
    cx.update(warp_pomodoro::theme::apply_kit_theme);

    let handle = cx.open_window(size(px(760.), px(720.)), |window, cx| {
        MainWindow::new(Launch::default(), window, cx)
    });
    let cx = VisualTestContext::from_window(handle.into(), cx).into_mut();
    cx.run_until_parked();
    cx.update(|window, cx| drop(window.draw(cx)));

    // the full dashboard: the rail and the ring around the clock, no card
    assert!(cx.debug_bounds("pomodoro-rail").is_some());
    assert!(cx.debug_bounds("timer-ring").is_some());
    assert!(cx.debug_bounds("mini-card").is_none());

    // the titlebar's collapse button turns the window into the mini card
    click(cx, "pomodoro-collapse");
    assert!(cx.debug_bounds("mini-card").is_some());
    assert!(cx.debug_bounds("pomodoro-rail").is_none());
    assert!(cx.debug_bounds("mini-clock").is_some());
    for control in ["mini-toggle", "mini-expand", "mini-pin", "mini-close"] {
        assert!(cx.debug_bounds(control).is_some(), "{control} renders");
    }
    // the mode is persisted and the launcher reads it back
    assert_eq!(settings_value(home)["window_mode"], "mini");
    assert_eq!(
        warp_pomodoro::ui::window::launch_mode(),
        WindowMode::Mini,
        "a saved mini opens the card"
    );

    // the pin is the settings switch, both ways
    assert_eq!(settings_value(home)["always_on_top"], true);
    click(cx, "mini-pin");
    assert_eq!(settings_value(home)["always_on_top"], false);
    click(cx, "mini-pin");
    assert_eq!(settings_value(home)["always_on_top"], true);

    // the card's own Start runs the clock; its expand returns to the dashboard
    click(cx, "mini-toggle");
    assert!(
        cx.debug_bounds("mini-toggle").is_some(),
        "the toggle stays put while the clock runs"
    );
    click(cx, "mini-expand");
    assert!(cx.debug_bounds("pomodoro-rail").is_some());
    assert!(cx.debug_bounds("timer-pause").is_some(), "still running");
    assert!(cx.debug_bounds("mini-card").is_none());
    assert_eq!(settings_value(home)["window_mode"], "full");
}
