#!/usr/bin/env bash
# Core plan C1's oracle: the Rust CLI's daemon commands against a running Go daemon, beside the Go CLI's own answer.
# A private doze home (never ~/.doze), a stack of three bun processes and a task (no modules, no network), brought up
# by the Go doze; then each command is run by both CLIs and stdout and the exit code compared. Meters (memory, CPU,
# "sleeps in") and durations are normalised; a verb is undone with the Go CLI between the two runs so each starts
# from the same state. The Go daemon is stopped at the end.
#
#   GODOZE=/path/to/go/doze RSDOZE=target/debug/doze scripts/c1-oracle-go.sh
#
# Needs bun on PATH. Exits 1 on any difference that is not listed as known below.
set -uo pipefail

: "${GODOZE:?the Go doze binary}"
: "${RSDOZE:?the Rust doze binary}"
RSDOZE=$(cd "$(dirname "$RSDOZE")" && pwd)/$(basename "$RSDOZE")
export DOZE_HOME=${DOZE_HOME:-/tmp/dz-c1-oracle}   # short: control sockets live under it
export DOZE_ZONE=off DOZE_DESK_INSTANCE=.c1oracle
case "$DOZE_HOME" in "$HOME/.doze"*) echo "refusing to use the real doze home" >&2; exit 2;; esac

stack=$(mktemp -d)
out=$(mktemp -d)
cd "$stack" || exit 2
cat > srv.ts <<'EOF'
const port = Number(process.env.PORT);
Bun.serve({ hostname: process.env.HOST || "127.0.0.1", port, fetch() { console.log("hit"); return new Response("ok"); } });
console.log(`listening on ${process.env.HOST}:${port}`);
console.error("a line on stderr");
EOF
cat > work.ts <<'EOF'
console.log("working against " + process.env.API);
setInterval(() => {}, 1000);
EOF
cat > doze.hcl <<'EOF'
stack = "c1"

app "api" {
  run  = "bun srv.ts"
  port = 18401
}

service "cache" {
  run  = "bun srv.ts"
  port = 18402
}

app "worker" {
  run = "bun work.ts"
  env = { API = app.api.address }
}

task "migrate" {
  run    = "echo migrated"
  manual = true
}
EOF

"$GODOZE" -y up >/dev/null 2>&1 || { echo "the Go doze did not bring the stack up" >&2; exit 2; }
trap '"$GODOZE" down >/dev/null 2>&1' EXIT

norm() {
  sed -E 's/[0-9]+(K|M|\.[0-9]G) /N /g; s/"(memBytes|ram|cpu|cpuPercent|rss|pid)": [0-9.e+-]+/"\1": N/g;
          s/sleeps in [0-9a-z:]+/sleeps in T/g; s/ in [0-9.]+(ms|s|µs|m[0-9.]*s?)/ in T/g;
          s/"took_ms": [0-9]+/"took_ms": N/g; s/[0-9]{2}:[0-9]{2}:[0-9]{2} /HH:MM:SS /g;
          s/[0-9]+%/-/g; s/ · -//g; s/ +/ /g' "$1"
}

fails=0
compare() { # $1: the command line; $2: what undoes it (empty for a read)
  read -ra cmd <<<"$1"
  read -ra undo <<<"${2:-}"
  "$GODOZE" "${cmd[@]}" >"$out/go" 2>/dev/null; local ge=$?
  [ ${#undo[@]} -gt 0 ] && "$GODOZE" "${undo[@]}" >/dev/null 2>&1
  "$RSDOZE" "${cmd[@]}" >"$out/rs" 2>/dev/null; local re=$?
  [ ${#undo[@]} -gt 0 ] && "$GODOZE" "${undo[@]}" >/dev/null 2>&1
  if [ "$ge" != "$re" ] || ! diff <(norm "$out/go") <(norm "$out/rs") >"$out/diff"; then
    echo "DIFF  doze $1  (exit go $ge, rust $re)"; head -20 "$out/diff"; fails=$((fails + 1))
  else
    echo "same  doze $1"
  fi
}

for c in "status" "status --json" "status --graph" "ps" "logs api" "logs" "--json logs api" "logs worker -n 1" \
         "logs api --run -1" "logs api --since 1h" "logs --run 99 api" "logs nope" "wake nope" "task nope" \
         "task api" "-q status" "pin --off cache"; do
  compare "$c"
done
for pair in "sleep cache|wake cache" "sleep|wake" "restart api|-q status" "pin cache|pin --off cache" \
            "pin --toggle cache|pin --off cache" "off worker|on worker" "on worker|-q status" "task migrate|-q status" \
            "-y reset cache|-q status" "--json sleep cache|wake cache" "--json pin cache|pin --off cache"; do
  compare "${pair%%|*}" "${pair#*|}"
done

# A task's `manual` flag is read from doze.hcl (C2): before the task first runs, Go's list says "manual — never
# run" and this one "never ran". The task has run above, so the lists agree here.
compare "task"
echo "$fails difference(s)"
[ "$fails" -eq 0 ] || exit 1
