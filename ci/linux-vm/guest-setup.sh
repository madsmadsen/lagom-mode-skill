#!/usr/bin/env bash
# Run in the guest as user dev: build tools, git, rustup stable (minimal + clippy + rustfmt).
set -euo pipefail
sudo cloud-init status --wait >/dev/null 2>&1 || true
sudo DEBIAN_FRONTEND=noninteractive apt-get update -qq
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential git pkg-config curl ca-certificates
if [ ! -x "$HOME/.cargo/bin/rustup" ]; then
  curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal -c clippy -c rustfmt >/dev/null
fi
. "$HOME/.cargo/env"
git config --global user.email ci@example.invalid
git config --global user.name ci
git config --global commit.gpgsign false
git config --global init.defaultBranch main
rustc --version; cargo --version; cargo clippy --version; git --version; uname -m; ldd --version | head -1
