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
| Next gate | none open — see "Open work" below for what starts as its own mini-round |
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

## Open work (each its own mini-round, not started)

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
