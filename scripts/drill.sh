#!/usr/bin/env bash
# Live drill (AR18, standing rule 14): run the ignored live tests
# against the REAL mailbox binary on scratch ports/data dirs. Never
# touches the live hub on LXC 109.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

MAILBOX_BIN="${MAILBOX_BIN:-$HOME/Projects/mailbox/target/release/mailbox}"
if [ ! -x "$MAILBOX_BIN" ]; then
  echo "mailbox binary not found at $MAILBOX_BIN — build it first:" >&2
  echo "  (cd ~/Projects/mailbox && cargo build --release)" >&2
  exit 2
fi

export MAILBOX_BIN
exec cargo test --all -- --ignored --test-threads=4 "$@"
