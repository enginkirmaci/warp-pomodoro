//! WarpPomodoro — a focus timer whose live state Warp Station reads.
// no console window behind the packaged Windows exe; debug builds keep it for logs
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use warp_pomodoro::ui::window::{Launch, MainWindow};
use warp_pomodoro::{assets, ipc, logging, theme};

use gpui::prelude::*;
use gpui::{App, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};
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
            warpui_kit_core::bootstrap::init(cx, None, false, |cx: &mut gpui::App| {
                theme::apply_kit_theme(cx);
            });

            let bounds = Bounds::centered(
                None,
                size(px(theme::scaled(720.)), px(theme::scaled(700.))),
                cx,
            );
            let _ = gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(theme::scaled(420.)), px(theme::scaled(520.)))),
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
