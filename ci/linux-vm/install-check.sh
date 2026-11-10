#!/usr/bin/env bash
# AC-9 (MU-10, MU-11) for real: the release mutator installs a missing cargo-llvm-cov, with the
# notice on stderr before the install command, and does not install it a second time.
# Host side. Needs the base guest from `vm.sh create arm64 arm64-base` plus guest-setup.sh (Rust, no
# cargo-llvm-cov). Clones it to a fresh overlay so the base stays clean, boots it (network on),
# pushes sources over ssh (no shared folder), runs the guest part, stops the guest.
#   install-check.sh [BASE]    BASE defaults to arm64-base (must be stopped)
# Guest part (guest part is this file run with `--guest` as user dev): see `guest()` below.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=${WORK:-/tmp/linux-vm}

guest() {
  . "$HOME/.cargo/env"
  export CARGO_TARGET_DIR=$HOME/target/mutator CARGO_TERM_COLOR=never
  if command -v cargo-llvm-cov >/dev/null 2>&1 || [ -e "$HOME/.cargo/bin/cargo-llvm-cov" ]; then
    echo "FAIL: cargo-llvm-cov is already installed; the guest is not fresh" >&2; exit 1
  fi
  (cd "$HOME/src/mutator" && cargo build --release 2>&1 | tail -1)
  rm -rf "$HOME/fixture" && cp -R "$HOME/src/mutator/parity/fixtures/rust" "$HOME/fixture"
  cd "$HOME/fixture"
  git init -q && git add -A && git commit -q -m fixture
  bin=$CARGO_TARGET_DIR/release/mutator
  for run in 1 2; do
    echo "=== run $run: mutator --test-command 'sh check.sh' (coverage needed)"
    set +e
    "$bin" --test-command 'sh check.sh' >"$HOME/run$run.out" 2>"$HOME/run$run.err"
    code=$?
    set -e
    echo "--- run $run exit code: $code"
    echo "--- run $run stderr (first 12 lines, last 4 lines):"
    head -12 "$HOME/run$run.err"; echo '   ...'; tail -4 "$HOME/run$run.err"
    echo "--- run $run: notice lines: $(grep -c 'Installing cargo-llvm-cov' "$HOME/run$run.err")," \
      "cargo install lines: $(grep -c 'Installing cargo-llvm-cov v' "$HOME/run$run.err")," \
      "line of notice: $(grep -n 'Neither cargo-llvm-cov' "$HOME/run$run.err" | cut -d: -f1 | head -1)," \
      "first line of cargo output: $(grep -n -m1 -E 'Updating|Compiling|Downloaded|Downloading' "$HOME/run$run.err" | cut -d: -f1)"
    echo "--- run $run stdout tail:"; tail -6 "$HOME/run$run.out"
    ls "$HOME/.cargo/bin" | tr '\n' ' '; echo
    ls .mutation* 2>/dev/null || ls -a | head -20
  done
}

if [ "${1:-}" = "--guest" ]; then guest; exit; fi

BASE=${1:-arm64-base}
NAME=arm64-cov
rm -f "$WORK/$NAME.qcow2"
"$HERE/vm.sh" clone arm64 "$NAME" "$BASE"
trap '"$HERE/vm.sh" stop arm64 || true' EXIT
"$HERE/vm.sh" start arm64 "$NAME"
"$HERE/vm.sh" push arm64
"$HERE/vm.sh" ssh arm64 'bash ~/src/ci/linux-vm/install-check.sh --guest' | tee "$WORK/install-check.log"
