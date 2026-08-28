# hub-clients

Consumer binaries for the mailbox hub that run on Kenny's PC as systemd
user services. First binary: `desk-courier` (desktop toasts from topic
`notify.kenny`). vault-courier (P2) may join later via its own gate —
not certain, see docs/SCOPE.md non-goals.

This project follows the dev procedure in `~/Projects/dev-procedure/`
(`/project-flow`). Standing rules apply to every change:
`~/Projects/dev-procedure/STANDING_RULES.md`.
Enforcement is **git-native** (`.githooks/` via `core.hooksPath`), so
gates hold from any session or terminal. After a fresh clone, run:
`git config core.hooksPath .githooks`.

## Procedure status

| Field | Value |
|---|---|
| Current phase | 8 done (AFK-provisional) — awaiting ratification, then Phase 9 |
| Last completed gate | Phase 0 (2026-08-28, Kenny's form answers); phases 1–8 built AFK along recommendations 2026-08-29 |
| Next gate | **docs/AFK_QUEUE.md — present FIRST on Kenny's return** (R1–R6 ratification rounds as forms), then Phase 9 tag & release (always Kenny's go) |
| AFK mode | **ON** since 2026-08-28 ("develop al zoveel mogelijk gebaseerd op deze antwoorden") |

State in one line: 62 automated tests + 6 live drill tests green, S6a–d
drilled on the real desktop against a scratch hub, hardening + docs
done; NOT done (needs Kenny): GitHub remote + branch protection, app
token on the live hub + latch project, enabling the systemd unit,
ratification forms, release.

**AFK rule in force:** phases 1–8 run along the recommended choices;
every gate that normally needs Kenny becomes a ratification round in
`docs/AFK_QUEUE.md`. Phase 9 (tag & release) and anything outward-facing
(GitHub repo creation, enabling the service on the PC, touching the live
hub) waits for Kenny — queued, never done silently.

## Project documents

| Doc | Purpose |
|---|---|
| docs/SCOPE.md | goals, non-goals, success criteria, constraints (Phase 0) |
| docs/FEATURES.md | rated feature list with permanent IDs (Phase 2) |
| docs/ARCHITECTURE_DECISIONS.md | frozen AR decisions incl. tech choice (Phases 3-4) |
| docs/REALIZATION_PLAN.md | milestones + status table + gate log (Phase 5+) |
| docs/TEST_PLAN.md | what is proven where + accepted limitations (Phase 7) |
| docs/AFK_QUEUE.md | pending ratification rounds — first thing on Kenny's return |

## Gates (enforced)

Commits are blocked unless `.claude/hooks/gates.sh` passes and the
message carries IDs in brackets (`[K3, AR2]`, `[L1]`, `[meta]`).
Enforced twice: `.githooks/` (git-native, any session) and
`.claude/hooks/check-commit.sh` (sessions opened here). CI re-runs the
gates on every push once a remote exists (queued for Kenny's go).
