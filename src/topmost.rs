//! Always-on-top for the mini card, best effort per platform.
//!
//! Wayland has no protocol through which a normal window can ask to stay above
//! others, so the intent is applied where the platform allows it:
//!
//! - **Windows:** `SetWindowPos(HWND_TOPMOST)` on the window's own HWND, a real
//!   topmost window.
//! - **Linux/Hyprland:** the compositor's own IPC. The mini card is asked to
//!   float (so the layout cannot tile it) and then, when the pin is on, to
//!   pin (visible on every workspace, above the tiled ones). The address comes
//!   from `hyprctl -j clients` matched by this process id and window class, so
//!   no other window is ever touched.
//! - **macOS / other compositors:** the intent is stored in the settings and
//!   reported unsupported; the card still works, it just cannot stay above.
//!
//! Nothing here aborts the app: every failure is a `false` return and a log
//! line. The calls are a handful of quick IPC round trips on a user action
//! (switching modes or toggling the pin), never on the tick.

use gpui::Window;

/// The window class the compositor sees (the GPUI `app_id`).
pub const APP_ID: &str = "warp-pomodoro";

/// Asks the platform to make the window exactly this size after a mode
/// switch. Windows resizes through GPUI's `SetWindowPos` already; Wayland's
/// `xdg_toplevel` size request is advisory, so a compositor that exposes its
/// own IPC (Hyprland) is asked directly. Returns whether that happened.
pub fn resize(window: &Window, width: f32, height: f32) -> bool {
    #[cfg(windows)]
    {
        let _ = (window, width, height);
        false
    }

    #[cfg(target_os = "linux")]
    {
        let _ = window;
        hyprland::resize(width, height)
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (window, width, height);
        false
    }
}

/// Applies the intent for the current window: float the mini card and, when
/// `on_top` is set, raise it above the others. Returns whether the platform
/// took the pin (false means "stored, unsupported here").
pub fn apply(window: &Window, mini: bool, on_top: bool) -> bool {
    #[cfg(windows)]
    {
        let _ = mini;
        windows_topmost(window, on_top)
    }

    #[cfg(target_os = "linux")]
    {
        let _ = window;
        hyprland::apply(mini, on_top)
    }

    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = (window, mini, on_top);
        false
    }
}

#[cfg(windows)]
fn windows_topmost(window: &Window, on_top: bool) -> bool {
    use raw_window_handle::RawWindowHandle;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
    };

    // `Window` has an inherent `window_handle()` of GPUI's own; the native one
    // comes from the trait, called explicitly.
    let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window) else {
        return false;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return false;
    };
    let hwnd = HWND(win32.hwnd.get() as *mut core::ffi::c_void);
    let insert_after = if on_top { HWND_TOPMOST } else { HWND_NOTOPMOST };
    // SAFETY: the HWND comes from GPUI's own window handle and stays valid for
    // the window's lifetime; no message is pumped by this call.
    match unsafe {
        SetWindowPos(
            hwnd,
            Some(insert_after),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    } {
        Ok(()) => true,
        Err(err) => {
            tracing::info!("always-on-top could not be set: {err}");
            false
        }
    }
}

#[cfg(target_os = "linux")]
mod hyprland {
    use serde_json::Value;
    use std::process::Command;

    /// The mini card floats; leaving it puts the window back in the layout so
    /// the compositor owns the full window's size again. The pin follows the
    /// setting. All of it is best effort: a session without Hyprland (or
    /// without its Lua dispatchers) answers false.
    pub fn apply(mini: bool, on_top: bool) -> bool {
        let Some(clients) = run(&["-j", "clients"]) else {
            return false;
        };
        let Some((address, floating, pinned)) = own_window(&clients) else {
            return false;
        };

        let mut ok = true;
        if mini != floating {
            ok &= set_float(&address, mini);
        }
        let want_pin = mini && on_top;
        if want_pin != pinned {
            ok &= toggle_pin(&address);
        }
        ok
    }

    /// Asks Hyprland to size this window exactly. The dispatcher takes the
    /// target size; the address keeps it on this process's window only.
    pub fn resize(width: f32, height: f32) -> bool {
        let Some(clients) = run(&["-j", "clients"]) else {
            return false;
        };
        let Some((address, _, _)) = own_window(&clients) else {
            return false;
        };
        let lua = format!(
            "hl.dispatch(hl.dsp.window.resize({{ x = {:.0}, y = {:.0}, window = \"address:{address}\" }}))",
            width.max(1.),
            height.max(1.)
        );
        let ok = run(&["eval", &lua]).is_some();
        tracing::debug!("compositor resize to {width}x{height} on {address}: {ok}");
        ok
    }

    /// This process's window as the compositor sees it: its address and the
    /// two states the intent depends on.
    fn own_window(json: &str) -> Option<(String, bool, bool)> {
        let pid = i64::from(std::process::id());
        let clients: Value = serde_json::from_str(json).ok()?;
        let window = clients.as_array()?.iter().find(|window| {
            window.get("pid").and_then(Value::as_i64) == Some(pid)
                && window.get("class").and_then(Value::as_str) == Some(super::APP_ID)
        })?;
        Some((
            window.get("address")?.as_str()?.to_string(),
            window
                .get("floating")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            window
                .get("pinned")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ))
    }

    /// Floats exactly this window, or returns it to the layout. The Hyprland
    /// builds here speak the Lua dispatchers (`hl.dsp.window.float`); a stock
    /// build rejects that and the classic dispatcher takes over (it acts on
    /// the focused window, which is this one whenever the user toggles from
    /// the card).
    fn set_float(address: &str, float: bool) -> bool {
        let action = if float { "set" } else { "unset" };
        let lua = format!(
            "hl.dispatch(hl.dsp.window.float({{ action = \"{action}\", window = \"address:{address}\" }}))"
        );
        run(&["eval", &lua]).is_some() || {
            tracing::debug!("Lua float dispatcher unavailable; trying the classic one");
            let classic = if float { "setfloating" } else { "settiled" };
            run(&["dispatch", classic]).is_some()
        }
    }

    fn toggle_pin(address: &str) -> bool {
        let lua = format!("hl.dispatch(hl.dsp.window.pin({{ window = \"address:{address}\" }}))");
        run(&["eval", &lua]).is_some() || {
            tracing::debug!("Lua pin dispatcher unavailable; trying the classic one");
            run(&["dispatch", "pin"]).is_some()
        }
    }

    /// One `hyprctl` call; `None` when the binary is missing or the compositor
    /// refused (both mean "not available here", never an error path).
    fn run(args: &[&str]) -> Option<String> {
        let output = Command::new("hyprctl").args(args).output().ok()?;
        if !output.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_window_is_matched_by_pid_and_class() {
            let pid = std::process::id();
            let json = format!(
                r#"[{{"pid": 1, "class": "warp-pomodoro", "address": "0xother", "floating": true, "pinned": false}},
                    {{"pid": {pid}, "class": "warp-pomodoro", "address": "0xours", "floating": false, "pinned": true}},
                    {{"pid": {pid}, "class": "zen", "address": "0xnotours", "floating": true, "pinned": false}}]"#
            );
            assert_eq!(
                own_window(&json),
                Some(("0xours".to_string(), false, true)),
                "the pid alone is not enough; the class gates it too"
            );
        }

        #[test]
        fn another_pid_is_not_this_window() {
            let json = r#"[{"pid": 1, "class": "warp-pomodoro", "address": "0xother", "floating": false, "pinned": false}]"#;
            assert_eq!(own_window(json), None);
        }
    }
}
