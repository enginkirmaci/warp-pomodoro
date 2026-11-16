//! WarpPomodoro — a focus timer whose live state Warp Station reads.
// no console window behind the packaged Windows exe; debug builds keep it for logs
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use warp_pomodoro::settings::{Settings, WindowBox, WindowMode};
use warp_pomodoro::ui::window::{Launch, MainWindow, launch_mode};
use warp_pomodoro::{assets, ipc, logging, theme};

use gpui::prelude::*;
use gpui::{App, Bounds, Pixels, TitlebarOptions, WindowBounds, WindowOptions, point, px, size};
use gpui_platform::application;

/// The launch contract: `warp-pomodoro [--start] [--version|-V]`. Unknown args
/// are ignored so a stale desktop shortcut never blocks the window.
fn parse_launch() -> Launch {
    Launch {
        start: std::env::args().skip(1).any(|arg| arg == "--start"),
    }
}

fn main() {
    // The support contract, before forwarding and logging: `--version` answers
    // with the same line the About screen copies.
    if std::env::args()
        .skip(1)
        .any(|arg| arg == "--version" || arg == "-V")
    {
        let build = warpui_kit_core::build_info!();
        println!(
            "{}",
            build.version_line(warpui_kit_core::about::app(
                warpui_kit_core::about::AppId::Pomodoro
            ))
        );
        return;
    }
    // A second launch forwards `focus` to the live instance and exits before
    // any migration or logging runs; a stale endpoint simply fails to connect.
    if ipc::try_forward() {
        return;
    }
    let _log_guard = logging::initialize();
    // crashes reach the rolling log
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("panic: {info}");
        default_hook(info);
    }));

    let launch = parse_launch();
    application()
        .with_assets(assets::EmbedAssets::new())
        .run(move |cx: &mut App| {
            // the settings are read before the first frame: the reduce-motion
            // choice (or the desktop's own preference) has to reach the
            // bootstrap, and the window opens at the saved box below
            let settings = Settings::load();
            warpui_kit_core::bootstrap::init(
                cx,
                None,
                settings.reduce_motion || warpui_kit_core::motion::system_prefers_reduced(),
                |cx: &mut gpui::App| {
                    theme::apply_kit_theme(cx);
                },
            );

            // the window opens in the mode the last session left (or the
            // capture hook asks for) and at that mode's saved box
            let mode = launch_mode();
            let saved = match mode {
                WindowMode::Full => settings.bounds_full,
                WindowMode::Mini => settings.bounds_mini,
            };
            let (width, height) = match saved {
                Some(boxed) => (boxed.w as f32, boxed.h as f32),
                None => match mode {
                    WindowMode::Full => {
                        (theme::scaled(theme::FULL_W), theme::scaled(theme::FULL_H))
                    }
                    WindowMode::Mini => {
                        (theme::scaled(theme::MINI_W), theme::scaled(theme::MINI_H))
                    }
                },
            };
            let bounds = restore_bounds(saved, cx)
                .unwrap_or_else(|| Bounds::centered(None, size(px(width), px(height)), cx));
            // A Wayland compositor fixes the window's minimum once, when the
            // surface is created, and enforces it on every resize; the collapse
            // to the mini card must be able to shrink below the full window's
            // box, so Linux asks for the card's minimum from the start. On
            // Windows the hint is per-mode (GPUI's `SetWindowPos` resize is not
            // clamped by it anyway).
            #[cfg(target_os = "linux")]
            let min_size = size(px(theme::scaled(300.)), px(theme::scaled(92.)));
            #[cfg(not(target_os = "linux"))]
            let min_size = match mode {
                WindowMode::Full => size(px(theme::scaled(420.)), px(theme::scaled(480.))),
                WindowMode::Mini => size(px(theme::scaled(300.)), px(theme::scaled(92.))),
            };
            let _ = gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(min_size),
                    // Hyprland and launchers match the desktop entry's
                    // StartupWMClass on this
                    app_id: Some("warp-pomodoro".into()),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Warp Pomodoro".into()),
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                cx,
                move |window, cx| cx.new(|cx| MainWindow::new(launch, window, cx)),
            );
            cx.activate(true);
        });
}

/// The saved box for this launch, when it still meets a display: a window
/// remember on a monitor that is gone opens centered instead of off-screen.
fn restore_bounds(saved: Option<WindowBox>, cx: &App) -> Option<Bounds<Pixels>> {
    let boxed = saved?;
    let bounds = Bounds::new(
        point(px(boxed.x as f32), px(boxed.y as f32)),
        size(px(boxed.w as f32), px(boxed.h as f32)),
    );
    let on_screen = cx
        .displays()
        .iter()
        .any(|display| overlaps(display.visible_bounds(), bounds));
    on_screen.then_some(bounds)
}

fn overlaps(a: Bounds<Pixels>, b: Bounds<Pixels>) -> bool {
    a.origin.x < b.origin.x + b.size.width
        && b.origin.x < a.origin.x + a.size.width
        && a.origin.y < b.origin.y + b.size.height
        && b.origin.y < a.origin.y + a.size.height
}
