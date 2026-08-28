# Architecture decisions — hub-clients (desk-courier)

Phases 3+4 output, drafted 2026-08-29 during the AFK build. Every
decision below is Claude's recommendation, attacked by the
architecture-critic agent before the provisional freeze; surviving
objections are recorded per decision as ⚔ counter-arguments. Kenny's
ratification is queued (`docs/AFK_QUEUE.md` R3). Changes after
ratification go through mini-rounds only.

## Phase 3 — tech choice

### AR1 · Language & toolchain: Rust, pinned 1.97.1
Rust — the house language for hub consumers (mailbox, latch, homelab
are all Rust; the study assumed "any future Rust binary"). Toolchain
pinned in `rust-toolchain.toml` at **1.97.1** (the version installed on
the PC) and CI asks for exactly that (standing rule 7 — the mailbox
1.97-vs-1.98 clippy drift is the precedent). Edition 2024.

### AR2 · Dependency policy: reluctant, no async runtime
A single-subscription long-poll loop is sequential by nature; an async
runtime buys nothing. Blocking HTTP via **ureq**, JSON via
**serde/serde_json**, config via **toml**, signals via **signal-hook**.
No tokio, no reqwest, no anyhow/thiserror (hand-rolled error enums in a
codebase this size). Plain HTTP only — the hub is LAN-only by design
(mailbox N3), so no TLS stack ships at all.

### AR3 · Workspace layout: core/shell split
Cargo workspace, two crates:
- `courier-core` — pure logic, **zero ambient I/O**: envelope parsing,
  render decision table, dedup bookkeeping, policy diffing, backoff
  schedule. Everything unit-testable without a hub or a desktop.
- `desk-courier` — the binary shell: HTTP calls, subprocesses, signals,
  filesystem, clock.
CI enforces the boundary mechanically (grep for I/O crate imports in
core, the Almanac AR13 pattern). A future vault-courier would be a
third crate reusing `courier-core` — but nothing is factored for it in
advance (SCOPE S8: its existence is uncertain).

## Phase 4 — architecture

### AR4 · Envelope v1 draft, pinned tolerant-reader
The study §7 draft is a serde struct with a pinned regression vector
(`tests/vectors/`). Reading is **tolerant**: unknown fields ignored
(pipeline-v2 owns the final schema and will evolve it — forward
compatibility is survival), missing optional fields default. Hard
requirements: `v == 1` and at least one renderable title/message in
some language — anything else is poison (AR5). A pipeline-v2 schema
change is a mini-round trigger (SCOPE S7).

### AR5 · At-least-once handling: the settle table
Delivery is at-least-once (mailbox N4); every message ends settled:

| Case | Action |
|---|---|
| Parse OK, not seen before, `notify-send` exit 0 | mark seen → **ack** |
| Parse OK, id already seen (redelivery after crash) | **ack** without rendering (S6b) |
| `notify-send` non-zero exit or spawn failure | **nack** — transient, hub redelivers with backoff |
| `v != 1`, unparseable JSON, or no renderable content | **nack `dead=true`** — poison pill, visible in dead letters (M9) |

Order on success: render → mark seen → ack. A crash between mark and
ack redelivers a marked id → the dedup branch acks it silently. A crash
between render and mark redelivers an unmarked id → worst case one
duplicate toast — the acceptable direction (a toast twice beats a toast
never).

### AR6 · Dedup store: bounded, atomic, fail-open
Last 512 envelope ids in `$XDG_STATE_HOME/desk-courier/seen.json`
(fallback `~/.local/state/…`), written atomically (temp + rename,
standing rule 12). Missing or corrupt file → start empty and say so in
the log: fail-open, because the failure direction of an empty dedup
store is a duplicate toast, never a lost one. 512 ≫ any 10-minute
backlog (S4 caps how much can ever be pending).

### AR7 · Policy bootstrap: the policy is code
On startup the courier GETs the `desktop` subscription policy and, if
it differs from the configured desired state, PUTs the **complete**
policy — hub K7 semantics: a write replaces every field, so the courier
always writes all of them: `ttl_ms` 600 000 (S4), lease and attempts at
the hub defaults it read back. The desired TTL lives in config with the
600 000 default. Idempotent by construction (diff before write).

