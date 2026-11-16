# warp-pomodoro — agent instructions

Read [`../../AGENTS.md`](../../AGENTS.md) first. This file adds only the
app-specific facts. The user-facing description is in
[`README.md`](README.md).

## Shape

One crate, `warp-pomodoro`, with a real library boundary: `src/lib.rs` exposes
the modules and the binary is a thin bootstrap. The probes build the real
`MainWindow` through that boundary. There is no `crates/` split.

## Where things live

- `src/main.rs` — the launch parser, startup order and the one window.
- `src/timer.rs` — the headless state machine: phases, remaining time, the
  cycle counter, completion and skip rules, the snapshot and the day counts.
  Display-free and I/O-free; its unit tests drive it a second at a time.
- `src/settings.rs` — `~/.warp/pomodoro/settings.json` through the platform
  store: versioned snake_case, strict, clamped on load. Carries the window
  mode, each mode's saved box, the always-on-top intent and the sound switch
  beside the timer knobs.
- `src/sound.rs` — the alert chime: the bundled WAV written beside the
  settings, the system player (`pw-play`/`paplay`/`aplay`, `PlaySoundW`,
  `afplay`), and the custom-file fallback. Failures log once, never block.
- `src/topmost.rs` — the always-on-top intent per platform: `SetWindowPos` on
  Windows, Hyprland's own IPC on Linux (float the card by its address, pin it
  when the setting is on), unsupported elsewhere (the setting is stored and
  the Settings page says so).
- `src/history.rs` — `~/.warp/pomodoro/history.json`: completed sessions,
  newest last, capped at the last 500 completed work sessions.
- `src/state.rs` — the `state.json` writer: at most once a second and on every
  mutation, with one paused snapshot on a clean exit.
- `src/ipc.rs` — the door server (thread + `std::net` + `std::sync::mpsc` to
  the UI tick), the blocking `Client`, and `try_forward`.
- `src/notify.rs` — the phase-end desktop notification (`notify-send` on
  Linux, `tauri-winrt-notification` on Windows).
- `src/ui/window.rs` — the titlebar, the rail, the 250 ms tick, the door
  command pump, the mode switch (full/mini), the window box tracking and the
  frame driver.
- `src/ui/ring.rs` — the countdown ring: the arc maths (unit tested), the head
  dot and the bloom, painted through `PathBuilder` + `paint_path`.
- `src/ui/mini.rs` — the mini card layout and its four controls.
- `src/ui/{timer,history,settings,about}.rs` — the four pages.
- `src/paths.rs`, `logging.rs`, `assets.rs`, `theme.rs` — the recipe wrappers.

## Launch contract

```text
warp-pomodoro [--start] [--version|-V]
```

- Bare launch opens the window, paused at the start of a work phase. A session
  is never persisted as running across launches.
- `--start` starts (or resumes) a work session at window open. Warp Station
  uses it instead of the door when the app is down.
- `--version` / `-V` prints the family build line, first, before forwarding.
- A second launch forwards `focus` and exits before logging or migration.
- Unknown args are ignored so a stale desktop shortcut never blocks the
  window.

## Window modes and motion

- The window opens in `settings.window_mode` (the first run opens the full
  window) and remembers each mode's box; `WARP_POMODORO_MODE=full|mini`
  overrides the mode for one launch (the capture hook, read once).
- Switching modes saves the old box, writes the new mode, asks the platform
  for the new intent (float/pin on Linux, topmost on Windows) and resizes.
  The tick tracks `window.bounds()` every quarter second, so a drag or resize
  is never lost; the close saves the settings file.
- The ring requests animation frames only while the countdown runs or a bloom
  plays, and never while the window is hidden. Paused means zero frames. The
  kit's `motion::reduced()` keeps the ring static (no bloom, no pulse).

## Data and door contract

- Everything lives under `~/.warp/pomodoro/` (`paths::app_dir("pomodoro")`):
  `settings.json`, `history.json`, `state.json`, `ipc.port` + `ipc.token`.
- `state.json` carries exactly the snapshot fields plus `schema_version`:
  `phase` (`work`/`short_break`/`long_break`), `running`, `remaining_seconds`,
  `phase_seconds`, `long_break_every`, `completed_in_cycle`, `today_work`,
  `week_work`, `updated_at` (RFC 3339 local) and `version`.
- The door is the family framing from `warpui_kit_platform::door`:
  `{"auth": token}` → `{"ok": true}`, then one request line
  `{"id", "cmd"}` and one answer frame. `ping` is answered by the door thread
  with `protocol: 1`, `app: "warp-pomodoro"`, the version and the pid — not
  warp-agent's `door::PROTOCOL`.
- The command vocabulary is `ping`, `status`, `start`, `pause`, `reset`,
  `skip`, `focus`. The UI tick applies each state command and answers with the
  fresh snapshot; the endpoint files are removed on a clean exit.
- The token is regenerated on every launch and never logged.

## Tests

```sh
cargo test -p warp-pomodoro
cargo build -p warp-pomodoro
```

- `src/timer.rs` unit tests cover the state machine, the cycle, skip/auto-start
  rules and the day counts.
- `tests/settings_probe.rs` — the versioned store, defaults, clamping, strict
  schema and one-time import, under a scratch HOME.
- `tests/window_probe.rs` — the real window: the rail reaches History/Settings/
  About, Start flips to Pause and writes the snapshot, a step writes the
  settings file.
- `tests/door_probe.rs` — a wrong token is refused, `ping` answers locally, and
  status/start/pause/skip round-trip through a fake UI tick against the real
  server in a temp dir.
- The capture hook is `WARP_POMODORO_PAGE=timer|history|about|settings`, read
  once at window construction.

## App-specific contracts

- Warp Station's Pomodoro section is the door's only client: it polls `status`
  once a second while its section is on screen, falls back to `state.json`
  when the door is down, and launches `--start` for its Start verb. The
  snapshot fields, the command names and the `--start` flag are one contract —
  change both sides together.
- Durations, the long-break cycle, auto-start and notifications live in this
  app's settings, never in Warp Station.
- Text size and palette come from `~/.warp/ui.json` through warpui-kit-core;
  this app stores neither and its accent follows the live ramp.
- The About screen lists the five suite members; Pomodoro is the current app.

## Do not

- Add a second workspace, lockfile, palette or accent picker.
- Build `~/.warp` paths by hand; use `AppPaths` / `warpui_kit_platform::paths`.
- Keep the timer's state in the UI instead of `src/timer.rs`; the state machine
  must stay display-free and I/O-free.
- Add a file watcher for `state.json` or `~/.warp/ui.json`.
- `unwrap` in a render path.

## Public history and secrets

- Never commit a token, PAT, password, private IP, Wi-Fi SSID, home path or
  personal email in file content. Use placeholders in docs and examples.
- A commit's author and committer are public. Use a public identity (the
  GitHub noreply address), never a private address.
- Pass credentials through GitHub Actions secrets only; never echo or log
  them.

  Before publishing, force-pushing or adding a remote, run this over the
  current tree and every ref:

  ```sh
  git grep -n -i -E '192\.168\.[0-9]+\.[0-9]+|10\.[0-9]+\.[0-9]+\.[0-9]+|/home/[a-z]+|ghp_|gho_|github_pat_|ghs_' -- .
  ```
