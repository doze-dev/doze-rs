#!/usr/bin/env bash
# The Linux half of `just prepush`, run inside the Lima VM (arm64 Ubuntu 24.04) by tools/prepush/prepush.ts, which sends the working
# tree to /tmp/prepush-src.tgz and this script on stdin (the first stdin line, read before, is GH_TOKEN for modules.sh).
#
#   bash linux.sh <gate>...      gates: linux-clippy linux-test linux-os.sh linux-modules.sh linux-weight-doze
#
# Everything builds here, in ~/prepush/target: nothing is cross-built on the Mac. Memory: with 4 GiB, fat-LTO and test links of the
# aws crates were OOM-killed at -j3 and -j4 (2026-11-27; doze-cli's release-doze link at -j3 too), so the owner gave the VM 8 GiB
# (2026-11-27). It has 4 CPUs; cargo runs at PREPUSH_VM_JOBS, default 3: up to three links at once fit in 8 GiB, and the 8 GiB
# /swapfile (added when there is no swap) absorbs a peak instead of the OOM killer. If links are killed again, lower PREPUSH_VM_JOBS.
# Reports each gate as `@@prepush <gate> start|pass|fail <note>` for the host to read.
set -uo pipefail
home=~/prepush
src=$home/src
export CARGO_TARGET_DIR=$home/target
export CARGO_BUILD_JOBS=${PREPUSH_VM_JOBS:-3} RUST_TEST_THREADS=4 RUSTFLAGS="-D warnings" CARGO_TERM_COLOR=never
# No debug info, as on the Mac (prepush.ts): it keeps the target a fraction of the size.
export CARGO_PROFILE_DEV_DEBUG=0
max_gb=${PREPUSH_VM_TARGET_MAX_GB:-50}  # a full run leaves about 41 GB (2026-11-27)
mkdir -p "$home"

# One run at a time in this VM (other worktrees share it).
exec 9> "$home/lock"
flock -w 7200 9 || { echo "prepush: the VM is busy with another run for 2 h"; exit 1; }

# The tree: unpacked beside, then rsynced so that unchanged files keep their times and cargo stays incremental.
rm -rf "$home/stage" && mkdir -p "$home/stage" "$src"
tar -xzf /tmp/prepush-src.tgz -C "$home/stage" && rm -f /tmp/prepush-src.tgz
# macOS tar can add AppleDouble ._ files (xattrs); the tests that walk the sources must never see them.
find "$home/stage" -name '._*' -type f -delete
rsync -a --delete --checksum "$home/stage/" "$src/" && rm -rf "$home/stage"
cd "$src"

# Keep the target under control: past the cap it starts again.
free_gb=$(df --output=avail --block-size=1G / | tail -1 | tr -d " ")
if [ -d "$CARGO_TARGET_DIR" ] && { [ "$(du -s --block-size=1G "$CARGO_TARGET_DIR" | cut -f1)" -gt "$max_gb" ] || [ "$free_gb" -lt 15 ]; }; then
  echo "prepush: $CARGO_TARGET_DIR is over ${max_gb} GB or the disk has ${free_gb} GB free; removing it (a full rebuild follows)"
  rm -rf "$CARGO_TARGET_DIR"
fi

# Once: swap for the links, rustup, and what the CI images have (postgres for os.sh's tree test, gh for modules.sh).
if [ "$(swapon --show --noheadings | wc -l)" -eq 0 ]; then
  echo "prepush: adding an 8 GiB /swapfile (a link peak goes to swap, not the OOM killer)"
  sudo fallocate -l 8G /swapfile && sudo chmod 600 /swapfile && sudo mkswap /swapfile > /dev/null && sudo swapon /swapfile
  grep -q '^/swapfile ' /etc/fstab || echo '/swapfile none swap sw 0 0' | sudo tee -a /etc/fstab > /dev/null
fi
if ! command -v cargo > /dev/null; then
  [ -x ~/.cargo/bin/cargo ] || curl -fsSL https://sh.rustup.rs | sh -s -- -y --default-toolchain none --profile minimal
  . ~/.cargo/env
fi
need=()
ls -d /usr/lib/postgresql/*/bin > /dev/null 2>&1 || need+=(postgresql)
command -v gh > /dev/null || need+=(gh)
if [ ${#need[@]} -gt 0 ]; then
  echo "prepush: installing ${need[*]}"
  sudo apt-get update -qq && sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq "${need[@]}" > /dev/null ||
    echo "prepush: WARNING: apt-get could not install ${need[*]}; the gates that need them will fail (a VM stopped mid-write once left /var/lib/apt/extended_states as zeros: move it aside)"
  # The CI images ship postgres's binaries; nothing here needs the system's server running.
  if [[ " ${need[*]} " == *" postgresql "* ]]; then sudo systemctl disable --now postgresql > /dev/null 2>&1 || true; fi
fi

rc=0
gate() {
  local name=$1; shift
  echo "@@prepush $name start"
  local t0=$SECONDS
  if "$@"; then echo "@@prepush $name pass arm64 Linux, $((SECONDS - t0)) s"
  else echo "@@prepush $name fail failed (see the proof log)"; rc=1; fi
}
for g in "$@"; do
  case "$g" in
    linux-clippy) gate "$g" bash -c 'cargo clippy --workspace --exclude doze-app --all-targets -- -D warnings' ;;
    linux-test) gate "$g" bash -c 'cargo test --workspace --exclude doze-app' ;;
    linux-os.sh) gate "$g" .github/ci/os.sh ;;
    linux-modules.sh)
      if [ -z "${GH_TOKEN:-}" ]; then echo "@@prepush $g fail no GitHub token for the engine downloads"; rc=1
      else RUNNER_TEMP=$home/runner gate "$g" .github/ci/modules.sh aarch64-unknown-linux-gnu; rm -rf "$home/runner"; fi ;;
    # The bytes only: the host judges them against the line (tools/prepush/weight.ts).
    linux-weight-doze)
      echo "@@prepush $g start"
      if cargo build --locked --profile release-doze -p doze-cli; then
        echo "@@prepush $g pass bytes=$(stat -c %s "$CARGO_TARGET_DIR/release-doze/doze")"
      else echo "@@prepush $g fail the release-doze build failed"; rc=1; fi ;;
    *) echo "@@prepush $g fail unknown gate"; rc=1 ;;
  esac
done
echo "@@prepush-disk target $(du -sh "$CARGO_TARGET_DIR" 2>/dev/null | cut -f1), source $(du -sh "$src" | cut -f1), free $(df -h --output=avail / | tail -1 | tr -d ' ')"
exit $rc
