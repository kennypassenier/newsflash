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

## 2026-08-30 · Deployment + field test (Phase 9)

*(Logged at the time as "dogfood"; the procedure renamed that step to
"field test" on 2026-08-30. Same step, clearer name.)*

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
- Field test (Phase 9 rule): `latch run -- newsflash send-test` →
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
- Field test repeated under the new binary name: `newsflash send-test` →
  `rendered 01M17WYFNS48Y7XENT7WV86V1X` — a real toast, new binary,
  same live hub.
- Read-back (rule 13a habit, applied to a local system too): every old
  artifact (`~/.cargo/bin/desk-courier`, `~/.config/desk-courier/`,
  `~/.local/state/desk-courier/`, the old unit file) confirmed absent
  after cleanup — zero stragglers.

## 2026-08-30 · M10 interactive action buttons (pipeline-v2 K12)

Empirical checks BEFORE building (FORM_PROTOCOL §5.6 — test the
proposal against reality before freezing it), all on the real desktop:

- `notify-send --expire-time=2000 -A gelezen=Gelezen -A snooze=Snooze`
  blocked for exactly 2.007s, empty stdout, exit 0 — confirms `-A`
  implies `--wait` and still honours `--expire-time`.
- `--expire-time=0` with `-A` blocks **forever** (only returned when
  killed by an external `timeout 3`) — confirms critical toasts with
  buttons never self-resolve, driving AR23/AR24's design (ack must not
  wait for the interaction).
- **Live-drill discovery mid-build (Kenny):** after that killed test
  process exited, the leftover critical toast stayed visible on screen
  but its action buttons were gone — once the client dies, Plasma
  drops the buttons even though the notification body persists. This
  directly shaped AR25: the interactive watcher's safety-cap timeout
  must never apply to a persistent (critical) toast, or the buttons
  would die silently long before Kenny answers.

Full interactive lifecycle drilled on a scratch hub
(`127.0.0.1:18940`), real `newsflash` binary, real KDE Plasma 6.7.4:

1. **Default buttons, no click (`info`, 10s):** toast "M10 drill"
   rendered with "Gelezen"/"Snooze"; journal recorded
   `toast dismissed or timed out, no action chosen` after the 10s
   window — the timeout path, live, correct.
2. **Custom single action, no click:** a hand-published envelope with
   `actions:[{"id":"ik_pak_het_op","label":{"nl":"Ik pak het op"}}]`
   rendered with that one custom button in place of the defaults;
   same timeout path confirmed.
3. **A real click, critical priority:** toast "M10 klik-test" sent at
   `critical` (persists until dismissed) specifically so Kenny had
   unlimited time; he clicked **"Gelezen"**. Journal:
   `action "gelezen" chosen` →
   `action_result 01M19G0G9MVV63HE91WNDT5Q82 published to notify.actions`.
   Read back from the hub, the envelope matched AR26's shape exactly:
   `{"v":1,"id":"action-...","kind":"action_result","source":"newsflash",
   "data":{"original_envelope_id":"test-...","action_id":"gelezen"}}`.

This is the load-bearing evidence for K12's test bar: a real button, a
real human click, a real reply envelope, on the real hub contract.

Loop-level (mock hub + real binary, `loop_tests.rs`): a new test
scripts a clicking `notify-send` shim and asserts, on the real binary,
that (a) the message is acked **before** the click is even known about
(AR24 — proves settlement never waits on the interaction) and (b) the
resulting POST to `notify.actions` carries the exact original payload
id and chosen action id. Closes the one gap the manual desktop drill
above cannot: proof that `run.rs` wires the pieces together correctly,
not just that each piece works in isolation.

## 2026-08-30 · Stress test — 300-message flood, button-count ladder, the Plasma popup-limit discovery

Kenny's own words framing this session: "kijk...wat er gebeurt." All on
the scratch hub (`127.0.0.1:18941` this time — a second scratch
instance, same rule as always: never the live hub).

