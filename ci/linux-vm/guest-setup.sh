#!/usr/bin/env bash
# Run in the guest as user dev: build tools, git, rustup stable (minimal + clippy + rustfmt), Python 3.12.
# The base guest has Python 3.12 only for the Python parity tests that `cargo test` runs in
# tools/mutator (tests/parity_map.rs) and tools/dryer (tests/parity_python.rs). Ubuntu 22.04 ships
# Python 3.10, and the mutator test needs tomllib from 3.11 or later, so the deadsnakes PPA provides
# 3.12. guest-timing.sh installs a Python of its own with uv.
# The proof that the guest runs with no Python at all is guest-nopython.sh, which clones this guest.
set -euo pipefail
sudo cloud-init status --wait >/dev/null 2>&1 || true
sudo DEBIAN_FRONTEND=noninteractive apt-get update -qq
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential git pkg-config curl ca-certificates
if [ ! -x "$HOME/.cargo/bin/rustup" ]; then
  curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal -c clippy -c rustfmt >/dev/null
fi
. "$HOME/.cargo/env"
# Idempotent: a second run keeps the installed 3.12 and adds no PPA again.
if ! command -v python3.12 >/dev/null 2>&1; then
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq software-properties-common
  sudo add-apt-repository -y ppa:deadsnakes/ppa
  sudo DEBIAN_FRONTEND=noninteractive apt-get update -qq
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq python3.12
fi
git config --global user.email ci@example.invalid
git config --global user.name ci
git config --global commit.gpgsign false
git config --global init.defaultBranch main
rustc --version; cargo --version; cargo clippy --version; git --version; python3.12 --version; uname -m; ldd --version | head -1
