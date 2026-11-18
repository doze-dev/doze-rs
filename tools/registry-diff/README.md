# registry-diff: doze-kafka's schema registry against Confluent's, live

The audit harness of the schema registry (`crates/kafka-registry`, `docs/port/behaviours/registry.md`). It stands up the
reference (Confluent's `cp-schema-registry` 7.7.1, the version the oracle was recorded from, and the current 8.x, both on a
real Apache Kafka in Docker) beside doze-kafka, sends all of them the same requests, and compares the answers byte for
byte. Nothing in it says what Confluent answers: every expected answer is the one the reference gives at run time.

Ports are 40000-40099 only: Kafka 40001, Confluent 7.7.1 40081, Confluent 8.x 40082, doze-kafka's broker 40010, its registry
40011, its console 40012. Containers are named `regdiff-*` and removed by name; doze-kafka is stopped by its pid file.

## Run

```sh
cargo build --release -p doze-kafka                      # or a debug build; pass its path to `up`
tools/registry-diff/stack.sh up [path/to/doze-kafka]     # fresh state for all four; waits until they answer
bun tools/registry-diff/diff.ts                          # the route scenarios (scenarios.ts)
bun tools/registry-diff/diff.ts --only corpus --corpus 100   # the generated corpus: 100 chains per format
bun tools/registry-diff/persist.ts                       # doze-kafka across a restart (TERM) and a kill (KILL)
tools/registry-diff/clients/run.sh                       # Confluent's serializers and console tools, both stacks
tools/registry-diff/stack.sh down
```

Run the scenarios on a fresh stack (`up`): Confluent's own answers change once a context subject has been registered, and
each registry has its own leader group (with one, 8.x forwarded its writes to 7.7.1). `REGDIFF_WORK` (default `/tmp/doze-registry-diff`) holds
doze-kafka's data and log; `CONFLUENT_8` picks the 8.x tag (default 8.3.2); `REGDIFF_REF`, `REGDIFF_REF8` (empty: skip 8.x)
and `REGDIFF_DOZE` point the comparison elsewhere.

## What each part does

- `stack.sh`: the four servers. `doze-restart [binary]` and `doze-kill` restart doze-kafka on the same data.
- `lib.ts`: one request to every target; the answer kept as status, `Content-Type`, `WWW-Authenticate`, `Allow` and the body's
  bytes. Ids are made symbolic before comparing (`"id":n`, `maxId`, `/referencedby` arrays, `Schema n not found`, `id=n` in an
  invalid-schema message), each target's in the order it gave them, so a registry that reuses an id where the other gives a new
  one still differs. A path names an id as `@k`, the k-th this target gave in the scenario. Each scenario starts from an empty
  registry (every subject deleted for good, the global level and mode reset).
- `scenarios.ts`: every Confluent route, its query parameters and errors, bodies of every shape, references, soft and hard
  delete and re-register, IMPORT, the three languages' stored text and refusals, `format=serialized` descriptors.
- `corpus.ts`: chains of three schemas per format (v1, v2, v3, each a random change of the one before, seeded), each run at all
  seven levels: v1 and v2 registered at NONE, the level set, v3 checked against the subject (`/versions` and `/versions/latest`,
  verbose), then registered at one level; for Protobuf the three descriptors are fetched too.
- `known.ts`: the steps where doze differs on purpose or by a recorded gap, each with its REG-D. Any other difference fails.
- `diff.ts`: runs them, prints a line per scenario, writes `summary.json`, `dozeVsRef.json` (doze against 7.7.1),
  `ref8VsRef.json` (8.x against 7.7.1), `dozeVsRef8.json` and each route scenario's exchanges to `--out`
  (default `results/latest`). Exit 1 on a difference `known.ts` does not list.
- `persist.ts`: everything answered before a restart or a kill is answered the same after, the registration just before the
  stop is kept, the next id is new, a permanently deleted id stays gone.
- `clients/Serde.java`, `clients/run.sh`: Confluent's `KafkaAvroSerializer`, `KafkaProtobufSerializer`,
  `KafkaJsonSchemaSerializer` and their deserializers with `auto.register.schemas` on and off, `use.latest.version`, a 409, a
  lookup miss, the wire header (magic byte, id, Protobuf message indexes `[0]`, `[1]`, `[1, 0]`), a produce and consume through
  the broker; then `kafka.tools.ConsoleProducer`/`ConsoleConsumer` with Confluent's Avro reader and formatter against doze. The
  jars are copied out of the `cp-schema-registry` image (`REGDIFF_JARS`, default `~/.cache/doze-registry-diff/jars-7.7.1`), so
  nothing comes from a Maven repository; the JDK is interop's (`interop/.prototools`). Both stacks' lines and headers must match.

## Results

`results/2026-11-18/` holds the audit's final run on a fresh stack: `routes/` and `corpus/` (`summary.json`, `dozeVsRef.json` with
each difference's REG-D, `routes/ref8VsRef.json`, the route exchanges), `routes.log`, `corpus.log`, `persist.log`, `clients.log`
and `clients/`. What each difference was and what was done about it is in `docs/port/behaviours/registry.md` (the rules marked
"audit") and in the audit's report.
