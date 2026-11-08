# doze-kafka in Rust: a ground-up implementation

Status: living plan. Approved 2026-11-08 (ground-up, Rust idioms, Go as domain inspiration only).

Target: this workspace (`~/Code/doze-dev/doze-rs`). Domain reference: `~/Code/doze-dev/doze-kafka`
at `origin/main` (`4081fa6`), read for *what* a Kafka-compatible local broker must do, never for
*how* to structure Rust. Nothing is translated line by line.

## Context and settled decisions

The owner wants doze-kafka ported slowly and properly, as a test of how well a
port can be done: best practices, lean code, no bloated files, and a design that
is natural in Rust rather than a transliteration of the Go.

- **IP and port only.** doze-kafka no longer uses doze-names. No name registry,
  DNS setup, `doctor`, `--name`, `--zone`, extra listeners, or console instance
  label. `--listen` defaults to `127.0.0.1:9092`. Kept: advertised host/port
  rules (a loopback listener advertises itself, a wildcard advertises
  `localhost`/`127.0.0.1`, port 0 advertises the bound port), `/healthz`, the
  console URL prefix, and an explicit allowed-hosts guard (DNS-rebinding
  defence) with no `.doze` special case.
- **Data: fresh start.** The Rust broker owns its on-disk layout and metadata
  format. It never reads a Go data directory. Record batches are Kafka
  RecordBatch v2 bytes (that is the wire format); everything else is ours.
- **The console reads the broker through a direct in-process handle**, not a
  Kafka client over loopback.
- **The oracle is black-box and language-neutral:** the 81 recordings from real
  Apache Kafka 4.3.1 (`conformance/`), the Playwright console suite (34 specs,
  external mode), and real clients (franz-go, librdkafka, Java). The Go
  conformance harness gets a `processTarget` (a branch of doze-kafka) so the
  unmodified scenarios run against any binary.
- **Behaviour contract:** Kafka protocol behaviour, the console's DOM, the
  schema registry's REST, the CLI flags' meaning. Log-line text, banner format
  and internal file names are *not* contracts; the Rust edition's docs are
  regenerated for the Rust edition.
- Go stays the reference until parity; behaviour fixes land in Go first.

## What the exploration found (domain knowledge worth keeping)

- A Kafka-compatible local broker is mostly: framing and versioned requests, a
  partition log (sealed and active segments, sparse index), produce/fetch with
  long-poll, consumer groups (classic and KIP-848), idempotence, transactions,
  retention and compaction, ACLs, SASL/TLS, a console and a schema registry.
- Fidelity rests on recorded real-Kafka behaviour: exact error codes and
  wording, and the *order* of checks (authorise before exist, etc.).
- Failure semantics worth keeping: a failed fsync is never retried (partition
  offline until restart); an `acks=all` produce is durable before it is
  acknowledged; recovery truncates a torn tail and rebuilds the index; all
  storage errors map to `KAFKA_STORAGE_ERROR`.
- Weight is a product feature (flat memory, no mmap, tiny idle heap).
- The Go console polls (3 to 5 s), uses 14 templates (2.8k lines), 59 routes, and
  a reflection-based traffic inspector (about 2.1k lines) that has no Rust
  analogue and is redesigned.
- Prior art is thin: Rust brokers found (Nisshi, Crabka) skip transactions
  and/or KIP-848. `kafka-protocol` 0.18 (Kafka 4.1 specs) and `redb` 4.5 are
  real, maintained foundations.

## Design: what Rust makes natural (and what it replaces)

