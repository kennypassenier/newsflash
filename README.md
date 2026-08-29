# hub-clients

Consumer binaries for the [kyu](../kyu) message hub that run on
Kenny's PC as systemd **user** services.

**desk-courier** — long-polls subscription `desktop` on topic
`notify.kenny` and renders each message as a desktop toast via
`notify-send`, with an optional soft chime. Messages older than the
subscription's 10-minute TTL expire at the hub — recorded, never
silent — so a day logged out never greets you with a pile of stale
toasts. The courier is an extra notification channel, never a critical
path: every failure mode degrades to exactly "no toasts".

*(vault-courier, a second consumer appending house events to the
Obsidian vault, may join this workspace later through its own gate —
decision pending.)*

## Quick start

See `docs/OPERATIONS_RUNBOOK.md` R1 for the numbered install. In short:
`cargo install --path desk-courier`, copy `config.example.toml` to
`~/.config/desk-courier/config.toml`, provide `KYU_TOKEN` via
`latch run` (or a 0600 `token_file`), install the systemd user unit
from `systemd/`.

```
desk-courier --version
desk-courier send-test --title Proef --message "Werkt het?"
```

## One-time activation after a fresh clone

The quality gates are git-native and a clone cannot carry the setting:

```
git config core.hooksPath .githooks
```

Without it, commits skip the gates (fmt, clippy warnings-as-errors,
full test suite, core I/O boundary, tree-change check) and the
commit-message ID rule. CI re-runs the same gates on every push.

Branch protection on `main` requires the `gates` check, up to date,
with no bypass (admins included) — so a direct push of an unverified
commit is refused. The daily flow: push your commit to a side branch
first (`git push origin main:ci-verify`), wait for green, then push
`main` — the same commit now carries a green check and is accepted.

## Development

- `cargo test --all` — unit + mock-hub tests (CI-safe).
- `./scripts/drill.sh` — the `#[ignore]` live tests against a real
  local kyu binary (`KYU_BIN` to override the path). Drills run
  against scratch hubs only; the live hub is never touched.
- Docs: `docs/SCOPE.md` (what and why), `docs/FEATURES.md` (rated
  feature list), `docs/ARCHITECTURE_DECISIONS.md` (frozen decisions),
  `docs/USER_GUIDE.md` (per feature), `docs/DEBUGGING_GUIDE.md`,
  `docs/OPERATIONS_RUNBOOK.md`, `docs/TEST_PLAN.md`.

This project follows the dev procedure in `~/Projects/dev-procedure`;
project state for future sessions lives in `CLAUDE.md`.
