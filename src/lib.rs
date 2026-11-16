//! warp-pomodoro: a Pomodoro focus timer whose live state Warp Station reads
//! through the family's token-authenticated door. The library boundary exists
//! so the headless probes (tests/) can build the real window; the binary is a
//! thin bootstrap over it.

pub mod assets;
pub mod history;
pub mod ipc;
pub mod logging;
pub mod notify;
pub mod paths;
pub mod settings;
pub mod sound;
pub mod state;
pub mod theme;
pub mod timer;
pub mod topmost;
pub mod ui;

/// The version the door's `ping` and the snapshot's `version` report.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
