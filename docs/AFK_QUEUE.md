# AFK queue

Kenny went AFK at the Phase 0 gate (2026-08-28) with the instruction:
*"develop al zoveel mogelijk gebaseerd op deze antwoorden."* Per
PROCEDURE's AFK rule (L2), work continues along the recommended choices;
every gate that normally needs him is queued here as a **ratification
round** ("the gate becomes a ratification mini-round" — PROCEDURE ground
rules). **This file is the first thing to present when he returns.**

## Pending ratification rounds (in order)

_Appended per phase as the AFK build progresses — see sections below._

### R1 · Phase 1 — build vs buy (greenfield gate)

Alternatives examined for "queue → desktop toast":

| Alternative | Verdict |
|---|---|
| ntfy + its desktop client | Rejected: ties the house to a second hub; mailbox already rejected ntfy-style dumb push in its own Phase 1, from the other side |
| Shell script (`curl` + `jq` + `notify-send` loop under systemd) | Genuine option, honestly weighed: ~30 lines, zero build. Rejected because it reproduces the house's documented failure class — untyped parsing that drifts (three silent format bugs in one month, study §3), no dedup, no tests, no policy bootstrap |
| Own small binary | **Chosen (recommendation)** — typed envelope parsing, testable, dedup, one place for the shared hub-client code vault-courier would reuse |

### R2 · Phase 2 — feature ratings + freeze
See `docs/FEATURES.md` — every rating there is Claude's recommendation,
built as rated, awaiting Kenny's real ratings. Includes the three
mandatory items (update mechanism M5, ecosystem integration M6,
backup & restore M7).

### R3 · Phase 3+4 — tech choice + architecture freeze
See `docs/ARCHITECTURE_DECISIONS.md` — includes the architecture-critic
round and its surviving objections.

### R4 · Phase 5 — realization plan + enforcement
See `docs/REALIZATION_PLAN.md`. Hooks installed locally (git-native +
Claude-layer); CI workflow written but **no GitHub remote yet** — see
"Deliberately not done" below.

## Deliberately not done (needs Kenny's explicit go)

- **GitHub repo creation + branch protection** (standing rules 6, 13:
  outward-facing). The CI workflow and branch-protection settings are
  ready in the repo; offer: Claude can create the private repo, push,
  and enable protection with the gh token, then read the settings back
  (rule 13a).
- **Enabling the systemd user service** on the PC
  (`systemctl --user enable --now desk-courier`) — deployment onto the
  real desktop is Kenny's go. The unit file ships in the repo; test runs
  during drills started the binary directly, in the foreground.
- **Anything against the live hub** (`10.10.10.9:8080`) — all
  development ran against a local scratch hub (S11c). Includes minting
  the real `desk-courier` app token on the live hub's `/apps` page.
- **Phase 9 — tag & release**: always Kenny's explicit go.

## Factual notes (no decision needed)

- The study said "notify-send/dunst"; the actual daemon on this machine
  is **KDE Plasma 6.7.4**. The interface is `notify-send` either way —
  recorded in SCOPE.md, no scope impact.
