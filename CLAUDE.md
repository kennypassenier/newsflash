# hub-clients

Consumer binaries for the kyu hub that run on Kenny's PC as systemd
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
| Current phase | 10 · Retrospective |
| Last completed gate | Phase 9 (2026-08-30): v0.1.0 tagged + Release published on Kenny's go; dogfood done (live toast through the enabled unit) |
| Next gate | Phase 10 retro form → diff on ~/Projects/dev-procedure |
| AFK mode | off since 2026-08-29 |

Deployed and running: unit enabled, token via latch (`KYU_TOKEN`),
policy asserted on the live hub. Only Phase 10 (retro + ecosystem
candidacy, diff on dev-procedure) remains.