# Proposals: one cohesive notification system

Written 2026-09-24, from what is actually on the hub: 16,774 real
messages on `notify.kenny` (28 Aug → 9 Sep 2026), read through a
temporary subscription `claude-peek` (the `desktop` subscription was
not touched), plus kyu's own CHANGELOG. Kenny's standing: *nothing is
set in stone*. When a field is awkward for the desktop, the fix is
proposed where the field is made (pipeline-v2, HA, kyu), not worked
around here.

Each proposal says what newsflash does **today** (built, both
desktops) and what would make the whole chain cleaner.

## What real messages carry

| field | in messages | meaning (as observed) | newsflash today |
|---|---|---|---|
| `v`, `id`, `ts`, `source`, `kind`, `audience`, `priority`, `title`, `message`, `ack_id` | all | the v1 draft | used as before |
| `click_url` | all; mostly `""`, else `/control-panel/…` or a full `http://10.10.10.6:8989/…` | link to open | **opened on click**. Paths resolve against `link_base_url` (Home Assistant) |
| `ephemeral` | all; `true` on 35 (the "Beweging / Vijf minuten buiten" reminders) | short-lived | **removed after `ephemeral_minutes`** (default 10): from the screen and from notification history |
| `gate_outcome` | all; `deferred` 10,346 · `live` 6,423 | whether a gate (quiet hours?) held it back | not used (see ask-5) |
| `tts` | all | speech text | not used: the speaker channel's job |
| `data.action_buttons` | all; non-empty once (plant care, 3 buttons) | the buttons, in the HA companion-app shape `{action, title}` | **shown as buttons**, and a click publishes `action_result` with `action_id` = the `action` string |
| `data.notification_types` | all; `["push"]` 16,619 · `["push","tts"]` 84 · `["tts","glow"]` 35 · `["color","tts"]` 28 | which channels | not used (see ask-4) |
| `data.color`, `light_*`, `parents_speaker`, `speak_to_parents`, `push_targets`, `tts_target`, `allow_duplicates`, `flip_active_at_publish` | all | other channels' settings | not used (S9: no routing here) |

Pinned as test vectors so any change upstream trips a test:
`courier-core/tests/vectors/live_2026-09_action_buttons.json` and
`live_2026-09_ephemeral.json`.

## ask-1 · Lifetime: keep `ephemeral`, add an optional duration

`ephemeral: true` says *that* a message is short-lived, not *how*
short. newsflash uses the config's `ephemeral_minutes` (default 10,
the same as the subscription TTL).

**Proposal (pipeline-v2):** keep `ephemeral`, and add an optional
`expires_in_minutes` for when the sender knows (e.g. "Vijf minuten
buiten" → 5). newsflash already honours it, most specific first:
`expires_in_minutes`, then `ephemeral`, then the per-priority config
(`expire_info_minutes`, …).

## ask-2 · One button format

Two shapes exist today:

- **Live** (what HA sends): `data.action_buttons: [{action, title}]`.
  This is the companion-app shape: single-language titles, and it
  lives inside `data`.
- **Contract** (K12, what newsflash was built for): top-level
  `actions: [{id, label: {nl, en}}]`, max 2.

**Proposal (pipeline-v2):** make top-level `actions` the
channel-neutral source. Give each button bilingual labels, with
optional `style` (`success`/`critical`) and `url` (a link button).
Derive `data.action_buttons` from `actions` for the companion app.
The desktop then gets both languages, coloured buttons on Windows, and
link buttons that don't round-trip through HA.

**newsflash today:** reads both. `actions` wins when present, else
`data.action_buttons`, else the default Gelezen/Snooze. The cap is
**5 on both desktops**. It was 2 (K12), but the live plant-care
message carries 3 and Plasma was measured rendering 20 (D3).

## ask-3 · Absolute links

`click_url` for HA dashboards is a bare path (`/control-panel/homelab`).
The phone app resolves it against HA; a desktop cannot.

**Proposal (pipeline-v2):** send absolute URLs (the LAN
`http://10.10.10.2:8123/…` or `https://ha.kp-soft.dev/…`).

**newsflash today:** `link_base_url = "http://10.10.10.2:8123"` in the
config resolves paths. Without it, path links are simply not offered.
Only http(s) is ever opened, so a message can never launch another
protocol handler.

## ask-4 · "desktop" as a notification type

Every message on `notify.kenny` becomes a desktop notification.
`data.notification_types` never says "desktop": the 35 ephemeral
"Beweging" reminders are `["tts","glow"]`, meaning spoken on
`media_player.kenny_pc` plus the office lights. Today they are also a
desktop notification.

**Proposal (pipeline-v2):** add `desktop` to the notification-type
vocabulary, and publish to `notify.kenny` only when it's included. The
routing decision stays upstream (S9), and the desktop keeps rendering
whatever reaches it. **Kenny's call (2026-09-26): `push` implies
`desktop`.** A message with `push` keeps reaching the desktop; one that
is only spoken or only lights (63 of the 16,774 read) does not.

## ask-5 · Deferred messages arrive quietly

62% of messages carry `gate_outcome: "deferred"`.

**Measured 2026-09-26** in `script.notification_dispatch` (Home
Assistant): `gate_outcome` is set at the moment of publishing, from the
dispatcher's own gate. A non-critical message is `live` when Do Not
Disturb is off, `schedule.notification_active_hours` is on and
`input_boolean.media_session_active` is off; otherwise it is `deferred`
(parked on `todo.notifications` for the hourly bulletin) or, when it is
`ephemeral`, `dropped`. Critical is always `live`. The hub copy is
published at that same moment, not later: "deferred" means "Kenny asked
not to be disturbed right now", not "released late".

**So this is no longer an ask:** the field already says what the desktop
needs. Showing a `deferred` message without popup and sound (straight
into Notification Center / Plasma history), and a `dropped` one not at
all, is newsflash's own change, decided in its own mini-round.

## ask-6 · Don't send the desktop's missed-message notices to the desktop

**98.7% of the retained messages (16,561) are "kyu: berichten
verlopen … abonnement desktop".** In words: "the desktop missed
messages", delivered to the desktop. They are useless there (it wasn't
there to see them), and each one expires in turn while the PC is off.
kyu 3.3.0 (2026-09-20) already cut the flood to one per day (its
CHANGELOG records the 27,991-event week).

**Proposal (HA / pipeline-v2):** route `message.expired` notices about
subscription `desktop` to the phone (push), or mark them
`ephemeral: true`. Never route an expiry notice about a notice.

## ask-7 · Windows-only presentation fields (feat-win-3–feat-win-9)

Coloured buttons, link buttons, hero images, replace-in-place tags,
progress bars, text/dropdown inputs: see `docs/WINDOWS.md`. They are
proposed additions, read leniently, and ignored on Linux (inputs,
images, tags and progress have no Plasma counterpart through
notify-send).

## Housekeeping from the read

- The temporary subscription **`claude-peek`** still exists on
  `notify.kenny`, with the unread tail (9 → 24 Sep) pending. Left
  alone, kyu flags it after 7 idle days and archives it after 30; each
  step produces one "kyu" notice. Kenny decides how to clean it up.
- The bulk read hit the app token's rate limit. The Windows courier
  uses the same token, so it saw a few `429`s for about two minutes
  (15:33–15:35 UTC) and backed off as designed; nothing was lost.
  Future bulk reads should use their own token and be paced.
