#!/usr/bin/env bash
# Starts a release broker on a free port with a fresh data directory, runs the Go clients against it
# (sarama, franz-go, kafka-go, librdkafka), stops the broker, and exits with the clients' status.
# Usage: interop/run.sh [-only <scenario>]   (see interop/README.md)
#        interop/run.sh -sasl                 the SASL cells instead: PLAIN, SCRAM-SHA-256 and SCRAM-SHA-512 for each library
#        interop/run.sh -tls                  the TLS cells: builds the broker with --features tls, makes a throwaway CA and certificate with
#                                             openssl, and runs each library over SASL_SSL (SCRAM-SHA-512) and over SSL (no SASL)
set -euo pipefail
sasl=""
tls=""
if [ "${1:-}" = "-sasl" ]; then sasl=1; shift; fi
if [ "${1:-}" = "-tls" ]; then tls=1; shift; fi
command -v kcat >/dev/null || { echo "interop needs kcat (librdkafka): brew install kcat" >&2; exit 2; }
command -v cc >/dev/null || { echo "interop needs a C compiler (it builds a librdkafka transactional producer)" >&2; exit 2; }
command -v go >/dev/null || { echo "interop needs go" >&2; exit 2; }
here="$(cd "$(dirname "$0")" && pwd)"
root="$(dirname "$here")"
work="$(mktemp -d)"
trap 'for pid in "${broker:-}" "${broker_ssl:-}"; do if [ -n "$pid" ]; then kill "$pid" 2>/dev/null; wait "$pid" 2>/dev/null; fi; done; [ -z "${KEEP_LOG:-}" ] || cp "$work/broker.log" "$KEEP_LOG"; rm -rf "$work"' EXIT

# librdkafka's transactional producer (kcat has none): a small C program built against the librdkafka kcat uses.
if [ -z "$sasl" ] && [ -z "$tls" ]; then
  prefix="$(brew --prefix librdkafka 2>/dev/null || echo /usr/local)"
  cc -o "$work/txnprod" "$here/c/txnprod.c" -I"$prefix/include" -L"$prefix/lib" -lrdkafka
  export TXNPROD="$work/txnprod"
fi

features=()
if [ -n "$tls" ]; then features=(--features tls); fi
cargo build --release -p doze-kafka ${features[@]+"${features[@]}"} --manifest-path "$root/Cargo.toml"
port="$(( (RANDOM % 20000) + 30000 ))"
# -sasl: every mechanism on, one user; the clients log in with PLAIN and both SCRAM hashes (interop/go/sasl.go).
security=()
if [ -n "$sasl" ]; then
  security=(--sasl-mechanisms PLAIN,SCRAM-SHA-512,SCRAM-SHA-256 --sasl-user "alice:${SASL_PASSWORD:-alice-secret-pw}")
  set -- -sasl "$@"
fi
bin="${CARGO_TARGET_DIR:-$root/target}/release/doze-kafka"
common=(--default-partitions 3 --consumer-session-timeout 6s --log-cleaner-interval 500ms)
# -tls: a throwaway CA and a server certificate for localhost and 127.0.0.1 (two steps for the key: LibreSSL, which macOS ships, writes
# a key the broker cannot read from the one-line `req -newkey ec`; -sha256 because LibreSSL signs with SHA-1 otherwise). Two brokers:
# one with TLS and SCRAM-SHA-512 (SASL_SSL) and one with TLS alone (SSL).
if [ -n "$tls" ]; then
  certs="$work/certs"; mkdir "$certs"
  openssl ecparam -name prime256v1 -genkey -noout -out "$certs/ca.key"
  openssl req -x509 -new -sha256 -key "$certs/ca.key" -out "$certs/ca.pem" -days 2 -subj "/CN=doze interop CA" \
    -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" >/dev/null 2>&1
  openssl ecparam -name prime256v1 -genkey -noout -out "$certs/key.pem"
  openssl req -new -key "$certs/key.pem" -out "$certs/leaf.csr" -subj "/CN=localhost" >/dev/null 2>&1
  printf 'basicConstraints=CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n' >"$certs/leaf.ext"
  openssl x509 -req -in "$certs/leaf.csr" -CA "$certs/ca.pem" -CAkey "$certs/ca.key" -CAcreateserial -sha256 -out "$certs/cert.pem" \
    -days 2 -extfile "$certs/leaf.ext" >/dev/null 2>&1
  port_ssl="$(( port + 1 ))"
  "$bin" --listen "127.0.0.1:$port_ssl" --data-dir "$work/data-ssl" "${common[@]}" \
    --tls-cert "$certs/cert.pem" --tls-key "$certs/key.pem" >"$work/broker-ssl.log" 2>&1 &
  broker_ssl=$!
  security=(--tls-cert "$certs/cert.pem" --tls-key "$certs/key.pem" --sasl-mechanisms SCRAM-SHA-512 --sasl-user "alice:${SASL_PASSWORD:-alice-secret-pw}")
  set -- -tls-ca "$certs/ca.pem" -broker-ssl "127.0.0.1:$port_ssl" "$@"
fi
"$bin" --listen "127.0.0.1:$port" --data-dir "$work/data" "${common[@]}" \
  ${security[@]+"${security[@]}"} >"$work/broker.log" 2>&1 &
broker=$!
sleep 1
(cd "$here/go" && go run . -broker "127.0.0.1:$port" "$@") || { cat "$work/broker.log"; [ -z "$tls" ] || cat "$work/broker-ssl.log"; exit 1; }
