# Operations runbook — newsflash

Numbered procedures, written in Phase 8 from what was actually executed
(drill log has the evidence). One-time repo activation after a fresh
clone: `git config core.hooksPath .githooks` (see README).

## R1 · First-time install on the PC

1. Build and install the binary:
   ```
   cd ~/Projects/newsflash && cargo install --path newsflash
   ```
2. Config:
   ```
   mkdir -p ~/.config/newsflash
   cp config.example.toml ~/.config/newsflash/config.toml
   ```
   Set `hub_url` (live hub: `http://10.10.10.9:8080`).
3. Token (M6): mint an app token named `newsflash` on the hub's
   `/apps` page, then store it in latch for this project:
   ```
   cd ~/Projects/newsflash && latch init   # once
   # put KYU_TOKEN=<token> in the latch env for this project
   ```
   *(No latch? Fallback: put the token in
   `~/.config/newsflash/token`, `chmod 600` it, and set `token_file`
   in the config — the unit file comments show the EnvironmentFile
   variant.)*
4. Unit:
   ```
   mkdir -p ~/.config/systemd/user
   cp systemd/newsflash.service ~/.config/systemd/user/
   systemctl --user daemon-reload
   systemctl --user enable --now newsflash
   ```
5. Verify: `journalctl --user -u newsflash -n 5` shows the startup
   summary and `hub reachable`; then
   `newsflash send-test --title Proef --message "Werkt het?"` pops a
   toast within a second or two.

Status 2026-08-29: steps 1–2 and the unit's `systemd-analyze verify`
are drilled; steps 3–4 wait for Kenny (token minting is outward-facing,
enabling the unit is deployment — AFK queue).

## R2 · Update (M5 — no self-update, by decision)

1. `cd ~/Projects/newsflash && git pull`
2. `cargo install --path newsflash`
3. `systemctl --user restart newsflash`
4. `journalctl --user -u newsflash -n 3` — the startup summary line
   shows the new version.

## R3 · Restore from zero (M7)

State inventory: config (this repo has the example; the real file is
one copy), token (lives in latch, rides latch's own escrow), dedup
cache (throwaway — worst case one duplicate toast). Therefore:

1. Clone the repo, activate hooks:
   `git clone <repo> ~/Projects/newsflash && cd ~/Projects/newsflash && git config core.hooksPath .githooks`
2. Run R1. Done — there is nothing else to restore, by design.

Drilled 2026-08-29: clean clone → release build → live publish against
the scratch hub succeeded first try (DRILL_LOG.md).

## R4 · Token rotated or revoked

Symptom: journal shows `hub rejected the token (401/403)` with the
remedy line; toasts stop; the courier keeps retrying (that is the
designed behaviour — nothing crashes).

1. Mint a new `newsflash` token on the hub's `/apps` page; revoke
   the old one there.
2. Update the latch secret (or the token file).
3. `systemctl --user restart newsflash`.

## R5 · No toasts, hub fine — triage order

1. `systemctl --user status newsflash` — running at all? (Logged
   out = not running, by design: AR20.)
2. `journalctl --user -u newsflash -n 20` — the state lines say
   which of the named states holds: `hub unreachable`, `auth rejected`,
   `no notification daemon`, `topic does not exist yet`, `subscription
   was archived` (self-heals), or quiet 204 polling (all healthy).
3. Hub side: the topic page on the hub dashboard shows the `desktop`
   subscription, its backlog and dead letters; `newsflash send-test`
   publishes a known-good envelope past every upstream suspect.
4. Longer story: docs/DEBUGGING_GUIDE.md.

## R6 · Scratch hub for development (standing rule 14)

```
KYU_LISTEN=127.0.0.1:18925 KYU_DATA_DIR=/tmp/dc-scratch \
  ~/Projects/kyu/target/release/kyu
./scripts/drill.sh        # the ignored live tests against it
```
The live hub on LXC 109 is never touched by development — only as an
agreed explicit step.
