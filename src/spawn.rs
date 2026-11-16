//! Detached children this app starts and forgets: the chime player and the
//! notification helper outlive the call that starts them, so nothing waits for
//! the `Child`. Reaping it on a thread keeps the app from collecting one
//! zombie per phase end for the whole session.

use std::process::{Child, Command};

/// Reap `child` on a detached thread and forget it.
pub(crate) fn reap(child: Child) {
    std::thread::spawn(move || {
        let mut child = child;
        let _ = child.wait();
    });
}

/// Spawns `command` and reaps it on a detached thread.
pub(crate) fn detached(command: &mut Command) -> std::io::Result<()> {
    reap(command.spawn()?);
    Ok(())
}
