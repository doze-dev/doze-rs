#!/usr/bin/env bash
# Kafka's kafka-streams-groups.sh against one broker, with a Kafka Streams word count (Serve.java) running and then stopped:
#   tools/kafka-oracle/streams/tool.sh <kafka_2.13-4.4.0 dir> <host:port> <out file>
# Run it against real Kafka and against doze-kafka, then diff the two outputs after normalising ids (member and process ids,
# the port). JAVA_HOME is the JDK of interop/.prototools unless set; the classpath is `bun interop/java/fetch.ts streams`.
set -u
kafka="$1"; broker="$2"; out="$3"
here="$(cd "$(dirname "$0")" && pwd)"
export JAVA_HOME="${JAVA_HOME:-$HOME/.proto/tools/jdk/temurin-21.0.13+9/Contents/Home}"
cp="$(cd "$here/../../../interop" && bun java/fetch.ts streams)"
app="tool-app-$$"
: > "$out"
k() { echo "### $*" >> "$out"; "$kafka/bin/kafka-streams-groups.sh" --bootstrap-server "$broker" "$@" >> "$out" 2>&1; echo "exit=$?" >> "$out"; }
"$JAVA_HOME/bin/java" -cp "$cp" "$here/Serve.java" "$broker" "$app" 60 > "$out.serve.log" 2>&1 &
sleep 18
k --list
k --list --state
k --describe --group "$app"
k --describe --group "$app" --members
k --describe --group "$app" --members --verbose
k --describe --group "$app" --state --verbose
k --describe --group "$app" --offsets --verbose
k --reset-offsets --group "$app" --all-input-topics --to-earliest --dry-run
k --delete --group "$app"
k --reset-offsets --group "$app" --all-input-topics --to-earliest --execute
wait
sleep 2
k --describe --group "$app" --state
k --reset-offsets --group "$app" --all-input-topics --to-earliest --execute
k --delete-offsets --group "$app" --input-topic "$app-in"
k --delete --group "$app" --delete-all-internal-topics
k --describe --group "$app"
k --delete --group nosuch
