# Realization plan — hub-clients (desk-courier)

Phase 5 output, drafted 2026-08-29 during the AFK build. Milestone
approval is queued as ratification round R4 (`docs/AFK_QUEUE.md`); the
enforcement machinery (hooks, gates) is installed before L0 per the
procedure. CI workflow is written; the GitHub remote + branch
protection wait for Kenny's go (outward-facing, standing rule 13).

## Milestones

| ID | Milestone | Feature IDs | Exit criteria | Status |
|---|---|---|---|---|
| L0 | Walking skeleton — workspace (`courier-core` + `desk-courier`), `--version`, hooks live, CI written, gates green on empty crates | [meta] | `cargo test --all` green; commit blocked without IDs; gates block a broken build | done 2026-08-29 |
| L1 | Core logic — envelope v1 parsing (pinned vector), settle/decision table, dedup bookkeeping, toast mapping, backoff schedule; all pure, all unit-tested | AR4, AR5, K4, M1, M3, M9 | Every decision table row has a test; regression vector pinned; core has zero I/O imports (gate) | done 2026-08-29 (43 core tests) |
| L2 | Hub client shell — long-poll receive, ack/nack, policy bootstrap, bearer auth; mock-hub unit tests + live tests against the real kyu binary | K1, K3, K5, K9, AR7 | Live round-trip on a scratch hub: publish → consume → ack → gone; policy bootstrap idempotent; token never logged (scan test) | done 2026-08-29 (mock+live, drill log) |
| L3 | Rendering shell — `notify-send`/`paplay` via PATH shims, urgency mapping wired, `send-test` subcommand | K2, K6, M8 | Fake-renderer argv asserted per priority; sound fires only when configured; `send-test` publishes a valid v1 envelope | done 2026-08-29 (shim tests + real-desktop toast) |
| L4 | The loop — config load/validation, resilience (backoff, no crash), graceful shutdown, dedup persistence, logging | K8, M2, M4, M11 | Live test: hub killed and restarted mid-run, courier survives and resumes; SIGTERM settles in-flight; startup summary logged | done 2026-08-29 (S6d/M4 drills) |
| L5 | Ship shape — systemd unit + activation/update/restore runbook procedures, config example, full S6 drill on the scratch hub | K7, M5, M7 | S6a–S6d each demonstrated live and recorded in the drill log; unit verifies; runbook procedures executed once | done 2026-08-29 — unit verified; R3 restore drilled from a clean checkout; R2 update: build path drilled, the restart step and R1 activation await the enabled unit (Kenny) |

Order rationale: pure core first (cheapest tests), then the two shells
(hub, desktop) each against their real dependency, then the loop that
composes them, then packaging. Every milestone from L2 on includes at
least one live drill on the scratch resource (standing rule 14): the
**scratch hub** is a local run of `~/Projects/kyu/target/release/kyu`
with a temp data dir on a high port — never the live hub on LXC 109.

## Enforcement (installed before L0)

- `.githooks/pre-commit` → `.claude/hooks/gates.sh` (fmt, clippy
  warnings-as-errors, full suite, core I/O boundary, **tree-change
  check** — the gate fails if the working tree changed while it ran,
  standing rule 7) and `.githooks/commit-msg` (feature IDs mandatory).
  Activated via `git config core.hooksPath .githooks`.
- `.claude/hooks/check-commit.sh` — the same gates for sessions opened
  in this directory (PreToolUse hook in `.claude/settings.json`).
- `rust-toolchain.toml` pins 1.97.1; CI asks for exactly that version.
- `.github/workflows/ci.yml` — same gates on every push. **Pending
  Kenny:** repo creation on GitHub + branch protection (require the
  check, require up-to-date, no bypass — standing rule 6).

## Gate log (from Phase 7 on — standing rule 5)

| Gate | Date | Decided | Landed in |
|---|---|---|---|
| Phase 7 · hardening | 2026-08-29 | AFK-provisional: 12 gaps closed same day, 3 accepted, 4 Later — per the auditor's recommendations; security review ran, no findings. Ratification = R5 | docs/TEST_PLAN.md |
| Phase 8 · documentation | 2026-08-29 | AFK-provisional: README + USER_GUIDE + DEBUGGING_GUIDE + OPERATIONS_RUNBOOK + ARCHITECTURE_REFERENCE drafted from code/tests; per-document approval = R6 | README.md + docs/ |
| **AFK ratification (R1–R6)** | 2026-08-29 | Kenny ratified all 18 gate items (F1–F18: Akkoord/Goedkeuren across the board — build-vs-buy, feature freeze incl. M5/M6/M7 and K6-off, architecture freeze incl. critic embeds + S6b hub-id + cold start, plan/build, hardening outcome, all six documents). Follow-ups chosen: F19 GitHub repo+protection = Claude does it; F20 token = Kenny mints and reports; F21 unit enable = Claude, once F20 lands; F22 = prepare Phase 9 | docs/AFK_QUEUE.md (closed) + this row |
| Upstream rename verified | 2026-08-29 | The hub was renamed mailbox → **kyu** at its 2.0.0 (Kenny's rename; same HTTP contract, new names: `KYU_*` env vars, `kyu-*` headers, `kyu_*` metrics, `kyu.events`). A peer session swept this repo (commit f6e6496); this session verified the sweep rather than trusting it: full suite green, all 6 live tests green against the real kyu 2.0.0 binary, and a read-only probe confirmed the live hub on LXC 109 already serves `kyu_*` metrics. Consequence for F20: the latch secret is `KYU_TOKEN`, minted on the kyu hub's `/apps` page | this row + commit f6e6496 |
| F19 executed | 2026-08-29 | Public repo `github.com/kennypassenier/hub-clients` created (house norm: public so protection works), main pushed, branch protection enabled and **read back** (rule 13a): required check `gates`, up-to-date required, enforce for admins, no force pushes, no PR requirement. First CI run green in 37 s. Daily-flow consequence (rule 6): direct pushes to main are refused until the commit's check is green — work on a branch, wait for green, fast-forward | README (activation note) + this row |
| Phase 9 · release | 2026-08-30 | Kenny approved all four items (P1 dogfood evidence, P2 version 0.1.0 + changelog, P3 mechanics: tag on main HEAD, no assets by decision, P4 "tag and publish immediately"). v0.1.0 tagged and the GitHub Release published, then read back: draft=false, published 2026-08-29T22:32Z UTC, `git describe` on main sees the tag | CHANGELOG.md + the Release + this row |
