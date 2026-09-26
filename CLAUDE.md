# newsflash

A consumer for the kyu hub that runs on Kenny's PC as a systemd user
service: desktop toasts from topic `notify.kenny`. Was `desk-courier`
in the `hub-clients` workspace repo; renamed 2026-08-30, then split
into this standalone repo the same day (`hub-clients` retired —
deleted, GitHub + local, history kept here).

This project follows the dev procedure in `~/Projects/dev-procedure/`
(`/project-flow`). Standing rules apply to every change:
`~/Projects/dev-procedure/STANDING_RULES.md`.
Enforcement is **git-native** (`.githooks/` via `core.hooksPath`), so
gates hold from any session or terminal. After a fresh clone, run:
`git config core.hooksPath .githooks`.

## Procedure status

| Field | Value |
|---|---|
| Current phase | **COMPLETE** — phases 0-10 all gated and closed (2026-08-28 → 2026-08-30 as hub-clients/desk-courier) |
| Last completed gate | Phase 10 retro (2026-08-30): six lessons adopted, diff committed to dev-procedure (2e0f8c0) |
| Next gate | none open — mini-round windows-port ratified 2026-09-26 (form answers recorded in `docs/WINDOWS.md`, `docs/FEATURES.md`, `docs/ARCHITECTURE_DECISIONS.md`) |
| Next action | none: every 2026-09-26 form answer is built and installed; only the unnamed flake stays open |
| AFK mode | off since 2026-08-29 |

Deployed and running: unit enabled, token via latch (`KYU_TOKEN`,
project still internally named `hub-clients` in latch — cosmetic,
not urgent), policy asserted on the live hub.

**M10 — interactive action buttons: BUILT 2026-08-30.** pipeline-v2
approved the contract (their K12) the same day the session was marked
BLOCKED BY that project; Kenny relayed the approved design and asked
for the build, which shipped the same turn — AR23–AR27 in
`docs/ARCHITECTURE_DECISIONS.md`, live-verified with a real click on
a real critical toast on Kenny's own desktop (`docs/DRILL_LOG.md`).
Stress-tested the same day (300-message flood, button-count ladder,
multi-toast drills): S6e (independently-answerable simultaneous toasts)
confirmed live; found and accepted a hard, external Plasma popup-limit
(AR28, SCOPE S6f) — only `critical` reliably keeps its buttons under
load; AR11 gained a per-priority `--icon` (the real visual
differentiator — an urgency-based attempt was tried, tested, and
reverted the same session, see AR28's sibling amendment).

**Windows port — BUILT 2026-09-24, RATIFIED 2026-09-26 and merged to
`main` (PR #2).** Kenny asked for a Windows 11 equivalent (dual boot
with Garuda, one `desktop` subscription). New crate `newsflash-win`;
`run.rs` now drives a `Desktop` trait (Linux behaviour unchanged, all
tests green); toast XML is pure core (`courier-core/src/wintoast.rs`).
Live-drilled on Kenny's Windows 11 25H2 against a scratch mock hub:
render, ack, policy, real mouse clicks by Kenny (Gelezen; Verstuur with
typed text + dropdown) → `action_result` with inputs, live
in-place progress updates, graceful stop/reinstall. Finding: per-user
COM `LocalServer32` is ignored on that build, so clicks need the daemon
running (it autostarts at logon). Everything — decisions arch-win-1–arch-win-7,
features feat-win-1–feat-win-11 and the envelope extensions proposed to pipeline-v2 —
is in `docs/WINDOWS.md`.

**Later the same day:**
- **feat-2:** Linux `install`/`uninstall`.
- **feat-3:** the setup wizard, both OSes.
- **Live message fields** read from the hub and built on both OSes:
  `data.action_buttons` (cap now 5), `ephemeral` + lifetimes, and
  `click_url` + `link_base_url` (arch-1–arch-5 in
  `docs/ARCHITECTURE_DECISIONS.md`, feat-1–feat-6 in `docs/FEATURES.md`).
- **Cohesion proposals** to pipeline-v2/HA/kyu: `docs/PROPOSALS.md`.

## Open work (each its own mini-round, not started)

- **Flake, unnamed (rule 8a):** `live_link_button_opens_the_page_and_publishes_no_action_result`
  (`newsflash/tests/loop_tests.rs`) failed once on 2026-09-26 in `cargo test --all`
  with "courier did not exit in time" (10 s after SIGTERM) at load
  average 54 on the WSL box; 6 plain reruns and 4 reruns under 48
  busy-loop CPU hogs all passed in ~2.5 s. CPU load alone does not
  reproduce it; the cause is not named yet.
- **Garuda picks everything up through `resume`:** workstation ws-tools
  (450e016) rebuilds newsflash when the version moves (0.2.0 since PR #5)
  and then runs `newsflash install`, which copies it to `~/.local/bin`,
  writes the chime, sets `sound_file` and restarts the unit (feat-9).
  Nothing manual is left on Garuda. Windows runs 0.2.0 since
  2026-09-27 00:27 local (installed from WSL with `newsflash-win.exe
  install`; send-test rendered, log line `newsflash 0.2.0 starting`).
- **The seven asks are filed** in the vault as `Notification Pipeline V2
  Desktop Cohesion Proposals.md` (2026-09-26); pipeline-v2 decides them
  (ask-4 already decided by Kenny: push implies desktop).

Done 2026-09-26: Windows port ratified and merged (PR #2); popup
durations configurable (feat-7, PR #3); `claude-peek` found already
archived (explicit idle policy 60 s / 120 s, 11,526 deliveries
`lapsed`, nothing held) and kyu 3.x has no delete-subscription call, so
nothing was changed on CT 109. Quiet delivery for deferred/dropped
(feat-8) and the soft-pulse chime chosen the same evening.

- **Envelope v2 mini-round** when pipeline-v2 freezes its final
  schema (the pinned v1 vector test is the tripwire).
- **Two requirements filed with pipeline-v2** (2026-08-30, from the
  stress-test session): D1, a message-level override for its own
  display duration (`docs/DRILL_LOG.md` 2026-08-30 entry); D3, a
  richer default action set (Kenny proposed Dismiss/Snooze 5m/1h/24h)
  backed by a live measurement that Plasma renders up to 20 buttons
  with no hard cap, though labels blur past ~6-8. Filed as
  `Notification Pipeline V2 Duration Override And Richer Actions
  Requirement.md` in the Obsidian vault — newsflash builds its half
  once pipeline-v2 rules on the contract, same pattern as K12/M10.
