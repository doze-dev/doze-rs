#!/usr/bin/env bash
# macOS setup and trust end to end, with root, on this Mac. Run once, as yourself, with no doze stack running:
#
#   ./scripts/names-setup-e2e-darwin.sh        (sudo asks for your password once; macOS also shows its own
#                                                dialog when the doze CA is added to the System keychain)
#
# What it does, in order (and undoes on exit, also on failure or Ctrl-C):
#   1. builds the doze-names binary and the setup_root test as you;
#   2. keeps a copy of Go's setup (the launchd job dev.doze.netd, its daemon copy, /etc/resolver/doze), because ours
#      installs over the same names;
#   3. runs crates/doze-names/tests/setup_root.rs for a throwaway DOZE_HOME (never ~/.doze): install (the launchd job,
#      bootout before bootstrap, and /etc/resolver/doze), check (the daemon up and serving that home), a name answered
#      through the system resolver, a declared domain routed (/etc/resolver/dev.n5-e2e.test) and a throwaway doze CA
#      trusted in the System keychain, untrust (both gone), uninstall;
#   4. puts Go's job, daemon and resolver file back and starts Go's daemon again.
set -euo pipefail

[[ "$(uname -s)" == Darwin ]] || { echo "macOS only"; exit 2; }
if [[ $EUID -eq 0 ]]; then
  echo "run it as yourself, not under sudo: setup has to know whose zone the daemon serves"; exit 2
fi
cd "$(dirname "$0")/.."

# The daemon answers on 127.0.0.1:5323. A doze program already holding it would answer from ~/.doze instead of the
# test's home, and the test would fail for a reason that is not the code's.
# Go's daemon (lsof shows it as "dev.doze." or "doze-netd") is stopped by the install itself, so it does not count.
if lsof -nP -iUDP@127.0.0.1:5323 2>/dev/null | grep -v -E '^(COMMAND|dev\.doze|doze-netd)' | grep -q .; then
  echo "a doze program holds the resolver (127.0.0.1:5323); stop your stacks (doze down) and run this again:"
  lsof -nP -iUDP@127.0.0.1:5323
  exit 2
fi

JOB=/Library/LaunchDaemons/dev.doze.netd.plist
KEEP=(/Library/LaunchDaemons/dev.doze.netd.plist /Library/PrivilegedHelperTools/dev.doze.netd /etc/resolver/doze)
BACKUP="$(mktemp -d "${TMPDIR:-/tmp}/doze-names-keep.XXXXXX")"
HOME_DIR="$(mktemp -d "${TMPDIR:-/tmp}/doze-names-e2e.XXXXXX")"

echo "==> building (as $USER)"
cargo build -q -p doze-names --features setup --bin doze-names
cargo test -q -p doze-names --features setup --test setup_root --no-run

restore() {
  set +e
  echo "==> putting Go's setup back"
  sudo launchctl bootout system "$JOB" 2>/dev/null
  sudo rm -f /etc/resolver/dev.n5-e2e.test
  if [[ -s "$BACKUP/keep.tar" ]]; then
    sudo tar -xpf "$BACKUP/keep.tar" -C /
    [[ -f $JOB ]] && sudo launchctl bootstrap system "$JOB"
  fi
  sudo launchctl print system/dev.doze.netd >/dev/null 2>&1 && echo "    Go's daemon: running" || echo "    Go's daemon: not installed"
  echo "    test home (kept for its logs): $HOME_DIR"
}

sudo -v
existing=()
for f in "${KEEP[@]}"; do [[ -e $f ]] && existing+=("${f#/}"); done
if [[ ${#existing[@]} -gt 0 ]]; then
  sudo tar -cpf "$BACKUP/keep.tar" -C / "${existing[@]}"
  echo "==> kept: ${existing[*]}"
fi
trap restore EXIT

echo "==> end to end for $HOME_DIR"
DOZE_NAMES_ROOT_E2E=1 DOZE_HOME="$HOME_DIR" \
  cargo test -q -p doze-names --features setup --test setup_root -- --nocapture
echo "==> all passed"
