#!/usr/bin/env bash
# Run in the guest as user dev, after `vm.sh push`: build, test and clippy of each crate.
# Usage: guest-test.sh [crate...]   (default: crapper dryer mutator)
# Build output goes to the guest's own disk (CARGO_TARGET_DIR). Summary lines go to ~/results.txt.
set -uo pipefail
. "$HOME/.cargo/env"
export CARGO_TARGET_DIR=$HOME/target
export CARGO_TERM_COLOR=never
crates=("$@"); [ ${#crates[@]} -gt 0 ] || crates=(crapper dryer mutator)
out=$HOME/results.txt
{ echo "== $(date -u +%FT%TZ) $(uname -m) python3=[$(command -v python3 || true)] python3.12=[$(command -v python3.12 || true)] python=[$(command -v python || true)]"; } >> "$out"
for c in "${crates[@]}"; do
  cd "$HOME/src/$c" || exit 1
  # One target dir per crate: the crates share dependency names but not features.
  export CARGO_TARGET_DIR=$HOME/target/$c
  log=$HOME/log-$c
  s=$(date +%s); cargo build --release >"$log.build" 2>&1; b=$?; tb=$(( $(date +%s) - s ))
  s=$(date +%s); cargo test --no-fail-fast >"$log.test" 2>&1; t=$?; tt=$(( $(date +%s) - s ))
  # Sum "N passed; M failed; K ignored" over all test binaries.
  counts=$(awk '/^test result:/ {p+=$4; f+=$6; i+=$8} END {printf "passed=%d failed=%d ignored=%d", p, f, i}' "$log.test")
  s=$(date +%s); cargo clippy --all-targets >"$log.clippy" 2>&1; l=$?; tl=$(( $(date +%s) - s ))
  w=$(grep -c '^warning' "$log.clippy" || true)
  echo "$c: build=$b (${tb}s) test=$t $counts (${tt}s) clippy=$l warnings=$w (${tl}s)" >> "$out"
done
cat "$out"
