#!/bin/sh
# Records the Rust `doze down` against the Go daemon (docs/port/core/behaviours/cli-c2c.md, Oracle). Go's
# `doze __daemon` is started the way Go's daemonctl starts it, in a session of its own with its output in run/doze.log,
# under a private DOZE_HOME (never ~/.doze); it is detached twice so init reaps it, as it would be under a starter that
# exits. Then the Rust doze runs `--json down` in the project, and the daemon's pid, socket and pid file must be gone.
#
# Usage: down.sh <Go doze binary, built from the doze rust-oracle branch> <Rust doze binary (DOZE_BIN)>
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
mkdir -p "$work/p" "$DOZE_HOME"
printf 'stack = "oracle"\n\napp "web" {\n  run = "sleep 300"\n}\n' > "$work/p/doze.hcl"
cfg="$work/p/doze.hcl"

# setsid, the way Go's SysProcAttr{Setsid: true} does, in a grandchild of this shell.
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
echo "go daemon: pid $pid, socket $sock"

out=$(cd "$work/p" && "$rust" --json down) || {
	echo "rust doze down failed: $out" >&2
	exit 1
}
echo "$out"
echo "$out" | grep -q '"stopped": true' || { echo "not stopped" >&2; exit 1; }
if kill -0 "$pid" 2>/dev/null; then echo "pid $pid still alive" >&2; exit 1; fi
[ ! -e "$sock" ] || { echo "socket left: $sock" >&2; exit 1; }
[ ! -e "$run/doze.pid" ] || { echo "pid file left" >&2; exit 1; }
# What down remembered for the next up, in the record Go reads.
cat "$DOZE_HOME"/projects/*/project.json
echo "ok: the Rust doze down stopped Go's daemon (pid $pid) and left no pid, socket or pid file"
