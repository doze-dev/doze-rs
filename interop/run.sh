#!/usr/bin/env bash
# Starts a release broker on a free port with a fresh data directory, runs the Go clients against it
# (sarama, franz-go, kafka-go, librdkafka), stops the broker, and exits with the clients' status.
# Usage: interop/run.sh [-only <scenario>]   (see interop/README.md)
#        interop/run.sh -sasl                 the SASL cells instead: PLAIN, SCRAM-SHA-256 and SCRAM-SHA-512 for each library
set -euo pipefail
sasl=""
if [ "${1:-}" = "-sasl" ]; then sasl=1; shift; fi
command -v kcat >/dev/null || { echo "interop needs kcat (librdkafka): brew install kcat" >&2; exit 2; }
command -v cc >/dev/null || { echo "interop needs a C compiler (it builds a librdkafka transactional producer)" >&2; exit 2; }
command -v go >/dev/null || { echo "interop needs go" >&2; exit 2; }
here="$(cd "$(dirname "$0")" && pwd)"
root="$(dirname "$here")"
work="$(mktemp -d)"
trap 'if [ -n "${broker:-}" ]; then kill "$broker" 2>/dev/null; wait "$broker" 2>/dev/null; fi; [ -z "${KEEP_LOG:-}" ] || cp "$work/broker.log" "$KEEP_LOG"; rm -rf "$work"' EXIT

# librdkafka's transactional producer (kcat has none): a small C program built against the librdkafka kcat uses.
if [ -z "$sasl" ]; then
  prefix="$(brew --prefix librdkafka 2>/dev/null || echo /usr/local)"
  cc -o "$work/txnprod" "$here/c/txnprod.c" -I"$prefix/include" -L"$prefix/lib" -lrdkafka
  export TXNPROD="$work/txnprod"
fi

cargo build --release -p doze-kafka --manifest-path "$root/Cargo.toml"
port="$(( (RANDOM % 20000) + 30000 ))"
# -sasl: every mechanism on, one user; the clients log in with PLAIN and both SCRAM hashes (interop/go/sasl.go).
security=()
if [ -n "$sasl" ]; then
  security=(--sasl-mechanisms PLAIN,SCRAM-SHA-512,SCRAM-SHA-256 --sasl-user "alice:${SASL_PASSWORD:-alice-secret-pw}")
  set -- -sasl "$@"
fi
"${CARGO_TARGET_DIR:-$root/target}/release/doze-kafka" --listen "127.0.0.1:$port" --data-dir "$work/data" \
  --default-partitions 3 --consumer-session-timeout 6s --log-cleaner-interval 500ms \
  ${security[@]+"${security[@]}"} >"$work/broker.log" 2>&1 &
broker=$!
sleep 1
(cd "$here/go" && go run . -broker "127.0.0.1:$port" "$@") || { cat "$work/broker.log"; exit 1; }
