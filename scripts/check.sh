#!/usr/bin/env bash
# Everything the CI workflow ran until 2026-09-29, when Kenny moved every
# build and check to his own machine ("alle builds lokaal"):
#
#   scripts/check.sh                    # WINDOWS_TESTS=skip to go without Windows
#
# 1. the commit gates in full (fmt, clippy -D warnings, all tests, AR3);
# 2. the Windows shell: clippy -D warnings and the courier-core +
#    newsflash-win tests, cross-built with cargo-xwin in docker and run on
#    real Windows through WSL interop (scripts/windows-tests.sh);
# 3. the Windows release build (newsflash-win.exe, newsflash-winw.exe) into
#    dist/windows/, what the job uploaded as an artifact.
# On Garuda step 2 cannot run the tests and says so; the script refuses
# unless WINDOWS_TESTS=skip, so a missing Windows run is never silent.
set -euo pipefail
root="$(git rev-parse --show-toplevel)"
cd "$root"

echo "[1/3] gates"
GATE_FULL=1 .claude/hooks/gates.sh

xwin() {
  docker run --rm --user "$(id -u):$(id -g)" -e HOME=/tmp -e XWIN_CACHE_DIR=/src/target-windows/xwin \
    -v "$HOME/.cargo/registry:/usr/local/cargo/registry" -v "$HOME/.cargo/git:/usr/local/cargo/git" \
    -v "$root:/src" -w /src messense/cargo-xwin \
    sh -c 'rustup target add x86_64-pc-windows-msvc >/dev/null 2>&1; sub=$1; shift; cargo xwin "$sub" --locked --target x86_64-pc-windows-msvc --target-dir target-windows "$@"' sh "$@"
}

echo "[2/3] Windows: clippy -D warnings, tests"
xwin clippy -p newsflash-win --all-targets -- -D warnings
if [ "${WINDOWS_TESTS:-}" = skip ]; then
  echo "  Windows tests SKIPPED on request (WINDOWS_TESTS=skip)"
else
  scripts/windows-tests.sh -p courier-core -p newsflash-win || {
    rc=$?; [ $rc -eq 3 ] && echo "  no Windows here; rerun on WSL, or WINDOWS_TESTS=skip to go without" >&2; exit $rc; }
fi

echo "[3/3] Windows release build"
xwin build --release -p newsflash-win
mkdir -p dist/windows
cp target-windows/x86_64-pc-windows-msvc/release/newsflash-win.exe \
   target-windows/x86_64-pc-windows-msvc/release/newsflash-winw.exe dist/windows/
echo "check passed; Windows binaries in dist/windows/"
