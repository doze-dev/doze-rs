#!/usr/bin/env bash
# The Rust daemon's names on this Mac, through the real system resolver and the Rust network daemon (core plan D2,
# owner-run). Run once, as yourself, from the repository, with no doze stack running:
#
#   ./scripts/core-d2-names-e2e-darwin.sh     (sudo asks once at the start; macOS shows its own dialog when the doze
#                                               CA is added to the System keychain)
#
# Nothing touches ~/.doze: the run uses a throwaway DOZE_HOME, and the Rust network daemon serves that home for the run.
# The apps are bun one-liners (bun must be on PATH).
#
# What it does, in order (and undoes on exit, also on failure or Ctrl-C):
#   1. builds the Rust doze (release-doze) as you;
#   2. keeps a copy of Go's setup (the launchd job dev.doze.netd, its daemon copy, /etc/resolver/doze) and the doze roots
#      the System keychain trusts, because the Rust doze installs over the same names;
#   3. doze dns-setup (one sudo), --check must pass: the Rust netd translates 198.19.0.0/17 for the throwaway home;
#   4. two stacks, each an app on port 18431 (Go's TestNamesThroughTheSystemResolver): each gets an address of its own
#      (198.19.x.y, not 127.0.0.1), the system resolver answers each name, and http://<name>:18431 and the port-less
#      http://<name> through the front door each reach their own project;
#   5. one of them declares the domain dev.core-d2-e2e.test: doze trust (one sudo), https through the front door with
#      the system's trust;
#   6. the stacks down (their names gone from the resolver), doze trust --remove and doze dns-setup --uninstall (one sudo
#      each), and Go's setup and trusted roots put back.
set -euo pipefail

[[ "$(uname -s)" == Darwin ]] || { echo "macOS only"; exit 2; }
if [[ $EUID -eq 0 ]]; then
  echo "run it as yourself, not under sudo: setup has to know whose zone the daemon serves"; exit 2
fi
cd "$(dirname "$0")/.."
REPO="$PWD"
command -v bun >/dev/null || { echo "bun must be on PATH (the test apps are bun one-liners)"; exit 2; }

# The resolver answers on 127.0.0.1:5323 and the front door on :80 and :443. A doze program already holding them would
# answer from ~/.doze instead of the test's home. Go's network daemon is replaced by the install itself.
for sock in "-iUDP@127.0.0.1:5323" "-iTCP:80 -sTCP:LISTEN" "-iTCP:443 -sTCP:LISTEN"; do
  # shellcheck disable=SC2086
  if lsof -nP $sock 2>/dev/null | grep -v -E '^(COMMAND|dev\.doze|doze-netd)' | grep -q .; then
    echo "something holds a port the zone needs ($sock); stop your stacks (doze down) and run this again:"
    # shellcheck disable=SC2086
    lsof -nP $sock
    exit 2
  fi
done

DOMAIN=dev.core-d2-e2e.test
JOB=/Library/LaunchDaemons/dev.doze.netd.plist
KEEP=("$JOB" /Library/PrivilegedHelperTools/dev.doze.netd /etc/resolver/doze)
BACKUP="$(mktemp -d "${TMPDIR:-/tmp}/doze-d2-keep.XXXXXX")"
# Short: each stack's control socket lives under the home, and a socket path has a hard length limit.
WORK="$(mktemp -d /tmp/dzd2.XXXXXX)"
export DOZE_HOME="$WORK/home"
unset DOZE_ZONE DOZE_ZONE_RESOLVER DOZE_ZONE_INGRESS DOZE_ZONE_INGRESS_TLS
mkdir -p "$DOZE_HOME"

echo "==> building the Rust doze (as $USER)"
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO/target}" cargo build -q --profile release-doze -p doze-cli
DOZE="${CARGO_TARGET_DIR:-$REPO/target}/release-doze/doze"
"$DOZE" version

