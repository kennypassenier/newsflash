# Test plan — newsflash

Phase 7 output, 2026-08-29 (AFK — gate decisions are Claude's
recommendations, built as decided; ratification queued as R5 in
`docs/AFK_QUEUE.md`). Describes every suite, what each mock cannot
express and what covers that instead, the hardening round's outcome,
and every accepted limitation.

## The suites

| Suite | Runs | What it proves |
|---|---|---|
| `courier-core` unit tests (41) | `cargo test`, CI | The pure decision logic: envelope v1 tolerant reader + poison classes, settle table (every row), dedup bookkeeping, toast mapping (language fallback, priority table, truncation, markup escape), backoff schedule, hub-response parsing, error classification |
| Vector tests (2) | `cargo test`, CI | The envelope v1 DRAFT pinned — a pipeline-v2 schema change trips these, which is the mini-round tripwire (SCOPE S7) |
| `config_tests` (1 sweep) | `cargo test`, CI | Every broken-config class fails naming field + remedy (M2); token source order incl. inline-token refusal and 0600 check (AR10) |
| `state_tests` (4) | `cargo test`, CI | Dedup store on a real filesystem: round-trip, corrupt-file fail-open, dir creation, atomic replace (K4/AR6) |
| `mock_hub_tests` (7) | `cargo test`, CI | Request shapes on the wire (URLs, auth header, one-field policy PUT), against the **captured** real `envelope=json` response — the mock serves drill evidence, not fiction (AR18); plus the K9 plaintext scan spawning the real binary |
| `render_tests` (1 sweep) | `cargo test`, CI | Subprocess argv per priority, `--` separator, escape, failure and timeout branches, daemon probe, detached chime (AR12/AR19/AR22) |
| `loop_tests` (6) | `cargo test`, CI | The REAL binary against a scripted mock hub: archived→unarchive→resume, auth-rejected naming + remedy + no token in output, hold-without-consuming while the daemon is absent, exactly one render across a SIGKILL + redelivery (S6b), render-failure→nack+re-probe, SIGTERM settle + the journal lifecycle lines (M4/M11) |
| `live_hub_tests` (6, `#[ignore]`) | `scripts/drill.sh`, local | What no mock can express, against the real kyu binary: publish/receive/ack round-trip, policy bootstrap effect + read-back, redelivery with attempt count, poison → visible dead letter, unarchive no-op, hub dying and returning mid-run (K8/S6d) |
| Manual drills | logged in `docs/DRILL_LOG.md` | Real-desktop rendering (Plasma), S6c end-to-end TTL expiry with the courier down, systemd unit verify, latch-under-systemd, M7 restore-from-zero |

## What each fake cannot express (AR18, standing rule 9)

- **PATH shims** cannot prove a daemon displays anything → real-desktop
  drill (DRILL_LOG, S6a).
- **The scripted mock hub** cannot prove lease/redelivery/TTL/archive
  semantics → live tests against the real kyu binary.
- **The live scratch hub** cannot prove behaviour of the *live* hub's
  token door (scratch runs open) → the auth path is mock-proven (401
  classification, header shape) and the real token flow is part of
  Kenny's activation step.

## Security review (Phase 7, mandatory — network + auth)

`/security-review` ran 2026-08-29 over the full diff (all code).
**Findings above the bar: none.** Attack surfaces assessed clean:
subprocess spawns (argv-array, no shell, `--` separator, trusted
config paths only), untrusted payload flow (never reaches paths, URLs
or execution; settle key is the hub id), token handling (wire-only;
scan-tested twice — send-test path and the loop harness), plain-HTTP
baseline (accepted N3 design, nothing new worsens it; bodies bounded),
serde on untrusted JSON (size-budgeted, no side-effectful fields).

## Hardening round (test-gap-auditor, 2026-08-29)

17 gaps reported; decisions (AFK-provisional):

**Closed the same day** — G1 (M7 restore runbook + drill), G2 (M5
update runbook; L5 status corrected), G3 (loop harness — `loop_tests`),
G4 (state fs tests + SIGKILL one-render test), G5 (`live_k8_s6d`), G6
(SIGTERM settle test), G7 (hold-without-consuming test), G8 (journal
line assertions + unknown-priority warn line), G9 (loop-output token
scan incl. a 401 branch), G12 (argv per priority), G16 (empty
topic/subscription refused, ttl overflow refused), G11-part
(`send-test --priority` validated).

**Accepted as known limitation:**
- ~~K7 runtime verification~~ — **closed 2026-08-30**: unit enabled,
  live-hub policy asserted and read back, field-test toast rendered
  through the running service (DRILL_LOG).
- **Second-SIGTERM abort path** untested (registration order is the
  documented signal-hook pattern; failure mode is a bounded stop
  timeout, not data loss) (G6-part).
- **K6 sound branches at loop level** — fires-only-when-configured is
  proven at render level; the loop-level branch rides the config
  default (off) (G15-part; the default-off decision itself is a queued
  form item).

**Later, by decision (not covered — recorded consciously):**
- **G10** S6c as an automated live test — the manual drill (metrics +
  `kyu.events` + no toast) is strong evidence; an automated variant
  costs 1–2 min per run. Revisit if the TTL path ever regresses.
- **G13** "published mid-poll arrives at once" — hub-owned behaviour,
  hub-tested (`l2_a_message_published_mid_poll_arrives_at_once`).
- **G17** hub_client edge branches (2 MiB cap, unreadable 200 bodies,
  policy-GET 500) — every failure direction is backoff-and-log.
- **G11-rest** full CLI sweep (`--help`/unknown-arg exits).

## Coverage note

No coverage percentage is enforced; the registry above (feature ID →
suite) is the coverage instrument, per the procedure's L6 rule. CI
runs every non-ignored suite on every push once the remote exists.
