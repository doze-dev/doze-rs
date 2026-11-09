#!/usr/bin/env bash
# Starts a release broker on a free port with a fresh data directory, runs the Go clients against it
# (sarama, franz-go, kafka-go), stops the broker, and exits with the clients' status.
# Usage: interop/run.sh [-only sarama|franz-go|kafka-go]
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(dirname "$here")"
work="$(mktemp -d)"
trap 'if [ -n "${broker:-}" ]; then kill "$broker" 2>/dev/null; wait "$broker" 2>/dev/null; fi; rm -rf "$work"' EXIT

cargo build --release -p doze-kafka --manifest-path "$root/Cargo.toml"
port="$(( (RANDOM % 20000) + 30000 ))"
"$root/target/release/doze-kafka" --listen "127.0.0.1:$port" --data-dir "$work/data" \
  --default-partitions 3 >"$work/broker.log" 2>&1 &
broker=$!
sleep 1
(cd "$here/go" && go run . -broker "127.0.0.1:$port" "$@") || { cat "$work/broker.log"; exit 1; }