| Concern | Go design | Rust-native design |
|---|---|---|
| Request dispatch | map of API key → handler, startup panic if one is missing | an exhaustive `match` over the generated `Request` enum: a missing API is a *compile* error; each arm calls one `async fn` in its own module |
| Parsing input | validate fields as they're used | *parse, don't validate*: `ValidBatch`, `TopicName`, `Offset`, `ProducerId` are constructed only through checks; storage accepts only `ValidBatch` |
| Framing | hand-rolled read loop with deadlines | a `tokio_util::codec::Decoder`/`Encoder` (`KafkaCodec`) with per-API size caps; timeouts at the connection layer with `tokio::time::timeout`; connection code generic over `AsyncRead + AsyncWrite` so TCP, TLS, unix and in-memory test streams share it |
| Partition log | one `Mutex`, reads under the lock, `appendSeq/syncedSeq` group commit | **single writer, many readers.** One writer task owns the active segment (`&mut self`, no lock). Readers are cheap `Clone` handles over an immutable segment snapshot (`arc-swap`) and an atomic committed length, so reads never block the writer. Group commit falls out of the writer draining its queue then one `fsync`; an append returns a `Ticket` whose `.durable().await` resolves at the sync |
| Segments | one struct with flags | typestate: `ActiveSegment` (can append) is consumed into `SealedSegment` (immutable); no "append to sealed" bug is expressible |
| Fault injection | global hooks, build tags, a power-cut model in another package | storage is generic over a small `Fs` trait; `RealFs` for production, `SimFs` (deterministic in-memory disk with crash points, torn writes, ENOSPC schedules) for tests. No globals, no feature-flagged code paths in production |
| Time | `time.AfterFunc` + generation counters | group, txn and expiry logic is *sans-IO*: pure state machines `step(event, now) -> Vec<Effect>`; a thin actor feeds them events and arms `tokio_util::time::DelayQueue`. Tests drive them with synthetic time and `proptest` event sequences, checking invariants |
| Consumer groups | coarse mutex per manager, parked channels, documented lock order | one actor task per group owns its `GroupMachine`; joiners wait on `oneshot`s; the classic↔KIP-848 handoff is a message between owners arbitrated by a single `claims` registry. No lock order exists to violate |
| Auth state | flags on the connection | `enum Session { Unauthenticated, Handshaken(Mechanism), Authenticated(Principal) }`; SCRAM is a pure state machine; ACLs are an immutable `ArcSwap` snapshot read lock-free |
| Long-poll fetch | per-topic notifier with subscribe-before-read | a `tokio::sync::watch` of each partition's committed end offset: wait for change, no missed-wakeup race by construction |
| Errors | error values plus sentinel checks | `thiserror` enums per crate; a `KafkaError` that maps to a protocol `ErrorCode` once, at the boundary; `anyhow` only in the binary |
| Config | pointer-typed file struct + manual precedence | `clap` derive + `serde` (`deny_unknown_fields`) into a raw layer, then `TryFrom` into a validated `Config`; secrets are `secrecy` types with redacted `Debug`; precedence flags > file > defaults |
| Shutdown | context cancel + wait | `CancellationToken` + `TaskTracker`; `Server::builder()…bind().await? -> Running` with `local_addr()` and `shutdown().await`, the same API the binary and Rust tests use (no separate embed layer) |
| Console HTML | Go templates, contextual escaping, id strings checked by a regex test | typed view-models rendered by **maud** components (HTML as Rust, auto-escaped, compile-checked). htmx target ids are `const`s shared by the markup and its `hx-*` attributes, so a dangling `hx-target` doesn't compile. htmx, Alpine, CodeMirror, CSS and fonts are browser assets, copied unchanged via `rust-embed`. Updates stay as htmx `hx-trigger="every 3s"` polling |
| Traffic inspector | reflection over message structs | a small `Summarize` trait implemented per request/response (derive or macro for the repetitive field walks); snapshot-tested with `insta` |
| Registry formats | per-format code paths | `trait SchemaFormat { parse, canonical, compatible(mode) }` implemented for Avro, Protobuf, JSON Schema |
| Observability | slog | `tracing` spans and events, `tracing-subscriber` text/JSON; optional `tokio-console` feature in place of pprof |

## Quality rules (enforced by tooling)

1. **File size:** soft 300 lines, hard 450 per `.rs` file including inline tests;
   larger tests live in a sibling `tests.rs`. `xtask lint-size` fails CI at the
   hard limit and lists soft offenders. Near the limit, split by responsibility.
2. **Function size/complexity:** `clippy.toml` `too-many-lines-threshold = 70`,
   `cognitive-complexity-threshold = 20`; `too_many_arguments` denied.