step() { echo; echo "==> $*"; }
fail() { echo "FAILED: $*"; exit 1; }
stacks=()
restore() {
  set +e
  step "cleaning up"
  for s in "${stacks[@]}"; do (cd "$s" && "$DOZE" -y down >/dev/null 2>&1); done
  (cd "$WORK/p0" 2>/dev/null && "$DOZE" trust --remove >/dev/null 2>&1)
  "$DOZE" dns-setup --uninstall >/dev/null 2>&1
  sudo launchctl bootout system "$JOB" 2>/dev/null
  sudo rm -f "/etc/resolver/$DOMAIN"
  if [[ -s "$BACKUP/keep.tar" ]]; then
    sudo tar -xpf "$BACKUP/keep.tar" -C /
    [[ -f $JOB ]] && sudo launchctl bootstrap system "$JOB"
  fi
  for pem in "$BACKUP"/root-*.pem; do
    [[ -f $pem ]] && sudo security add-trusted-cert -d -r trustRoot -p ssl -p basic -k /Library/Keychains/System.keychain "$pem" && echo "    trusted again: $(basename "$pem")"
  done
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
security find-certificate -a -c "doze" -p /Library/Keychains/System.keychain 2>/dev/null \
  | awk -v d="$BACKUP" '/BEGIN CERT/{n++} n{print > (d "/root-" n ".pem")}'
ls "$BACKUP"/root-*.pem >/dev/null 2>&1 && echo "==> kept the trusted doze roots: $(ls "$BACKUP"/root-*.pem | wc -l | tr -d ' ')"
trap restore EXIT

step "doze dns-setup (one sudo), then --check"
"$DOZE" dns-setup
"$DOZE" dns-setup --check

step "two stacks, one port: each app on an address of its own"
addrs=()
for i in 0 1; do
  stack=namestest; [[ $i == 1 ]] && stack=namestest2
  dir="$WORK/p$i"; mkdir -p "$dir"
  host="api.$stack.doze"
  echo "$host" > "$dir/which.txt"
  # The app: its project's files, on the private port doze gives it.
  cat > "$dir/serve.ts" <<'TS'
Bun.serve({
  hostname: process.env.HOST,
  port: Number(process.env.PORT),
  fetch: (r) => new Response(Bun.file("." + new URL(r.url).pathname)),
});
TS
  extra=""; [[ $i == 0 ]] && extra="  domains = [\"$DOMAIN\"]"
  cat > "$dir/doze.hcl" <<EOF
stack = "$stack"

defaults {
  names = true
}

app "api" {
  run  = "bun serve.ts"
  port = 18431
$extra
}
EOF
  (cd "$dir" && "$DOZE" -y up)
  stacks+=("$dir")
  ip=""
  for _ in $(seq 1 30); do
    ip="$(dscacheutil -q host -a name "$host" 2>/dev/null | awk '/ip_address/{print $2; exit}')"
    [[ -n $ip ]] && break; sleep 1
  done
  [[ -n $ip ]] || fail "$host does not resolve through the system resolver"
  [[ $ip == 198.19.* ]] || fail "$host resolved to $ip, want an address of its own (198.19.x.y)"
  addrs+=("$ip")
  echo "    $host → $ip"
done
[[ ${addrs[0]} != "${addrs[1]}" ]] || fail "both stacks' apps are at one address"

for host in api.namestest.doze api.namestest2.doze; do
  got="$(curl -fsS --max-time 15 "http://$host:18431/which.txt")" || fail "http://$host:18431"
  [[ $got == "$host" ]] || fail "http://$host:18431 answered $got"
  echo "    http://$host:18431 → its own project"
  got="$(curl -fsS --max-time 15 "http://$host/which.txt")" || fail "http://$host (front door)"
  [[ $got == "$host" ]] || fail "http://$host answered $got"
  echo "    http://$host (port-less, the front door) → its own project"
done

step "doze trust (one sudo; macOS may also show its keychain dialog), then https://$DOMAIN"
(cd "$WORK/p0" && "$DOZE" trust)
(cd "$WORK/p0" && "$DOZE" trust --check)
got="$(curl -fsS --max-time 15 "https://$DOMAIN/which.txt")" || fail "https://$DOMAIN"
[[ $got == "api.namestest.doze" ]] || fail "https://$DOMAIN answered $got"
echo "    https://$DOMAIN → the app, trusted by the system"

step "the stacks down: their names leave the resolver"
for s in "${stacks[@]}"; do (cd "$s" && "$DOZE" -y down); done
stacks=()
sleep 2
if dscacheutil -q host -a name api.namestest.doze 2>/dev/null | grep -q ip_address; then
  # The system caches answers for their TTL; ask the zone's server itself.
  if dig +short -p 5323 @127.0.0.1 api.namestest.doze 2>/dev/null | grep -q .; then fail "api.namestest.doze still resolves"; fi
fi

step "doze trust --remove and doze dns-setup --uninstall (one sudo each)"
(cd "$WORK/p0" && "$DOZE" trust --remove)
[[ ! -e "/etc/resolver/$DOMAIN" ]] || fail "the route is still there"
"$DOZE" dns-setup --uninstall
if "$DOZE" dns-setup --check; then fail "uninstall left the setup in place"; fi

step "all passed"
