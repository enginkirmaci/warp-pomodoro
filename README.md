# warp-pomodoro

A Pomodoro focus timer with a live Warp Station rail page — the timer app of
the [warp](https://github.com/enginkirmaci/warp-suite) family. Rust/GPUI
(gpui-kit 0.9), one crate.

## What it does

- Work / short break / long break on one clock: the durations, the long-break
  cycle, whether breaks and focus sessions start on their own, and the desktop
  notification switch all live in `~/.warp/pomodoro/settings.json` (versioned
  snake_case, strict). A work session counts as complete when the clock
  reaches zero; a skip moves on without counting it.
- The window is the family shell, compact: the custom titlebar, the rail
  (Timer, History, About and Settings at the bottom) and one detail pane. The
  timer page is a large mono countdown, the phase, the cycle dots, today's and
  the week's completed focus sessions, a Start/Pause primary and Reset/Skip
  secondaries. History shows today's sessions and the seven-day summary as one
  bar row per day.
- Completed sessions are logged to `~/.warp/pomodoro/history.json` (newest
  last, capped at the last 500 completed work sessions). Breaks are logged
  beside them; nothing is recorded for a skipped work session.
- While the app runs it publishes a token-authenticated door
  (`~/.warp/pomodoro/ipc.port` + `ipc.token`) and keeps
  `~/.warp/pomodoro/state.json` fresh at least once a second. Warp Station's
  Pomodoro section reads the live state through the door, falls back to
  `state.json` when the app is closed, and drives Start/Pause/Reset. A second
  launch forwards `focus` to the running window and exits.

## Launch contract

```
warp-pomodoro [--start] [--version|-V]
```

- Bare launch opens the window, paused at the start of a work phase.
- `--start` starts (or resumes) a work session as soon as the window opens.
  Warp Station's page uses it when the door is down.
- `--version` / `-V` prints the family build line (`Warp Pomodoro 0.1.0 (build
  1 · abc1234) · dev`) and exits.
- Unknown args are ignored so a stale desktop shortcut never blocks the
  window.

A second launch with a live instance forwards `focus` and exits before any
logging or migration runs. Closing the window writes one final paused
snapshot and removes the endpoint files.

## Data files

Everything lives under `~/.warp/pomodoro/`:

| File | What |
| --- | --- |
| `settings.json` | durations, long-break cycle, auto-start, notifications |
| `history.json` | completed sessions, newest last |
| `state.json` | the live snapshot Warp Station reads when the door is down |
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

## Build

```sh
cargo build --release
```

The release workflow publishes a Linux tarball (binary, desktop entry,
hicolor icons) and the Windows portable zip from `packaging/`.

## Sibling deps

`warpui-kit-core` and `warpui-kit-platform` resolve from the pinned
`kits-v0.1.40` tag of [warp-suite](https://github.com/enginkirmaci/warp-suite).
