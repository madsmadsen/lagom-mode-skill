#!/usr/bin/env bash
# Run in a throwaway guest overlay as user dev: remove every Python interpreter and prove it.
# dpkg --force-depends keeps the rest of the system (netplan, cloud-init) installed but broken,
# which is fine for a guest that is only used to run `guest-test.sh` and is never rebooted.
set -uo pipefail
# The package pattern and the rm below also remove the deadsnakes 3.12 of guest-setup.sh: `libpython3`
# matches libpython3.12-*, and `/usr/bin/python3.*` matches /usr/bin/python3.12.
pkgs=$(dpkg-query -W -f='${Package}\n' 2>/dev/null | grep -E '^(python3|python3\.[0-9]+|libpython3|python3-.*)' || true)
# shellcheck disable=SC2086
sudo dpkg --purge --force-depends $pkgs >/dev/null 2>&1
sudo rm -f /usr/bin/python /usr/bin/python3 /usr/bin/python3.* /usr/local/bin/python*
rm -rf "$HOME/.local/share/uv" "$HOME/.local/bin/uv" "$HOME/.local/bin/uvx" "$HOME/orig" "$HOME/.cache/uv"
rm -rf "$HOME/target"   # no artefact from a run that had Python
status=0
for p in python python2 python3 python3.10 python3.11 python3.12; do
  if command -v "$p" >/dev/null 2>&1; then echo "FAIL: $p is $(command -v "$p")"; status=1; fi
done
found=$(find /usr /opt /bin /sbin /usr/local "$HOME" -xdev \( -name 'python*' -o -name 'pypy*' \) -type f -perm -u+x 2>/dev/null | head)
[ -z "$found" ] || { echo "FAIL: interpreter files remain: $found"; status=1; }
echo "which python3: [$(which python3 2>/dev/null)]  command -v python: [$(command -v python 2>/dev/null)]"
[ "$status" -eq 0 ] && echo "no-python: proven (no python/python3/python3.x on PATH, no executable python* file)"
exit "$status"
