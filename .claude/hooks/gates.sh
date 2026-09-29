#!/usr/bin/env bash
# hub-clients quality gates (standing rules 6/7): format, lint with
# warnings as errors, full test suite, the AR3 core I/O boundary, and
# the tree-change check (a gate that rewrites the tree while running —
# e.g. cargo touching Cargo.lock after git add — must fail, kyu
# retro 2026-08-28). Called by .githooks/pre-commit and
# .claude/hooks/check-commit.sh; non-zero exit blocks the commit.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

tree_state() { git status --porcelain=v1 | sha256sum; }
before=$(tree_state)
# Kenny, 2026-09-16, standing rule 49 (commit-floor and rust-suite):
# format and lint always run, and the suite is skipped when no Rust
# source moved. Measured across sixteen projects: 41% of commits touch
# only documentation or configuration and paid for the suite anyway. Per
# crate was measured and rejected — `cargo test -p <crate>` is not faster
# than the whole workspace, because cargo runs every test binary either
# way.
. "$(git rev-parse --show-toplevel)/.githooks/gate-cache.sh"


cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
gate_glob suite '*.rs' 'Cargo.toml' 'Cargo.lock' '*/Cargo.toml' -- \
  cargo test --all

# The Windows shell (newsflash-win) is cfg(windows): the lint above only
# sees its stubs. Cross-lint it when the toolchain is here (cargo-xwin +
# the msvc target, docs/WINDOWS.md "Building"); scripts/check.sh always
# does, in docker, so a machine without it is not blocked here.
if command -v cargo-xwin >/dev/null 2>&1 \
  && rustup target list --installed 2>/dev/null | grep -q x86_64-pc-windows-msvc; then
  XWIN_ACCEPT_LICENSE=1 cargo xwin clippy -p newsflash-win \
    --target x86_64-pc-windows-msvc --all-targets -- -D warnings
else
  echo "note: cargo-xwin/msvc target absent — Windows lint left to scripts/check.sh" >&2
fi

# AR3: courier-core stays free of ambient I/O. The dependency list is
# the primary fence; this grep catches std back doors.
if grep -rnE '^[[:space:]]*use[[:space:]]+(ureq|std::(fs|net|process|io))' courier-core/src/; then
  echo "GATE FAILED — courier-core imports ambient I/O (AR3)." >&2
  echo "Move the I/O to the newsflash shell; core stays pure." >&2
  exit 1
fi

after=$(tree_state)
if [ "$before" != "$after" ]; then
  echo "GATE FAILED — the working tree changed while the gates ran (standing rule 7)." >&2
  echo "Something (cargo?) rewrote a file after staging. Re-add and commit again:" >&2
  git status --porcelain=v1 >&2
  exit 1
fi

gate_cache_done
