# Drill log — desk-courier

Live-drill evidence per standing rule 14 / AR18. Every drill ran on a
**scratch hub** (local `mailbox` binary, `127.0.0.1:18925`, temp data
dir) — the live hub on LXC 109 was never touched.

## 2026-08-29 · L2 — hub contract drills (scripts/drill.sh)

All five `#[ignore]` live tests green against the real mailbox binary:

- `live_s6a_publish_receive_ack_round_trip`
- `live_k5_ar7_policy_bootstrap_is_idempotent_and_effective` (incl.
  rule-13a read-back: `effective.ttl_ms = 600000`, lease stays default)
- `live_k3_an_unacked_message_is_redelivered_with_a_higher_attempt`
- `live_m9_poison_lands_visibly_in_the_dead_letters`
- `live_ar21_unarchive_on_a_healthy_subscription_is_a_safe_noop`

Drill discoveries, all folded back into code + AR record:
1. First poll on a fresh hub answers **404** (topic missing) → new
   error class + quiet waiting state.
2. A message published before the subscription's first poll is
   invisible to it → first poll of every run uses `from=beginning`.
3. A policy PUT on an unpolled subscription answers **404** → the
   poll-before-policy startup order is mandatory, not stylistic.
4. The real `envelope=json` response captured and pinned as
   `courier-core/tests/vectors/hub_envelope_response.json`; the mock
   hub serves exactly those bytes.

## 2026-08-29 · L4/L5 — full-loop drills on the real desktop

The debug binary ran as a real process against the scratch hub; toasts
rendered by the real KDE Plasma 6.7.4 daemon (two visible test toasts).

- **S6a** — `send-test` → toast within ~1 s; journal line
  `rendered 01M157PQ… (payload id test-…, attempt 1)`.
- **S6d** — hub killed: exactly one state-change line
  (`hub unreachable (… Connection refused …) — backing off`), no crash,
  no log spam; hub restarted: `hub reachable`, policy re-asserted,
  next message toasted.
- **M4** — SIGTERM during an open poll window: exited 0 right after the
  window closed, final line `shutdown: in-flight work settled, dedup
  store persisted`.
- **S6b** — `seen.json` holds the three delivered hub ids; atomic file
  in `$XDG_STATE_HOME/desk-courier/`.
- **S6c** — with `ttl_ms=60000` on the subscription and the courier
  down, a published message expired hub-side: metrics show
  `mailbox_deliveries{…,state="expired"} 1`, `mailbox.events` carries
  `{"event":"message.expired","count":1,…}`, and the restarted courier
  rendered nothing — zero stale toasts, expiry recorded, never silent.
- **First drill mishap, recorded honestly:** the first S6d attempt
  killed a wrapper pid instead of the hub and "passed" without an
  outage ever happening; caught by the missing `hub unreachable` line
  and redone against the real pid. Evidence needs reading, not
  assuming.

## 2026-08-29 · AR10/AR20 — unit + latch drills

- `systemd-analyze --user verify` on the unit: clean after two real
  fixes it caught (`StartLimit*` belongs in `[Unit]`; latch lives at
  `~/.cargo/bin/latch`, not `/usr/bin`).
- `latch --version` runs fine non-interactively under a systemd user
  scope (`systemd-run --user`): latch 2.2.0.
- `latch run` resolves its project from the **working directory** →
  the unit sets `WorkingDirectory=%h/Projects/hub-clients`. The full
  end-to-end (latch project + real `MAILBOX_TOKEN` secret + enabled
  unit) waits for Kenny: token minting on the live hub's /apps page is
  queued (AFK queue, "Deliberately not done").
