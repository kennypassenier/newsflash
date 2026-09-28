# Features — hub-clients (newsflash)

Phase 2 output, drafted 2026-08-29 during the AFK build. Ratings use the
fixed scale **Essential · Desired · Later · Don't do**. Every rating
below is Claude's recommendation, built as rated; Kenny's ratification
is queued (`docs/AFK_QUEUE.md` R2). IDs are permanent: they appear in
commits, test names, docs and forms from here on. Changes after
ratification go through mini-rounds only (`FORM_PROTOCOL.md` §5).

All features are newsflash's. There is no vault-courier — Kenny closed
that idea for good on 2026-08-30 (SCOPE non-goal S8).

## Round 1 — from Kenny's scope answers and the study

| ID | Feature | Rating | Test expectation |
|---|---|---|---|
| K1 | Long-poll consume — subscription `desktop` on topic `notify.kenny`, `envelope=json`, honoring the hub's wait window; a message published mid-poll arrives at once | Essential | Live test against a real local kyu binary: publish → consumed within the poll window. Mock-hub unit tests for request shape (URL, `as=`, headers). |
| K2 | Toast rendering via `notify-send` — title + body from the envelope in the selected language (M3), app-name `newsflash` | Essential | Unit tests with a PATH-shimmed fake `notify-send` asserting exact argv; one live drill on the real desktop (S6a) recorded in the milestone report. |
| K3 | Ack after success, nack on transient failure — ack only after `notify-send` exits 0; non-zero exit → nack (hub redelivers per policy) | Essential | Live test: failing renderer → message redelivered; succeeding renderer → acked, gone. Unit test for the ack/nack decision table. |
| K4 | Redelivery dedup on envelope id, persisted across restarts (S6b) | Essential | Test delivering the same id twice (including across a process restart) asserting exactly one render; dedup store survives kill -9 (atomic writes). |
| K5 | TTL policy bootstrap — on startup the courier PUTs the `desktop` subscription policy (TTL 10 min, full-field write per hub K7 semantics) so the policy is code, not a hand-configured setting (S4) | Essential | Live test: after startup, GET policy shows `ttl_ms=600000`; bootstrap is idempotent (second startup changes nothing). |
| K6 | Optional soft chime per toast — `sound_file` config played via `paplay`; cyberpunk-soft house style; no file configured = silent | Desired | Unit test with fake `paplay` asserting it fires only when configured and never blocks/fails the toast on player error. |
| K7 | systemd user service — unit file in repo + numbered activation procedure (gmediarender pattern) | Essential | Unit file ships + `systemd-analyze --user verify` clean; activation itself is Kenny's go (AFK queue) — runtime-verified only after that. |
| K8 | Resilience — hub unreachable → bounded backoff retry loop, no crash, visible log lines; recovery is automatic (S6d) | Essential | Live test: kill the scratch hub mid-run, assert the courier survives N cycles and resumes when the hub returns. |
| K9 | Bearer token auth — token from environment (latch-injectable, M6) or config; asserted never to appear in logs or argv | Essential | Unit test for the auth header; plaintext-scan test over captured log output (standing rule 10). |

## Round 2 — Claude's proposals (gaps, hardening, quality-of-life)

