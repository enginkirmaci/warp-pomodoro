# warp-pomodoro

A Pomodoro focus timer with a live Warp Station rail page — the timer app of
the [warp](https://github.com/enginkirmaci/warp-suite) family. Rust/GPUI
(gpui-kit 0.9), one crate.

## What it does

- Work / short break / long break on one clock: the durations, the long-break
  cycle, whether breaks and focus sessions start on their own, and the desktop
  notification switch all live in `~/.warp/pomodoro/settings.json` (versioned
  snake_case; unknown keys survive a save). A work session counts as complete
  when the clock reaches zero; a skip moves on without counting it.
- The window is the family shell, compact: the custom titlebar, the rail
  (Timer, History, About and Settings at the bottom) and one detail pane. The
  timer page is a **countdown ring** that drains with the phase, the large mono
  countdown, the phase, the cycle dots, today's and the week's completed focus
  sessions, a Start/Pause primary and Reset/Skip secondaries. A phase end
  plays a short chime and a 600 ms ring bloom; the ring only asks for frames
  while it moves, and reduced motion keeps it static.
- **The mini card** is the same timer in a small always-on-top window: the
  ring, the countdown, the phase and cycle, and four controls — start/pause,
  expand, pin and close. The whole card drags the window (a native caption
  move on Windows, where GPUI's own move call is a no-op) and a double click
  expands it. The app opens in the mode the last session left (the first run
  opens the full window); the full window remembers its size and position, and
  the mini card — a fixed design — remembers only where it sat.
  Always-on-top is real on Windows (`SetWindowPos`); on Linux it is best
  effort through the compositor (Hyprland floats and pins the card), and the
  Settings page says so when the compositor cannot.
- History shows today's sessions and the seven-day summary as one bar row per
  day.
- Completed sessions are logged to `~/.warp/pomodoro/history.json` (newest
  last, capped at the last 500 completed work sessions). Breaks are logged
  beside them; nothing is recorded for a skipped work session.
- While the app runs it publishes a token-authenticated door
  (`~/.warp/pomodoro/ipc.port` + `ipc.token`) and keeps
  `~/.warp/pomodoro/state.json` on every **edge** — start, pause, reset, skip,
  a phase end, a settings change. A running session costs zero writes between
  its edges, so `updated_at` is when the state last moved, not a heartbeat:
  the live countdown is what the door answers, and a reader of the file
  derives the ticking clock from `updated_at`. Warp Station's Pomodoro section
  reads the live state through the door, falls back to `state.json` when the
  app is closed, and drives Start/Pause/Reset. A second launch forwards
  `focus` to the running window and exits.

## Launch contract

```
warp-pomodoro [--start] [--version|-V]
```

- Bare launch opens the window, paused at the start of a work phase.
- `--start` starts (or resumes) a work session as soon as the window opens.
  Warp Station's page uses it when the door is down.
- `--version` / `-V` prints the family version line (`Warp Pomodoro 0.2.13
  (abc1234) · dev`) and exits.
- Unknown args are ignored so a stale desktop shortcut never blocks the
  window.
- The window opens in the mode saved in the settings; `WARP_POMODORO_MODE=full`
  or `=mini` overrides it for one launch (the capture hook).

A second launch with a live instance forwards `focus` and exits before any
logging or migration runs. Closing the window writes one final paused
snapshot and removes the endpoint files.

## Data files

Everything lives under `~/.warp/pomodoro/`:

| File | What |
| --- | --- |
| `settings.json` | durations, cycle, auto-start, notifications, sound, window mode and boxes |
| `history.json` | completed sessions, newest last |
| `state.json` | the live snapshot Warp Station reads when the door is down |
| `chime.wav` | the bundled alert chime, written out of the binary on first use |
| `chime-<n>.wav` | the chime scaled to volume `n` (percent), written on first use |
| `ipc.port` + `ipc.token` | the door, present only while the app runs |

All three JSON documents are versioned snake_case with a top-level
`schema_version`.

## Door

`127.0.0.1:<port>` from `ipc.port`, NDJSON, authenticated by the token in
`ipc.token`:

```text
client → {"auth": "<token>"}            server → {"ok": true}
client → {"id": 7, "cmd": "status"}     server → {"id": 7, "ok": true, …snapshot}
```

Commands: `ping` (answered by the door thread), `status`, `start`, `pause`,
`reset`, `skip` and `focus`. `ping` reports `protocol: 1`, `app:
"warp-pomodoro"`, the version and the pid. Unknown commands answer
`{"ok": false, "error": "unknown command: <cmd>"}`.

## Sound

A phase end plays a short bundled chime (`assets/audio/chime.wav`, written to
`~/.warp/pomodoro/chime.wav` the first time). Linux hands it to the first of
`pw-play`, `paplay`, `aplay` that exists; Windows uses `PlaySoundW`; macOS
`afplay`. The Settings page has the switch, a **volume** slider (a percentage
of the recorded amplitude; the player arguments carry no volume, so a PCM WAV
is scaled into `~/.warp/pomodoro/chime-<n>.wav` and that copy is played, while
a format this build cannot rescale plays as recorded), a custom-file field (a
WAV or OGG outside the app folder wins over the bundled chime) and a **Test**
button. A machine without a player gets one log line; the timer never blocks
on the bell.

## Build

```sh
cargo build --release
```

The release workflow publishes a Linux tarball (binary, desktop entry,
hicolor icons) and the Windows portable zip from `packaging/`.

## Sibling deps

`warpui-kit-core`, `warpui-kit-platform` and `warpui-kit-markdown` resolve
from the pinned `kits-v0.2.76` tag of
[warp-suite](https://github.com/enginkirmaci/warp-suite).
