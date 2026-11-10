#!/usr/bin/env bash
# Run in the guest as user dev (python allowed here): crapper AC-17 / CR-NFR-2 timing.
# Installs uv, Python 3.12, the pinned original (crapper/parity/upstream.toml) into ~/orig,
# then runs the parity runner on the stored-coverage runs with --timing.
# Then runs the dryer parity runner (DR-NFR-2: both times per case) with the pinned dryer original.
# Needs ~/target/{crapper,dryer}/release/ binaries from guest-test.sh.
set -euo pipefail
. "$HOME/.cargo/env"
export PATH="$HOME/.local/bin:$PATH"
cd "$HOME/src/crapper"
if [ ! -x "$HOME/orig/venv/bin/python" ]; then
  command -v uv >/dev/null || curl -LsSf https://astral.sh/uv/install.sh | sh >/dev/null
  uv python install 3.12 >/dev/null
  mkdir -p "$HOME/orig"
  commit=$(sed -n 's/^commit = "\(.*\)"/\1/p' parity/upstream.toml)
  repo=$(sed -n 's/^repository = "\(.*\)"/\1/p' parity/upstream.toml)
  git clone -q "$repo" "$HOME/orig/upstream"
  git -C "$HOME/orig/upstream" checkout -q "$commit"
  uv venv -q --python 3.12 "$HOME/orig/venv"
  uv pip install -q --python "$HOME/orig/venv/bin/python" tree-sitter==0.26.1 tree-sitter-language-pack==1.25.0 -e "$HOME/orig/upstream"
fi
PY=$(uv python find 3.12)
"$PY" parity/run.py --original-python "$HOME/orig/venv/bin/python" --port-bin "$HOME/target/crapper/release/crapper" \
  --only python-existing clojure-existing java-existing go-existing typescript-existing rust-existing --timing 2>&1 | tail -40

cd "$HOME/src/dryer"
dcommit=$(sed -n 's/^commit = "\(.*\)"/\1/p' parity/upstream.toml)
drepo=$(sed -n 's/^repository = "\(.*\)"/\1/p' parity/upstream.toml)
if [ ! -d "$HOME/orig/dryer" ]; then
  git clone -q "$drepo" "$HOME/orig/dryer"
  git -C "$HOME/orig/dryer" checkout -q "$dcommit"
fi
echo "== dryer DR-NFR-2"
DRYER_ORIGINAL_PYTHON="$HOME/orig/venv/bin/python" "$PY" parity/run_parity.py \
  --original "$HOME/orig/dryer" --port "$HOME/target/dryer/release/dryer" 2>&1 | tail -40
