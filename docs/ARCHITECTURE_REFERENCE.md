# Architecture reference — desk-courier as built

Phase 8 document: the system as it exists, written from the code.
The *decisions* and their reasoning live in
`ARCHITECTURE_DECISIONS.md`; this is the map.

## Shape

```
notify.kenny topic (kyu hub, LXC 109)
        │  long-poll GET /t/notify.kenny/next?as=desktop&envelope=json&wait=20
        ▼
┌─ desk-courier (systemd user service, Garuda PC) ─────────────┐
│ run.rs        the one loop: poll → settle → repeat           │
│ hub_client.rs HTTP shell (ureq, AR19 timeouts)               │
│ render.rs     notify-send / paplay / busctl via PATH         │
│ state.rs      ~/.local/state/desk-courier/seen.json (atomic) │
│ config.rs     ~/.config/desk-courier/config.toml + token     │
│ logx.rs       sd-priority lines on stderr → journald         │
└──────────────┬───────────────────────────────────────────────┘
               ▼
        KDE Plasma notification daemon (toast)
```

`courier-core` (separate crate, zero ambient I/O — enforced by gate
and CI) holds everything decidable without the world: envelope
parsing, the settle table, dedup bookkeeping, toast mapping, backoff,
hub-response parsing, error classification. `desk-courier` is the
shell that gives it a hub, a desktop, a filesystem and a clock.

## The loop, one cycle

1. Daemon probe (only while unhealthy — AR22): `busctl … GetServerInformation`;
   failing → hold, no polling, messages wait at the hub under the TTL.
2. Policy assertion (after the first poll of a connection — AR7): GET
   policy, PUT `{"ttl_ms":600000}` only if the explicit set differs.
3. Receive: first poll of a run carries `from=beginning` (cold start);
   204 = quiet loop; errors classify into unreachable / auth /
   archived (→ auto-unarchive) / topic-missing / other, each its own
   named journal state, all on the linear 2–60 s backoff.
4. Settle (pure, AR5): poison → nack `dead=true`; seen hub id → ack
   silent; stale → ack logged; else render → mark seen → persist → ack
   (that order: a crash duplicates at worst, never loses). Settle-call
   4xx = gone anyway, move on; one bounded retry on transport.
5. SIGTERM: finish the in-flight settle, persist, exit 0; second
   signal aborts.

## Data

- **Envelope v1 DRAFT** (pipeline-v2 owns the final form): pinned in
  `courier-core/tests/vectors/envelope_v1.json`; tolerant reader;
  256 KiB budget; summary/body truncated 200/1000 chars; body
  markup-escaped.
- **Hub `envelope=json` response**: pinned real capture in
  `…/vectors/hub_envelope_response.json`; the settle/dedup key is the
  hub message `id` from this frame, never the payload's own id.
- **Dedup store**: last 512 hub ids, JSON array, temp+rename writes,
  corrupt → logged empty start (fail-open).

## Trust and secrets

Token from `KYU_TOKEN` (latch-injected via the unit's
`latch run`, resolved through `WorkingDirectory`) or a 0600
`token_file`; inline config tokens refused. The token exists in memory
and the `authorization` header only — scan-tested on both binary
paths. Payload bytes are untrusted producer input: they reach argv
positionals after `--` (never a shell), log lines, and nothing else.

## Dependencies (AR2)

`ureq` (blocking HTTP, no TLS — LAN-only hub), `serde`/`serde_json`,
`toml`, `signal-hook`, and `tiny_http` in dev. No async runtime.
Toolchain pinned 1.97.1 (`rust-toolchain.toml`), edition 2024.

## Deployment

systemd **user** unit (`systemd/desk-courier.service`): bound to
`graphical-session.target` both ways, crash-loop brake
(`StartLimitBurst=5`/120 s), `TimeoutStopSec=45`. Update = runbook R2
(git pull + `cargo install` + restart; no self-update, AR15). Restore
= runbook R3 (repo + latch are the state; the dedup cache is
throwaway).
