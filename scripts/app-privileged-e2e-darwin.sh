#!/usr/bin/env bash
# The app's privileged actions end to end, with the real macOS password prompt, on this Mac. Run once, as yourself:
#
#   ./scripts/app-privileged-e2e-darwin.sh
#
# You will see the system's password prompt three times: type the password the first time, press Cancel the
# second time, and type it again the third time (each run says which before it asks).
#
# Nothing here changes your machine as root: every script it runs as root only prints (who it runs as, a count). It
# never touches ~/.doze, your Doze.app or your daemons: the app runs on a private, throwaway DOZE_HOME, and the doze it
# runs for the setup is a stand-in that asks with the same system prompt Go's doze uses (osascript ... with
# administrator privileges), around a script that only prints.
#
# What it checks, in order:
#   1. the app's own prompt (doze-os, used for the PATH link, stale routes and leftovers): a script runs as root
#      (uid 0) and its lines stream back as they are printed (cargo test, crates/doze-desk/tests/admin_prompt_e2e.rs);
#   2. Cancel in the prompt is heard as a cancel, not a failure (same test file);
#   3. the app itself (a development build, driven by its own script, no Accessibility permission needed): Settings ›
#      Machine shows the names not in place, Repair opens the run sheet showing exactly what will run (the command, and
#      the root script the CLI prints with --print), Run raises ONE prompt, the output streams into the sheet, the sheet
#      says "this machine is set up" in words once doctor has looked again.
set -euo pipefail

[[ "$(uname -s)" == Darwin ]] || { echo "macOS only"; exit 2; }
if [[ $EUID -eq 0 ]]; then
  echo "run it as yourself, not under sudo: the point is the system's own prompt"; exit 2
fi

cd "$(dirname "$0")/.."
WORK="$(mktemp -d "${TMPDIR:-/tmp}/doze-app-privileged-e2e.XXXXXX")"
APP_PID=""
cleanup() {
  set +e
  if [[ -n "$APP_PID" ]] && kill -0 "$APP_PID" 2>/dev/null; then
    echo "==> stopping the test app (pid $APP_PID)"
    kill "$APP_PID"
  fi
  echo "    everything is under $WORK (app log, driver log)"
}
trap cleanup EXIT

echo "==> building (as $USER)"
cargo build -q -p doze-app --features fixtures
cargo test -q -p doze-desk --test admin_prompt_e2e --no-run

# E2E_DRY=1 (for the script's own test, not for you): no prompt at all; steps 1 and 2 skipped, step 3 runs the root
# script as yourself.
if [[ -z "${E2E_DRY:-}" ]]; then
echo "==> 1 and 2: the app's own prompt (type the password, then press Cancel)"
DOZE_ADMIN_E2E=1 cargo test -q -p doze-desk --test admin_prompt_e2e -- --ignored --test-threads=1 --nocapture
fi

echo "==> 3: the app's run sheet (type the password once more)"
HOME_DIR="$WORK/home"
STACK="$WORK/stack"
BIN="$WORK/bin"
mkdir -p "$HOME_DIR/projects/e2e" "$STACK" "$BIN"
cat >"$STACK/doze.hcl" <<'HCL'
# A stack for the app to read; it is never started.
app "web" {
  run  = "sleep 1000"
  port = 39999
}
HCL
printf '{"schema":1,"config":"%s/doze.hcl","name":"e2e"}\n' "$STACK" >"$HOME_DIR/projects/e2e/project.json"

# The stand-in doze: names "not set up" until its dns-setup ran; dns-setup asks with the system prompt, as Go's doze
# does under DOZE_ADMIN_PROMPT=1, around a script that only prints.
cat >"$BIN/doze" <<'SH'
#!/bin/sh
here=$(cd "$(dirname "$0")" && pwd)
echo "$(date +%T) $* (DOZE_ADMIN_PROMPT=${DOZE_ADMIN_PROMPT:-})" >>"$here/calls.log"
script='echo "running as uid $(id -u)"; for i in 1 2 3; do echo "step $i of 3"; sleep 1; done'
case "$*" in
  "--json doctor")
    ok=false; [ -f "$here/set-up" ] && ok=true
    printf '{"schema":"doze/v1","command":"doze doctor","ok":%s,"data":{"ok":%s,"checks":[{"name":"names: network","detail":"e2e: not set up yet","ok":%s}]}}\n' $ok $ok $ok ;;
  "dns-setup --print") echo "$script" ;;
  "dns-setup")
    echo "› setting up (e2e stand-in: one admin prompt)"
    if [ -n "${E2E_DRY:-}" ]; then out=$(/bin/sh -c "$script" | sed 's/uid [0-9]*/uid 0 (dry run, not root)/'); else
    out=$(/usr/bin/osascript -e 'on run argv' -e 'do shell script (item 1 of argv) with prompt (item 2 of argv) with administrator privileges' -e 'end run' "/bin/sh -c '$script'" "doze needs an administrator's password to check the app's run sheet (doze e2e).") || {
      echo "the administrator prompt was cancelled" >&2; exit 1; }
    fi
    echo "$out" | tr '\r' '\n'
    touch "$here/set-up"
    echo "✓ done" ;;
  *) exit 1 ;;
esac
SH
chmod +x "$BIN/doze"

cat >"$WORK/drive.txt" <<'TXT'
wait 2500
key cmd-,
wait 800
key j
wait-for repair 20000
click repair
wait-for admin-run 15000
assert admin-step-0
assert Run (one password)
click admin-run
wait-for this machine is set up 180000
assert this machine is set up
wait 1500
quit
TXT

env -u DOZE_APP_ADMIN_RUNNER E2E_DRY="${E2E_DRY:-}" DOZE_HOME="$HOME_DIR" DOZE_ZONE=on PATH="$BIN:/usr/bin:/bin" \
  DOZE_APP_CLI="$BIN/doze" DOZE_APP_SCRIPT="$WORK/drive.txt" DOZE_APP_SCRIPT_LOG="$WORK/drive.log" \
  ./target/debug/doze-app >"$WORK/app.log" 2>&1 &
APP_PID=$!
for _ in $(seq 1 240); do kill -0 "$APP_PID" 2>/dev/null || break; sleep 1; done
APP_PID_DONE=$APP_PID
if kill -0 "$APP_PID" 2>/dev/null; then echo "the app did not finish in 4 minutes"; exit 1; fi
APP_PID=""

echo "==> what the app saw"
grep -E " (step|run|out|note|toast|ok|FAIL) " "$WORK/drive.log" | grep -v "^ *[0-9]* step" || true
echo "==> what the stand-in doze was asked"
cat "$BIN/calls.log"
if grep -q " FAIL " "$WORK/drive.log"; then echo "==> FAILED (app $APP_PID_DONE)"; exit 1; fi
grep -q "out running as uid 0" "$WORK/drive.log" || { echo "==> FAILED: no root output streamed into the sheet"; exit 1; }
echo "==> all passed"
