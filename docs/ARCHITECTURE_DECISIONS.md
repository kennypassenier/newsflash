# Architecture decisions — hub-clients (newsflash)

Phases 3+4 output, drafted 2026-08-29 during the AFK build. The draft
was attacked by the architecture-critic agent (fresh context) on
2026-08-29; its three BLOCKING findings are incorporated (AR5/AR6 id
choice, AR7 rewrite, AR19 timeouts), its SERIOUS findings are adopted
where marked, and surviving objections stand as ⚔ counter-arguments.
Kenny's ratification is queued (`docs/AFK_QUEUE.md` R3). Changes after
ratification go through mini-rounds only.

## Phase 3 — tech choice

### AR1 · Language & toolchain: Rust, pinned 1.97.1
Rust — the house language for hub consumers (kyu, latch, homelab
are all Rust). Toolchain pinned in `rust-toolchain.toml` at **1.97.1**
(the version installed on the PC) and CI asks for exactly that
(standing rule 7 — the kyu 1.97-vs-1.98 clippy drift is the
precedent). Edition 2024.

### AR2 · Dependency policy: reluctant, no async runtime
A single-subscription long-poll loop is sequential by nature; an async
runtime buys nothing. Blocking HTTP via **ureq**, JSON via
**serde/serde_json**, config via **toml**, signals via **signal-hook**.
No tokio, no reqwest, no anyhow/thiserror (hand-rolled error enums in a
codebase this size). Plain HTTP only — the hub is LAN-only by design
(kyu N3), so no TLS stack ships at all. A blocking stack lives or
dies by its timeouts: **AR19 is part of this decision**, not an
implementation detail.

### AR3 · Workspace layout: core/shell split
Cargo workspace, two crates:
- `courier-core` — pure logic, **zero ambient I/O**: hub-response and
  envelope parsing, the settle table (including its *timing* rows —
  staleness is data in core, per the critic), dedup bookkeeping, toast
  mapping, backoff schedule.
- `newsflash` — the binary shell: HTTP, subprocesses, signals,
  filesystem, clock.
The gate and CI enforce the boundary mechanically (dependency list +
I/O-import grep). A future vault-courier would be a third crate reusing
`courier-core` — but nothing is factored for it in advance (SCOPE S8:
its existence is uncertain).

## Phase 4 — architecture

### AR4 · Envelope v1 draft, pinned tolerant-reader, bounded
The study §7 draft is a serde struct with a pinned regression vector
(`courier-core/tests/vectors/`). Reading is **tolerant**: unknown
fields ignored (pipeline-v2 owns the final schema — forward
compatibility is survival), missing optional fields default, unknown
`priority` reads as `info`. Hard requirements: `v == 1`, a non-empty
`id`, at least one renderable text — anything else is poison (AR5).
The poison log line distinguishes "no `v` field" (almost certainly a
hand-typed test publish from the hub dashboard's W9 box) from a wrong
version. **Size budget (critic):** payloads over 256 KiB are poison
without parsing (a toast never needs that; an unbounded `data` blob
must not ride into `notify-send` argv), and rendered text is truncated
before spawn — summary to 200 chars, body to 1000 (AR11). A
pipeline-v2 schema change is a mini-round trigger (SCOPE S7).

### AR5 · At-least-once handling: the settle table, keyed by hub id
**The settle/dedup key is the HUB message id** (`kyu-id` /
envelope-response `id`) — never the payload envelope's `id` (critic,
blocking): redelivery is a hub-level event reusing the hub id; a
poison payload has no parseable id at all yet must still be nacked by
hub id; and a producer republishing the same logical event as a new
hub message must still toast (suppressing intentional re-sends is the
lost-toast direction). The payload id appears in logs for tracing only.
*(This sharpens SCOPE S6b's "dedup on the envelope id" — flagged for
the ratification round.)*

| Case | Action |
|---|---|
| Parse OK, hub id unseen, not stale, `notify-send` exit 0 | mark seen → **ack** |
| Hub id already seen (redelivery after a crash between mark and ack) | **ack** without rendering (S6b) |
| Parse OK but stale — `published_at` + TTL < now (client-side check; the hub can only expire *unclaimed* messages, so a message claimed just before suspend would otherwise render hours late) | **ack** without rendering, logged as expired client-side |
| `notify-send` non-zero exit, spawn failure, or child timeout (AR19) | **nack** — transient, hub redelivers with backoff |
| Payload > 256 KiB, unparseable JSON, base64 (binary) payload, `v != 1`, missing id, nothing to render | **nack `dead=true`** — poison, visible in dead letters (M9) |
| Ack/nack itself rejected with 4xx (lease lost after suspend, message expired/settled meanwhile) | log, **treat as settled**, move on — never retry-loop a settle call |
| Poll answers 204 (empty window) | normal flow, not an error |

