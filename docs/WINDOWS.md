# newsflash on Windows 11

`newsflash-win` is the Windows twin of the Linux courier: it long-polls
the **same** kyu subscription (`desktop` on `notify.kenny`) and renders
each message as a native Windows toast in Notification Center, with
working action buttons. Goal (Kenny, 2026-09-24): get notifications
whether he logs in to Garuda or to Windows, from one subscription.

Built 2026-09-24 on branch `windows-port`. Ratified by Kenny on 2026-09-26 (mini-round windows-port form: windows-desktop, live-fields and installers all Akkoord) and merged to `main` via PR #2.
The feat-win IDs below follow the same style as FEATURES.md.

## How dual boot and kyu fit together

- **One subscription, one consumer at a time.** Only one OS is booted,
  so only one courier ever polls `desktop`. Whichever OS you log in to
  picks up the backlog on its first poll (`from=beginning`, AR7). The
  10-minute TTL still expires anything stale while neither is logged in
  (S4). No hub change is needed.
- **Same policy on both sides.** Both couriers assert `ttl_ms` on
  connect (AR7). Keep `ttl_minutes`, `topic` and `subscription`
  identical in both configs. If they differ, each boot rewrites the
  other's policy and logs an override warning.
- **Same config format.** `config.toml` has the same keys on both OSes.
  The Windows-only key (`critical_scenario`) is ignored by the Linux
  binary, and the Linux binary ignores unknown keys.
- **Separate dedup stores.** Each OS keeps its own `seen.json`. A
  cross-OS duplicate can only happen if a toast was shown and the
  machine rebooted inside the ack window (the lease expires and the
  message is redelivered on the other OS). That's rare, and it's the
  accepted direction anyway: a duplicate beats a lost toast (AR5).
- **Same reply path.** A click on either OS publishes the same
  `action_result` envelope to `notify.actions` (AR26), with
  `source: "newsflash"`. pipeline-v2 cannot tell, and doesn't need to
  tell, which desktop answered.
- **Never run the Linux build inside WSL** on the Windows side. It
  would be a second consumer on the same subscription, competing with
  `newsflash-win` for every message.

## Install (per user, no admin rights)

**Easiest: the wizard.** Double-click `newsflash-winw.exe` (or
`newsflashw.exe`). While newsflash isn't installed, that opens the setup
window; see `docs/SETUP.md`. The steps below are the command-line
equivalent.

