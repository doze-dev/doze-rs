#!/usr/bin/env bash
# The Rust doze's privileged commands end to end, with root, on this Mac (core plan P1, owner-run step 3). Run once, as
# yourself, from the repository, with no doze stack running:
#
#   ./scripts/core-privileged-e2e-darwin.sh     (sudo asks once at the start; macOS shows its own dialog when the
#                                                 doze CA is added to the System keychain)
#
# GO_DOZE names the Go doze that brings the stack up and down (the Rust core has no `up` yet); it defaults to the
# `doze` on your PATH. Nothing touches ~/.doze: the run uses a throwaway DOZE_HOME.
#
# What it does, in order (and undoes on exit, also on failure or Ctrl-C):
#   1. builds the Rust doze (release) as you;
#   2. keeps a copy of Go's setup (the launchd job dev.doze.netd, its daemon copy, /etc/resolver/doze), because the
#      Rust doze installs over the same names, and stops Go's daemon for the run;
#   3. doze dns-setup --check (reports), doze dns-setup (one sudo), --check again (must pass): the Rust netd is the
#      launchd job now, serving the throwaway home;
#   4. a copy of examples/shop with domains = ["dev.core-p1-e2e.test"] on its web app, brought up by the Go doze;
#      curl http://web.shop.doze/healthz through the Rust netd and the Go daemon's front door;
#   5. doze trust (one sudo: the route and the doze CA in the System keychain), curl https://dev.core-p1-e2e.test/healthz
#      with the system's trust;
#   6. doze doctor (the report), doze doctor --fix (nothing to do), doze trust --remove (one sudo);
#   7. the stack down, doze dns-setup --uninstall (one sudo), and Go's setup put back and started again.
set -euo pipefail

[[ "$(uname -s)" == Darwin ]] || { echo "macOS only"; exit 2; }
if [[ $EUID -eq 0 ]]; then
  echo "run it as yourself, not under sudo: setup has to know whose zone the daemon serves"; exit 2
fi
cd "$(dirname "$0")/.."
REPO="$PWD"

GO_DOZE="${GO_DOZE:-$(command -v doze || true)}"
SHOP="${SHOP:-$HOME/Code/doze-dev/doze/examples/shop}"
[[ -x "$GO_DOZE" ]] || { echo "GO_DOZE must name the Go doze (it brings the stack up)"; exit 2; }
[[ -f "$SHOP/doze.hcl" ]] || { echo "SHOP must name the Go repository's examples/shop"; exit 2; }

# The resolver answers on 127.0.0.1:5323. A doze program already holding it would answer from ~/.doze instead of the
# test's home. Go's network daemon (lsof shows it as "dev.doze." or "doze-netd") is replaced by the install itself.
if lsof -nP -iUDP@127.0.0.1:5323 2>/dev/null | grep -v -E '^(COMMAND|dev\.doze|doze-netd)' | grep -q .; then
  echo "a doze program holds the resolver (127.0.0.1:5323); stop your stacks (doze down) and run this again:"
  lsof -nP -iUDP@127.0.0.1:5323
  exit 2
fi

DOMAIN=dev.core-p1-e2e.test
JOB=/Library/LaunchDaemons/dev.doze.netd.plist
KEEP=("$JOB" /Library/PrivilegedHelperTools/dev.doze.netd /etc/resolver/doze)
BACKUP="$(mktemp -d "${TMPDIR:-/tmp}/doze-core-keep.XXXXXX")"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/doze-core-e2e.XXXXXX")"
export DOZE_HOME="$WORK/home"
STACK="$WORK/shop"
mkdir -p "$DOZE_HOME"

echo "==> building the Rust doze (as $USER)"
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO/target}" cargo build -q --release -p doze-cli
DOZE="${CARGO_TARGET_DIR:-$REPO/target}/release/doze"
"$DOZE" version

step() { echo; echo "==> $*"; }
up=0
restore() {
  set +e
  step "cleaning up"
  if [[ $up == 1 ]]; then (cd "$STACK" && "$GO_DOZE" -y down >/dev/null 2>&1); fi
  sudo launchctl bootout system "$JOB" 2>/dev/null
  sudo rm -f "/etc/resolver/$DOMAIN"
  if [[ -s "$BACKUP/keep.tar" ]]; then
    sudo tar -xpf "$BACKUP/keep.tar" -C /
    [[ -f $JOB ]] && sudo launchctl bootstrap system "$JOB"
  fi
  sudo launchctl print system/dev.doze.netd >/dev/null 2>&1 && echo "    Go's daemon: running" || echo "    Go's daemon: not installed"
  echo "    work folder (kept for its logs): $WORK"
}

sudo -v
existing=()
for f in "${KEEP[@]}"; do [[ -e $f ]] && existing+=("${f#/}"); done
if [[ ${#existing[@]} -gt 0 ]]; then
  sudo tar -cpf "$BACKUP/keep.tar" -C / "${existing[@]}"
  echo "==> kept Go's setup: ${existing[*]}"
fi
trap restore EXIT

step "doze dns-setup --check (before: reports what is missing, exit 1 is expected)"
"$DOZE" dns-setup --check || true

step "doze dns-setup (one sudo)"
"$DOZE" dns-setup
"$DOZE" dns-setup --check
sudo launchctl print system/dev.doze.netd | grep -E "program|arguments" -A3 | head -8

step "examples/shop with a domain, up with the Go doze"
cp -R "$SHOP" "$STACK"
rm -f "$STACK/doze.lock.tmp"
awk -v d="$DOMAIN" '{ print } /^app "web" \{/ { print "  domains = [\"" d "\"]" }' "$SHOP/doze.hcl" > "$STACK/doze.hcl"
(cd "$STACK" && "$GO_DOZE" -y up)
up=1
for _ in $(seq 1 60); do curl -fsS --max-time 5 "http://web.shop.doze/healthz" >/dev/null 2>&1 && break; sleep 2; done
curl -fsS --max-time 10 "http://web.shop.doze/healthz"; echo "   ← http://web.shop.doze through the Rust netd"

step "doze trust (one sudo; macOS may also show its keychain dialog)"
(cd "$STACK" && "$DOZE" trust)
(cd "$STACK" && "$DOZE" trust --check)
curl -fsS --max-time 10 "https://$DOMAIN/healthz"; echo "   ← https://$DOMAIN, trusted by the system"

step "doze doctor, and doctor --fix with nothing to do"
(cd "$STACK" && "$DOZE" doctor) || true
(cd "$STACK" && "$DOZE" doctor --fix)

step "doze trust --remove (one sudo)"
(cd "$STACK" && "$DOZE" trust --remove)
[[ ! -e "/etc/resolver/$DOMAIN" ]] || { echo "the route is still there"; exit 1; }

step "the stack down, then doze dns-setup --uninstall (one sudo)"
(cd "$STACK" && "$GO_DOZE" -y down)
up=0
"$DOZE" dns-setup --uninstall
if "$DOZE" dns-setup --check; then echo "uninstall left the setup in place"; exit 1; fi

step "all passed"
