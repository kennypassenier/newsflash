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
| Current phase | **COMPLETE** — phases 0-10 all gated and closed (2026-08-28 → 2026-08-30) |
| Last completed gate | Phase 10 retro (2026-08-30): six lessons adopted, diff committed to dev-procedure (2e0f8c0) |
| Next gate | none — new work (e.g. a vault-courier gate, envelope-v2 mini-round when pipeline-v2 freezes its schema) starts as a mini-round or new phase round in a fresh session here |
| AFK mode | off since 2026-08-29 |

Deployed and running: unit enabled, token via latch (`KYU_TOKEN`),
policy asserted on the live hub. Only Phase 10 (retro + ecosystem
candidacy, diff on dev-procedure) remains.