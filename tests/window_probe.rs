//! The real window, headless: the rail opens History and Settings, the primary
//! button flips Start to Pause (and writes the snapshot), a settings step
//! lands in the versioned settings file, and a click on the volume slider
//! writes the stepped value. One scratch HOME for the binary.

use std::path::PathBuf;
use std::sync::OnceLock;

use gpui::{
    Modifiers, Point, ScrollDelta, ScrollEvent, ScrollSource, TestAppContext, TouchPhase,
    VisualTestContext, px, size,
};
use warp_pomodoro::ui::window::{Launch, MainWindow};

/// A scratch HOME for the whole binary: the window publishes its snapshot and
/// door endpoint under it, never in the developer's own `~/.warp`.
fn scratch_home() -> &'static PathBuf {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let home =
            std::env::temp_dir().join(format!("warp-pomodoro-window-probe-{}", std::process::id()));
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
fn the_rail_opens_pages_and_start_flips_the_timer(cx: &mut TestAppContext) {
    let home = scratch_home();
    cx.update(gpui_kit::init);
    cx.update(warp_pomodoro::theme::apply_kit_theme);

    let handle = cx.open_window(size(px(760.), px(720.)), |window, cx| {
        MainWindow::new(Launch::default(), window, cx)
    });
    let cx = VisualTestContext::from_window(handle.into(), cx).into_mut();
    cx.run_until_parked();
    cx.update(|window, cx| drop(window.draw(cx)));

    // the timer page opens paused at 25:00 with the Start verb
    assert!(cx.debug_bounds("timer-page").is_some());
    assert!(cx.debug_bounds("timer-clock").is_some());
    assert!(cx.debug_bounds("timer-start").is_some());
    assert!(
        cx.debug_bounds("timer-pause").is_none(),
        "a paused timer has no Pause verb"
    );

    // Start runs the countdown: the primary flips to Pause
    click(cx, "timer-start");
    assert!(cx.debug_bounds("timer-pause").is_some());
    assert!(
        cx.debug_bounds("timer-start").is_none(),
        "a running timer has no Start verb"
    );
    // the click wrote the live snapshot, paused=false
    let state: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(home.join(".warp/pomodoro/state.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(state["schema_version"], 1);
    assert_eq!(state["phase"], "work");
    assert_eq!(state["running"], true);
    assert_eq!(state["remaining_seconds"], 1500);
    // the window publishes the door while it is alive
    assert!(home.join(".warp/pomodoro/ipc.port").exists());
    assert!(home.join(".warp/pomodoro/ipc.token").exists());

    // Pause parks it again
    click(cx, "timer-pause");
    assert!(cx.debug_bounds("timer-start").is_some());

    // the rail reaches History and Settings
    click(cx, "rail-history");
    assert!(cx.debug_bounds("history-page").is_some());
    assert!(cx.debug_bounds("history-day-6").is_some(), "seven day rows");

    click(cx, "rail-settings");
    assert!(cx.debug_bounds("settings-page").is_some());
    // a fresh home means no file yet: the page shows the settled defaults
    assert!(!home.join(".warp/pomodoro/settings.json").exists());
    assert_eq!(warp_pomodoro::settings::Settings::load().work_minutes, 25);
    // one step writes the versioned file and retunes the live timer
    click(cx, "settings-step-work-plus");
    let after = settings_value(home);
    assert_eq!(after["work_minutes"], 26);
    assert_eq!(after["schema_version"], 1);

    // the volume slider sits in the Sound section, below the fold: scroll the
    // page, then click the track. The settled default is 70 and a click lands
    // on the nearest 5% step and writes the file.
    assert_eq!(after["sound_volume"], 70);
    cx.simulate_event(ScrollEvent {
        position: Point {
            x: px(400.),
            y: px(400.),
        },
        delta: ScrollDelta::Pixels(Point {
            x: px(0.),
            y: px(-420.),
        }),
        modifiers: Modifiers::default(),
        touch_phase: TouchPhase::Moved,
        source: ScrollSource::Pointer,
    });
    cx.update(|window, cx| drop(window.draw(cx)));
    let slider = cx
        .debug_bounds("settings-volume-slider")
        .expect("the volume row scrolled into view");
    let target = Point {
        x: slider.origin.x + slider.size.width * 0.95,
        y: slider.origin.y + slider.size.height / 2.,
    };
    cx.simulate_click(target, Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| drop(window.draw(cx)));
    assert_eq!(
        settings_value(home)["sound_volume"],
        95,
        "a click on the track steps the volume and saves it"
    );
    assert!(cx.debug_bounds("settings-volume-value").is_some());

    // and About opens on Pomodoro, the app you are in
    click(cx, "rail-about");
    assert!(cx.debug_bounds("about-page").is_some());
    assert!(cx.debug_bounds("about-row-pomodoro").is_some());
}

fn settings_value(home: &std::path::Path) -> serde_json::Value {
    serde_json::from_str(
        &std::fs::read_to_string(home.join(".warp/pomodoro/settings.json")).unwrap(),
    )
    .unwrap()
}
