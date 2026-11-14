#!/usr/bin/env bash
# Starts a release broker on a free port with a fresh data directory, runs the Go clients against it
# (sarama, franz-go, kafka-go, librdkafka), stops the broker, and exits with the clients' status.
# Usage: interop/run.sh [-only <scenario>]   (see interop/README.md)
#        interop/run.sh -sasl                 the SASL cells instead: PLAIN, SCRAM-SHA-256 and SCRAM-SHA-512 for each library
#        interop/run.sh -tls                  the TLS cells: builds the broker with --features tls, makes a throwaway CA and certificate with
#                                             openssl, and runs each library over SASL_SSL (SCRAM-SHA-512) and over SSL (no SASL)
#        interop/run.sh -kafka-version N      the broker stands for Kafka major N (1 to 4, default 4): started with --kafka-version N, and
#                                             the clients configured for it (-kafka-major N); combines with -only, -sasl and -tls
#        interop/run.sh -versions             once per major, 1 to 4 (a broker and a fresh data directory each), then a summary per
#                                             major; with -sasl, the SASL cells per major
set -euo pipefail
sasl=""
tls=""
major=""
versions=""
rest=()
while [ $# -gt 0 ]; do
  case "$1" in
    -sasl) sasl=1; shift ;;
    -tls) tls=1; shift ;;
    -versions) versions=1; shift ;;
    -kafka-version)
      major="${2:-}"
      case "$major" in 1 | 2 | 3 | 4) ;; *) echo "-kafka-version takes 1, 2, 3 or 4" >&2; exit 2 ;; esac
      shift 2 ;;
    *) rest+=("$1"); shift ;;
  esac
done
set -- ${rest[@]+"${rest[@]}"}

# -versions: this script once per major, each with a broker of its own; the per-major summary counts the lines the Go program
# prints (ok, skip, FAIL), and the skips are listed with their reasons.
if [ -n "$versions" ]; then
  if [ -n "$major" ]; then echo "-versions runs every major; leave out -kafka-version" >&2; exit 2; fi
  out="$(mktemp -d)"
  trap 'rm -rf "$out"' EXIT
  status=0
  for m in 1 2 3 4; do
    echo "=== Kafka $m ==="
    if ! "$0" ${sasl:+-sasl} ${tls:+-tls} -kafka-version "$m" "$@" 2>&1 | tee "$out/$m.log"; then status=1; echo "Kafka $m: FAILED"; fi
  done
  echo
  echo "=== per major ==="
  for m in 1 2 3 4; do
    printf 'Kafka %s: %3d passed, %2d skipped, %2d failed\n' "$m" \
      "$(grep -c '^ok ' "$out/$m.log" || true)" "$(grep -c '^skip ' "$out/$m.log" || true)" "$(grep -c '^FAIL' "$out/$m.log" || true)"
    grep -E '^(skip|FAIL)' "$out/$m.log" | sed 's/^/    /' || true
  done
  exit "$status"
fi
major="${major:-4}"
set -- -kafka-major "$major" "$@"
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
common=(--kafka-version "$major" --default-partitions 3 --consumer-session-timeout 6s --log-cleaner-interval 500ms)
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