Order on success: render → mark seen → ack. A crash between mark and
ack redelivers a marked id → acked silently. A crash between render
and mark → worst case one duplicate toast — the accepted direction (a
toast twice beats a toast never). The critic attacked every crash
window of this ordering and it survived unchanged.

### AR6 · Dedup store: bounded, atomic, fail-open
Last 512 **hub message ids** in
`$XDG_STATE_HOME/newsflash/seen.json` (fallback `~/.local/state/…`),
written atomically (temp + rename, standing rule 12). Missing or
corrupt file → start empty and say so in the log: fail-open, because
an empty seen-set costs at worst a duplicate toast, never a lost one.
512 ≫ any 10-minute backlog; a `from=beginning` replay can churn the
bound, whose failure direction is again duplicates — accepted.
⚔ **Counter-argument (critic, embedded):** a render-then-crash bug
under systemd restart amplifies `critical` toasts (persistent, AR11)
into a wall of duplicates. Accepted direction, mitigated by AR20's
`StartLimitBurst` — the unit stops the loop after 5 rapid failures.

### AR7 · Policy bootstrap: own one field, assert on every reconnect
*(Rewritten after the critic refuted the "write all fields" draft: hub
K7's replace-semantics means omitted fields revert to defaults — the
guide's own example PUTs a subset. Freezing read-back defaults as
explicit values would pin stale hub defaults forever and falsify the
dashboard's "explicitly set" display.)*

The courier owns exactly one policy field: it PUTs
**`{"ttl_ms": 600000}`** (config default, S4) and nothing else, so
lease/attempts/backoff track the hub's defaults forever. Startup order
honors the first-poll trap (hub K2: a subscription starts existing
when it first polls; drill-proven 2026-08-29: a policy PUT on an
unpolled subscription answers 404): **poll once → then assert the
policy** —
asserting means GET, compare the *explicitly-set* field set against
`{ttl_ms}`, PUT only on difference, and log when the diff overrode a
human's dashboard edit (the policy is code; the override is intended
and must be visible). Bootstrap failure is **transient** (backoff and
retry inside the loop — never a startup exit; S6d holds at the exact
moment systemd watches exit codes). The policy is re-asserted on
**every reconnect** (AR9's recovered transition), so a hub restored
from a backup predating the policy cannot silently shed the TTL and
rebuild the stale-toast pile S4 forbids.

**Cold start (drill-found, 2026-08-29):** a fresh subscription only
sees what is published after its first poll — the L2 drill proved a
message published before that poll is invisible to it. The first poll
of every run (and the first after an unarchive) therefore adds
`from=beginning`: replay is idempotent and bounded (hub K8), dedup
absorbs the seen, the staleness check acks the old. Steady-state cost:
zero. *(Flagged for ratification: an addition after the critic round.)*

### AR8 · Error model
`courier-core`: hand-rolled error enums, every variant carrying a
remedy (standing rule 11). Shell: errors bubble to the main loop,
which classifies **transient** (retry per AR9), **auth** (distinct —
see AR9), or **fatal-config** (exit non-zero with remedy; the only
exiting class). Runtime trouble never kills the process (S6d).

### AR9 · Backoff: linear, capped, classified
Linear 2 s per step, capped at 60 s, one log line per state change.
Three named error classes, never merged (critic: a revoked token must
not impersonate designed degradation):
- **unreachable/5xx** — "hub down", backoff, resume silently on
  recovery; re-assert policy (AR7).
- **401/403** — "auth rejected", same backoff (a re-minted token
  arrives via latch + unit restart; exiting would just make systemd
  loop), but a *distinct* state and log line carrying the remedy:
  "re-mint at /apps, restart the unit". Nothing silent.
- **409 archived** — see AR21.
- **404 topic-missing** — the topic has never been published to
  (drill-proven on a fresh hub): a normal pre-pipeline-v2 state, its
  own quiet "waiting" log line, plain backoff.

### AR10 · Token handling: environment first, never inline
Token source order: `KYU_TOKEN` env var (what `latch run` injects
— M6), else a `token_file` path from config (0600, checked once at
startup — the TOCTOU race is deliberately not hardened further: a
racer on this single-admin machine already owns the account). Present
but empty = fatal-config with remedy, same as absent-with-no-file. An
inline `token = "…"` key in config is **refused at startup with a
remedy** (standing rule 10). The token never appears in logs, argv or
error messages; a plaintext-scan test asserts it.
⚔ **Counter-argument (critic, embedded):** "latch run works under a
systemd user unit, non-interactively, at login" is an assumption on
the startup-critical path that paper cannot close — it is an explicit
drill (AR18), and the documented fallback is a plain `EnvironmentFile`
(M6).

### AR11 · Toast mapping
| Priority | Urgency | Expire |
|---|---|---|
| `info` | normal | 10 s |
| `warning` | normal | 30 s |
| `critical` | critical | 0 (persistent until dismissed) |

App-name `newsflash`; title/message in the configured language with
cross-language fallback (M3); summary truncated to 200 chars, body to
1000 (AR4). **Recorded choice (critic):** `critical` urgency bypasses
Plasma's Do-Not-Disturb — wanted for `notify.kenny` (the doorbell
beats the screen-share), chosen, not accidental. Sound (K6): only when
`sound_file` is configured; spawned after a successful toast on a
detached thread; player failure is logged and never fails the message;
at SIGTERM the thread may die mid-chime — harmless, do not "fix" it
with a join that delays shutdown (AR14).

