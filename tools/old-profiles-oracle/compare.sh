#!/usr/bin/env bash
# Runs the probe against doze-kafka at one profile and diffs it with what the real broker of that major answered:
#   tools/old-profiles-oracle/compare.sh <doze-kafka binary> <major 1|2|3> <port> <out dir> [scenario,...]
# doze-kafka is started with --kafka-version <major> on 127.0.0.1:<port> with a fresh data directory, asked the versions the
# real broker listed (kafka-<release>.txt, its all-apis lines), and stopped. The diff is <out dir>/kafka-<release>.diff.
set -euo pipefail
bin="$1"
major="$2"
port="$3"
out="$4"
only="${5:-}"
here="$(cd "$(dirname "$0")" && pwd)"
case "$major" in
  1) release=1.1 ;;
  2) release=2.8 ;;
  3) release=3.9 ;;
  *) echo "major must be 1, 2 or 3" >&2; exit 2 ;;
esac
mkdir -p "$out"
data="$(mktemp -d "$out/doze-$release.XXXX")"
DOZE_ZONE=off "$bin" --listen "127.0.0.1:$port" --data-dir "$data/data" --meta-db "$data/meta.db" \
  --kafka-version "$major" --group-initial-rebalance-delay 0s > "$data/broker.log" 2>&1 &
pid=$!
trap 'kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true' EXIT
for _ in $(seq 1 100); do
  if nc -z 127.0.0.1 "$port" 2>/dev/null; then break; fi
  sleep 0.1
done
cd "$here/../../interop"
cp="$(bun java/fetch.ts 3.9)"
args=(-broker "127.0.0.1:$port" -major "$major" -apis "$here/kafka-$release.txt")
if [[ -n "$only" ]]; then args+=(-only "$only"); fi
java -cp "$cp" "$here/Probe.java" "${args[@]}" > "$out/doze-$release.txt" 2>&1 || true
want="$here/kafka-$release.txt"
if [[ -n "$only" ]]; then
  pattern="^($(echo "$only" | sed 's/,/|/g')): "
  grep -aE "$pattern" "$want" > "$out/kafka-$release.only.txt" || true
  want="$out/kafka-$release.only.txt"
fi
diff -a "$want" "$out/doze-$release.txt" > "$out/kafka-$release.diff" && echo "kafka $release: no difference" || echo "kafka $release: $(grep -ac '^[<>]' "$out/kafka-$release.diff") lines differ ($out/kafka-$release.diff)"