1. Build (see [Building](#building)) or download the `newsflash-windows`
   CI artifact. You need `newsflash-win.exe` and `newsflash-winw.exe`
   side by side.
2. `newsflash-win.exe install` does the following:
   - copies both binaries to `%LOCALAPPDATA%\Programs\newsflash\` as
     `newsflash.exe` (console CLI) and `newsflashw.exe` (windowless);
   - registers the toast identity, the click activator and logon
     autostart (all under HKCU);
   - adds the install folder to your **user PATH** (once; existing
     entries and `%VARS%` are kept as they are), so a *new* terminal
     finds `newsflash`. `uninstall` removes the entry again;
   - writes a starter config to `%APPDATA%\newsflash\config.toml` if
     none exists.
3. Set `hub_url` in that config (live hub `http://10.10.10.9:8080`).
4. `newsflash set-token` asks for the app token without echoing it and
   stores it DPAPI-encrypted, so only this Windows account can decrypt
   it. This takes latch's place on Windows. Resolution order:
   `KYU_TOKEN` env → DPAPI store → `token_file`.
5. `newsflash install` again. This time it starts the courier.
6. `newsflash send-test` publishes a test toast. `newsflash demo` shows
   the local feature tour (no hub involved).

`newsflash status` shows the registration, whether the daemon runs, the
Windows notification setting, the config in force and the log tail.

| | Linux | Windows |
|---|---|---|
| service | systemd user unit | HKCU `Run` key → `newsflashw.exe` at logon; ends at logoff (AR20) |
| stop | `systemctl --user stop` | `newsflash stop` (named event, same settle-then-exit as SIGTERM, M4) |
| journal | journald | `%LOCALAPPDATA%\newsflash\newsflash.log` (UTC, rotates at 1 MB) |
| token | latch → `KYU_TOKEN` | `newsflash set-token` (DPAPI) |
| dedup store | `~/.local/state/newsflash/seen.json` | `%LOCALAPPDATA%\newsflash\seen.json` |
| update | `cargo build` + restart | rebuild, then `newsflash-win.exe install` (stops, replaces, restarts) |
| remove | disable the unit | `newsflash uninstall` (keeps config, data and the binaries; the command prints their paths) |

**Toast rendered but no popup?** Check Do Not Disturb first (the bell
in the taskbar; Settings → System → Notifications). Windows 11 can turn
it on automatically, e.g. "when using an app in full-screen mode", which
is easy to trigger with several monitors. Toasts then go silently into
Notification Center (Win+N), still with working buttons. Seen on the
2026-09-24 drill: even PowerShell's own toasts didn't pop up until it
cleared. The log is the evidence: `popup timed out` is only logged for a
popup that was actually on screen. `rendered` alone means Windows
accepted the toast, not that it popped up.

If the config is unusable at startup, the windowless daemon writes the
remedy to the log **and** shows it as a toast, since there's no journal
to read (AR8).

## Behaviour mapping (AR11 on Windows)

| priority | Linux (Plasma) | Windows 11 |
|---|---|---|
| `info` | 10 s, `dialog-information` | `duration="short"` (~7 s popup), blue *i* logo |
| `warning` | 30 s, `dialog-warning` | `duration="long"` (~25 s popup), amber *!* logo |
| `critical` | persistent, critical urgency | `scenario="reminder"`: stays on screen until answered; red *!* logo |
| unknown | as info, logged | as info, logged |

Windows has no free-form expire time, so short/long/reminder are the
closest honest equivalents. `critical_scenario` in the config can switch
critical toasts to `"urgent"`, which breaks through Do Not Disturb.
Windows asks once whether to allow it, and it replaces "stay on screen".
The other option is `"alarm"`: a reminder plus a looping sound. Popups
that time out stay in Notification Center (for up to 3 days) with
working buttons.

**Hold, don't consume (AR22).** If newsflash's notifications, or all
notifications, are switched off in Settings, the courier holds and
messages wait at the hub under the TTL. Do Not Disturb is not a hold:
toasts go quietly into Notification Center, which is the right place
for them.

## Action buttons (M10 on Windows)

Clicks arrive through COM. The AUMID's `CustomActivator` points at our
`INotificationActivationCallback` class, and the running daemon
registers the class object. Each button's `arguments` carry the action
id, the envelope id, the `ack_id` and the hub id, so the click handler
needs no state. It publishes the same `action_result` as Linux, plus
`data.inputs` when the toast had inputs (feat-win-9).

- Works from the popup **and** from Notification Center.
- Works for toasts shown before a restart: the new daemon re-registers
  the class.
- `ToastNotifier.Show` never blocks, so "shown" (ack) and "answered"
  (publish) are decoupled by construction. That's AR24 with no watcher
  thread, and none of AR25's dead-button problem.
- **Daemon not running:** Microsoft's design is that Windows starts
  `newsflashw.exe -Embedding` through the per-user `LocalServer32`
  registration (implemented: `app::com_launch`). **On Kenny's Windows 11
  25H2 this does not happen.** COM answers `REGDB_E_CLASSNOTREG` for
  *any* per-user out-of-process server, including a throwaway dummy
  CLSID, from every context tested (UAC on, medium integrity). Others
  see the same thing ([PI-Desktop#281](https://github.com/vastsa/PI-Desktop/issues/281)).
  In practice, clicks work whenever the courier runs, which is always
  after logon. A click on an old toast while it's stopped does nothing.
  The registration is kept because it's harmless and works on builds
  that honour it.

Drill without a mouse: `cargo xwin run --example simulate_click --target
x86_64-pc-windows-msvc -- <hub id> <envelope id> <action id> [k=v …]`
makes the exact COM call Windows makes on a click. Use it against a
scratch hub only.

## What Windows toasts can do: built, and not built

### Built (W-series, all exercised by `newsflash demo`)

| ID | Feature | Driven by |
|---|---|---|
| feat-win-1 | Priority → popup duration / reminder / urgent / alarm, per-priority logo | `priority` (+ config `critical_scenario`) |
| feat-win-2 | Up to **5** buttons (same cap on Linux since AR27's revision); more are truncated and logged | `actions`, or the live `data.action_buttons` |
| feat-win-3 | Green / red buttons (Windows 11 button styles) | `actions[].style`: `"success"` / `"critical"` |
| feat-win-4 | Link buttons that open a URL instead of replying | `actions[].url` (http/https only) |
| feat-win-5 | Clicking the toast body opens a URL; path links (`/control-panel/…`) resolve against `link_base_url` | `click_url` (live field) |
| feat-win-6 | Hero image (e.g. a doorbell snapshot) | `image`: plain-http URL, fetched by the courier (≤ 3 MB, png/jpeg/gif), cached 3 days |
| feat-win-7 | Replace-in-place: a newer toast with the same tag replaces the older one | `tag` |
| feat-win-8 | Progress bar; with `tag`, **live updates in place** (no new popup) | `progress` (+ `tag`) |
| feat-win-9 | Quick-reply text box and dropdowns; values ride back in `data.inputs` | `inputs` |
| feat-win-10 | Grouping in Notification Center under the producer, plus "via …" attribution | `source` (existing field) |
| feat-win-11 | Timestamp = when the message was published, not when this OS booted | hub `published_at` |
| feat-win-12 | Removes itself from Notification Center after its lifetime (`ExpirationTime`); same keys as Linux (arch-3) | live `ephemeral` → `ephemeral_minutes`; `expire_<priority>_minutes`; proposed `expires_in_minutes` |
| — | Chime: toast silent, WAV via `PlaySound` (K6) | config `sound_file` (.wav) |

### Live fields vs proposed fields

Read from the hub on 2026-09-24 (`docs/PROPOSALS.md`): pipeline-v2
already sends `ephemeral`, `click_url` and `data.action_buttons`, and
newsflash uses all three on both desktops. Everything in the next
section is **proposed**.

### Envelope extensions (feat-win-3–feat-win-9, feat-win-12): a proposal for pipeline-v2

These are **optional, additive, and not part of the ratified v1
contract**. They are proposed to pipeline-v2 as the envelope v2 /
D3 conversation (same pattern as K12). No producer emits them yet; try
them with `newsflash send-json <file>`. Linux ignores the presentation
fields (feat-win-3, feat-win-6–feat-win-9) and honours the ones with a Plasma equivalent: link
buttons (`url`) and the lifetime (`expires_in_minutes`, feat-win-12).
Each one is read **leniently**: a wrong-typed extension is dropped,
never dead-lettered, so experimenting can't poison a message that would
render fine without it.

```json
{
  "v": 1, "id": "01J…", "source": "home-assistant", "priority": "warning",
  "title": {"nl": "Iemand aan de deur", "en": "Someone at the door"},
  "message": {"nl": "Beweging om 14:02"},
  "click_url": "http://homeassistant.local:8123/lovelace/cameras",
  "image": "http://homeassistant.local:8123/api/camera_proxy/camera.voordeur?token=…",
  "tag": "voordeur",
  "progress": {"value": 0.6, "status": {"nl": "Wassen"}, "title": {"nl": "Wasmachine"}, "label": "3/5"},
  "inputs": [
    {"id": "reply", "type": "text", "placeholder": {"nl": "Bericht…"}},
    {"id": "snooze_minutes", "type": "selection", "default": "60",
     "choices": [{"id": "5", "label": {"nl": "5 min"}}, {"id": "60", "label": {"nl": "1 uur"}},
                 {"id": "1440", "label": {"nl": "24 uur"}}]}
  ],
  "actions": [
    {"id": "send", "label": {"nl": "Verstuur"}, "style": "success"},
    {"id": "snooze", "label": {"nl": "Snooze"}},
    {"id": "camera", "label": {"nl": "Camera"}, "url": "http://homeassistant.local:8123/lovelace/cameras"}
  ]
}
```

A click on "Verstuur" publishes:
`{"v":1,"kind":"action_result","source":"newsflash","data":{"original_envelope_id":"01J…","action_id":"send","inputs":{"reply":"…","snooze_minutes":"60"}}}`.
This already covers D3 (Kenny's Dismiss / Snooze 5m/1h/24h): a
`snooze_minutes` dropdown plus a `snooze` button, answered by HA.

Rules:
- `progress.value` is 0.0–1.0 (clamped) or `"indeterminate"`.
- A text input with a `default` is prefilled; a selection's `default`
  is preselected.
- Tags are capped at 64 UTF-16 units. Longer tags keep a readable
  prefix plus a hash.
- Live update (feat-win-8) refreshes the title, body and progress fields. Other
  parts of a toast (buttons, image) don't change in place.

Safety:
- Only `http://`/`https://` URLs can be opened, so a producer can never
  launch `file:`, `ms-settings:` or any other protocol handler.
- Image fetches are plain http only (no TLS stack, AR2), size-capped,
  and stored under a sanitised name.
- Input *values* go to the hub but never to the log (only their keys
  do).

### Possible, not built (and why)

| Capability | Why not (yet) |
|---|---|
| Context-menu items (right-click on a toast) | Counts against the same 5-button limit; no need seen yet |
| Button icons (16×16, all-or-nothing per toast) | Would need an icon set and a contract field; labels read fine |
| System snooze/dismiss (Windows re-shows the toast itself) | Snooze belongs to HA (pipeline-v2's contract), not the desktop |
| `incomingCall` scenario (ringing, centred layout) | Only fits a real doorbell-call flow; an easy follow-up if wanted |
| Adaptive groups/columns (two-column layouts, styled text) | Needs a richer schema than title/message; wait for envelope v2 |
| Custom sound per toast | Unpackaged apps can only use the built-in `ms-winsoundevent` sounds; K6's WAV chime covers "own sound" |
| Taskbar badge counts, pending-update visual, background activation | Need a packaged app (MSIX) identity |
| Scheduled toasts (show at a time) | Scheduling is HA's job; the courier stays dumb (S9) |
| Remove a toast when it's handled elsewhere (acked on the phone) | Needs a "handled" signal on the hub; `tag` + `ToastNotificationHistory.Remove` would do it |

## Building

Native on Windows: `cargo build --release -p newsflash-win`.

Cross-compiling from Linux (how it was built, from WSL):

```
rustup target add x86_64-pc-windows-msvc
cargo install cargo-xwin --locked
XWIN_ACCEPT_LICENSE=1 cargo xwin build --release -p newsflash-win --target x86_64-pc-windows-msvc
```

`XWIN_ACCEPT_LICENSE=1` accepts Microsoft's CRT/SDK license, which
cargo-xwin downloads. From WSL the `.exe` files run directly on the
Windows desktop through interop.

Gates: `cargo test --all` on Linux covers every toast decision
(`courier_core::wintoast`: priority mapping, escaping, limits, URL
safety, activation codec, live-update binding) and the platform-neutral
shell (`images`, `logfile`, `winconfig`). The WinRT/COM shell is
`cfg(windows)`. The local gate cross-lints it when cargo-xwin is
installed, and CI's `windows` job builds, lints and tests it natively.

## Windows-port decisions (ratified 2026-09-26)

- **arch-win-1 · One loop, two desktops.** `newsflash::run` now drives a
  `Desktop` trait: `render::LinuxDesktop` (unchanged behaviour, all
  Linux tests green) and `toast::WinDesktop`. The settle table, dedup,
  TTL, backoff, archive recovery and policy are shared, so the two OSes
  can't drift apart on the one subscription.
- **arch-win-2 · Toast decisions are pure core** (`wintoast`, AR3). The shell
  only loads XML.
- **arch-win-3 · Two binaries** (`newsflash.exe` console, `newsflashw.exe`
  windowless), like `python`/`pythonw`. No console window ever flashes
  at logon or on a click, and the CLI still prints normally.
- **arch-win-4 · Per-user registration only** (HKCU): no admin, no MSIX.
  Trade-off: no package identity, so no badges, background activation
  or custom audio files (see the table above).
- **arch-win-5 · COM activator for clicks** (not protocol activation), because
  protocol activation cannot carry input values. The not-running case
  is documented under Action buttons.
- **arch-win-6 · Extensions are lenient and proposed, not contract.** Only
  pipeline-v2 can make them official.
- **arch-win-7 · `critical` → `reminder` by default** (stay on screen = the
  Linux promise). `urgent` is an opt-in because it replaces that
  promise with a DND bypass.
