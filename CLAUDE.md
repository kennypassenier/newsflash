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
| Next action | executing the 2026-09-26 form answers: claude-peek cleanup, deferred meaning from pipeline-v2, filing the asks, popup durations, chime candidates |
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

- **Clean up the temporary kyu subscription `claude-peek`** on
  `notify.kenny` (created 2026-09-24 to read the live messages; it
  still holds the unread tail).
- **ask-4/ask-5 need Kenny's call:** does `push` imply the desktop, and what
  does `gate_outcome: deferred` mean for it (quiet delivery?).

- **Ratify + merge the Windows port**, and file the feat-win-3–feat-win-9 envelope
  extensions with pipeline-v2 (they subsume D3's snooze picker).
- **Envelope v2 mini-round** when pipeline-v2 freezes its final
  schema (the pinned v1 vector test is the tripwire).
- Chime file for K6 (sound is off until Kenny picks one).
- **Configurable per-priority durations** (Kenny, 2026-08-30): the
  `info`/`warning`/`critical` → duration mapping (currently hardcoded
  in `courier-core/src/toast.rs::urgency_expire` — 10s/30s/persistent,
  AR11) should become tunable. Note: "critical stays until explicitly
  dismissed" is **already true today** (`expire_ms: 0`, standing since
  0.1.0) — the open part is making the durations for `info`/`warning`
  (and possibly `critical`'s persistence itself) configurable rather
  than fixed constants.
- **Two requirements filed with pipeline-v2** (2026-08-30, from the
  stress-test session): D1, a message-level override for its own
  display duration (`docs/DRILL_LOG.md` 2026-08-30 entry); D3, a
  richer default action set (Kenny proposed Dismiss/Snooze 5m/1h/24h)
  backed by a live measurement that Plasma renders up to 20 buttons
  with no hard cap, though labels blur past ~6-8. Filed as
  `Notification Pipeline V2 Duration Override And Richer Actions
  Requirement.md` in the Obsidian vault — newsflash builds its half
  once pipeline-v2 rules on the contract, same pattern as K12/M10.
