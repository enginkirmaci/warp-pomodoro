//! The door: auth is refused with a wrong token, `ping` is answered by the
//! door thread itself, and status/start/pause round-trip against the real
//! server with a fake UI tick applying each command to the timer. Each test
//! publishes into its own temp dir.

use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use chrono::Local;
use serde_json::{Value, json};
use warp_pomodoro::ipc::{self, Client, CommandKind, Door, DoorCommand};
use warp_pomodoro::settings::Settings;
use warp_pomodoro::timer::Timer;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("warp-pomodoro-door-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn connect(door: &Door) -> Client {
    Client::connect(door.port(), Duration::from_secs(2)).expect("the door is listening")
}

/// A fake UI tick: owns the timer and answers each command with the snapshot,
/// exactly like the window's tick does.
fn fake_tick(commands: mpsc::Receiver<DoorCommand>) -> JoinHandle<Timer> {
    std::thread::spawn(move || {
        let mut timer = Timer::new(&Settings::default());
        while let Ok(command) = commands.recv() {
            match command.kind {
                CommandKind::Status => {}
                CommandKind::Start => timer.start(),
                CommandKind::Pause => timer.pause(),
                CommandKind::Reset => timer.reset(),
                CommandKind::Skip => timer.skip(),
                CommandKind::Focus => {
                    let _ = command.reply.send(Ok(json!({})));
                    continue;
                }
            }
            let snapshot = timer.snapshot(&[], Local::now());
            let _ = command.reply.send(Ok(
                serde_json::to_value(snapshot).expect("the snapshot serializes")
            ));
        }
        timer
    })
}

#[test]
fn a_wrong_token_is_refused_while_ping_is_answered_locally() {
    let dir = scratch_dir("auth");
    let (sender, _commands) = mpsc::channel();
    let door = Door::start_in(dir.clone(), sender).expect("the door binds");

    // the endpoint is published as the contract's pair
    assert_eq!(
        ipc::read_endpoint_in(&dir).map(|(port, _)| port),
        Some(door.port())
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join("ipc.token"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "the token file is private");
    }

    // wrong token: refused, and the door closes the connection
    let mut refused = connect(&door);
    assert!(refused.authenticate("not-the-token").is_err());
    assert!(refused.request("ping").is_err());

    // right token: ping comes back from the door thread without the UI tick
    let mut client = connect(&door);
    client.authenticate(door.token()).expect("auth accepted");
    let pong = client.request("ping").expect("ping answered");
    assert_eq!(pong["protocol"], 1);
    assert_eq!(pong["app"], "warp-pomodoro");
    assert_eq!(pong["version"], env!("CARGO_PKG_VERSION"));
    assert!(pong["pid"].as_u64().unwrap() > 0);

    // an unknown command is refused by the door thread itself
    let err = client.request("dance").unwrap_err();
    assert!(err.contains("unknown command: dance"), "{err}");

    // dropping the door removes the endpoint it published
    drop(client);
    drop(door);
    assert!(ipc::read_endpoint_in(&dir).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn status_start_and_pause_round_trip_through_the_ui_tick() {
    let dir = scratch_dir("roundtrip");
    let (sender, commands) = mpsc::channel();
    let door = Door::start_in(dir.clone(), sender).expect("the door binds");
    let tick = fake_tick(commands);

    let mut client = connect(&door);
    client.authenticate(door.token()).expect("auth accepted");

    let status: Value = client.request("status").expect("status answered");
    assert_eq!(status["phase"], "work");
    assert_eq!(status["running"], false);
    assert_eq!(status["remaining_seconds"], 1500);
    assert_eq!(status["phase_seconds"], 1500);
    assert_eq!(status["long_break_every"], 4);
    assert_eq!(status["completed_in_cycle"], 0);
    assert_eq!(status["today_work"], 0);
    assert_eq!(status["week_work"], 0);

    let started: Value = client.request("start").expect("start answered");
    assert_eq!(started["running"], true);
    let paused: Value = client.request("pause").expect("pause answered");
    assert_eq!(paused["running"], false);

    // focus is the second launch's verb: it answers an empty payload
    let focused: Value = client.request("focus").expect("focus answered");
    assert_eq!(focused["ok"], true);
    assert!(focused.get("phase").is_none());

    let skipped = client.request("skip").expect("skip answered");
    assert_eq!(skipped["phase"], "short_break");

    drop(client);
    drop(door);
    // the fake tick ends with the last sender gone
    let _ = tick.join();
    assert!(ipc::read_endpoint_in(&dir).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
