# Debugging guide — newsflash

The evidence trail first, then symptom → cause tables. Written in
Phase 8 from the code; journal lines quoted here are the literal
strings from `run.rs`/`logx.rs`.

## The evidence trail

1. **Journal** — `journalctl --user -u newsflash -n 50`. One line
   per lifecycle event, sd-priority prefixed. The startup summary names
   the config in force (hub, topic, subscription, language, TTL, sound
   — never the token). State *changes* log once; repeated failures do
   not spam.
2. **Hub dashboard** — the topic page for `notify.kenny` shows the
   `desktop` subscription, its backlog, dead letters (with payloads and
   a Requeue button) and the policy in force with what is explicit.
3. **Dedup store** — `~/.local/state/newsflash/seen.json`, a plain
   JSON array of the last delivered hub ids, oldest first.
4. **Known-good injection** — `newsflash send-test …` publishes a
   valid envelope, bypassing every upstream suspect.

## Symptom → cause

### No toasts at all

| Journal shows | Cause | Fix |
|---|---|---|
| unit not running | logged out, or unit never enabled | by design at logout (AR20); otherwise runbook R1 step 4 |
| `hub unreachable (…) — backing off` | hub/LXC down, network | courier self-heals on the backoff; check the hub's /healthz |
| `hub rejected the token (401/403) …` | token revoked/rotated | runbook R4 — the log line carries the remedy |
| `topic does not exist yet …` | nothing has ever published to `notify.kenny` on this hub | normal before the pipeline ships; `send-test` creates the topic |
| `subscription was archived … unarchiving` | PC off ≥30 days | self-heals; the lapsed backlog was disposable (10-min TTL) |
| `no notification daemon on the session bus — holding` | notification service not up (login race) or crashed | courier holds without consuming; messages wait under the TTL; restart plasmashell or relogin |
| quiet polling, nothing else | healthy and idle | `send-test` to prove the chain |

### A specific message never toasted

| Where it shows | Cause | Fix |
|---|---|---|
| journal: `…: past the 10min TTL client-side … acked unrendered` | claimed right before a suspend, rendered after resume | by design (S4); nothing to fix |
| journal: `…: poison (…) — dead-lettered. Remedy: …` + hub dead letters | payload is not a valid v1 envelope | fix the producer; Requeue on the dashboard after |
| hub metrics `state="expired"` / `kyu.events` `message.expired` | courier was down longer than the TTL | by design (S4) — the expiry is the record |
| journal: `…: redelivery of a seen id — acked silently` | at-least-once redelivery of an already-shown toast | by design (K4); the toast appeared the first time |

### Toast content looks wrong

| Symptom | Cause |
|---|---|
| English text though language is nl | the envelope carried no `nl` translation — fallback (M3) is deliberate; fix the producer |
| `…` at the end | truncation budget (200/1000 chars, AR4) |
| literal `&amp;`-style text | producer pre-escaped its content; the courier escapes exactly once |

### Duplicate toast

One duplicate after a crash or a corrupt/absent dedup store is the
accepted failure direction (AR5/AR6: a toast twice beats a toast
never). A *stream* of duplicates would mean the store cannot persist —
check the journal for `cannot write dedup store` lines and the
permissions of `~/.local/state/newsflash/`.

## Poking the hub directly

```
curl -H "authorization: Bearer $KYU_TOKEN" \
  "http://10.10.10.9:8080/api/t/notify.kenny/subs/desktop/policy"
curl -H "authorization: Bearer $KYU_TOKEN" \
  "http://10.10.10.9:8080/api/t/notify.kenny/subs/desktop/dead"
```
The hub's own USER_GUIDE (`~/Projects/kyu/docs/USER_GUIDE.md`) is
the authority on those endpoints.
