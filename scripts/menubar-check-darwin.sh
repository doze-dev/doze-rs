#!/usr/bin/env bash
# The menu-bar item by hand, in about two minutes: the last, human check of what the scripted run checked line by line
# (every menu line chosen the way a click chooses it, on a real daemon). Run as yourself:
#
#   DOZE=/path/to/rust/doze ./scripts/menubar-check-darwin.sh     # a real daemon on a throwaway stack (needs bun)
#   ./scripts/menubar-check-darwin.sh                              # no DOZE: the app's fixture daemon instead
#
# DOZE is a Rust doze that serves a stack (`doze __daemon --config …`, from the core branches). Everything runs on a
# private, throwaway DOZE_HOME under one temporary folder: it never touches ~/.doze, your Doze.app or your daemons,
# and it stops only the processes it started (by their PIDs). The app is a development build (cargo, doze-app).
set -euo pipefail

[[ "$(uname -s)" == Darwin ]] || { echo "macOS only"; exit 2; }
cd "$(dirname "$0")/.."
# /tmp, not $TMPDIR: the stack's socket path under it must stay within the 104 bytes a unix socket allows.
WORK="$(mktemp -d "${DOZE_MENUBAR_TMP:-/tmp}/doze-menubar.XXXXXX")"
DAEMON_PID=""
APP_PID=""
cleanup() {
  set +e
  [[ -n "$APP_PID" ]] && kill -0 "$APP_PID" 2>/dev/null && kill "$APP_PID"
  [[ -n "$DAEMON_PID" ]] && kill -0 "$DAEMON_PID" 2>/dev/null && kill "$DAEMON_PID"
  echo "    logs are under $WORK"
}
trap cleanup EXIT

echo "==> building the app"
cargo build -q -p doze-app --features fixtures
APP="${CARGO_TARGET_DIR:-target}/debug/doze-app"

export DOZE_HOME="$WORK/home" DOZE_ZONE=off
mkdir -p "$DOZE_HOME"
if [[ -n "${DOZE:-}" ]]; then
  command -v bun >/dev/null || { echo "the throwaway stack's apps run on bun; install it or leave DOZE unset"; exit 2; }
  STACK="$WORK/shop"
  mkdir -p "$STACK"
  cat >"$STACK/doze.hcl" <<'HCL'
stack = "shop"

defaults {
  sleep = "2m"
}

app "api" {
  run   = "bun serve.ts"
  port  = 38181
  ready = http("/healthz")
}

app "web" {
  run   = "bun serve.ts"
  port  = 38180
  ready = http("/healthz")
  env   = { API_URL = app.api.url }
}

app "flaky" {
  run   = "bun flaky.ts"
  port  = 38182
  ready = http("/healthz")
}
HCL
  cat >"$STACK/serve.ts" <<'TS'
Bun.serve({ hostname: process.env.HOST ?? "127.0.0.1", port: Number(process.env.PORT), fetch: () => new Response("ok") });
console.log(`listening on ${process.env.PORT}`);
TS
  cat >"$STACK/flaky.ts" <<TS
// Exits at once while $WORK/crash exists: the crash that puts an item in Needs you.
import { existsSync } from "node:fs";
if (existsSync("$WORK/crash")) { console.error("flaky: crash flag set"); process.exit(3); }
Bun.serve({ hostname: process.env.HOST ?? "127.0.0.1", port: Number(process.env.PORT), fetch: () => new Response("ok") });
TS
  echo "==> a throwaway daemon for $STACK"
  (cd "$STACK" && exec "$DOZE" __daemon --config "$STACK/doze.hcl") >"$WORK/daemon.log" 2>&1 &
  DAEMON_PID=$!
  sleep 3
  kill -0 "$DAEMON_PID" 2>/dev/null || { echo "the daemon did not start:"; cat "$WORK/daemon.log"; exit 1; }
  echo "==> the app (it opens its window; the bat is in the menu bar)"
  DOZE_APP_CLI="$DOZE" "$APP" "$STACK" >"$WORK/app.log" 2>&1 &
  APP_PID=$!
  CRASH="touch $WORK/crash, then"
  UNCRASH="rm $WORK/crash, then"
else
  echo "==> no DOZE: the app's fixture daemon (shop); its 'web' crashes 60 s in and is up again 50 s later"
  DOZE_APP_FIXTURE_CRASH=web@60+110 "$APP" --fixture shop >"$WORK/app.log" 2>&1 &
  APP_PID=$!
  CRASH="wait for the minute mark;"
  UNCRASH="(or wait, it comes back by itself)"
fi

cat <<EOF

Ten clicks. Each says what you should see; note any that does not.

  1. The bat in the menu bar: about as tall as the icons beside it, crisp, a solid perch on top. Click it: the menu
     opens (Stacks › shop, Open doze, Search or do…, Settings…, Check for Updates…, Quit doze).
  2. Open doze: the window comes forward on Home.
  3. shop › Services › web › Wake: a moment later the line says "web · awake" (open the menu again to see).
  4. shop › Services › web › Logs: the window shows web's page on its Logs tab.
  5. shop › Services › api › Sleep (with web awake, which needs api): the window asks "Sleep api?"; press Cancel.
  6. $CRASH shop › Services › flaky › Wake (fixture: wait for web to crash): the bat gets its dot, and the menu
     starts with "Needs you · 1".
  7. $UNCRASH Needs you › the item › Restart: the item goes and the dot with it.
  8. Search or do…: the window opens its palette.
  9. Check for Updates…: Settings › About, saying which release is the newest (it only reads GitHub).
 10. Quit doze · stacks keep running: the app and the bat go; this script then says whether the daemon still runs.

EOF
wait "$APP_PID" || true
APP_PID=""
if [[ -n "$DAEMON_PID" ]]; then
  if kill -0 "$DAEMON_PID" 2>/dev/null; then
    echo "==> after Quit: the stack's daemon still runs (pid $DAEMON_PID), as it should; stopping it now"
  else
    echo "==> after Quit: the stack's daemon is GONE; Quit should have left it running"
  fi
fi
