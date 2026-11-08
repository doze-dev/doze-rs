# Phase 3: the minimum broker

Status: plan, 2026-11-08. Goal: real clients (franz-go, librdkafka, the Java client) produce to and consume from
topics without groups (partitions assigned by the client), and the Go conformance scenarios for Metadata,
CreateTopics, DeleteTopics, ListOffsets, Produce and Fetch pass against the Rust binary. At the end: the first weight
comparison with Go (`docs/port/weight.md`).

Not here: consumer groups (4, 5), idempotence and transactions (6), compaction and config APIs (7), SASL/TLS/ACLs (8),
the console (9), the registry (10). The broker advertises only the APIs it serves.

## Decisions that shape everything else

**1. Advertise only what is served.** ApiVersions lists the profile's APIs filtered by an `IMPLEMENTED` set that
grows a phase at a time (Phase 3: ApiVersions, Metadata, CreateTopics, DeleteTopics, ListOffsets, Produce, Fetch,
InitProducerId). A client then never negotiates an API it will be refused. Dispatch is still an exhaustive `match` over
`Request`: the unserved variants share one arm, so a new API is a compile error until someone decides about it.

**2. Tokio, small and deliberate.** A multi-thread runtime with 2 workers by default (`--worker-threads`), because idle
threads and their stacks are weight. Blocking file work never runs on a worker: appends and syncs run in
`block_in_place` inside the partition's own task; reads go through `spawn_blocking` (whose threads exit after ten
idle seconds). Nothing polls: timers are armed only while something is dirty.

**3. One task owns each partition's writer.** A `PartitionActor` owns the `PartitionLog` and an `mpsc` of commands.
It takes one command, drains whatever else is queued, appends it all, and syncs **once** if any command wanted
durability; replies go out after the sync. That is group commit by construction. An `acks=1` append replies at once
and arms the actor's own deadline (`--fsync-interval`, default 500 ms); nothing is armed while the log is clean.
Readers are `LogReader` clones held by the handlers: a fetch never goes through the actor.

**4. One request at a time per connection, in order.** Kafka mutes a connection until a response is sent; clients are
written for that (a long-poll Fetch holds its connection). So a connection is a plain loop: read a frame, split the
header, gate it, decode the body, handle, encode, write. The loop is generic over `AsyncRead + AsyncWrite`, so TCP,
unix sockets (Phase 11) and in-memory test streams share it.

**5. The metadata store is one concrete type, `MetaStore`** (decided in 3a: no trait, because nothing else would
implement it; the simulated disk is what tests swap, and it sits under the type). It holds topics, producer-id
blocks, and in later phases offsets and the rest. Reads come from an in-memory registry updated after a commit. The redb implementation follows the conditions the research set: the file is created under a
temporary name, committed, flushed, renamed and its directory flushed; any commit error is fatal to the store; always
`Durability::Immediate`; cache set explicitly; one writer; our own versioned bytes (postcard + a version byte). redb
runs over an adapter that implements its `StorageBackend` on our `Fs`, so the same simulated disk that crash-tests the
logs crash-tests the metadata.

**6. Topic names are a type.** `TopicName` (Kafka's legal characters, 1..=249, not `.` or `..`) is parsed once at the
edge. The collision rules (names that differ only in `.` against `_`, or only in case, because partition directories
share a file system) are the store's.

**7. Produce is checked in layers.** `kafka-wire` (done) gives shape, CRC and Kafka's check order. The broker adds what
needs the topic: `max.message.bytes`, zstd needs Produce v7, a producer writes base offset 0, no control batches from
clients. Record-level validation (counts, lengths, decompression with caps) comes in its own slice with the codecs,
each behind a feature (`gzip`, `snappy`, `lz4`, `zstd`), pure Rust where there is one. Until that slice, a compressed
batch is accepted on its header and CRC alone.

**8. Descriptors are budgeted, not assumed.** Storage holds two descriptors per segment, which is 100,000 for 5,000
partitions of ten segments. The fix lands in this phase: sealed segments open their files on first read and keep a
bounded number; a partition idle for a minute is closed (flushed, and reopened vouched).

**9. Flags are the oracle's flags.** The conformance harness starts the binary with flags for later phases
(`--group-initial-rebalance-delay`, `--consumer-session-timeout`, `--offsets-retention`, `--acl-default`, ...); the
binary accepts them as hidden no-ops until their phase, so the unmodified scenarios can run.

## Slices

| | Slice | Gate |
|---|---|---|
| 3a | Domain types (`TopicName`, `TopicId`, `PartitionId`); `kafka-meta` over redb on `Fs` | round-trip, crash simulation of the metadata (acknowledged topics survive any cut) |
| 3b | `kafka-server`: listener, connection loop, codec, gate, `Server` builder/handle, shutdown; ApiVersions served | a raw client gets ApiVersions; recorded ApiVersions scenarios pass |
| 3c | Broker core: topic registry, partition manager and actors; Metadata, CreateTopics, DeleteTopics | createtopics-*, metadata-autocreate scenarios |
| 3d | Produce and InitProducerId (layer 7 first half); acks 0/1/all, group commit, fsync timer | produce-acks-zero, produce-validation (non-record parts), full-disk scenarios |
| 3e | Fetch (long-poll over the end-offset channel) and ListOffsets (incl. by timestamp) | listoffsets, fetch-above-the-end |
| 3f | Codecs and record validation; descriptor budget; idle close | produce-inflated-size, produce-decompressed-size |
| 3g | `doze-kafka` binary: config, logging, clean stop, the oracle's flags | the process target runs the Phase 3 scenarios |
| 3h | franz-go, librdkafka and Java clients by hand; weight baseline; independent audit | the checkpoint |

Each slice ends green on the usual gates (fmt, clippy, tests, size, deny, docs, fuzz-check) and is committed. Tests
follow PLAN rule 10: oracles where logic is intricate, regressions for bugs found, one direct test per contract
behaviour, none per mutant.
