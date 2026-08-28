# Scope — hub-clients

Phase 0 output. Approved via the Phase 0 gate form on 2026-08-28 — every
item below reflects Kenny's actual answer, not the draft. Frozen except
through a mini-round (`FORM_PROTOCOL.md` §5) once later phases are under
way.

Bootstrapped from the Mailbox Integration Study
(`ObsidianVault/Home Assistant/Documentation/Mailbox Integration Study.md`,
§5.P3, §5.P2, §7, §8b), where desk-courier (P3) was rated **Essential**
and vault-courier (P2) **Desired** on 2026-08-28.

## Repo structure (S1 — decided)

**One workspace repo** (`~/Projects/hub-clients`) holding the consumer
binaries that run on Kenny's PC. First binary: `desk-courier`. The two
planned binaries share almost everything — envelope parsing, the hub
client (long-poll, ack, retry), token handling, the systemd user-service
pattern — so one repo means one CI, one hook set, one procedure
administration. Trade-off accepted: one version number and one release
tag covers the workspace.

## Mission (S2)

`desk-courier` runs on Kenny's Garuda PC, long-polls subscription
`desktop` on topic `notify.kenny` at the mailbox hub, and renders each
message as a desktop toast via `notify-send`. After a successful toast
the message is acked; a failed render leaves it unacked so the hub
redelivers.

This closes the measured gap that the PC speaker
(`media_player.kenny_pc`) only exists while logged in *and* that TTS is
the only desktop channel: speech interrupts, toasts persist in the
corner.

**Factual note (2026-08-28):** the study said "notify-send/dunst"; the
actual notification daemon on this machine is KDE Plasma 6.7.4. The
interface is `notify-send` (Desktop Notifications spec); the daemon
behind it is whatever the desktop session runs. No scope impact.

## Sound (S3)

Optionally, a toast plays a subtle sound in the house chime style:
cyberpunk-but-soft, never metallic. Configurable off; whether it
defaults on or off is a Phase 2 decision.

## Subscription policy: short TTL (S4)

The `desktop` subscription carries a policy with a **TTL of 10
minutes**: a toast about a doorbell an hour ago is noise. After a day
logged out, the backlog expires at the hub side — **recorded** (the hub
counts and reports expiries via `mailbox.events`), never silent — and
login greets you with zero stale toasts. The exact value is a Phase 2/4
detail; the scope statement is: short TTL, expiry visible, never a pile.

## Run form: systemd user service (S5)

`desk-courier` runs as a systemd **user** service on the Garuda PC —
the same pattern as the gmediarender/DLNA service (see
`ObsidianVault/Home Assistant/Manuals/PC As DLNA Speaker.md`).
Deliberately user-scoped: it lives only inside the desktop session, and
that is exactly right — a toast without a session has no screen to
appear on, and the TTL (S4) cleans up the gap.

## Success criteria (S6)

Each of these must end up test-proven:

- **S6a** — a test envelope published on the scratch hub appears as a
  toast within seconds.
- **S6b** — a redelivered message (at-least-once delivery) produces no
  duplicate toast: dedup on the envelope id.
- **S6c** — consumer down longer than the TTL → backlog expires,
  recorded at the hub side; on return, no pile of stale toasts.
- **S6d** — hub unreachable → the service does not crash, keeps
  reconnecting calmly, logs it visibly; when the hub returns, toasts
  flow again.

## Contract (S7)

- Incoming message format: **envelope v1 DRAFT** from the study §7
  (JSON: `v`, `id`, `ts`, `source`, `kind`, `audience`, `priority`,
  `title`/`message`/`tts` each carrying both `nl` and `en`, `ack_id`,
  `click_url`, `data`).
- The **pipeline-v2 project owns the final envelope schema** (its
  Phase 4). A schema change there is a **mini-round trigger** here,
  never a silent adjustment.
- Toward the hub, `~/Projects/mailbox/docs/USER_GUIDE.md` is the
  interface authority (K2 receive, K3 ack, K7 policy, W2 tokens).

## Non-goals

- **Vault-courier is not built now (S8).** No code, no half work. It
  would come later through its own gate, expected in this repo. Kenny's
  note on the gate form (2026-08-28): *he is not yet sure it will come
  at all — the usefulness is still unproven to him.* Design nothing
  speculative for it; shared code is factored for desk-courier's needs
  only.
- **No routing or audience logic in the client (S9).** desk-courier
  renders what arrives on its subscription and decides nothing about
  who gets what. Routing is upstream: the topic name is the address
  (`notify.kenny` ≠ `notify.parents`), the HA dispatcher picks the
  topic, and the hub itself never routes or transforms (mailbox N5).
- **Never a critical path (S10).** desk-courier is an *extra* channel,
  never a link something else waits on. Hub down or courier dead = no
  toasts, and nothing else changes: push, TTS and the todo fallback
  work exactly as today. This is the study's degradation doctrine ("the
  hub may enrich critical paths, never gate them") and must survive
  verbatim into the Phase 4 freeze.

## Hard constraints (S11)

- **Platform:** this one Garuda PC (10.10.10.10), inside the desktop
  session. (Programming language is deliberately not a scope item —
  Phase 3 decides.)
- **Hub access:** own app token (Bearer), minted on the hub's `/apps`
  page. How the binary obtains its token (latch or otherwise) is the
  Phase 2 ecosystem item.
- **Scratch hub:** development runs against a scratch hub (local
  mailbox binary or docker compose on the PC). The live hub on LXC 109
  (`http://10.10.10.9:8080`) is only touched as an explicitly agreed
  step.
- **Test envelopes:** until pipeline-v2's shadow-publish (6a) is live,
  this project publishes its own test envelopes to the scratch hub —
  sufficient for all phases through hardening.

## Build vs buy (Phase 1 record)

Greenfield round, 2026-08-28 (AFK — recommendation built, ratification
queued in `docs/AFK_QUEUE.md` R1):

- **ntfy + its desktop client** — rejected. Ties the house to a second
  hub; mailbox's own Phase 1 already rejected ntfy-style dumb push from
  the other side. The hub of record is mailbox (ECOSYSTEM.md).
- **Shell script** (`curl` + `jq` + `notify-send` loop under systemd) —
  the honest cheap option: ~30 lines, zero build. Rejected because it
  reproduces the house's documented failure class: untyped parsing that
  drifts (three silent format bugs in one month, study §3), no dedup,
  no tests, no policy bootstrap, no gates.
- **Own small binary** — **chosen**. Typed envelope parsing pinned by
  regression vectors, testable, dedup, and one home for the hub-client
  code a later vault-courier would reuse.

Ecosystem consult (Phase 1 mandatory): **mailbox** is the counterparty
by definition; **latch** is the token-injection candidate (Phase 2 item
M6); **homelab** does not apply (this runs on the PC, not an LXC).
