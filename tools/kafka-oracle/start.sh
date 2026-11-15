#!/usr/bin/env bash
# Starts real Apache Kafka (KRaft, broker and controller in one process) as the oracle for the APIs no recording covers:
#   tools/kafka-oracle/start.sh <kafka_2.13-4.x dir> <data dir>     (Kafka on 127.0.0.1:39400, its controller on 39401)
# The data dir is made fresh. JAVA_HOME is the JDK of interop/.prototools unless set. Prints the PID; stop it with kill.
# Then:  cargo run -p kafka-wire --example oracle_diff -- 127.0.0.1:39400 <doze-kafka host:port>
set -euo pipefail
kafka="$1"
data="$2"
here="$(cd "$(dirname "$0")" && pwd)"
export JAVA_HOME="${JAVA_HOME:-$HOME/.proto/tools/jdk/temurin-21.0.13+9/Contents/Home}"
export PATH="$JAVA_HOME/bin:$PATH"
rm -rf "$data"
mkdir -p "$data/logs"
export LOG_DIR="$data/logs"
sed "s|^log.dirs=.*|log.dirs=$data/kafka|" "$here/server.properties" > "$data/server.properties"
"$kafka/bin/kafka-storage.sh" format -t "$("$kafka/bin/kafka-storage.sh" random-uuid)" -c "$data/server.properties" >/dev/null
nohup "$kafka/bin/kafka-server-start.sh" "$data/server.properties" > "$data/logs/stdout.log" 2>&1 &
pid=$!
for _ in $(seq 1 60); do
  if grep -q "Kafka Server started" "$data/logs/stdout.log" 2>/dev/null; then echo "$pid"; exit 0; fi
  sleep 1
done
echo "Kafka did not start: $data/logs/stdout.log" >&2
exit 1