### AR12 · Subprocess contract, no D-Bus library
`notify-send` and `paplay` resolved via `PATH`; tests inject fakes by
prepending to `PATH`. No zbus/notify-rust: the daemon is whatever the
session runs (KDE Plasma today) and the CLI is the stable seam.
Hygiene (critic): argv always passes `--` before positionals (a body
starting with `-` is data, not an option), and the body is
markup-escaped (`&`, `<`, `>`) because Plasma parses a markup subset —
pinned by tests. Every child runs under a **10 s hard timeout** (AR19);
a timed-out child is killed and settles as the transient row (AR5) —
a D-Bus stall must never freeze the loop.
⚔ **Counter-argument (critic, embedded):** subprocess exit codes are
coarser than D-Bus errors, and exit 0 does not strictly prove a
rendered toast — accepted; the real-desktop drill covers the last inch
and the simplicity/testability trade is worth it.

### AR13 · Concurrency: one loop
Single-threaded main loop (poll → settle → repeat); the detached sound
thread (AR11) is the only other thread. No shared state beyond the
dedup store, owned by the loop.

### AR14 · Graceful shutdown
SIGTERM/SIGINT set a flag (signal-hook); the loop finishes settling
the in-flight message, persists the dedup store, exits 0. A second
signal aborts immediately. With AR19's timeouts, "finish settling" is
bounded — `systemctl --user stop` never hangs into its stop timeout
(AR20). A hard kill costs at most one duplicate toast (AR5).

### AR15 · No self-update (M5 decision)
Single-machine tool, updated by `git pull` + `cargo build --release` +
`systemctl --user restart` as a numbered runbook procedure.
`--version` reports the Cargo version. No update authenticity model —
the distribution channel is the local git checkout.

### AR16 · Degradation doctrine (frozen sentence)
The courier is an extra channel, never a gate. Every failure mode
degrades to exactly "no toasts": hub down, courier dead, daemon
absent, logged out. Nothing upstream waits on it; no existing channel
changes behaviour (SCOPE S10, study §7 doctrine, verbatim). The
critic's corollary is embedded across AR9/AR21/AR22: the doctrine
makes *silent* failures look like *designed* degradation, which is why
every failure class gets its own named, logged state. Honesty note
(critic): S4's "expiry recorded" means published to `kyu.events`,
which nothing consumes until hub-bridge P8 ships — recorded, but only
dashboard-visible today.

### AR17 · Config
`~/.config/newsflash/config.toml` (XDG), full example committed as
`config.example.toml` (no secrets — AR10). Validated at startup;
every rejection names the field and the remedy (M2). Timeouts (AR19)
and the truncation budget (AR4) have documented defaults here, not
knobs.

### AR18 · Testing architecture (what each fake cannot express)
- **PATH-shim fakes** for `notify-send`/`paplay` prove argv, exit and
  timeout handling. *Cannot express:* whether a real daemon displays
  anything — covered by a live desktop drill per relevant milestone.
