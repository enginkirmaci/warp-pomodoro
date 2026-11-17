//! The door Warp Station (and a second launch) talks to: a token-authenticated
//! NDJSON server on `127.0.0.1`, exactly the family's framing. `ping` is
//! answered by the door thread itself; every state command relays to the UI
//! tick through `std::sync::mpsc` and is answered after the tick applies it, so
//! the wire never reports a state the UI has not taken.
//!
//! ```text
//! client → {"auth": "<token>"}        server → {"ok": true}
//! client → {"id": 7, "cmd": "start"}  server → {"id": 7, "ok": true, …snapshot}
//! ```

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read as _, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use warpui_kit_platform::door::{self, ANSWER_TIMEOUT, MAX_LINE, token_matches};

use crate::paths::AppPaths;

/// The pomodoro door's own protocol number. `door::PROTOCOL` stays warp-agent's
/// (and is a different number on purpose: the two vocabularies must not be
/// confused).
pub const PROTOCOL: u32 = 1;

/// The command vocabulary: the app owns it, Warp Station is the client.
pub mod cmd {
    pub const PING: &str = "ping";
    pub const STATUS: &str = "status";
    pub const START: &str = "start";
    pub const PAUSE: &str = "pause";
    pub const RESET: &str = "reset";
    pub const SKIP: &str = "skip";
    pub const FOCUS: &str = "focus";
}

/// The UI's answer to one door command.
pub type Reply = Sender<Result<Value, String>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandKind {
    Status,
    Start,
    Pause,
    Reset,
    Skip,
    Focus,
}

impl CommandKind {
    /// Whether the command moves the timer, or only reports it. A door client
    /// polls `status` once a second while it looks at the timer, so a poll must
    /// not count as a change: it neither writes the snapshot nor repaints.
    pub fn changes_state(self) -> bool {
        matches!(
            self,
            CommandKind::Start | CommandKind::Pause | CommandKind::Reset | CommandKind::Skip
        )
    }
}

/// One command waiting for the UI tick. `ping` never gets here.
pub struct DoorCommand {
    pub kind: CommandKind,
    pub reply: Reply,
}

/// The bound door. Dropping it stops the accept loop and removes the endpoint
/// files this instance published (a newer instance's files are left alone).
pub struct Door {
    port: u16,
    token: String,
    dir: PathBuf,
    stop: Arc<AtomicBool>,
    accept: Option<std::thread::JoinHandle<()>>,
}

impl Door {
    /// Binds a loopback port and publishes port + token under `~/.warp/pomodoro`.
    pub fn start(commands: Sender<DoorCommand>) -> anyhow::Result<Self> {
        Self::start_in(AppPaths::config_dir(), commands)
    }

    /// Test seam for [`Door::start`]: the door publishes into `dir` instead.
    pub fn start_in(dir: PathBuf, commands: Sender<DoorCommand>) -> anyhow::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        let token = door::random_token();
        std::fs::create_dir_all(&dir)?;
        // token first: a reader that sees a port is entitled to its token. Both
        // files are created 0o600 — restricting after a default-mode write
        // leaves a window where the token is world-readable (`restrict_perms`
        // stays as belt-and-braces).
        let token_path = door::token_file_in(&dir);
        door::write_private(&token_path, &token)?;
        door::restrict_perms(&token_path);
        let port_path = door::port_file_in(&dir);
        door::write_private(&port_path, &format!("{port}\n"))?;
        door::restrict_perms(&port_path);