**300-message flood (S6e evidence + first sighting of the popup
limit).** 300 envelopes published back-to-back, mixed priority
(50% info / 35% warning / 15% critical, 10 with 3-50 actions to stress
AR27's truncation under load). All 300 rendered, 0 crashes, thread
count peaked at 77 concurrent watchers and settled back to 1, memory
flat at ~5MB RSS throughout. `actions_are_truncated` fired exactly 10
times — one per extreme envelope, matching AR27 exactly even at this
volume. This run is also where the "knoppen verdwijnen bij een stapel"
phenomenon was first spotted (Kenny: he saw buttons on ~4 toasts, none
on the rest) — everything below traces that down.

**Button-count ladder (2/4/6/8/12/20 real actions, bypassing
newsflash's own 2-action cap via raw `notify-send`).** All rendered,
all clickable, up to 20 — no hard button-count ceiling in Plasma
itself. Past roughly 6-8 buttons, labels truncate to an unreadable
ellipsis ("Kno..." instead of "Knop 1") — a legibility ceiling, not a
technical one. Basis for the D3 requirement filed with pipeline-v2
(see below).

**Isolating "knoppen verdwijnen": three controlled tests, one real
mechanism traced to source.**
1. A single isolated `critical` toast, and separately 8 simultaneous
   `critical` toasts (`Kritiek-burst #1`-`#8`) — buttons held in every
   case, no matter how many or how long.
2. A single isolated `normal`-urgency toast — buttons held the full
   30s, unprompted, no stacking. Ruled out a flat Plasma popup-timeout
   as the cause (a live guess, tested and rejected the same way R6/H14
   were: against reality, not just plausibility).
3. `Cap-test #1-5` (5 simultaneous `normal` toasts, 1s apart): the
   first 4 held buttons the full 45s, the 5th lost them within
   seconds of appearing. This pinned the number: **4 simultaneous**.

Read KDE's own source for the exact mechanism rather than guess further
(`plasma-workspace`, `applets/notifications/global/Globals.qml`, via
GitHub) — findings and the accepted-limitation decision are AR28 in
`docs/ARCHITECTURE_DECISIONS.md`. Also checked and ruled out as a
config knob: `~/.config/plasmanotifyrc` (nothing relevant), the
Notifications KCM (`kcm_notifications.so` `strings`-searched for any
max-popup wording — none).

**The AR11 icon pivot — a wrong hypothesis, caught and corrected in
one session.** First response to "info and warning look identical" was
mapping `info`→`low` urgency, on the assumption Plasma styles urgency
levels differently. Built it, live-tested it (two isolated toasts, one
`low` one `normal`) — Kenny: "nope, lijken nog altijd hetzelfde."
Confirmed by reading Plasma's popup QML component tree directly: no
urgency-conditional styling anywhere. Reverted the urgency split same
session, replaced with a `--icon` per priority
(`dialog-information`/`dialog-warning`/`dialog-error`) — live-confirmed
this actually renders distinctly, both via raw `notify-send` and
through the rebuilt real `newsflash` binary against the scratch hub.

**The reflow-on-close finding (the deepest one, Kenny's own
observation).** With icons in place, re-ran the 3-priority scenario
twice:
- Sent near-simultaneously (courier draining a 3-message backlog at
  its own pace, spawns roughly 300-800ms apart): 2 of 3 (`info`,
  `warning`) lost buttons quickly — well under the count-4 limit,
  contradicting the count/height explanation alone.
- Same 3 messages, this time spaced 1s apart (matching the successful
  `Cap-test` spacing): all 3 held their buttons. Confirms spawn timing,
  not just simultaneous count, matters.
- Mid-hold, Kenny dismissed the `critical` toast by clicking one of its
  buttons. The instant it closed, **both remaining toasts — which had
  already been stably showing buttons for tens of seconds — lost
  theirs.** Nothing about their own state changed; only the popup stack
  around them did. This is `positionPopups()` re-running on every
  popup-set change (not just newsflash's own toasts closing) and
  re-applying the height-fill visibility check to whoever remains —
  documented as the third mechanism in AR28.

**Outcome:** accepted as a known, external Plasma limitation (Kenny,
2026-08-30) — see AR28 and SCOPE S6f. Standing guidance: only
`critical` carries a reliable, whole-lifetime guarantee that action
buttons stay answerable. Two requirements filed with pipeline-v2 from
this session — per-message duration override (D1) and a richer
default action set given the 20-button headroom found above (D3) — see
the Obsidian vault doc `Notification Pipeline V2 Duration Override And
Richer Actions Requirement.md`.