- **Mock hub** proves request shapes and error branches. Its
  `envelope=json` body is **not invented**: the mock serves a pinned
  vector captured from the real kyu binary during the L2 drill
  (critic: the guide never documents that shape; a guessed mock would
  green-light fiction for months). *Cannot express:* real
  lease/redelivery/TTL/dead-letter/archive semantics — covered by
  live tests (`#[ignore]`, run by `scripts/drill.sh`) against the real
  kyu binary (`KYU_BIN`, default
  `~/Projects/kyu/target/release/kyu`) on a scratch data dir
  and port. CI runs the mock suite; drills run locally and their
  output is milestone-report evidence.
- **Named drills no fake can express:** latch-under-systemd (AR10),
  the login race / daemon-absent hold (AR22).

### AR19 · Timeouts (critic, blocking — the difference between
"degrades to no toasts" and "silently frozen until a manual restart")
- Poll wait window: **20 s** (`wait=20` — shorter than the hub's 30 s
  default so a SIGTERM mid-poll settles well inside AR20's stop
  timeout; idle cost: 3 polls/min).
- HTTP connect: **5 s**.
- HTTP read: **poll wait + 10 s = 30 s**; non-poll calls 10 s. A
  zombie TCP connection after suspend/resume or a hub-host power loss
  self-heals in ≤ ~30 s via the normal AR9 path — no special resume
  code.
- Subprocess: **10 s** kill-and-settle-transient (AR12).

### AR20 · systemd unit topology (critic: the unit file is
architecture, not packaging)
- `After=graphical-session.target` + `PartOf=graphical-session.target`
  — starts after the session (and its notification service) is up;
  **stops at logout**, so the S4/S5 story holds: no session-less
  courier acking toasts nobody sees, and the TTL cleans the gap.
- `Restart=on-failure`, `StartLimitIntervalSec=120`,
  `StartLimitBurst=5` — restarts survive crashes; a crash loop stops
  before AR6's duplicate-amplification wallpapers the screen.
- `TimeoutStopSec=45` — bounded stop, comfortably above AR19's worst
  case (a 30 s poll read plus a settle).

### AR21 · Archived subscription: auto-unarchive, loudly
Hub K11: idle 7 days → flagged, 30 days → archived; polling never
revives an archive, and an archived poll answers **409** with a
remedy. Without handling, a PC off for five weeks comes back to
permanent, silent no-toasts — the worst failure class this project
recognizes (critic, M-B). On 409-archived the courier POSTs
`/api/t/notify.kenny/subs/desktop/unarchive` itself and logs loudly
(the lapsed backlog is disposable *by design* — 10-minute TTL means
nothing of value lapsed). Auto-unarchive is the option that cannot be
mis-tuned; a per-subscription idle override was considered and
rejected (it trades one silent state for another).

### AR22 · Notification-daemon probe: hold, don't consume
`notify-send` failing because no notification service is on the bus
yet (login race) or anymore must not burn delivery attempts —
consume-and-nack ×5 dead-letters real, renderable messages (critic).
Before the first poll and after any render failure, the shell probes
the daemon (`busctl --user call … GetServerInformation` — a query, not
a notification; busctl over gdbus because systemd is guaranteed on the
target platform); while the probe fails, the courier **holds** (no
polling — messages wait at the hub under the TTL, which is exactly
the designed semantics) on the AR9 schedule with its own named state.
AR20's `After=graphical-session.target` makes this a rare path, not
the normal startup.

## Freeze

**Frozen 2026-08-29** — built after the architecture-critic round
(BLOCKING findings incorporated, SERIOUS findings adopted as marked,
⚔ objections embedded) and ratified by Kenny the same day
(ratification form F7, with the S6b id-sharpening as F8 and the
cold-start amendment as F9, all Akkoord). Changes from here go through
mini-rounds only.

## Amendment 2026-08-30 — M10 interactive action buttons (pipeline-v2 K12)

Kenny asked for interactive action buttons (read-confirmation, a PC-first
freezer flow); pipeline-v2 owns the envelope contract and approved it
(their K12): optional `actions` field (max 2), default "gelezen"/"snooze"
pair when absent, reply as an `action_result` envelope on
`notify.actions`, client stays dumb (SCOPE S9 intact — no timeout/path
logic here).