### AR8 · Error model
`courier-core`: one hand-rolled error enum per module boundary, every
variant carrying a remedy string (standing rule 11). Shell: errors
bubble to the main loop, which classifies transient (retry/backoff) vs
fatal-config (exit with remedy). The process exits non-zero only for
config/startup errors; runtime trouble never kills it (S6d).

### AR9 · Backoff: linear, capped, logged
Hub unreachable → linear backoff 2 s per step, capped at 60 s, one log
line per state change (down → each failed retry escalation → recovered).
Linear matches the hub's own redelivery philosophy (predictable, no
cap-math). Poll errors and publish-side errors share the schedule.

### AR10 · Token handling: environment first, never inline
Token source order: `MAILBOX_TOKEN` env var (what `latch run` injects —
M6), else a `token_file` path from config (must be 0600). An inline
`token = "…"` key in config is **refused at startup with a remedy** —
secrets do not live in config files that sit next to a git-tracked
example (standing rule 10). The token never appears in logs, argv, or
error messages; a plaintext-scan test asserts it.

### AR11 · Toast mapping
| Priority | Urgency | Expire |
|---|---|---|
| `info` | normal | 10 s |
| `warning` | normal | 30 s |
| `critical` | critical | 0 (persistent until dismissed) |

App-name `desk-courier`, title = envelope title, body = message, in the
configured language with cross-language fallback (M3). Sound (K6): only
when `sound_file` is configured; spawned after a successful toast on a
detached thread; player failure is logged and never fails the message.

### AR12 · Subprocess contract, no D-Bus library
`notify-send` and `paplay` are resolved via `PATH` — the courier speaks
the Desktop Notifications spec through the tool built for it, and tests
inject fakes by prepending to `PATH`. No zbus/notify-rust dependency:
the daemon is whatever the session runs (KDE Plasma today), and the CLI
is the stable seam. ⚔ **Counter-argument (critic):** subprocess exit
codes are a coarser signal than D-Bus errors, and `notify-send` exit 0
does not strictly prove a toast was displayed — accepted: the drill on
the real desktop covers the last inch, and the simplicity/testability
trade is worth it.

### AR13 · Concurrency: one loop
Single-threaded main loop (poll → settle → repeat); the detached sound
thread (AR11) is the only concurrency. No shared state beyond the dedup
store, owned by the loop.

### AR14 · Graceful shutdown
SIGTERM/SIGINT set a flag (signal-hook); the loop finishes settling the
in-flight message, persists the dedup store, exits 0. A second signal
aborts immediately. `systemctl --user stop` is therefore always clean
(M4); a hard kill costs at most one duplicate toast (AR5's accepted
direction).

### AR15 · No self-update (M5 decision)
Single-machine tool, updated by `git pull` + `cargo build --release` +
`systemctl --user restart` as a numbered runbook procedure. `--version`
reports the Cargo version. No update authenticity model needed — the
"distribution channel" is the local git checkout.

### AR16 · Degradation doctrine (frozen sentence)
The courier is an extra channel, never a gate. Every failure mode
degrades to exactly "no toasts": hub down, courier dead, daemon absent,
logged out. Nothing upstream waits on it; no existing channel changes
behaviour (SCOPE S10 — the study §7 doctrine, surviving into this
freeze verbatim as required).

### AR17 · Config
`~/.config/desk-courier/config.toml` (XDG), full example committed as
`config.example.toml` (no secrets — AR10). Validated at startup; every
rejection names the field and the remedy (M2).

### AR18 · Testing architecture (what each fake cannot express)
- **PATH-shim fakes** for `notify-send`/`paplay` prove argv and exit
  handling. *Cannot express:* whether a real daemon displays anything —
  covered by one live desktop drill per relevant milestone (rule 9).
- **Mock hub** (tiny in-process HTTP server) proves request shapes and
  error branches. *Cannot express:* real lease/redelivery/TTL/dead-letter
  semantics — covered by live tests (`#[ignore]`, run by `scripts/drill.sh`)
  against the real mailbox binary (`MAILBOX_BIN`, default
  `~/Projects/mailbox/target/release/mailbox`) on a scratch data dir and
  port. CI runs the mock suite; drills run locally and their output is
  milestone-report evidence (the CI remote cannot build the private
  mailbox repo).

## Freeze

**Provisionally frozen 2026-08-29 for the AFK build**, after the
architecture-critic round (objections embedded above as ⚔). Real freeze
is Kenny's, via ratification round R3.