3. **No panics or unsafe in production:** `#![forbid(unsafe_code)]`; clippy
   `unwrap_used`, `expect_used`, `indexing_slicing`, `panic`, `todo`,
   `unimplemented` denied outside tests. Wire input is never trusted.
4. **Layering by crate** (Cargo enforces the arrows): the wire crate has no I/O;
   storage doesn't know Kafka requests; broker doesn't know sockets; console
   doesn't know the network.
5. **Typed domain**: newtypes with checked arithmetic (`Offset`, `PartitionId`,
   `Epoch`, `ProducerId`, `TopicName`, `GroupId`, `MemberId`); no bare integers
   across module boundaries; `#[non_exhaustive]` on public enums.
6. **Ownership over locks:** state is owned by one task and driven by messages;
   a `Mutex` is allowed only for short sync critical sections, never held across
   `.await` (clippy `await_holding_lock` denied).
7. **`foo.rs` + `foo/`**, no `mod.rs`; one concern per module, named for what it
   does; one handler per file.
8. **Weight is a feature:** baselines for binary size, idle RSS, startup, and
   `cargo tree` crate count live in `docs/port/weight.md` and may only rise with
   a written reason (checked in CI from Phase 3).
9. **A phase is done on evidence,** not translated code: its black-box oracle
   scenarios pass, its own tests/fuzz/simulations pass, the size and weight gates
   are green, and an independent audit (a separate agent, not the author) has
   scored it against the behaviour catalogue and these rules. (Memory: a
   self-judged 21/21 was 3/19 to an audit.)

## Workspace and crates

```
doze-rs/
  Cargo.toml  clippy.toml  rustfmt.toml  deny.toml  rust-toolchain.toml
  docs/port/   PLAN.md  weight.md  behaviours/<area>.md
  xtask/       lint-size, weight, conformance, fuzz-check
  crates/
    kafka-wire/      sans-IO. KafkaCodec, request/response enums, the four
                     version profiles as const tables, ValidBatch (RecordBatch
                     v2 parse/CRC32C/stamp), codecs, ErrorCode. Wraps
                     `kafka-protocol`.
    kafka-storage/   Fs trait (+ RealFs), segments (typestate), sparse index,
                     PartitionLog (writer + readers), recovery plan, retention,
                     compaction planner/executor. `SimFs` lives here too, behind the `sim` feature.
    kafka-meta/      redb tables: topics, committed offsets, producer ids,
                     transactions, ACLs, group liveness, registry records.
    kafka-broker/    request handlers and the pure state machines: groups
                     (classic, KIP-848, migration), idempotency, transactions,
                     ACL, SCRAM. No sockets.
    kafka-server/    listener, TLS, connection task, Session, shutdown,
                     the `Server` builder/handle.
    kafka-console/   axum + maud, view-models, static assets, hostguard layer,
                     traffic recorder.
    kafka-registry/  Confluent-compatible schema registry.
    doze-kafka/      the binary: clap, config, tracing, wiring, `serve` mode.
    kafka-testkit/   deterministic clock helpers, in-memory streams,
                     a raw-protocol test client, proptest strategies.
```

Features gate weight: codecs (`gzip`, `snappy`, `lz4`, `zstd`), `tls`, `console`,
`registry`, `tokio-console`.

## Testing strategy (Rust-native; Go's tests are not translated)