| ID | Feature | Rating | Test expectation |
|---|---|---|---|
| M1 | Priority → toast urgency mapping: `info`→normal, `warning`→normal, `critical`→critical (persistent until dismissed) + matching `--expire-time` + `--icon` (2026-08-30 amendment: one freedesktop icon per priority, the actual visual differentiator — see AR11 revision) | Essential | Unit tests per priority asserting the exact `notify-send` urgency/expire/icon argv. |
| M2 | TOML config with startup validation and actionable error messages (standing rule 11) — hub URL, topic, subscription, language, sound, TTL | Essential | Test per broken-config class asserting startup fails naming the field and the remedy; defaults documented. |
| M3 | Language selection — config `language = "nl"` (default) picks `title.nl`/`message.nl`; missing translation falls back to the other language rather than dropping the toast | Essential | Unit tests: nl present → nl; nl missing → en fallback; both missing → poison path (M9). |
| M4 | Graceful shutdown — SIGTERM/SIGINT finishes the in-flight render/ack, then exits 0 (clean `systemctl --user stop`) | Essential | Test sending SIGTERM mid-cycle asserting the in-flight message is settled (acked or nacked), never left to lease expiry. |
| M5 | **Update & distribution (mandatory item):** no self-update, by decision — single-machine tool updated by `git pull` + `cargo build --release` + `systemctl --user restart`, as a numbered runbook procedure | — decision — | The runbook procedure exists and was executed once during the build (evidence in the milestone report). |
| M6 | **Ecosystem integration (mandatory item):** kyu is the counterparty (by definition); token injection via **latch** — the unit file runs `latch run -- newsflash` so the token never touches disk outside latch; plain env file documented as fallback. homelab n/a (runs on the PC) | Essential | Token reaches the process via environment in tests; unit file carries the latch wrapper; fallback documented. |
| M7 | **Backup & restore (mandatory item):** state = config (in git as example + tiny restore step), token (lives in latch, rides latch's own escrow), dedup cache (throwaway). No scheduled backup, by decision — restore-from-zero is a numbered runbook procedure and was drilled once | — decision — | The restore procedure rebuilds a working courier from a clean checkout; drilled during Phase 7 (evidence recorded). |
| M8 | `send-test` subcommand — publishes a valid v1 test envelope to a hub (the interim producer per S11d, and the drill tool) | Desired | Round-trip test: `send-test` → courier consumes → fake renderer sees the toast argv. |
| M9 | Poison-pill on malformed envelopes — unparseable JSON or no renderable content → nack `dead=true`, visible in the hub's dead letters instead of a retry loop | Essential | Live test: publish garbage → dead-lettered after one attempt, courier keeps running; unit tests for the poison decision table. |
| M10 | Interactive action buttons on the toast (default "gelezen"/"snooze", or up to 2 custom ids from the envelope); a click publishes an `action_result` envelope to `notify.actions` | Essential | Unit: `resolve_actions` (default pair, custom pair, truncation-to-2, label fallback, language pick), `interactive_wait_cap_ms` (bounded-vs-uncapped), `build_action_result` (shape). Shim: exact `-A` argv per case, `show_toast_interactive` returns before the shim's simulated interaction completes, `watch_interactive_toast` reports a real click / a timeout / respects and enforces the safety cap / never caps a persistent toast. Live: a real toast with real buttons on the real desktop, a real click captured and republished to `notify.actions` on the scratch hub (`docs/DRILL_LOG.md`). |
| | *Amendment history:* originally scoped as a plain `click_url` → Open button, **Later** (M10 row above superseded). Redefined 2026-08-30 (Kenny) to interactive action buttons; requirement filed with pipeline-v2 (owner of the envelope schema); their K12 mini-round approved the contract the same day (envelope `actions` field, default pair, `notify.actions` reply topic, client stays dumb). Built the same day — see `docs/ARCHITECTURE_DECISIONS.md` AR23–AR27. Rating raised **Later → Essential**: it is no longer a nice-to-have extension, it is the feature Kenny explicitly asked to resume the (briefly blocked) session for. | | |
| M11 | Logging: one startup summary line (config in force, hub URL — never the token) + one line per lifecycle event (consumed, rendered, acked, nacked, expired-policy write, reconnect), journald-friendly | Essential | Log-capture test asserting the summary and per-event lines; plaintext-scan shares K9's assertion. |

## Tally (provisional, AFK)

| Rating | Count | IDs |
|---|---|---|
| Essential | 16 | K1–K5, K7–K9 (8) · M1–M4, M6, M9, M11 (7) · M10 (2026-08-30 amendment) |
| Desired | 2 | K6, M8 |
| Later | 0 | — |
| Don't do | 0 | — |
| Decisions recorded | 2 | M5 (no self-update), M7 (no scheduled backup) |

## Freeze

**Frozen 2026-08-29** — ratified by Kenny (ratification form F2,
Akkoord, same day; M5/M6/M7 decisions ratified as F3/F4/F5, the K6
default-off as F6). Changes from here go through mini-rounds only.

## Round 3 — 2026-09-24 (ratified 2026-09-26, mini-round windows-port)

| ID | Feature | Rating | Test expectation |
|---|---|---|---|
| feat-1 | Windows 11 desktop (`newsflash-win`) on the same `desktop` subscription. Details: feat-win-1–feat-win-12 and arch-win-1–arch-win-7 in `docs/WINDOWS.md` | Essential | `wintoast` unit tests (on any OS); Windows crate tests run natively; live drill with real clicks (2026-09-24) |
| feat-2 | `newsflash install` / `uninstall` on Linux: binary to `~/.local/bin`, removable PATH drop-ins, systemd unit, app-menu entry | Essential | `newsflash/tests/install_tests.rs` (temporary HOME, shimmed systemctl) |
| feat-3 | Setup wizard on both OSes (`newsflash setup`): hub and token checked before anything is written | Desired | `newsflash-setup/src/tests.rs` (headless egui_kittest) + backend tests |
| feat-4 | Live button format `data.action_buttons`; cap 5 on both desktops (AR27 revision, arch-2) | Essential | core tests incl. the pinned live plant-care vector |
| feat-5 | Lifetimes: `ephemeral` → `ephemeral_minutes`, `expire_<priority>_minutes`, proposed `expires_in_minutes` (arch-3) | Essential | core lifetime tests; Linux close-by-id render test; Windows `ExpirationTime` via the builder tests |
| feat-6 | `click_url` opens on a body click; path links via `link_base_url` (arch-4) | Desired | `resolve_link` tests; builder tests |

## Round 4 — 2026-09-26 (Kenny's form answer `popup-durations: A`)

| ID | Feature | Rating | Test expectation |
|---|---|---|---|
| feat-7 | Popup durations for `info` and `warning` are configurable (`popup_info_seconds`, default 10; `popup_warning_seconds`, default 30; 1–3600). `critical` stays on screen until answered and has no key. Windows maps 16 s or more to `long`, less to `short` (arch-6) | Desired | `feat_7_configured_popup_durations_apply_to_info_and_warning_only` (core), `feat_7_configured_popup_durations_pick_short_or_long` (wintoast), config validation in `config_tests.rs` |
| feat-8 | The desktop follows Home Assistant's gate: `gate_outcome: deferred` arrives without popup and without chime (Linux: low urgency, which Plasma sends straight to the history; Windows: `SuppressPopup` + silent audio), `dropped` is acked and never shown, `critical` always pops up (arch-7) | Desired | `feat_8_the_gate_decides_quiet_skip_or_popup_and_critical_always_pops` (core), `feat_8_a_deferred_toast_is_silent_and_suppresses_its_popup` (wintoast), `feat_8_dropped_is_acked_unshown_and_deferred_renders_low` (loop, driven red by disabling the gate) |
| feat-9 | The chime ships in the binary: `newsflash install` (both OSes) writes `newsflash-soft-pulse.wav` to newsflash's data folder and sets `sound_file` when the config has none; a sound file Kenny already chose is kept | Desired | `feat_9_an_unset_sound_file_gets_the_shipped_chime`, `feat_9_a_sound_file_already_chosen_is_left_alone` |
| feat-10 | Windows: while a fullscreen app (a game, a presentation) is in front, a toast goes to Notification Center silently and pops up (with the chime) once the app is gone; critical still pops at once; a toast whose lifetime ended meanwhile is not popped up. Detection: the foreground window covers its monitor, or the shell reports D3D fullscreen / presentation (plain `BUSY` measured wrong, arch-8) (Kenny, 2026-09-28, after a toast crossed Oblivion Remastered) | Essential | `feat_10_fullscreen_holds_the_popup_silently_under_a_tag_but_not_critical` (wintoast); `newsflash status` shows the detected state |
