#!/usr/bin/env bash
# Runs Serde.java (Confluent's serializers and deserializers) and Confluent's console producer and consumer against
# Confluent's registry with real Kafka, and against doze-kafka's registry and broker, then compares the two runs' lines.
# The jars are Confluent's own, copied out of the cp-schema-registry image (the version the stack runs, 7.7.1 by default),
# so nothing is fetched from a Maven repository. Needs the stack up (../stack.sh up) and a JDK (interop/.prototools).
#
#   tools/registry-diff/clients/run.sh [out dir]
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../../.." && pwd)"
tag="${CONFLUENT_CLIENTS:-7.7.1}"
cache="${REGDIFF_JARS:-${HOME}/.cache/doze-registry-diff/jars-$tag}"
out="${1:-$here/../results/latest}"
mkdir -p "$out"
if [ ! -d "$cache/kafka-serde-tools" ]; then
  mkdir -p "$cache"
  docker rm -f regdiff-jars >/dev/null 2>&1 || true
  docker create --name regdiff-jars "confluentinc/cp-schema-registry:$tag" >/dev/null
  docker cp regdiff-jars:/usr/share/java/kafka-serde-tools "$cache/"
  docker cp regdiff-jars:/usr/share/java/schema-registry "$cache/"
  docker rm regdiff-jars >/dev/null
fi
cp="$cache/kafka-serde-tools/*:$cache/schema-registry/*"
classes="$(mktemp -d)"
trap 'rm -rf "$classes"' EXIT
(cd "$root/interop" && javac -nowarn -d "$classes" -cp "$cp" "$here/Serde.java") 2>&1 | grep -v '^Note:' || true
run() { # name registry bootstrap
  local status=0
  (cd "$root/interop" && java -cp "$cp:$classes" -Dorg.slf4j.simpleLogger.defaultLogLevel=warn -Dlog4j.configuration=file:/dev/null \
    Serde "$2" "$3" "e2e-$(date +%s)-") >"$out/clients-$1.log" 2>"$out/clients-$1.err" || status=$?
  grep -E '^(ok|FAIL|header|all ok|[0-9]+ failed)' "$out/clients-$1.log" | sed 's/^/  /'
  echo "  $1: exit $status"
  return 0
}
echo "== Confluent $tag registry, Apache Kafka"
run confluent http://127.0.0.1:40081 127.0.0.1:40001
echo "== doze-kafka registry and broker"
run doze http://127.0.0.1:40011 127.0.0.1:40010
# The console tools: kafka-avro-console-producer and -consumer, as the image runs them (kafka.tools.ConsoleProducer with
# Confluent's AvroMessageReader), against doze-kafka.
schema='{"type":"record","name":"Tool","fields":[{"name":"n","type":"int"}]}'
topic="tool-$(date +%s)"
(cd "$root/interop" && printf '{"n":1}\n{"n":2}\n' | java -cp "$cp" -Dlog4j.configuration=file:/dev/null kafka.tools.ConsoleProducer \
  --bootstrap-server 127.0.0.1:40010 --topic "$topic" \
  --line-reader io.confluent.kafka.formatter.AvroMessageReader \
  --property schema.registry.url=http://127.0.0.1:40011 --property value.schema="$schema") >"$out/console-producer.log" 2>&1 \
  && echo "  ok console-producer" || echo "  FAIL console-producer (see $out/console-producer.log)"
(cd "$root/interop" && java -cp "$cp" -Dlog4j.configuration=file:/dev/null kafka.tools.ConsoleConsumer \
  --bootstrap-server 127.0.0.1:40010 --topic "$topic" --from-beginning --max-messages 2 --timeout-ms 20000 \
  --formatter io.confluent.kafka.formatter.AvroMessageFormatter \
  --property schema.registry.url=http://127.0.0.1:40011) >"$out/console-consumer.log" 2>&1 || true
if grep -q '{"n":1}' "$out/console-consumer.log" && grep -q '{"n":2}' "$out/console-consumer.log"; then
  echo "  ok console-consumer"
else
  echo "  FAIL console-consumer (see $out/console-consumer.log)"
fi
echo "== the two runs, line for line (ok/FAIL/header, details cut)"
cut -d' ' -f1-2 "$out/clients-confluent.log" | grep -E '^(ok|FAIL|header)' >"$out/a" || true
cut -d' ' -f1-2 "$out/clients-doze.log" | grep -E '^(ok|FAIL|header)' >"$out/b" || true
grep '^header' "$out/clients-confluent.log" >"$out/ha" || true
grep '^header' "$out/clients-doze.log" >"$out/hb" || true
if diff "$out/a" "$out/b" >/dev/null && diff "$out/ha" "$out/hb" >/dev/null; then
  echo "  same results and the same wire headers"
else
  echo "  DIFFER:"; diff "$out/a" "$out/b" || true; diff "$out/ha" "$out/hb" || true
fi
rm -f "$out/a" "$out/b" "$out/ha" "$out/hb"
