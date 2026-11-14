#!/usr/bin/env bash
# The macOS network daemon end to end, with root, on this Mac. Run once, as yourself:
#
#   ./scripts/names-e2e-darwin.sh        (asks for your password once, via sudo)
#
# What it does, in order (and undoes on exit, also on failure or Ctrl-C):
#   1. builds doze-netd and the e2e tests as you;
#   2. if Go's daemon (launchd job dev.doze.netd) holds the 198.19 block, stops it for the duration;
#   3. starts the Rust doze-netd as root for a private, throwaway DOZE_HOME (never ~/.doze), on a private resolver port;
#   4. runs crates/doze-names/tests/e2e_darwin.rs against it (real tunnel, real route, real packets);
#   5. stops the Rust daemon (its own pid only), removes its route, and starts Go's daemon again if it was running.
set -euo pipefail

[[ "$(uname -s)" == Darwin ]] || { echo "macOS only"; exit 2; }
if [[ $EUID -eq 0 ]]; then
  echo "run it as yourself, not under sudo: it builds as you and asks for sudo only to start the daemon"; exit 2
fi

cd "$(dirname "$0")/.."
GO_JOB=/Library/LaunchDaemons/dev.doze.netd.plist
GO_LABEL=system/dev.doze.netd
BLOCK=198.19.0.0/17
HOME_DIR="$(mktemp -d "${TMPDIR:-/tmp}/doze-names-e2e.XXXXXX")"
LOG="$HOME_DIR/netd.log"
NETD_PID=""
GO_WAS_RUNNING=0

echo "==> building (as $USER)"
cargo build -q -p doze-names --features netd --bin doze-netd
cargo test -q -p doze-names --features netd --test e2e_darwin --no-run

cleanup() {
  set +e
  if [[ -n "$NETD_PID" ]]; then
    echo "==> stopping the Rust daemon (pid $NETD_PID)"
    sudo kill "$NETD_PID" 2>/dev/null
    for _ in $(seq 1 50); do sudo kill -0 "$NETD_PID" 2>/dev/null || break; sleep 0.1; done
  fi
  sudo route -n delete -net "$BLOCK" >/dev/null 2>&1
  if [[ $GO_WAS_RUNNING -eq 1 ]]; then
    echo "==> starting Go's daemon again"
    sudo launchctl bootstrap system "$GO_JOB" 2>/dev/null || sudo launchctl kickstart -k "$GO_LABEL"
  fi
  echo "    daemon log: $LOG"
}
trap cleanup EXIT

sudo -v
if sudo launchctl print "$GO_LABEL" >/dev/null 2>&1; then
  echo "==> stopping Go's daemon for the run"
  GO_WAS_RUNNING=1
  sudo launchctl bootout "$GO_LABEL"
  for _ in $(seq 1 50); do route -n get 198.19.0.10 2>/dev/null | grep -q 'destination: 198.19' || break; sleep 0.1; done
fi

RESOLVER="127.0.0.1:$((20000 + RANDOM % 20000))"
echo "==> starting the Rust daemon as root for $HOME_DIR (resolver $RESOLVER)"
# The shell prints its own pid and then becomes the daemon, so the pid is exactly the daemon's.
sudo env DOZE_HOME="$HOME_DIR" DOZE_ZONE_RESOLVER="$RESOLVER" sh -c 'echo $$ >"$1"; exec "$2" "$3" "$4" "$5"' _ \
  "$HOME_DIR/netd.pidfile" ./target/debug/doze-netd "$HOME_DIR/names.json" "$(id -u)" "$(id -g)" >"$LOG" 2>&1 &
for _ in $(seq 1 50); do [[ -s "$HOME_DIR/netd.pidfile" ]] && break; sleep 0.1; done
NETD_PID="$(cat "$HOME_DIR/netd.pidfile")"
for _ in $(seq 1 50); do grep -q translating "$LOG" && break; sleep 0.1; done
grep -q translating "$LOG" || { echo "the daemon did not come up:"; cat "$LOG"; exit 1; }
cat "$LOG"

echo "==> end-to-end tests"
DOZE_NAMES_E2E=foreground DOZE_HOME="$HOME_DIR" DOZE_ZONE_RESOLVER="$RESOLVER" \
  cargo test -q -p doze-names --features netd --test e2e_darwin -- --test-threads=1 --nocapture
echo "==> all passed"
