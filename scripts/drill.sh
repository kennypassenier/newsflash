#!/usr/bin/env bash
# Live drill (AR18, standing rule 14): run the ignored live tests
# against the REAL kyu binary on scratch ports/data dirs. Never
# touches the live hub on LXC 109.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

KYU_BIN="${KYU_BIN:-$HOME/Projects/kyu/target/release/kyu}"
if [ ! -x "$KYU_BIN" ]; then
  echo "kyu binary not found at $KYU_BIN — build it first:" >&2
  echo "  (cd ~/Projects/kyu && cargo build --release)" >&2
  exit 2
fi

export KYU_BIN
exec cargo test --all -- --ignored --test-threads=4 "$@"
