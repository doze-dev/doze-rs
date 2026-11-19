#!/usr/bin/env bash
# The reference stack for the registry comparison: a real Kafka (apache/kafka, KRaft, one node) and Confluent's
# cp-schema-registry 7.7.1 and 8.x in Docker, and doze-kafka with its registry beside them. Every port is in 40000-40099.
#
#   tools/registry-diff/stack.sh up [doze-kafka binary]   start all four (fresh state each time) and wait for them
#   tools/registry-diff/stack.sh down                     stop them (containers by name, doze-kafka by its pid file)
#   tools/registry-diff/stack.sh doze-restart [binary]    stop doze-kafka gracefully (TERM) and start it (or another build) on the same data
#   tools/registry-diff/stack.sh doze-kill                kill doze-kafka (KILL) and start it on the same data
#
# Ports: Kafka 40001 (host clients), Confluent 7.7.1 40081, Confluent 8.x 40082, doze-kafka broker 40010, its registry 40011,
# its console 40012. The 8.x tag is CONFLUENT_8 (default 8.3.2). State lives in $REGDIFF_WORK (default
# /tmp/doze-registry-diff); doze-kafka's log is $REGDIFF_WORK/doze.log.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
work="${REGDIFF_WORK:-/tmp/doze-registry-diff}"
c8="${CONFLUENT_8:-8.3.2}"
net=regdiff-net
kafka=regdiff-kafka

doze_bin() {
  if [ -n "${1:-}" ]; then echo "$1"; return; fi
  echo "${CARGO_TARGET_DIR:-$root/target}/release/doze-kafka"
}

wait_http() { # url name
  for _ in $(seq 1 180); do
    if curl -fsS -o /dev/null "$1" 2>/dev/null; then echo "$2 up at $1"; return 0; fi
    sleep 1
  done
  echo "$2 did not come up at $1" >&2
  return 1
}

start_doze() {
  local bin
  bin="$(cat "$work/doze.bin")"
  "$bin" --listen 127.0.0.1:40010 --data-dir "$work/doze-data" --schema-registry-addr 127.0.0.1:40011 \
    --console-addr 127.0.0.1:40012 >>"$work/doze.log" 2>&1 &
  echo $! >"$work/doze.pid"
  wait_http http://127.0.0.1:40011/ "doze-kafka registry"
}

stop_doze() { # signal
  [ -f "$work/doze.pid" ] || return 0
  local pid
  pid="$(cat "$work/doze.pid")"
  kill "-$1" "$pid" 2>/dev/null || true
  for _ in $(seq 1 50); do kill -0 "$pid" 2>/dev/null || break; sleep 0.2; done
  rm -f "$work/doze.pid"
}

down() {
  stop_doze TERM
  docker rm -f regdiff-sr7 regdiff-sr8 "$kafka" >/dev/null 2>&1 || true
  docker network rm "$net" >/dev/null 2>&1 || true
}

# Each registry its own leader group: with the default group the 8.x instance forwards every write to 7.7.1 (its leader),
# which writes it to its own topic, so 8.x never sees its own registrations and 7.7.1 sees both.
sr() { # name tag port
  docker run -d --name "$1" --network "$net" -p "127.0.0.1:$3:8081" \
    -e SCHEMA_REGISTRY_HOST_NAME="$1" \
    -e SCHEMA_REGISTRY_LISTENERS=http://0.0.0.0:8081 \
    -e SCHEMA_REGISTRY_KAFKASTORE_BOOTSTRAP_SERVERS="$kafka:9092" \
    -e SCHEMA_REGISTRY_KAFKASTORE_TOPIC="_schemas_$1" \
    -e SCHEMA_REGISTRY_SCHEMA_REGISTRY_GROUP_ID="$1" \
    "confluentinc/cp-schema-registry:$2" >/dev/null
}

case "${1:-}" in
  up)
    down
    rm -rf "$work"
    mkdir -p "$work/doze-data"
    doze_bin "${2:-}" >"$work/doze.bin"
    docker network create "$net" >/dev/null
    docker run -d --name "$kafka" --network "$net" -p 127.0.0.1:40001:40001 \
      -e KAFKA_NODE_ID=1 -e KAFKA_PROCESS_ROLES=broker,controller \
      -e KAFKA_LISTENERS=PLAINTEXT://:9092,CONTROLLER://:9093,EXTERNAL://:40001 \
      -e KAFKA_ADVERTISED_LISTENERS="PLAINTEXT://$kafka:9092,EXTERNAL://127.0.0.1:40001" \
      -e KAFKA_LISTENER_SECURITY_PROTOCOL_MAP=PLAINTEXT:PLAINTEXT,CONTROLLER:PLAINTEXT,EXTERNAL:PLAINTEXT \
      -e KAFKA_CONTROLLER_LISTENER_NAMES=CONTROLLER -e KAFKA_INTER_BROKER_LISTENER_NAME=PLAINTEXT \
      -e KAFKA_CONTROLLER_QUORUM_VOTERS="1@$kafka:9093" \
      -e KAFKA_OFFSETS_TOPIC_REPLICATION_FACTOR=1 -e KAFKA_TRANSACTION_STATE_LOG_REPLICATION_FACTOR=1 \
      -e KAFKA_TRANSACTION_STATE_LOG_MIN_ISR=1 -e KAFKA_GROUP_INITIAL_REBALANCE_DELAY_MS=0 \
      apache/kafka:3.8.0 >/dev/null
    sr regdiff-sr7 7.7.1 40081
    sr regdiff-sr8 "$c8" 40082
    start_doze
    wait_http http://127.0.0.1:40081/ "Confluent 7.7.1"
    wait_http http://127.0.0.1:40082/ "Confluent $c8"
    ;;
  down) down ;;
  doze-restart) stop_doze TERM; [ -z "${2:-}" ] || echo "$2" >"$work/doze.bin"; start_doze ;;
  doze-kill) stop_doze KILL; start_doze ;;
  *) echo "usage: $0 up [doze-kafka binary] | down | doze-restart | doze-kill" >&2; exit 2 ;;
esac