- **Behaviour catalogue** (`docs/port/behaviours/<area>.md`): for each area, a
  list of behaviours and edge cases distilled from the Go source, its docs and
  the recordings (e.g. "produce to an unknown topic with auto-create off →
  `UNKNOWN_TOPIC_OR_PARTITION`", "heartbeat during rebalance → `REBALANCE_IN_PROGRESS`").
  Each item links to the Rust test that covers it. This is traceability instead
  of translation, and it is what the independent audit checks against.
- **Pure state machines:** unit tests plus `proptest` over event sequences with
  stated invariants (generation only increases, the leader is a member, a
  sequence number never goes backwards, a committed transaction's markers are
  all written); synthetic time (no sleeps).
- **Storage:** `SimFs` deterministic simulation: random operations interleaved
  with power cuts and injected faults, asserting that every acknowledged record
  survives and nothing unacknowledged appears out of order; real-disk smoke tests.
- **Parsers and codecs:** `cargo-fuzz` target per parser (frame, header, batch,
  consumer-protocol bytes, SCRAM, registry JSON), with a check that every parser
  has one; round-trip tests against Kafka's own message specs.
- **Network/time:** in-memory duplex streams and `tokio::time::pause` for
  connection-level and timeout tests.
- **Console:** `insta` snapshots of rendered components; the Playwright suite as
  the black-box net.
- **Black-box oracles:** conformance via the Go harness's `processTarget`, e2e in
  external mode, client compatibility (franz-go, librdkafka, Java), and a
  `cargo llvm-cov` floor per crate; `cargo-mutants` weekly (optional).

## Dependencies (verified vs. to verify)

Verified 2026-11-08: `kafka-protocol` 0.18.0 (Kafka 4.1 specs; `Encodable`/
`Decodable`; `broker` feature; custom compression hook); `redb` 4.5.0 (ACID,
pure Rust, stable format); Nisshi as design reference.

Phase 1 spike must confirm before anything depends on them: coverage of every
API the broker serves (incl. `ConsumerGroupHeartbeat`/`Describe`, transactions,
telemetry), flexible versions and tagged fields everywhere, **decode hardening**
(a hostile array count must not allocate; the Go side patched exactly this), and
CRC32C record-batch handling. Fallback: generate our own message codec from
Kafka's JSON specs (41 request specs sit in the Go repo's
`internal/protocol/testdata/kafka-spec`).

Chosen, subject to the spike: `tokio`, `tokio-util`, `bytes`, `thiserror`,
`anyhow`, `clap`, `serde`, `toml`, `postcard` (metadata encoding, with a schema
version), `redb`, `arc-swap`, `crc32c`, `flate2`, `snap`, `lz4_flex`, `zstd`,
`rustls` (backend chosen by size), SCRAM by hand over `hmac`/`sha2`/`pbkdf2`,
`axum`, `tower`, `maud`, `rust-embed`, `secrecy`, `tracing`,
`apache-avro`, `protox` + `prost-reflect`, `jsonschema`, `proptest`, `insta`,
`cargo-fuzz`. Each added crate is justified against the weight baseline.

## Phases

Each phase begins with the behaviour catalogue for its area and ends with its
gates green plus an independent audit. Sessions are working sessions, honest
estimates, not calendar time.

| # | Phase | Gate | Sessions |
|---|---|---|---|
| 0 | **Rails.** Nine empty crates; `xtask lint-size`; clippy/fmt/deny config; CI; `docs/port/PLAN.md` (replacing `docs/kafka-port.md`); behaviour-catalogue skeleton. **Go branch of doze-kafka:** `processTarget` for the conformance harness, test-only flags for the knobs scenarios close over (retention check interval, assignment interval, rebalance delay, session timeout, ACL seed, offsets retention), a check-offsets equivalent. | conformance runs through `processTarget` against the **Go** binary and passes; Rust CI green | 2–3 |
| 1 | **Wire.** Spike (crate coverage/hardening), then `kafka-wire`: `KafkaCodec` with size caps, the four profiles as const tables, `ApiVersions`, `ValidBatch` (parse, CRC32C, stamp), codecs with inflate caps, `ErrorCode` + Kafka's wording. | fuzz targets clean; round-trip vs Kafka's specs; `apiversions` recordings match | 3–4 |
| 2 | **Storage.** `kafka-storage` + its `SimFs` first: `Fs` trait, typestate segments, sparse index, `PartitionLog` writer/readers/`Ticket`, recovery plan, retention. | simulation suite (power cuts, torn writes, ENOSPC, EIO) passes; real-disk smoke; flat-memory check | 5–7 |
| 3 | **Minimum broker.** `kafka-meta` (topics, producer ids), broker core (Metadata, CreateTopics, DeleteTopics, ListOffsets, Produce, Fetch with the `watch` long-poll), `kafka-server` (`Server` builder, connection, shutdown), `doze-kafka` CLI/config. Real clients produce and consume. **First weight baseline.** | produce/consume/metadata conformance via `processTarget`; franz-go, librdkafka, Java by hand | 5–7 |
| 4 | **Groups, classic.** `GroupMachine` + actor; join/sync/heartbeat/leave; offsets; expiry; coordinator and group admin APIs. | classic consumer-group conformance; property tests | 5–7 |
| 5 | **Groups, KIP-848 and migration.** heartbeat/describe, reconcile, assign; classic↔848 via `claims`. | 848 + migration conformance | 4–6 |
| 6 | **Idempotence and transactions.** pure `ProducerSequence` and `TxnMachine`, markers, last-stable-offset and aborted ranges, timeouts, recovery. | transaction conformance; `SimFs` txn durability | 4–6 |
| 7 | **Compaction, topic admin, configs.** compaction planner/executor, `CreatePartitions`, `DeleteRecords`, config APIs, limits. | remaining admin/compaction scenarios | 3–5 |
| 8 | **Security.** TLS, SASL PLAIN and SCRAM, ACL CRUD + enforcement, allowed-hosts guard. | auth/authorization conformance; secret-redaction tests | 3–4 |
| 9 | **Console.** assets copied, maud components and typed view-models, routes (exported as JSON for the e2e route gate), htmx polling, traffic recorder + `Summarize`, telemetry. | Playwright suite (34 specs) passes unchanged in external mode | 8–12 |
| 10 | **Schema registry.** REST routes, `SchemaFormat` for Avro/Protobuf/JSON, wire codec, storage in `kafka-meta`. Highest fidelity risk (Protobuf canonical form and compatibility). | registry conformance scenarios + compat corpora | 5–8 |
| 11 | **Module and release.** `serve` mode (unix sockets) so doze's Go module can re-exec the Rust binary; Rust-edition docs; inherited gates; cutover decision. | doze runs the Larder example on the Rust broker | 3–5 |

