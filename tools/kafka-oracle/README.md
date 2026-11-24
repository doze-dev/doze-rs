# Real Kafka as the oracle

For the APIs the replay suite has no recording of (OffsetForLeaderEpoch, DescribeLogDirs, DescribeClientQuotas,
AlterClientQuotas, the controller mutation quota), doze-kafka is held to a real Apache Kafka 4.x run beside it.

1. Download and check the binary distribution (`kafka_2.13-4.4.0.tgz` and its `.sha512` from downloads.apache.org).
2. `tools/kafka-oracle/start.sh <kafka_2.13-4.4.0> <fresh dir>`: Kafka on 127.0.0.1:39400 (KRaft, combined, `server.properties`).
3. A fresh doze-kafka on another port, then
   `cargo run -p kafka-wire --example oracle_diff -- 127.0.0.1:39400 <doze host:port>`: the same requests to both, the answers
   compared after normalising what must differ (paths, volume sizes, throttle times as zero or not, hash-map orders). Exits 1 on
   any difference. Run on 2026-11-15 against 4.4.0: 75 steps, 0 differences (`docs/port/behaviours/quotas.md`).
4. `java -cp '<kafka_2.13-4.4.0>/libs/*' tools/kafka-oracle/QuotaMath.java`: Kafka's own `Rate`, `TokenBucket` and throttle
   arithmetic on the inputs of `crates/kafka-broker/src/quotas/meter/tests.rs`, whose expected values are its output.

5. CreateAcls past the 10,000 bound (debt I8): Kafka started with `authorizer.class.name=org.apache.kafka.metadata.authorizer.StandardAuthorizer`
   and `super.users=User:ANONYMOUS` added, then `java -cp '<kafka_2.13-4.4.0>/libs/*' tools/kafka-oracle/AclProbe.java <host:port>` against
   each; Kafka's answers are `acl-probe-4.4.0.txt`. Run on 2026-11-23: doze-kafka's are the same.

Kafka's own tools against both, as the behaviours cite them: `kafka-log-dirs.sh --describe`, `kafka-configs.sh --alter
--entity-type users|clients ... --add-config producer_byte_rate=...`, `kafka-producer-perf-test.sh` and
`kafka-consumer-perf-test.sh` with a client id that has a quota. Ports: 39400 to 39401 here; never the owner's 29xxx or 19592.

## Streams groups (KIP-1071)

`docs/port/behaviours/streams-groups.md`, "The oracle". With Kafka on 39600 and doze-kafka on 39610 (2026-11-18):
- `interop/java/Streams.java` against each, with `JAVA_LOG=debug` and its output kept; then
  `bun tools/kafka-oracle/streams/compare-heartbeats.ts <kafka log> <doze log> v` compares every heartbeat answer of every client.
- `tools/kafka-oracle/streams/tool.sh <kafka dir> <host:port> <out>` against each: `kafka-streams-groups.sh` with a word count
  (`streams/Serve.java`) running and then stopped; the two outputs differ only by ids and row order.
