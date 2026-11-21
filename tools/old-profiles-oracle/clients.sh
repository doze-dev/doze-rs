#!/usr/bin/env bash
# Old clients end to end against doze-kafka at --kafka-version 1, 2 and 3 (debt K2):
#   tools/old-profiles-oracle/clients.sh <doze-kafka binary> <first port> <out dir>
#
# - librdkafka (kcat) with api.version.request=false and broker.version.fallback 0.8.2.2 (Produce v0, Fetch v0, ListOffsets
#   v0, message format v0), 0.9.0.1 (Produce v1, Fetch v1, the group APIs, a balanced consumer) and 0.10.0.1 (Produce v2,
#   Fetch v2, message format v1), each codec;
# - Kafka 1.1's own tools from confluentinc/cp-kafka:4.1.4 in Docker: the console producer and consumer (the Java client,
#   which negotiates), and the SimpleConsumerShell (the old Scala consumer: Fetch and ListOffsets of its own versions).
# Each produces and reads back what it produced. Uses the ports <first port> to <first port>+5. Prints one line per cell and
# exits non-zero if any failed.
set -uo pipefail
bin="$1"
port0="$2"
out="$3"
mkdir -p "$out"
failed=0
pid=""
stop() { if [[ -n "$pid" ]]; then kill "$pid" 2>/dev/null; wait "$pid" 2>/dev/null; pid=""; fi; }
trap stop EXIT
start() { # start <major> <listen> <advertised host> <port>
  local data
  data="$(mktemp -d "$out/clients-$1.XXXX")"
  DOZE_ZONE=off "$bin" --listen "$2:$4" --advertised-host "$3" --data-dir "$data/data" --meta-db "$data/meta.db" \
    --kafka-version "$1" --console=false --schema-registry=false --group-initial-rebalance-delay 0s > "$data/broker.log" 2>&1 &
  pid=$!
  for _ in $(seq 1 100); do nc -z 127.0.0.1 "$4" 2>/dev/null && return 0; sleep 0.1; done
  echo "doze-kafka did not start ($data/broker.log)"; return 1
}
cell() { # cell <name> <want> <got>
  if [[ "$2" == "$3" ]]; then echo "pass $1"; else echo "FAIL $1: want [$2] got [$3]"; failed=1; fi
}
want=$(printf 'm1\nm2\nm3\nm4\nm5')
for major in 1 2 3; do
  port=$((port0 + major))
  start "$major" 127.0.0.1 127.0.0.1 "$port" || { failed=1; continue; }
  b="127.0.0.1:$port"
  for fallback in 0.8.2.2 0.9.0.1 0.10.0.1; do
    for codec in none gzip snappy lz4; do
      t="rd-${fallback//./}-$codec"
      x=(-X api.version.request=false -X broker.version.fallback="$fallback")
      printf '%s\n' m1 m2 m3 m4 m5 | kcat -P -b "$b" -t "$t" -z "$codec" "${x[@]}" 2> "$out/kcat-$major-$t.err"
      got=$(kcat -C -b "$b" -t "$t" -o beginning -c 5 -e -q "${x[@]}" 2>> "$out/kcat-$major-$t.err")
      cell "kafka-version $major librdkafka fallback $fallback $codec" "$want" "$got"
    done
    if [[ "$fallback" != 0.8.2.2 ]]; then
      t="rd-${fallback//./}-none"
      got=$(perl -e 'alarm 60; exec @ARGV' kcat -G "g-${fallback//./}" -b "$b" -o beginning -c 5 -q "${x[@]}" "$t" 2> "$out/kcat-$major-group-$fallback.err")
      cell "kafka-version $major librdkafka fallback $fallback balanced consumer" "$want" "$got"
    fi
  done
  stop
  # Kafka 1.1's tools run in Docker and reach the broker on the host's address there.
  port=$((port0 + 3 + major))
  start "$major" 0.0.0.0 host.docker.internal "$port" || { failed=1; continue; }
  b="host.docker.internal:$port"
  img=confluentinc/cp-kafka:4.1.4
  for codec in none gzip snappy lz4; do
    t="java-$codec"
    printf '%s\n' m1 m2 m3 m4 m5 | docker run --rm -i --entrypoint kafka-console-producer "$img" \
      --broker-list "$b" --topic "$t" --compression-codec "$codec" > "$out/java-$major-$t.log" 2>&1
    got=$(docker run --rm --entrypoint kafka-console-consumer "$img" --bootstrap-server "$b" --topic "$t" \
      --from-beginning --max-messages 5 --timeout-ms 30000 2> "$out/java-$major-$t.err" | grep '^m')
    cell "kafka-version $major Kafka 1.1 console producer and consumer $codec" "$want" "$got"
    got=$(docker run --rm --entrypoint kafka-simple-consumer-shell "$img" --broker-list "$b" --topic "$t" --partition 0 \
      --offset -2 --max-messages 5 --no-wait-at-logend 2> "$out/simple-$major-$t.err" | grep '^m')
    cell "kafka-version $major Kafka 1.1 SimpleConsumerShell $codec" "$want" "$got"
  done
  stop
done
exit "$failed"
