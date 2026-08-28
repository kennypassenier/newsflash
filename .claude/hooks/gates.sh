#!/usr/bin/env bash
# hub-clients quality gates (standing rules 6/7): format, lint with
# warnings as errors, full test suite, the AR3 core I/O boundary, and
# the tree-change check (a gate that rewrites the tree while running —
# e.g. cargo touching Cargo.lock after git add — must fail, mailbox
# retro 2026-08-28). Called by .githooks/pre-commit and
# .claude/hooks/check-commit.sh; non-zero exit blocks the commit.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

tree_state() { git status --porcelain=v1 | sha256sum; }
before=$(tree_state)

cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all

# AR3: courier-core stays free of ambient I/O. The dependency list is
# the primary fence; this grep catches std back doors.
if grep -rnE '^[[:space:]]*use[[:space:]]+(ureq|std::(fs|net|process|io))' courier-core/src/; then
  echo "GATE FAILED — courier-core imports ambient I/O (AR3)." >&2
  echo "Move the I/O to the desk-courier shell; core stays pure." >&2
  exit 1
fi

after=$(tree_state)
if [ "$before" != "$after" ]; then
  echo "GATE FAILED — the working tree changed while the gates ran (standing rule 7)." >&2
  echo "Something (cargo?) rewrote a file after staging. Re-add and commit again:" >&2
  git status --porcelain=v1 >&2
  exit 1
fi