        let stop = Arc::new(AtomicBool::new(false));
        let accept = std::thread::Builder::new()
            .name("pomodoro-door".into())
            .spawn({
                let stop = stop.clone();
                let token = token.clone();
                move || accept_loop(listener, token, commands, stop)
            })?;
        Ok(Self {
            port,
            token,
            dir,
            stop,
            accept: Some(accept),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn token(&self) -> &str {
        &self.token
    }
}

impl Drop for Door {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // wake the blocking accept with one throwaway connection, then join
        let address = SocketAddr::from(([127, 0, 0, 1], self.port));
        let _ = TcpStream::connect_timeout(&address, Duration::from_millis(200));
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
        unpublish(self);
    }
}

/// The published endpoint, or None until this (or a newer) instance binds.
pub fn read_endpoint() -> Option<(u16, String)> {
    read_endpoint_in(&AppPaths::config_dir())
}

pub fn read_endpoint_in(dir: &Path) -> Option<(u16, String)> {
    door::read_endpoint_in(dir)
}

/// Clears the published endpoint when the files still point at this door.
pub fn unpublish(door: &Door) {
    let ours = |file: &Path, value: &str| {
        std::fs::read_to_string(file).is_ok_and(|current| current.trim() == value)
    };
    if ours(&door::port_file_in(&door.dir), &door.port.to_string()) {
        let _ = std::fs::remove_file(door::port_file_in(&door.dir));
    }
    if ours(&door::token_file_in(&door.dir), &door.token) {
        let _ = std::fs::remove_file(door::token_file_in(&door.dir));
    }
}

/// When a live instance owns the door, tell it to come forward and report true
/// so this launch can exit. Runs before logging and the kit bootstrap; stale
/// files simply fail to connect.
pub fn try_forward() -> bool {
    let Some((port, token)) = read_endpoint() else {
        return false;
    };
    let Ok(mut client) = Client::connect(port, Duration::from_secs(2)) else {
        return false;
    };
    if client.authenticate(&token).is_err() {
        return false;
    }
    // ping: the port could belong to some other loopback listener
    if client.request(cmd::PING).is_err() {
        return false;
    }
    client.request(cmd::FOCUS).is_ok()
}

/// A blocking door client: the handshake and one request line at a time.
pub struct Client {
    reader: BufReader<TcpStream>,
    next_id: u64,
}

impl Client {
    pub fn connect(port: u16, timeout: Duration) -> std::io::Result<Self> {
        let address = SocketAddr::from(([127, 0, 0, 1], port));
        let stream = TcpStream::connect_timeout(&address, timeout)?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        Ok(Self {
            reader: BufReader::new(stream),
            next_id: 1,
        })
    }

    /// Sends the token and waits for the welcome (or the refusal, and the
    /// connection is closed by the door).
    pub fn authenticate(&mut self, token: &str) -> Result<(), String> {
        self.send(&json!({"auth": token}))?;
        let answer = self.receive()?;
        if answer.get("ok").and_then(Value::as_bool) == Some(true) {
            return Ok(());
        }
        Err(answer
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("auth refused")
            .to_string())
    }

    /// One request line; the answer frame comes back whole.
    pub fn request(&mut self, command: &str) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"id": id, "cmd": command}))?;
        let answer = self.receive()?;
        if answer.get("ok").and_then(Value::as_bool) == Some(true) {
            return Ok(answer);
        }
        Err(answer
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("the door refused the command")
            .to_string())
    }

    fn send(&mut self, value: &Value) -> Result<(), String> {
        let stream = self.reader.get_mut();
        stream
            .write_all(format!("{value}\n").as_bytes())
            .and_then(|()| stream.flush())
            .map_err(|err| format!("write: {err}"))
    }

    fn receive(&mut self) -> Result<Value, String> {
        let line = read_line(&mut self.reader)
            .map_err(|err| format!("read: {err}"))?
            .ok_or_else(|| "the door closed the connection".to_string())?;
        serde_json::from_str(&line).map_err(|err| format!("bad answer frame: {err}"))
    }
}

fn accept_loop(
    listener: TcpListener,
    token: String,
    commands: Sender<DoorCommand>,
    stop: Arc<AtomicBool>,
) {
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(stream) = stream else {
            break;
        };
        let token = token.clone();
        let commands = commands.clone();
        let stop = stop.clone();
        let _ = std::thread::Builder::new()
            .name("pomodoro-door-conn".into())
            .spawn(move || {
                if let Err(err) = serve_connection(stream, &token, &commands, &stop) {
                    tracing::debug!("door connection ended: {err}");
                }
            });
    }
}

