//! Build facts and the Windows resource block come from the shared kit. The
//! app ships no Windows installer assets, so there is no icon to pass.

fn main() {
    warpui_kit_build::emit(warpui_kit_build::AppInfo::new(
        "Warp Pomodoro",
        "Focus timer for the Warp suite",
        "warp-pomodoro.exe",
    ));
}
