# Drill log — newsflash

Live-drill evidence per standing rule 14 / AR18. Every drill ran on a
**scratch hub** (local `kyu` binary, `127.0.0.1:18925`, temp data
dir) — the live hub on LXC 109 was never touched.

## 2026-08-29 · L2 — hub contract drills (scripts/drill.sh)

All five `#[ignore]` live tests green against the real kyu binary:

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
  in `$XDG_STATE_HOME/newsflash/`.
- **S6c** — with `ttl_ms=60000` on the subscription and the courier
  down, a published message expired hub-side: metrics show
  `kyu_deliveries{…,state="expired"} 1`, `kyu.events` carries
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
  end-to-end (latch project + real `KYU_TOKEN` secret + enabled
  unit) waits for Kenny: token minting on the live hub's /apps page is
  queued (AFK queue, "Deliberately not done").

## 2026-08-30 · Deployment + dogfood (Phase 9)

- Kenny allowed `ssh pve` via `settings.local.json`; the app token was
  minted ON LXC 109 (the hub's master token never left that machine)
  and flowed straight into `.env` → `latch commit && latch push`
  (`latch run` injects `KYU_TOKEN`, length 48 — value never displayed).
- `systemctl --user enable --now newsflash`: active, enabled.
- First contact with the LIVE hub behaved exactly as frozen: policy
  asserted (`effective.ttl_ms=600000` read back, lease/attempts still
  tracking hub defaults), and the cold-start replay found a backlog of
  retained envelopes on `notify.kenny` — every one **acked unrendered
  as stale** (journal lines), zero stale toasts. S4 held on its very
  first real run.
- Dogfood (Phase 9 rule): `latch run -- newsflash send-test` →
  `rendered 01M17T391XRY5R3V2N40BQXRAW` — the first real toast from the
  live hub, on the real desktop, through the enabled unit. **K7 is now
  runtime-verified**, closing the build-vs-runtime gap from TEST_PLAN.

## 2026-08-30 · Rename mini-round — desk-courier → newsflash

- Full sweep verified before the switch: 62 tests + all 6 live drills
  green post-rename (source, package, systemd unit, docs).
- Live cutover: old unit stopped, disabled, removed; new binary
  installed (`~/.cargo/bin/newsflash`); config AND the dedup store
  (`seen.json`) migrated to the new paths (no dedup-history loss, so
  no duplicate-toast risk on the next redelivery); new unit installed,
  enabled, started — `journalctl` shows the clean startup line under
  the new name.
- Dogfood repeated under the new name: `newsflash send-test` →
  `rendered 01M17WYFNS48Y7XENT7WV86V1X` — a real toast, new binary,
  same live hub.
- Read-back (rule 13a habit, applied to a local system too): every old
  artifact (`~/.cargo/bin/desk-courier`, `~/.config/desk-courier/`,
  `~/.local/state/desk-courier/`, the old unit file) confirmed absent
  after cleanup — zero stragglers.