### AR23 · `--action` forces a blocking wait — the render call splits in two
Empirically verified 2026-08-30 (`notify-send --expire-time=2000 -A …`
returns in exactly 2.007s; `--expire-time=0` blocks forever, confirmed
by `timeout 3` killing it): `-A`/`--action` implies `--wait`, so a toast
with buttons no longer returns instantly like a plain toast did. Every
ordinary toast now carries buttons (K12), so this affects all of them,
not a subset. Blocking the main poll loop on that would freeze message
consumption for the whole toast duration — **forever** for a critical
toast (`expire_ms == 0`). This amends AR13 (single loop): rendering an
interactive toast is now two steps, `show_toast_interactive` (spawns,
confirms no instant failure within a short grace period, returns) and
`watch_interactive_toast` (detached thread, waits out the full
interactive session, reports the click). The settle table (AR5) is
unaffected in shape: "delivered" still means "shown", not "answered" —
only the *evidence* for "shown" changed, from "process exited 0" to
"process spawned without instantly failing". See `courier_core::settle`
for the exact note on `PostRender`'s continued validity.

### AR24 · Ack timing: decoupled from the click, on purpose
The message is marked seen and acked as soon as `show_toast_interactive`
confirms the toast is showing — **not** when (or whether) Kenny answers.
Reasoning, stated hard: the hub's default lease is 30s, matching
`warning`'s own 10s-margin-short-of-that toast duration; waiting for an
answer before acking would race the lease on every bounded toast and
would **guarantee** redelivery-while-displayed on every `critical`
action toast (nobody answers within 30s consistently), duplicating the
exact K4/S6b "exactly one render" guarantee this project spent Phase 7
proving. Click handling therefore happens entirely after settlement, on
its own detached thread — fire-and-forget from the loop's perspective,
identical in spirit to the existing sound thread (AR11).

### AR25 · The interactive watcher's safety cap: bounded toasts only, never critical
**Live-drill finding (Kenny, 2026-08-30):** once the notify-send process
that registered a toast's buttons exits — including from being killed by
a timeout — Plasma drops the buttons from the still-visible notification.
A flat safety-cap timeout on the watcher thread would therefore silently
kill a critical toast's buttons long before anyone gets to click,
breaking Kenny's own explicit requirement that critical persists until
dismissed. Decided: `courier_core::toast::interactive_wait_cap_ms`
returns `None` (no cap at all) when `expire_ms == 0`; for a bounded
toast it returns `expire_ms` plus a margin
(`interactive_wait_margin_ms`, config default 30000 — standing rule
27: the margin is the operational knob, the no-cap branch for
persistent toasts is a pinned decision). Proven live: the drill toast
sent at `critical` priority sat on screen well past its would-be 30s
cap with buttons intact (see `docs/DRILL_LOG.md`).

### AR26 · The action_result envelope shape
No formal schema exists beyond pipeline-v2's prose, so newsflash
defines the minimal shape and documents it here (mirrors how the v1
envelope itself carries kind-specific fields under `data`):
```json
{"v":1,"id":"<fresh>","kind":"action_result","source":"newsflash",
 "ack_id":"<original ack_id, omitted if absent>",
 "data":{"original_envelope_id":"<original env.id>","action_id":"<clicked id>"}}
```
Published via `HubClient::publish_to` (generalised from the previous
subscribe-topic-only `publish`) to the pinned constant
`courier_core::action_result::ACTIONS_TOPIC` (`notify.actions` —
standing rule 27: a contract value, not configurable). `HubClient`
derives `Clone` (cheap — `ureq::Agent` is `Arc`-backed) so the detached
watcher thread gets its own handle rather than sharing one across the
thread boundary.

### AR27 · More than 2 actions: truncate and log, never poison
A producer sending 3+ actions is a minor upstream bug, not an
unrenderable envelope (AR4's poison bar is "cannot be understood at
all") — `courier_core::toast::resolve_actions` keeps the first 2 and
`actions_are_truncated` lets the shell log a warning (M11 pattern,
same direction as the existing unknown-priority warning).

## Amendment 2026-08-30 — icons, and a hard Plasma popup-limit (stress test)

Kenny stress-tested M10 with a 300-message flood, a knob-count ladder
(2 through 20 buttons), and a series of controlled multi-toast drills.
Two outcomes: a small AR11 revision (icons), and a significant,
externally-imposed limitation on non-critical toasts that this project
cannot fix and now documents instead.

### AR11 revision · Icons are the real visual differentiator, not urgency
First attempt: split `info`→`low` urgency from `warning`→`normal`,
hypothesising Plasma renders different urgencies with different
styling. **Wrong, and reverted the same session** — a direct read of
Plasma's own QML source (`plasma-workspace`,
`applets/notifications/`) turned up no urgency-conditional styling
anywhere in the popup component tree; urgency only changes *behaviour*
(persistence, DND bypass, sort order), never appearance. Live-confirmed
against Kenny's own desktop: an isolated `low`-urgency toast and an
isolated `normal`-urgency toast looked identical.
What actually works, live-confirmed the same way: a **freedesktop icon
per priority** (`--icon=dialog-information` / `dialog-warning` /
`dialog-error`, standard names shipped by every icon theme) — Plasma
always renders the icon regardless of urgency. `ToastSpec` gained an
`icon: &'static str` field; `courier_core::toast::icon_for` is a plain
priority match, `render::build_toast_command` adds `--icon=<icon>`
right after `--expire-time`. Kept the `Urgency` enum exactly as AR11
originally defined it (`info`/`warning` both `normal`) — the low/normal
split bought nothing and reverting it removed the only speculative,
untested piece of the AR11 table.

