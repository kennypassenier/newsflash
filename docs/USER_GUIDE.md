# desk-courier — user guide

Everything desk-courier does, one feature at a time. Written in Phase 8
from the code and tests; every claim names where it is proven
(unit/mock tests run in `cargo test --all`; `live_*` tests run against
the real mailbox binary via `scripts/drill.sh`; desktop-level behaviour
cites the drill log).

If you have two minutes: install per runbook R1, run
`desk-courier send-test --title Proef --message "Werkt het?"`, watch
the toast. That is the product.

## K1 · It receives by long-polling

The courier long-polls subscription `desktop` on `notify.kenny`
(`wait=20`, `envelope=json`). A message published mid-poll arrives at
once; an empty window is a quiet 204. The first poll of every run adds
`from=beginning`, so a freshly installed courier still picks up what
the topic retains (dedup and the TTL check keep that safe).

**Proven by:** `k1_receive_sends_the_right_request_and_parses_the_real_vector`,
`ar7_the_first_poll_of_a_run_replays_from_the_beginning`,
`live_s6a_publish_receive_ack_round_trip`.

## K2 · It renders toasts via notify-send

Title and message from the envelope, in your configured language
(M3), app-name `desk-courier`. The body is markup-escaped and both
fields are truncated to sane budgets, so a producer can neither smuggle
options nor markup into the toast.

**Proven by:** `l3_render_shell_via_path_shims`,
`ar12_the_body_is_markup_escaped_the_summary_is_not`,
`ar4_summary_and_body_are_truncated_with_an_ellipsis`; real-desktop
rendering: DRILL_LOG 2026-08-29 (S6a).

## K3 · Ack after success, redelivery after failure

Only a successful `notify-send` acks the message. A failed render nacks
it — the hub redelivers with backoff; after max attempts it lands in
the hub's visible dead letters, never a void.

**Proven by:** `ar5_render_success_marks_then_acks`,
`ar5_k3_render_failure_nacks_for_redelivery`,
`k3_ack_and_nack_hit_the_right_endpoints`,
`live_k3_an_unacked_message_is_redelivered_with_a_higher_attempt`.

## K4 · No duplicate toasts on redelivery

Delivery is at-least-once (the hub's contract), so the courier keeps
the last 512 delivered hub ids in
`~/.local/state/desk-courier/seen.json` (atomic writes, survives
restarts). A redelivered id is acked silently. A corrupt store starts
empty and says so — worst case one duplicate toast, never a lost one.

**Proven by:** `k4_a_seen_id_is_recognized`, `k4_round_trips_through_json`,
`ar5_s6b_a_seen_hub_id_acks_without_rendering`,
`ar6_corrupt_json_yields_none_for_a_logged_empty_start`; persisted
store across a run: DRILL_LOG (S6b).

## K5 · The 10-minute TTL is code, not a setting

On startup (after the first poll) and after every reconnect, the
courier asserts the subscription policy: it PUTs exactly
`{"ttl_ms": 600000}` — the one field it owns; lease/attempts keep
tracking the hub's defaults. A day logged out therefore expires at the
hub — counted in metrics and announced on `mailbox.events`, never
silent — and login shows zero stale toasts. Messages the hub could not
expire (claimed just before a suspend) are caught by the courier's own
TTL check and acked unrendered.

**Proven by:** `k5_ar7_policy_put_carries_exactly_the_one_owned_field`,
`ar7_a_policy_already_right_is_left_alone`,
`live_k5_ar7_policy_bootstrap_is_idempotent_and_effective`,
`ar5_a_stale_message_is_acked_unrendered`; end-to-end expiry with the
courier down: DRILL_LOG (S6c).

## K6 · Optional chime

Set `sound_file` in the config and every toast is followed by the
sound, played fire-and-forget — a broken player never fails a message.
No file configured = silence. House style for the file itself:
cyberpunk-soft, never metallic.

**Proven by:** `l3_render_shell_via_path_shims` (fires only when called,
`--` separator, detached).

## K7 · It runs as a systemd user service

`systemd/desk-courier.service`: starts after the graphical session,
**stops at logout** (a toast without a session has no screen — the TTL
covers the gap), restarts on failure with a crash-loop brake, bounded
stop. Install per runbook R1.

**Proven by:** `systemd-analyze --user verify` clean (DRILL_LOG);
runtime state: **build-verified, not yet runtime-verified** — enabling
the unit on the real session is Kenny's deployment step (AFK queue).

## K8 · Hub down never hurts

Unreachable hub → linear backoff (2 s … 60 s), one log line per state
change, automatic recovery, policy re-asserted after reconnect. Every
error state is named and distinct in the journal: `hub unreachable`,
`auth rejected` (with the re-mint remedy), `topic does not exist yet`,
`subscription was archived` (auto-unarchived, loudly), `no
notification daemon` (holds without consuming).

**Proven by:** `ar9_the_three_failure_classes_never_merge`,
`ar9_linear_then_capped`; live outage/recovery: DRILL_LOG (S6d).

## K9 · The token stays invisible

Token via `MAILBOX_TOKEN` (latch-injected) or a 0600 `token_file`; an
inline token in the config is refused with a remedy. It reaches the
wire as a Bearer header and appears nowhere else — not in logs, argv,
URLs or error text.

**Proven by:** `k9_the_token_reaches_the_wire_but_never_the_output`
(spawns the real binary, scans all output, checks the wire),
`m2_ar10_config_validation_names_field_and_remedy`.

## M8 · send-test

```
desk-courier send-test --title Proef --message "Werkt het?" --priority warning
```
Publishes one valid v1 envelope to the configured topic — the drill
tool, and the interim producer until the notification pipeline starts
shadow-publishing real envelopes.

**Proven by:** `k9_the_token_reaches_the_wire_but_never_the_output`
(asserts the published body parses as a valid envelope),
`live_s6a_publish_receive_ack_round_trip`.

## M9 · Garbage becomes a visible dead letter

A payload that is not a v1 envelope (wrong version, no text, binary,
oversized, hand-typed test JSON) is nacked `dead=true` on the first
attempt: it lands in the hub's dead-letter list with its payload, one
Requeue click away, instead of retry-looping. The journal line carries
the specific reason and remedy.

**Proven by:** `ar5_m9_a_parse_failure_is_poison_even_when_stale`,
`every_error_carries_a_remedy`,
`live_m9_poison_lands_visibly_in_the_dead_letters`.

## What desk-courier deliberately does not do

- **No routing or audience logic** — the topic name is the address;
  upstream decides who gets what (SCOPE S9).
- **No critical path** — hub down = no toasts, everything else in the
  house unchanged (SCOPE S10).
- **No TTS** — the envelope's `tts` field is the speaker channel's job.
- **No click actions** — `click_url` is feature M10, rated Later.
- **No self-update** — runbook R2 is the update path, by decision (M5).
