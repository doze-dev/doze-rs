#!/usr/bin/env bash
# The Rust doze's privileged commands end to end, with root, on a Linux machine you can change (the Lima VM
# doze-ubuntu). Run as yourself, from the repository, with sudo available (passwordless in the VM):
#
#   ./scripts/core-privileged-e2e-linux.sh
#
# What it does, in order (and undoes on exit, also on failure or Ctrl-C):
#   1. builds the Rust doze and the doze-names tool (release) as you;
#   2. keeps a copy of whatever doze setup the machine has (Go's units, daemon copy, sysctl, CA in the bundle, the
#      hosts file), because the Rust doze installs over the same names;
#   3. doze dns-setup --dry-run, --check (reports), doze dns-setup (sudo), --check again (must pass);
#   4. a name held by hand (doze-names claim) answered through systemd-resolved (resolvectl query);
#   5. a stack whose daemon recorded a domain (as Go's daemon does at start): doze trust --check (missing), doze trust
#      --rotate (a doze CA made and trusted, the domain routed on doze0), --check (in place), doctor --fix (nothing
#      to do), doze trust --remove (route and CA gone);
#   6. doze dns-setup --uninstall, --check fails; then the machine's own setup put back and its units restarted.
set -euo pipefail

[[ "$(uname -s)" == Linux ]] || { echo "Linux only"; exit 2; }
if [[ $EUID -eq 0 ]]; then
  echo "run it as yourself: setup has to know whose zone the daemon serves, and asks for sudo itself"; exit 2
fi
cd "$(dirname "$0")/.."
REPO="$PWD"
TARGET="${CARGO_TARGET_DIR:-$REPO/target}"

KEEP=(/etc/systemd/system/doze-names.service /etc/systemd/system/doze-names.service.d
      /etc/systemd/system/doze-netd.service /usr/local/libexec/doze-netd /etc/sysctl.d/60-doze.conf
      /usr/local/share/ca-certificates/doze-local-ca.crt /etc/hosts)
BACKUP="$(mktemp -d /tmp/doze-core-keep.XXXXXX)"
WORK="$(mktemp -d /tmp/doze-core-e2e.XXXXXX)"
export DOZE_HOME="$WORK/home"
STACK="$WORK/stack"
DOMAIN=dev.core-p1-e2e.test
mkdir -p "$DOZE_HOME" "$STACK"
printf 'stack = "p1"\n' > "$STACK/doze.hcl"

echo "==> building (as $USER)"
cargo build -q --release -p doze-cli
cargo build -q --release -p doze-names --features setup --bin doze-names
DOZE="$TARGET/release/doze"
NAMES="$TARGET/release/doze-names"

step() { echo; echo "==> $*"; }
claim=""
restore() {
  set +e
  step "putting the machine's own doze setup back"
  [[ -n $claim ]] && kill "$claim" 2>/dev/null
  sudo tar -xpf "$BACKUP/keep.tar" -C / 2>/dev/null
  sudo systemctl daemon-reload
  for u in doze-names.service doze-netd.service; do
    [[ -f /etc/systemd/system/$u ]] && sudo systemctl enable --now "$u" >/dev/null 2>&1 && sudo systemctl restart "$u"
  done
  [[ -f /etc/sysctl.d/60-doze.conf ]] && sudo sysctl -q -p /etc/sysctl.d/60-doze.conf
  sudo update-ca-certificates >/dev/null 2>&1
  systemctl is-active doze-names.service doze-netd.service 2>/dev/null | paste -sd' ' | sed 's/^/    units: /'
  resolvectl domain doze0 2>/dev/null | sed 's/^/    /'
  rm -rf "$WORK"
}

existing=()
for f in "${KEEP[@]}"; do [[ -e $f ]] && existing+=("${f#/}"); done
sudo tar -cpf "$BACKUP/keep.tar" -C / "${existing[@]}"
echo "==> kept: ${existing[*]}"
trap restore EXIT

step "doze dns-setup --dry-run (runs nothing)"
"$DOZE" dns-setup --dry-run | head -5
step "doze dns-setup --check (reports what is missing for this home)"
"$DOZE" dns-setup --check || true
step "doze dns-setup (sudo)"
"$DOZE" dns-setup
"$DOZE" dns-setup --check

step "a name held by hand, answered through systemd-resolved"
"$NAMES" claim web.p1.doze > "$WORK/claim.out" 2>"$WORK/claim.err" &
claim=$!
for _ in $(seq 1 50); do grep -q '^ready' "$WORK/claim.out" 2>/dev/null && break; sleep 0.2; done
cat "$WORK/claim.out"
want=$(awk '$1=="web.p1.doze"{print $2}' "$WORK/claim.out")
got=$(resolvectl query --legend=no web.p1.doze | awk '{print $2; exit}')
[[ -n $want && $got == "$want" ]] || { echo "web.p1.doze resolved to '$got', want '$want'"; cat "$WORK/claim.err"; exit 1; }
echo "    web.p1.doze → $got"
kill "$claim"; claim=""

step "a stack's recorded domain: trust --check, trust --rotate, --check, doctor --fix, --remove"
dir=$(cd "$STACK" && pwd -P)
printf '{\n  "%s": [\n    "%s"\n  ]\n}\n' "$DOMAIN" "$dir" > "$DOZE_HOME/routes.json"
if (cd "$STACK" && "$DOZE" trust --check); then echo "trust --check passed before trust ran"; exit 1; fi
(cd "$STACK" && "$DOZE" trust --rotate)
(cd "$STACK" && "$DOZE" trust --check)
resolvectl domain doze0 | grep -q "$DOMAIN" || { echo "$DOMAIN is not routed on doze0"; exit 1; }
grep -q "BEGIN CERTIFICATE" /usr/local/share/ca-certificates/doze-local-ca.crt
(cd "$STACK" && "$DOZE" doctor --fix)
(cd "$STACK" && "$DOZE" doctor) || true
(cd "$STACK" && "$DOZE" trust --remove)
if resolvectl domain doze0 | grep -q "$DOMAIN"; then echo "$DOMAIN is still routed"; exit 1; fi

step "doze dns-setup --uninstall (sudo)"
"$DOZE" dns-setup --uninstall
if "$DOZE" dns-setup --check; then echo "uninstall left the setup in place"; exit 1; fi

step "all passed"
