#!/usr/bin/env bash
# doze-os and the module protocol (ci.yml `os` on Linux and Windows, and part of the one `macos` job; see the job's
# comment for why every target runs it).
set -euo pipefail
step() { echo "::endgroup::"; echo "::group::$*"; "$@"; }
echo "::group::start"

# Postgres for the tree test: the Linux images ship one; the Windows x64 image sets PGBIN itself.
d=$(ls -d /usr/lib/postgresql/*/bin 2>/dev/null | sort -V | tail -1 || true)
if [ -n "$d" ]; then export PGBIN="$d"; fi

pkgs=(-p doze-os -p doze-module-proto -p doze-module -p doze-module-host -p doze-module-test -p doze-module-release
  -p doze-resp -p doze-module-valkey -p doze-module-kvrocks -p doze-module-temporal -p doze-module-mariadb -p doze-names
  -p doze-control -p doze-projects -p doze-journal -p doze-display -p doze-desk -p doze-cli -p doze-doctor
  -p doze-logstore -p doze-bind)
step cargo clippy "${pkgs[@]}" -p doze-module-derive --all-targets --features doze-control/test-support -- -D warnings
step cargo test "${pkgs[@]}" --no-fail-fast -- --nocapture
# doze-names with the network daemon (tun-rs; the root end-to-end test skips itself here).
step cargo clippy -p doze-names --features netd --all-targets -- -D warnings
step cargo test -p doze-names --features netd --no-fail-fast
# doze-names with machine setup and trust (every system's scripts are tested on every system).
step cargo clippy -p doze-names --features setup --all-targets -- -D warnings
step cargo test -p doze-names --features setup --no-fail-fast
echo "::endgroup::"
