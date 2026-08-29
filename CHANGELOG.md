# Changelog — hub-clients

## 0.1.0 — unreleased (Phase 9 pending)

First release: **desk-courier**, the desktop-toast consumer for the kyu
message hub.

- Long-polls subscription `desktop` on `notify.kenny` (envelope mode,
  cold-start replay on the first poll of a run) and renders each
  message as a desktop toast via `notify-send`, with priority → urgency
  mapping and an optional soft chime (off by default).
- At-least-once done right: ack only after a successful render; dedup
  on the hub message id, persisted atomically, exactly one toast across
  crashes and redeliveries; malformed payloads dead-letter visibly
  instead of retry-looping.
- The 10-minute-TTL story: the courier asserts its own subscription
  policy (one owned field, re-asserted on every reconnect); a day
  logged out expires at the hub — recorded, never silent — and its own
  staleness check catches what a suspend hid from the hub.
- Degrades to exactly "no toasts", loudly: named journal states for
  hub-down, auth-rejected (with remedy), topic-missing,
  archived-subscription (self-healing unarchive) and
  notification-daemon-absent (holds without consuming); linear capped
  backoff; hard timeouts on every read and every child process.
- Ships as a systemd user service bound to the graphical session, with
  the token injected by `latch run` (`KYU_TOKEN`), `send-test` as drill
  tool, and runbook procedures for install, update, restore, token
  rotation and triage.

Built against kyu 2.0.0. 62 automated tests + 6 live tests against the
real kyu binary; see docs/TEST_PLAN.md and docs/DRILL_LOG.md.
