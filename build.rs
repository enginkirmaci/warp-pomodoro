//! Build facts and the Windows resource block come from the shared kit. The
//! app ships no Windows installer, but it does ship the exe icon: Explorer and
//! the taskbar read it from the resource, and the app repo's `packaging`
//! folder is the one home of the art.

fn main() {
    warpui_kit_build::emit(
        warpui_kit_build::AppInfo::new(
            "Warp Pomodoro",
            "Focus timer for the Warp suite",
            "warp-pomodoro.exe",
        )
        .with_icon("packaging/windows/warp-pomodoro.ico"),
    );
}
