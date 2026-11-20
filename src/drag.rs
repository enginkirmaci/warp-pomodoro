//! Dragging the mini card by its face.
//!
//! GPUI's `Window::start_window_move` starts the compositor's own move loop on
//! Linux and macOS, but its Windows backend leaves the platform hook at the
//! no-op default: a Windows window drags through a hit-tested caption instead,
//! which this GPUI rev exposes as `WindowControlArea::Drag`. The card cannot be
//! one of those: its face is also a click surface (a double click expands it,
//! and the four controls take their own presses), and a real caption would let
//! the OS turn that double click into its own maximize. So Windows gets the
//! caption move explicitly, after the card has seen a real drag; everywhere
//! else the platform's own move call is the right one.
//!
//! The Windows call goes to `DefWindowProcW`, not through the message queue:
//! GPUI's window proc dispatches non-client presses into the app, and a
//! second, synthetic press would reach the card's own click tracking. The
//! default proc is the part that owns the move loop anyway.

use gpui::Window;

/// Starts a window move from the card's face. The press has already landed in
/// the app (a click or double click may still follow); the OS takes the pointer
/// for the length of the drag.
pub(crate) fn start(window: &Window) {
    #[cfg(windows)]
    {
        windows_move(window);
    }

    #[cfg(not(windows))]
    {
        window.start_window_move();
    }
}

#[cfg(windows)]
fn windows_move(window: &Window) {
    use raw_window_handle::RawWindowHandle;
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
    use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, HTCAPTION, WM_NCLBUTTONDOWN};

    let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(win32.hwnd.get() as *mut core::ffi::c_void);
    // SAFETY: the HWND is GPUI's own window handle and stays valid for the
    // window this handler runs for. The two calls are the ones the OS makes
    // for a caption drag: drop any client capture, then run the default
    // window proc, which owns the modal move loop. It returns when the user
    // releases, exactly as it does for a title bar.
    unsafe {
        let _ = ReleaseCapture();
        DefWindowProcW(
            hwnd,
            WM_NCLBUTTONDOWN,
            WPARAM(HTCAPTION as usize),
            LPARAM(0),
        );
    }
}
