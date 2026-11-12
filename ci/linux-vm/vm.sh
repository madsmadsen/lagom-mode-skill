#!/usr/bin/env bash
# Linux guests for the three ports, on a macOS arm64 host with qemu from Homebrew.
# Scratch state lives in $WORK (default /tmp/linux-vm). Nothing is shared with the guest:
# sources are copied in as a tar stream and built on the guest's own disk.
#
#   vm.sh fetch                  download the Ubuntu 22.04 (glibc 2.35) cloud images
#   vm.sh create ARCH NAME       overlay NAME on the pristine image, plus a cloud-init seed
#   vm.sh clone  ARCH NAME FROM  overlay NAME on top of overlay FROM (FROM must be stopped)
#   vm.sh start  ARCH NAME       boot NAME (arm64: HVF, x86_64: TCG), wait for ssh
#   vm.sh ssh    ARCH [cmd...]   run a command in the running guest
#   vm.sh push   ARCH            copy crapper dryer mutator and guest scripts (no target, .git, .metrics)
#   vm.sh stop   ARCH            shut the guest down, then kill qemu if it lingers
#   vm.sh kill                   kill every qemu started from $WORK
# ARCH is arm64 or x86_64.
set -euo pipefail

WORK=${WORK:-/tmp/linux-vm}
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
QEMU_DIR=${QEMU_DIR:-/opt/homebrew/bin}
SHARE=${QEMU_SHARE:-/opt/homebrew/share/qemu}
KEY=$WORK/key
SSH_OPTS=(-i "$KEY" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=5 -o BatchMode=yes)

arch_check() {
  case "$1" in
    arm64) PORT=2222; IMG=jammy-server-cloudimg-arm64.img ;;
    x86_64) PORT=2223; IMG=jammy-server-cloudimg-amd64.img ;;
    *) echo "ARCH must be arm64 or x86_64" >&2; exit 2 ;;
  esac
}

guest_ssh() { ssh "${SSH_OPTS[@]}" -p "$PORT" dev@127.0.0.1 "$@"; }

cmd_fetch() {
  mkdir -p "$WORK"
  for img in jammy-server-cloudimg-arm64.img jammy-server-cloudimg-amd64.img; do
    [ -f "$WORK/$img" ] || curl -sS -L -o "$WORK/$img" "https://cloud-images.ubuntu.com/jammy/current/$img"
  done
  [ -f "$KEY" ] || ssh-keygen -q -t ed25519 -N "" -f "$KEY"
}

cmd_create() {
  arch_check "$1"; local name=$2
  qemu-img create -q -f qcow2 -b "$WORK/$IMG" -F qcow2 "$WORK/$name.qcow2" 40G
  local seed=$WORK/seed-$name
  mkdir -p "$seed"
  sed "s|@PUBKEY@|$(cat "$KEY.pub")|" "$HERE/user-data.in" > "$seed/user-data"
  printf 'instance-id: %s\nlocal-hostname: %s\n' "$name" "$name" > "$seed/meta-data"
  mkisofs -quiet -V cidata -J -r -o "$WORK/$name-seed.iso" "$seed/user-data" "$seed/meta-data"
}

cmd_clone() {
  arch_check "$1"; local name=$2 from=$3
  qemu-img create -q -f qcow2 -b "$WORK/$from.qcow2" -F qcow2 "$WORK/$name.qcow2"
}

cmd_start() {
  arch_check "$1"; local name=$2
  local common=(-m "${VM_MEM:-6G}" -smp "${VM_CPUS:-6}" -display none -serial "file:$WORK/$name.serial.log"
    -daemonize -pidfile "$WORK/$1.pid"
    -nic "user,model=virtio-net-pci,hostfwd=tcp:127.0.0.1:$PORT-:22"
    -drive "file=$WORK/$name.qcow2,if=virtio,format=qcow2")
  [ -f "$WORK/$name-seed.iso" ] && common+=(-drive "file=$WORK/$name-seed.iso,if=virtio,format=raw,readonly=on")
  if [ "$1" = arm64 ]; then
    "$QEMU_DIR/qemu-system-aarch64" -machine virt -accel hvf -cpu host -bios "$SHARE/edk2-aarch64-code.fd" "${common[@]}"
  else
    "$QEMU_DIR/qemu-system-x86_64" -machine q35 -accel tcg,thread=multi -cpu max "${common[@]}"
  fi
  local tries=0
  until guest_ssh true 2>/dev/null; do
    tries=$((tries + 1))
    [ "$tries" -lt "${VM_BOOT_TRIES:-180}" ] || { echo "guest did not answer ssh" >&2; exit 1; }
    sleep 5
  done
  echo "guest $name ($1) is up on port $PORT"
}

cmd_ssh() { arch_check "$1"; shift; guest_ssh "$@"; }

cmd_push() {
  arch_check "$1"
  guest_ssh 'rm -rf ~/src && mkdir -p ~/src'
  # git decides what ships: tracked and untracked files minus .gitignore (target/, .metrics/, .work/).
  # The crates land in ~/src/<crate>, next to ~/src/ci/linux-vm, so the guest scripts keep their paths.
  (cd "$REPO/tools" && git ls-files -z -co --exclude-standard -- crapper dryer mutator \
    | COPYFILE_DISABLE=1 tar -c --no-xattrs --no-mac-metadata --null -T -) | guest_ssh 'tar -x --warning=no-unknown-keyword -C ~/src'
  (cd "$REPO" && git ls-files -z -co --exclude-standard -- ci/linux-vm \
    | COPYFILE_DISABLE=1 tar -c --no-xattrs --no-mac-metadata --null -T -) | guest_ssh 'tar -x --warning=no-unknown-keyword -C ~/src'
}

cmd_stop() {
  arch_check "$1"
  guest_ssh 'sudo shutdown -h now' 2>/dev/null || true
  local pid; pid=$(cat "$WORK/$1.pid" 2>/dev/null || true)
  for _ in $(seq 1 60); do
    [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null || return 0
    sleep 2
  done
  [ -z "$pid" ] || kill "$pid" 2>/dev/null || true
}

cmd_kill() { pkill -f "$WORK" || true; }

sub=${1:-}; shift || true
case "$sub" in
  fetch|create|clone|start|ssh|push|stop|kill) "cmd_$sub" "$@" ;;
  *) sed -n 2,16p "$0"; exit 2 ;;
esac
