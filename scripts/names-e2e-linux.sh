#!/usr/bin/env bash
# Linux setup and trust end to end, with root, on a Linux machine you can change (the Lima VM doze-ubuntu). Run as
# yourself, from the repository, with sudo available (passwordless in the VM):
#
#   ./scripts/names-e2e-linux.sh
#
# What it does, in order (and undoes on exit, also on failure or Ctrl-C):
#   1. builds the doze-names binary and the setup_root test as you;
#   2. keeps a copy of whatever doze setup the machine has (Go's units, daemon copy, sysctl, CA in the bundle, the
#      hosts file), because ours installs over the same names;
#   3. runs crates/doze-names/tests/setup_root.rs for a throwaway DOZE_HOME: install (doze0 with 192.0.2.53, the
#      resolved route, the daemon's unit, the hosts block, the sysctl), check, a name answered through
#      systemd-resolved (resolvectl query), a declared domain routed and the doze CA trusted, untrust, uninstall;
#   4. puts the machine's own setup back and restarts its units.
set -euo pipefail

[[ "$(uname -s)" == Linux ]] || { echo "Linux only"; exit 2; }
if [[ $EUID -eq 0 ]]; then
  echo "run it as yourself: setup has to know whose zone the daemon serves, and asks for sudo itself"; exit 2
fi
cd "$(dirname "$0")/.."

KEEP=(/etc/systemd/system/doze-names.service /etc/systemd/system/doze-names.service.d
      /etc/systemd/system/doze-netd.service /usr/local/libexec/doze-netd /etc/sysctl.d/60-doze.conf
      /usr/local/share/ca-certificates/doze-local-ca.crt /etc/hosts)
BACKUP="$(mktemp -d /tmp/doze-names-keep.XXXXXX)"
HOME_DIR="$(mktemp -d /tmp/doze-names-e2e.XXXXXX)"

echo "==> building (as $USER)"
cargo build -q -p doze-names --features setup --bin doze-names
cargo test -q -p doze-names --features setup --test setup_root --no-run

restore() {
  set +e
  echo "==> putting the machine's own doze setup back"
  sudo tar -xpf "$BACKUP/keep.tar" -C / 2>/dev/null
  sudo systemctl daemon-reload
  for u in doze-names.service doze-netd.service; do
    [[ -f /etc/systemd/system/$u ]] && sudo systemctl enable --now "$u" >/dev/null 2>&1 && sudo systemctl restart "$u"
  done
  [[ -f /etc/sysctl.d/60-doze.conf ]] && sudo sysctl -q -p /etc/sysctl.d/60-doze.conf
  sudo update-ca-certificates >/dev/null 2>&1
  systemctl is-active doze-names.service doze-netd.service 2>/dev/null | paste -sd' ' | sed 's/^/    units: /'
  resolvectl domain doze0 2>/dev/null | sed 's/^/    /'
  rm -rf "$HOME_DIR"
}

existing=()
for f in "${KEEP[@]}"; do [[ -e $f ]] && existing+=("${f#/}"); done
sudo tar -cpf "$BACKUP/keep.tar" -C / "${existing[@]}"
echo "==> kept: ${existing[*]}"
trap restore EXIT

echo "==> end to end for $HOME_DIR"
DOZE_NAMES_ROOT_E2E=1 DOZE_HOME="$HOME_DIR" \
  cargo test -q -p doze-names --features setup --test setup_root -- --nocapture
echo "==> all passed"