About 50–75 sessions. Phases 1–3 are the checkpoint: a working broker for real
clients with weight measured against Go.

## Risks

- **Wire crate coverage/hardening** (Phase 1 spike; fallback: own codegen).
- **LZ4 framing** (Kafka's non-standard block header; conformance test).
- **Group and transaction semantics** (largest correctness risk; the sans-IO
  design makes them testable but not simpler).
- **Registry parity** for Protobuf/JSON-Schema compatibility.
- **`F_FULLFSYNC`** on macOS: confirm `sync_all` matches the Go behaviour.
- **Console fidelity** after rewriting markup in maud: the e2e suite is the net;
  if it proves too costly, fall back to askama templates with the same typed
  view-models.
- **Time:** long project; checkpoint at Phase 3.

## Verification

- Per phase: oracle scenarios, the phase's own tests/fuzz/simulation, size and
  weight gates, independent audit against the behaviour catalogue.
- Phase 3: real clients by hand; first weight comparison with Go (binary 28.4 MB
  darwin/arm64, idle heap 1.37 MB, 422 modules).
- Phase 9: full Playwright suite in external mode.
- Phase 11: Larder on the Rust broker via doze.

## Critical files

Rust (to create): everything under `~/Code/doze-dev/doze-rs/crates/*`, `xtask/`,
`docs/port/`; Phase 0 replaces `docs/kafka-port.md`.
Go reference (read for behaviours only): `internal/protocol/versions.go`,
`internal/broker/*` (group, txn, produce, fetch, acls, scram),
`internal/storage/log/*`, `internal/storage/meta/store.go`, `console/*`,
`schemaregistry/*`, `conformance/{harness,normalize,scenarios*}.go`,
`e2e/playwright.config.ts`, `docs/storage-and-durability.md`.
Go-side branch (Phase 0): `conformance/harness.go` (+ a `processTarget` file),
test-only knobs in `internal/config`.

## Revision log

- **2026-11-08, Phase 0.** `SimFs` lives in `kafka-storage` behind the `sim` feature, not in
  `kafka-testkit`: storage's own tests would otherwise need a dev-dependency on a crate that
  depends on storage, which gives Cargo two copies of the crate. `kafka-testkit` enables the
  feature for everyone else. The domain newtypes (`Offset`, `TopicName`, …) live in `kafka-wire`,
  which storage, meta and broker already depend on.
