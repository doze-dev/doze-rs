#!/usr/bin/env bash
# Starts a release broker on a free port with a fresh data directory, runs the Go clients against it
# (sarama, franz-go, kafka-go, librdkafka), stops the broker, and exits with the clients' status.
# Usage: interop/run.sh [-only <scenario>]   (see interop/README.md)
set -euo pipefail
command -v kcat >/dev/null || { echo "interop needs kcat (librdkafka): brew install kcat" >&2; exit 2; }
command -v cc >/dev/null || { echo "interop needs a C compiler (it builds a librdkafka transactional producer)" >&2; exit 2; }
command -v go >/dev/null || { echo "interop needs go" >&2; exit 2; }
here="$(cd "$(dirname "$0")" && pwd)"
root="$(dirname "$here")"
work="$(mktemp -d)"
trap 'if [ -n "${broker:-}" ]; then kill "$broker" 2>/dev/null; wait "$broker" 2>/dev/null; fi; rm -rf "$work"' EXIT

# librdkafka's transactional producer (kcat has none): a small C program built against the librdkafka kcat uses.
prefix="$(brew --prefix librdkafka 2>/dev/null || echo /usr/local)"
cc -o "$work/txnprod" "$here/c/txnprod.c" -I"$prefix/include" -L"$prefix/lib" -lrdkafka
export TXNPROD="$work/txnprod"

cargo build --release -p doze-kafka --manifest-path "$root/Cargo.toml"
port="$(( (RANDOM % 20000) + 30000 ))"
"$root/target/release/doze-kafka" --listen "127.0.0.1:$port" --data-dir "$work/data" \
  --default-partitions 3 --consumer-session-timeout 6s >"$work/broker.log" 2>&1 &
broker=$!
sleep 1
(cd "$here/go" && go run . -broker "127.0.0.1:$port" "$@") || { cat "$work/broker.log"; exit 1; }
