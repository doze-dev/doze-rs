#!/bin/sh
# Records the Rust `doze up` against the Go daemon, and beside the Go `doze up` (docs/port/core/behaviours/cli-c2c.md,
# Oracle). Go's `doze __daemon` is started as down.sh starts it: in a session of its own, under a private DOZE_HOME
# (never ~/.doze), detached twice so init reaps it. Then, in the project:
#   1. the Rust `doze up` boots the app through Go's daemon (cold);
#   2. the Go `doze up` and the Rust `doze up` run again (warm), and their stdout and stderr must match once the
#      durations are masked;
#   3. the Rust `doze --json down` stops Go's daemon and leaves no pid, socket or pid file.
#
# Usage: up.sh <Go doze binary, built from the doze rust-oracle branch> <Rust doze binary (DOZE_BIN)>
# (The trust hint is not recorded here: an app with `domains` needs the machine's zone, which DOZE_ZONE=off keeps off.)
set -eu
go=$1
rust=$2
# Short: a Unix socket path must fit in 104 bytes on macOS.
work=$(mktemp -d /tmp/dzo.XXXXXX)
pid=
cleanup() {
	# Only the exact pid this script started, and only while it is still alive.
	if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then kill "$pid"; fi
	rm -rf "$work"
}
trap cleanup EXIT
export DOZE_HOME="$work/home"
export DOZE_ZONE=off
export NO_COLOR=1
mkdir -p "$work/p" "$DOZE_HOME"
printf 'stack = "oracle"\n\napp "web" {\n  run = "sleep 300"\n}\n' > "$work/p/doze.hcl"
cfg="$work/p/doze.hcl"

perl -MPOSIX -e 'exit 0 if fork; POSIX::setsid() or die "setsid: $!"; exec @ARGV or die "exec: $!"' \
	"$go" __daemon --config "$cfg" > "$work/daemon.out" 2>&1
sock=
i=0
while [ $i -lt 100 ]; do
	for s in "$DOZE_HOME"/projects/*/run/doze.admin.sock; do
		[ -S "$s" ] && sock=$s
	done
	[ -n "$sock" ] && [ -s "$(dirname "$sock")/doze.pid" ] && break
	sleep 0.1
	i=$((i + 1))
done
if [ -z "$sock" ]; then
	echo "Go's daemon did not come up:" >&2
	cat "$work/daemon.out" "$DOZE_HOME"/projects/*/run/doze.log >&2 2>/dev/null || true
	exit 1
fi
run=$(dirname "$sock")
pid=$(head -n 1 "$run/doze.pid")
echo "go daemon: pid $pid"

# Durations differ run to run: "in 38ms", "in 1.2s".
mask() { sed -E 's/ in [0-9.]+(µs|ms|s)/ in <t>/' "$1"; }
up() { (cd "$work/p" && "$1" up web > "$work/$2.out" 2> "$work/$2.err"); }

up "$rust" rust-cold || { echo "rust doze up (cold) failed:" >&2; cat "$work/rust-cold.out" "$work/rust-cold.err" >&2; exit 1; }
echo "--- rust up (cold): stdout"; cat "$work/rust-cold.out"
echo "--- rust up (cold): stderr"; cat "$work/rust-cold.err"
up "$go" go-warm || { echo "go doze up failed:" >&2; cat "$work/go-warm.err" >&2; exit 1; }
up "$rust" rust-warm || { echo "rust doze up (warm) failed:" >&2; cat "$work/rust-warm.err" >&2; exit 1; }
for s in out err; do
	mask "$work/go-warm.$s" > "$work/go.$s.m"
	mask "$work/rust-warm.$s" > "$work/rust.$s.m"
	if ! diff -u "$work/go.$s.m" "$work/rust.$s.m"; then
		echo "the Rust doze up's std$s differs from Go's" >&2
		exit 1
	fi
done
echo "--- go and rust up (warm) agree: stdout"; cat "$work/rust.out.m"
echo "--- stderr"; cat "$work/rust.err.m"

out=$(cd "$work/p" && "$rust" --json down) || { echo "rust doze down failed: $out" >&2; exit 1; }
echo "$out" | grep -q '"stopped": true' || { echo "not stopped: $out" >&2; exit 1; }
if kill -0 "$pid" 2>/dev/null; then echo "pid $pid still alive" >&2; exit 1; fi
[ ! -e "$sock" ] || { echo "socket left: $sock" >&2; exit 1; }
echo "ok: the Rust doze up booted through Go's daemon (pid $pid), matched Go's up, and down left nothing"