fn serve_connection(
    stream: TcpStream,
    token: &str,
    commands: &Sender<DoorCommand>,
    stop: &AtomicBool,
) -> std::io::Result<()> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(ANSWER_TIMEOUT))?;
    stream.set_write_timeout(Some(ANSWER_TIMEOUT))?;
    let mut reader = BufReader::new(stream);

    // auth handshake: one line, then the door stays NDJSON
    let Some(line) = read_line(&mut reader)? else {
        return Ok(());
    };
    let authed = serde_json::from_str::<Value>(&line)
        .ok()
        .and_then(|value| {
            value
                .get("auth")
                .and_then(Value::as_str)
                .map(|auth| token_matches(auth, token))
        })
        .unwrap_or(false);
    if !authed {
        return write_line(
            &mut reader,
            &json!({"ok": false, "error": "auth"}).to_string(),
        );
    }
    write_line(&mut reader, &json!({"ok": true}).to_string())?;

    while !stop.load(Ordering::SeqCst) {
        let Some(line) = read_line(&mut reader)? else {
            return Ok(());
        };
        if line.trim().is_empty() {
            continue;
        }
        let request = match serde_json::from_str::<Value>(&line) {
            Ok(value) => value,
            Err(err) => {
                write_line(
                    &mut reader,
                    &door::answer_frame(0, Err(format!("bad frame: {err}"))).to_string(),
                )?;
                continue;
            }
        };
        let Some(id) = request.get("id").and_then(Value::as_u64) else {
            write_line(
                &mut reader,
                &door::answer_frame(0, Err("missing id".into())).to_string(),
            )?;
            continue;
        };
        let name = request
            .get("cmd")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let result = match name.as_str() {
            // ping is answered here, so liveness costs the UI tick nothing
            cmd::PING => Ok(json!({
                "protocol": PROTOCOL,
                "app": "warp-pomodoro",
                "version": crate::VERSION,
                "pid": std::process::id(),
            })),
            cmd::STATUS => relay(CommandKind::Status, commands),
            cmd::START => relay(CommandKind::Start, commands),
            cmd::PAUSE => relay(CommandKind::Pause, commands),
            cmd::RESET => relay(CommandKind::Reset, commands),
            cmd::SKIP => relay(CommandKind::Skip, commands),
            cmd::FOCUS => relay(CommandKind::Focus, commands),
            other => Err(format!("unknown command: {other}")),
        };
        write_line(&mut reader, &door::answer_frame(id, result).to_string())?;
    }
    Ok(())
}

/// Hands one command to the UI tick and waits for its answer.
fn relay(kind: CommandKind, commands: &Sender<DoorCommand>) -> Result<Value, String> {
    let (reply, answer) = mpsc::channel();
    commands
        .send(DoorCommand { kind, reply })
        .map_err(|_| "the app is shutting down".to_string())?;
    match answer.recv_timeout(ANSWER_TIMEOUT) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err("the app did not answer".into()),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err("the app is shutting down".into()),
    }
}

/// Reads one NDJSON line. None drops the connection (EOF, or a line over the
/// cap — a frame that big is a protocol error, not a command).
fn read_line(reader: &mut BufReader<TcpStream>) -> std::io::Result<Option<String>> {
    let mut line = String::new();
    let read = reader
        .by_ref()
        .take(MAX_LINE as u64 + 1)
        .read_line(&mut line)?;
    if read == 0 || line.len() > MAX_LINE {
        return Ok(None);
    }
    Ok(Some(line))
}

fn write_line(reader: &mut BufReader<TcpStream>, line: &str) -> std::io::Result<()> {
    let stream = reader.get_mut();
    stream.write_all(format!("{line}\n").as_bytes())?;
    stream.flush()
}