### AR28 · The Plasma popup stack has a hard, non-configurable ceiling — critical is exempt, nothing else reliably is
Traced to KDE Plasma's own source
(`plasma-workspace/applets/notifications/global/Globals.qml`, read via
GitHub during the drill, not guessed) — **three separate mechanisms**,
each confirmed live on Kenny's desktop, that can strip a non-critical
toast's action buttons while the notification body stays visible:

1. **Count limit** (`Globals.qml` ~L430): the popup model's `limit` is
   `Math.ceil(availableScreenHeight / (Kirigami.Units.gridUnit × 4))`
   — computed from live screen geometry and Plasma's global UI scale,
   not a setting. Measured at **4** on Kenny's desktop across two
   independent tests (`Cap-test #1-5`, `AppTest #1-8`).
2. **Height fill** (`Globals.qml` ~L192, ~L415-421): even under that
   count, `positionPopups()` hides (not destroys — `popup.visible =
   false`, buttons and all) any popup whose stacked position would
   exceed `popupMaximumScreenFill` (hardcoded `readonly`, `0.8` — 80%
   of screen height) of cumulative popup height, except the topmost
   (`i === 0`) which is always shown. Explains why even 2-3 toasts can
   lose buttons when each is taller (icon + 2 action buttons make every
   M10 toast taller than a plain one).
3. **Reflow-on-close** (drill-found, not in the two mechanisms above —
   Kenny, 2026-08-30): `positionPopups()` re-runs whenever the popup
   set changes, including when ANY popup closes — not just newsflash's
   own. Live-observed: two non-critical toasts held their buttons
   stably for tens of seconds while a third (critical) toast stayed
   open; the moment Kenny dismissed the critical one, the *remaining*
   two immediately lost their buttons, despite nothing about their own
   state having changed. Removing a popup from the stack retroactively
   re-evaluates and can demote toasts that had already been stable —
   the risk to a non-critical toast's buttons is not fixed at spawn
   time, it persists for the toast's entire visible lifetime.

`sortMode: NotificationManager.Notifications.SortByTypeAndUrgency`
(`Globals.qml` ~L440) is why `critical` is exempt from all three:
critical-urgency notifications always sort first, landing in the
slot(s) mechanisms 1 and 2 never hide, and — separately confirmed live,
8 simultaneous critical toasts, all kept their buttons — in practice
also the slot mechanism 3 never demotes. None of the three mechanisms
read from `plasmanotifyrc` or appear in the Notifications KCM
(`kcm_notifications.so` was `strings`-searched for any max-popup wording
— nothing); they are fixed in Plasma's QML, changeable in practice only
by shrinking `Kirigami.Units.gridUnit` — Kenny's entire desktop's UI
scale, a wildly disproportionate trade for newsflash's popup count.

**Decision:** accepted as a known, external limitation (Kenny,
2026-08-30) — the only alternative is newsflash building its own
notification queue/reordering, which is exactly the client-side
decision logic SCOPE S9 rules out. **Standing guidance, adopted the
same day:** a message whose action button must reliably remain
answerable belongs on the `critical` path — no other priority carries
that guarantee, and the guarantee is not just "at spawn time," it holds
for the toast's entire displayed lifetime regardless of what else opens
or closes around it. See `docs/DRILL_LOG.md` for the full live-drill
trail (the 300-message flood, the button-count ladder, and the
three-mechanism isolation tests) and `docs/SCOPE.md` S6f for the
success-criteria-facing version of this note.
