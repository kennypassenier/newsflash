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

## Open work (each its own mini-round, not started)

- **M10 — interactive action buttons: UNBLOCKED, ready to build.**
  pipeline-v2 approved the contract (their K12, 2026-08-30): optional
  `actions` field on the envelope, default "Gelezen"/"Snooze" buttons,
  click flows back as an `action_result` envelope to
  `notify.actions`. See `docs/FEATURES.md` M10 amendment.
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
