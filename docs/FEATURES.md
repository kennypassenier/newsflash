# Features — hub-clients (desk-courier)

Phase 2 output, drafted 2026-08-29 during the AFK build. Ratings use the
fixed scale **Essential · Desired · Later · Don't do**. Every rating
below is Claude's recommendation, built as rated; Kenny's ratification
is queued (`docs/AFK_QUEUE.md` R2). IDs are permanent: they appear in
commits, test names, docs and forms from here on. Changes after
ratification go through mini-rounds only (`FORM_PROTOCOL.md` §5).

All features are desk-courier's. vault-courier gets its own feature
round if and when its gate opens (SCOPE non-goal S8 — existence itself
still uncertain).

## Round 1 — from Kenny's scope answers and the study

| ID | Feature | Rating | Test expectation |
|---|---|---|---|
| K1 | Long-poll consume — subscription `desktop` on topic `notify.kenny`, `envelope=json`, honoring the hub's wait window; a message published mid-poll arrives at once | Essential | Live test against a real local mailbox binary: publish → consumed within the poll window. Mock-hub unit tests for request shape (URL, `as=`, headers). |
| K2 | Toast rendering via `notify-send` — title + body from the envelope in the selected language (M3), app-name `desk-courier` | Essential | Unit tests with a PATH-shimmed fake `notify-send` asserting exact argv; one live drill on the real desktop (S6a) recorded in the milestone report. |
| K3 | Ack after success, nack on transient failure — ack only after `notify-send` exits 0; non-zero exit → nack (hub redelivers per policy) | Essential | Live test: failing renderer → message redelivered; succeeding renderer → acked, gone. Unit test for the ack/nack decision table. |
| K4 | Redelivery dedup on envelope id, persisted across restarts (S6b) | Essential | Test delivering the same id twice (including across a process restart) asserting exactly one render; dedup store survives kill -9 (atomic writes). |
| K5 | TTL policy bootstrap — on startup the courier PUTs the `desktop` subscription policy (TTL 10 min, full-field write per hub K7 semantics) so the policy is code, not a hand-configured setting (S4) | Essential | Live test: after startup, GET policy shows `ttl_ms=600000`; bootstrap is idempotent (second startup changes nothing). |
| K6 | Optional soft chime per toast — `sound_file` config played via `paplay`; cyberpunk-soft house style; no file configured = silent | Desired | Unit test with fake `paplay` asserting it fires only when configured and never blocks/fails the toast on player error. |
| K7 | systemd user service — unit file in repo + numbered activation procedure (gmediarender pattern) | Essential | Unit file ships + `systemd-analyze --user verify` clean; activation itself is Kenny's go (AFK queue) — runtime-verified only after that. |
| K8 | Resilience — hub unreachable → bounded backoff retry loop, no crash, visible log lines; recovery is automatic (S6d) | Essential | Live test: kill the scratch hub mid-run, assert the courier survives N cycles and resumes when the hub returns. |
| K9 | Bearer token auth — token from environment (latch-injectable, M6) or config; asserted never to appear in logs or argv | Essential | Unit test for the auth header; plaintext-scan test over captured log output (standing rule 10). |

## Round 2 — Claude's proposals (gaps, hardening, quality-of-life)

| ID | Feature | Rating | Test expectation |
|---|---|---|---|
| M1 | Priority → toast urgency mapping: `info`→normal, `warning`→normal, `critical`→critical (persistent until dismissed) + matching `--expire-time` | Essential | Unit tests per priority asserting the exact `notify-send` urgency/expire argv. |
| M2 | TOML config with startup validation and actionable error messages (standing rule 11) — hub URL, topic, subscription, language, sound, TTL | Essential | Test per broken-config class asserting startup fails naming the field and the remedy; defaults documented. |
| M3 | Language selection — config `language = "nl"` (default) picks `title.nl`/`message.nl`; missing translation falls back to the other language rather than dropping the toast | Essential | Unit tests: nl present → nl; nl missing → en fallback; both missing → poison path (M9). |
| M4 | Graceful shutdown — SIGTERM/SIGINT finishes the in-flight render/ack, then exits 0 (clean `systemctl --user stop`) | Essential | Test sending SIGTERM mid-cycle asserting the in-flight message is settled (acked or nacked), never left to lease expiry. |
| M5 | **Update & distribution (mandatory item):** no self-update, by decision — single-machine tool updated by `git pull` + `cargo build --release` + `systemctl --user restart`, as a numbered runbook procedure | — decision — | The runbook procedure exists and was executed once during the build (evidence in the milestone report). |
| M6 | **Ecosystem integration (mandatory item):** mailbox is the counterparty (by definition); token injection via **latch** — the unit file runs `latch run -- desk-courier` so the token never touches disk outside latch; plain env file documented as fallback. homelab n/a (runs on the PC) | Essential | Token reaches the process via environment in tests; unit file carries the latch wrapper; fallback documented. |
| M7 | **Backup & restore (mandatory item):** state = config (in git as example + tiny restore step), token (lives in latch, rides latch's own escrow), dedup cache (throwaway). No scheduled backup, by decision — restore-from-zero is a numbered runbook procedure and was drilled once | — decision — | The restore procedure rebuilds a working courier from a clean checkout; drilled during Phase 7 (evidence recorded). |
| M8 | `send-test` subcommand — publishes a valid v1 test envelope to a hub (the interim producer per S11d, and the drill tool) | Desired | Round-trip test: `send-test` → courier consumes → fake renderer sees the toast argv. |
| M9 | Poison-pill on malformed envelopes — unparseable JSON or no renderable content → nack `dead=true`, visible in the hub's dead letters instead of a retry loop | Essential | Live test: publish garbage → dead-lettered after one attempt, courier keeps running; unit tests for the poison decision table. |
| M10 | `click_url` as a toast action (Open button → `xdg-open`) | Later | Defined if picked up — needs libnotify action support and a wait loop per toast; deliberately deferred. |
| M11 | Logging: one startup summary line (config in force, hub URL — never the token) + one line per lifecycle event (consumed, rendered, acked, nacked, expired-policy write, reconnect), journald-friendly | Essential | Log-capture test asserting the summary and per-event lines; plaintext-scan shares K9's assertion. |

## Tally (provisional, AFK)

| Rating | Count | IDs |
|---|---|---|
| Essential | 15 | K1–K5, K7–K9 (8) · M1–M4, M6, M9, M11 (7) |
| Desired | 2 | K6, M8 |
| Later | 1 | M10 |
| Don't do | 0 | — |
| Decisions recorded | 2 | M5 (no self-update), M7 (no scheduled backup) |

## Freeze

**Provisionally frozen 2026-08-29 for the AFK build.** Real freeze is
Kenny's, via ratification round R2. Anything he re-rates is a mini-round
against the built state.
