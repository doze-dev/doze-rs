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
10. **Tests earn their place; the Go suite's size is a warning, not a model.** A test is kept if it is (a) an oracle
   that finds bugs by itself (a simulation, a fuzz target, a real-concurrency or measurement test), (b) a regression
   for a bug that was actually found and that no oracle reaches cheaply, or (c) the only check of a behaviour that
   defines the contract. It is not a test per mutant, per branch or per getter, nor one that re-checks what a type
   or another test pins. Method: mutate the code, record which tests catch each mutant, and cut the tests that are never
   the only catcher unless they define a contract. An audit's surviving mutants are questions ("does this behaviour
   matter, and does an oracle already reach it?"), not a to-do list of tests to add.

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
| 0 | **Rails.** Nine empty crates; `xtask lint-size`; clippy/fmt/deny config; CI; `docs/port/PLAN.md` (replacing `docs/kafka-port.md`); behaviour-catalogue skeleton. **Go branch of doze-kafka:** `processTarget` for the conformance harness, test knobs for what no flag covers, as environment variables behind a `testknobs` build (see `oracle.md`). | conformance runs through `processTarget` against the **Go** binary and passes; Rust CI green | 2–3 |
| 1 | **Wire.** Spike (crate coverage/hardening), then `kafka-wire`: `FrameDecoder` with size caps, the one `served_apis!` table (four profiles), `ApiVersions`, `Batch` (parse, CRC32C, stamp). **Moved to Phase 3:** codecs with inflate caps (nothing needs them until a produce is validated) and `ErrorCode` wording (the crate has the codes; handlers own the messages). | fuzz targets clean; round-trip vs Kafka's specs; `apiversions` recordings match | 3–4 |
| 2 | **Storage.** `kafka-storage` + its `SimFs` first: `Fs` trait, typestate segments, sparse index, `PartitionLog` writer/readers/`Ticket`, recovery plan, retention. | simulation suite (power cuts, torn writes, ENOSPC, EIO) passes; real-disk smoke; flat-memory check | 5–7 |
| 3 | **Minimum broker.** `kafka-meta` (topics, producer ids), broker core (Metadata, CreateTopics, DeleteTopics, ListOffsets, Produce, Fetch with the `watch` long-poll), `kafka-server` (`Server` builder, connection, shutdown), `doze-kafka` CLI/config. Real clients produce and consume. **First weight baseline.** | produce/consume/metadata conformance via `processTarget`; franz-go, librdkafka, Java by hand | 5–7 |
| 4 | **Groups, classic.** `GroupMachine` + actor; join/sync/heartbeat/leave; offsets; expiry; coordinator and group admin APIs. | classic consumer-group conformance; property tests | 5–7 |
| 5 | **Groups, KIP-848 and migration.** heartbeat/describe, reconcile, assign; classic↔848 via `claims`. | 848 + migration conformance | 4–6 |
| 6 | **Idempotence and transactions.** pure `ProducerSequence` and `TxnMachine`, markers, last-stable-offset and aborted ranges, timeouts, recovery. | transaction conformance; `SimFs` txn durability | 4–6 |
| 7 | **Compaction, topic admin, configs.** compaction planner/executor, `CreatePartitions`, `DeleteRecords`, config APIs, limits. | remaining admin/compaction scenarios | 3–5 |
| 8 | **Security.** TLS, SASL PLAIN and SCRAM, ACL CRUD + enforcement, allowed-hosts guard. | auth/authorization conformance; secret-redaction tests | 3–4 |
| 9 | **Console.** assets copied, maud components and typed view-models, routes (exported as JSON for the e2e route gate), htmx polling, traffic recorder + `Summarize`, telemetry. | Playwright suite (34 specs) passes unchanged in external mode | 8–12 |
| 10 | **Schema registry.** REST routes, `SchemaFormat` for Avro/Protobuf/JSON, wire codec, storage in `kafka-meta`. Highest fidelity risk (Protobuf canonical form and compatibility). | registry conformance scenarios + compat corpora | 5–8 |
| 11 | **Module and release.** ~~`serve` mode (unix sockets) so doze's Go module can re-exec the Rust binary~~ (obsolete: "all Rust, no Go stitching", 2026-11-12). Now module plan step 14: `doze-kafka` gains its doze module face on the Rust module SDK (protocol 2); inherited gates; cutover decision (the owner's). **Built 2026-11-14 (branch `p11-module`, `phase11.md`)**: oracle against the Go kafka module, 43 cases, every difference listed; the published example and the Larder example's kafka block booted through the core's boot sequence in `doze-module-test`. | ~~doze runs the Larder example on the Rust broker~~ the Larder block boots and converges through the harness's boot sequence; the real doze run waits for the Rust core | 3–5 |

| 10 | **Schema registry.** REST routes, `SchemaFormat` for Avro/Protobuf/JSON, wire codec, storage in `kafka-meta`. Highest fidelity risk (Protobuf canonical form and compatibility). 10a (model, API, Avro) built 2026-11-14: `phase10.md`. | registry conformance scenarios + compat corpora | 5–8 |
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
- **2026-11-08, Phase 0 done.** Rails committed (`05b3421`); Go branch `rust-oracle` (`259140f`) runs the
  conformance replay as a process, passing against the Go binary. The test knobs are environment variables
  behind a `testknobs` build (not flags); the contract the Rust binary implements is `docs/port/oracle.md`.
  Note for the Go repo: `cmd/doze-kafka/main.go` gains two nil hooks, which moves the exact-bytes lightness
  baseline; do not merge that branch without regenerating it.
- **2026-11-08, Phase 1 done.** Spike result: `kafka-protocol` 0.18 covers all 41 served APIs (the Go table has
  41 rows, not 42 as first counted) at every offered maximum version; its minimum versions for nine APIs are
  higher than Go advertised at majors 1 to 3 (it dropped what Kafka 4.0 removed), so profiles never advertise
  below what the crate can decode. It has no keys 4 to 7 (inter-broker), which is right: they read as unknown.
  **Defect found and fixed:** the crate sized `Vec::with_capacity` from the wire count, so a 5-byte request
  asked the allocator for ~144 GiB (measured). It is vendored with a two-line patch (`vendor/kafka-protocol/PATCHES.md`),
  held by `tests/hostile_input.rs`, which fails on the unpatched crate. Design: one `served_apis!` macro table
  generates `Request`, `Response`, decode, encode and the version rows. The version table is checked against
  the recorded ApiVersions of real brokers of each major (`tests/data/api-versions.json`); those brokers list no
  telemetry APIs, so the two telemetry APIs are pinned as the only extension. Fuzz targets for the three parsers
  (`cargo xtask fuzz-check` fails a marked parser with no target).
- **2026-11-08, Phase 1 audit and fixes.** An independent audit (a separate agent, scored 6/6/7/5 of 10 on
  fidelity, quality rules, idiom and test quality) found four blocking defects and the phase was not done.
  Fixed: (1) CI could not compile its own tests, because integration tests used a helper behind a feature;
  the crate now lists itself as a dev-dependency with the feature on. (2) The "Produce payload is whole batches"
  rule contradicted recorded Kafka (a partition carries exactly one batch; trailing bytes are dropped); it is now
  `Batch::from_produce`, with Kafka's check order and a `Refusal` class (`CORRUPT_MESSAGE` / `INVALID_RECORD` /
  `UNKNOWN_SERVER_ERROR`), and `Batches` walks a segment lazily. (3) The vendored allocation patch capped
  reservations at the bytes left, which still allowed 76 MB for a 1 MiB body and about 9.7 GiB for a 100 MiB Produce;
  it is now a flat 1024 elements, held by a test that fails when loosened. (4) Seven of sixteen mutants survived the
  tests, including "every response uses header v1"; the response, frame-cap, count-limit and stamping tests now
  read frames back with the crate's decoders, pin the literal sizes and diff bytes, and the mutants are killed.
  Also: the header is read once into typed fields (`Producer` is an `Option`, not a `-1` sentinel), the header and
  body of a request are separate steps so a server can refuse before decoding, `FrameDecoder` takes a `Trust`
  enum, a frame's length is signed, all 31 recorded ApiVersions exchanges and all offered (major, API, version)
  triples are replayed, fuzzing runs at 1 MiB inputs from a seed corpus, and `cargo doc` is a CI gate. Known and
  left for later phases: the three ACL APIs are not offered at `--kafka-version 1` (the crate cannot decode their
  only version there; pinned by a test), and the broker-level Produce rules (size limit, zstd version, CreateTime
  normalisation, recompression) belong to Phase 3.
- **2026-11-08, storage choice for metadata: redb confirmed.** Three independent investigations (ecosystem,
  measurement on this Mac, crash-safety source read). redb 4.5.0 commits in one fsync (bbolt needs two), idles at
  about 4 MB against 7 MB, spawns no threads and does not mmap. Conditions for `kafka-meta`: create the file under
  a temporary name, commit, sync, rename and sync the directory (redb does neither the directory sync nor survives a
  kill during the first create: 118 of 300 trials left an unopenable file); treat any commit or I/O error as fatal
  and restart; always `Durability::Immediate`; set the cache explicitly; pin the version and store our own
  versioned bytes; funnel commits through one writer task (redb's writer lock is unfair); retry `DatabaseAlreadyOpen`
  with backoff. Power loss is untested and needs a crash-injecting backend before 1.0.
- **2026-11-08, Phase 2 (storage), design as built.** `kafka-storage` is synchronous and runtime-free: a partition is a
  value, `PartitionLog<F: Fs>`, whose writes take `&mut self` (no lock), and the one writer task that owns it is
  Phase 3's (the broker). The only async-adjacent thing in the crate is `tokio::sync` channels (feature `sync`, no
  runtime): the end offset (`watch`, so a long-poll cannot miss a wake-up) and the flush state a `Ticket` waits on.
  Group commit falls out of it: the writer appends everything queued, calls `sync()` once, and every ticket is
  satisfied. Readers (`LogReader`, cloneable) read straight off the files up to what the writer has published, so
  a fetch never waits for an append; the segment list is an `ArcSwap` replaced whole on a roll. Active and sealed
  segments are different types. The sparse index is never in memory: a lookup is a binary search of preads, so a
  sealed segment costs the heap nothing and open costs the same for a log eight times the size (measured,
  `tests/flat_memory.rs`). `SimFs` lives in this crate behind `sim`, as planned.
  **Zero copy, on purpose:** a batch is written as its 61-byte header, rebuilt on the stack with the base offset (and
  under LogAppendTime the clock bit, newest timestamp and a CRC extended over the unchanged body), followed by the
  body borrowed from the request; nothing is copied to patch it. The wire crate's copy-on-write stamping API was removed.
  **The simulation** (`tests/simulation`) runs random workloads and, after the process is stopped or fails at every
  file operation, or the power is cut six ways, checks that every flush-acknowledged record is there in order byte for
  byte and the rest is a prefix of what was tried. It found three real defects (a hole in the log from names
  persisting out of order; a resurrected empty segment hiding records; a model gap about adopted batches) and each
  fix is held by it and by a unit test; breaking the log flush, the roll's directory flush, or the start-first rule
  each fails every sweep. Dropped from the Go design on purpose (fresh data): the `unvalidated` list, and the
  per-segment "unsynced" bookkeeping, because a segment is flushed, name included, before its successor exists.
- **2026-11-08, Phase 2 audit and fixes.** The first independent audit said **not done** (fidelity 6, rules 8, idiom and
  zero-copy 6, tests 5; 47 of 75 mutants killed) and found two blockers. Fixed:
  (B1) after DeleteRecords-to-the-end and a power cut the log end could go backwards, reissuing offsets: the log now
  carries on from a recorded start that outlived its records (a new segment begins there), and the open re-records the
  start before removing segments on its account. (B2) the simulation was too kind: its restart made unflushed data
  durable and every cut used one seed. `process_restart` now keeps the page cache and the pending state, cut seeds vary
  per workload, the live log is verified after every step, clean stops exercise the vouched path, and the model bounds
  the reopened start above (by the highest start the process reported, or retention's active-segment bound). With the
  stricter model it found, besides B1: the unvouched open marked the log dirty but not the index (a stale sealed index
  after restart then power cut); and a harness bug of mine (reading "offline" before the close that made it so).
  Also fixed: `Ticket::durable` hung for ever if the log was dropped while a reader lived (the log now says it is
  gone); a damaged committed header was silently skipped (now an error); stray names like `5.log` bricked a partition
  (only canonical 20-digit names count); `enforce_closed` did not record the start first or flush after; size-only
  retention read every segment (ages are now worked out only when an age bound is set, and only up to the first
  segment that is not too old); the flaky Phase 1 allocator test (the guard now covers a test's whole run).
  **Copies and allocations removed:** a fetch now reads one range (the bytes it returns plus 16 KiB of headroom) and walks
  headers in memory (15,435 preads for 1 MB became about a dozen, most of them the index's binary search); a vouched
  open walks only from the last index entry instead of reading the whole active segment; the scan window no longer
  re-zeroes its buffer on every refill; the index is rewritten and compared 4 KiB at a time. Left as a stated trade-off:
  two pwrites per batch (body, then header), because std has no stable positioned gather write and joining them would
  copy the body; revisit with a vectored write if profiling in Phase 3 shows it matters.
  **Dropped as unneeded:** an extra directory flush in DeleteRecords that the carry-on rule made redundant.
  **Known limit, documented (S11):** after a failed fsync only a machine restart clears the dropped pages; a process
  restart finds them in the page cache. Mutation check after the fixes: 13 of 13 sampled mutants of the new rules are
  killed (the audit's 28 survivors were closed with direct tests or by the stricter simulation); 109 unit tests, the
  simulation at 600 seeds per sweep (about 9 s in release), 5 real-disk tests, the flat-memory test, and a recovery fuzzer.
- **2026-11-08, Phase 2 test diet.** The owner's note, "I think [I] overdid tests in go cos of fear and possibly bad
  methodology. Do not overdo the same with rust", applied to my own work: after the audit I had written a test for each
  surviving mutant and the crate had 120 tests around a 40-line simulation. The method now in quality rule 10 was run:
  28 mutants across the crate, the tests that fail for each recorded, then every test that was not an oracle, the
  only catcher of some mutant, a regression for a bug actually found, or the only check of a contract behaviour was
  deleted. 120 tests became 61 (54 unit, 2 real-disk, 1 flat-memory, 4 simulation sweeps); the same 28 mutants were run
  again and none escaped. The behaviour catalogue names only tests that exist; a behaviour the simulation exercises
  says so. Phase 1 (`kafka-wire`, 88 tests, written before this rule) gets the same treatment next.
- **2026-11-08, Phase 2 second audit: done on evidence, no blockers.** It wrote its own checks, independent of the
  simulation: 6000 seeds of DeleteRecords (to the end and mid-offset, with and without unflushed records) under
  every data and name cut and chained restarts, never a log end below a recorded start or a reissued offset; 6000
  seeds of reads against a model (max_bytes 0 to `usize::MAX`, limits, index intervals from 1 to 2^30), no mismatch;
  a vouched/unvouched open matrix over damaged logs and indexes, always the exact prefix or an error, never wrong
  bytes; the simulation at 1000 seeds, the recovery fuzzer (1.3M runs) and the concurrent-reader test (20 of 20).
  Measured copies: an append of 8 x 4 KiB batches allocates 512 B and copies no data; LogAppendTime allocates nothing.
  Acted on: a seek past the log, and a log file cut under an open reader, are now damage (an error) and not a silent
  skip; the flush in DeleteRecords-to-the-end (of data about to be deleted) and the segment's duplicate cut after a
  failed write (the payload's rewind does it) are removed; the simulation's read step now checks every read against
  the model (the batch holding the offset, whole batches in order, within max_bytes and the limit, stopping early
  only at a segment boundary, the end or a limit), which replaced two example tests; one more redundant scan test
  went. Restored after the diet because nothing else reached them: a full disk mid-write (the simulation only
  injects one-shot failures) and the rebuild of a missing sealed index. Deliberately not done: no test for a log file
  cut under an open reader (outside what is guaranteed; the check is four lines), none for the mtime fallback of
  `enforce_closed` (real clients always send timestamps), and none per surviving mutant on purpose (rule 10).
- **2026-11-08, Phase 3 slice 3a (domain types, `kafka-meta`).** `TopicName`, `TopicId`, `PartitionId`,
  `PartitionCount` in `kafka-wire::ids`; `kafka-meta` is one concrete `MetaStore` (no trait) over redb 4.5.0 pinned
  with `=`, running on our `Fs` through a `StorageBackend` adapter, so the Phase 2 simulated disk crash-tests the
  metadata. Topics are held in memory after a commit; producer ids are reserved in durable blocks of 1000. Catalogue:
  `behaviours/meta.md` (M1-M7). Three sweeps (power cut after each step in six ways, process stopped after every file
  operation, every single failing operation with and without lost pages) at 200 seeds pass. The sweeps found one real
  defect: the database made at first start was closed by dropping it, and redb's close flushes once more; that flush's
  failure was swallowed and, under the kernel's lost-pages behaviour, the file then reported success for every later
  flush while writing nothing. Fixed by closing by name and failing the open (M5); the same hazard in the model is why
  `kill -9` forgets the store rather than dropping it (a kill runs no destructor) and a failed clean shutdown is
  followed by a reboot, as for logs. Mutation check (11 mutants): 10 killed at first; the survivors were an id table
  that nothing read (removed) and a version test whose record was malformed anyway (now a good record with its
  version byte changed). Locking is deferred: redb is given none, the binary will hold a data-directory lock (3g).
- **2026-11-08, Phase 3 slice 3b (server, ApiVersions).** `kafka-wire::profile::ApiSet` and `Features.served`: the
  APIs a broker has a handler for are neither advertised nor accepted, so "advertised == enforced" stays in one
  place (the profile). `kafka-broker::Broker` is a skeleton with an exhaustive `match` over `Request` (no
  wildcard; the unserved variants share one arm and move out one phase at a time) and `Outcome`
  (`Respond`/`NoResponse`/`Close`). `kafka-server`: `Server::bind(Config, Arc<Broker>) -> Running`
  (`local_addr`, `shutdown`), an accept loop with a connection limit (2048) and accept backoff, and a connection
  that is a generic `AsyncRead + AsyncWrite` loop (one request at a time, idle 10 min, 30 s for a started frame
  counted once, 30 s to write, a stop interrupts a waiting handler). `kafka-wire` re-exports `messages` and
  `ResponseError` so no other crate names `kafka-protocol`. Mutation check (8 mutants): 5 killed, two hung the
  suite (a hang is a failure for CI), two survived and were killed by strengthened tests (the trickle test now
  checks the connection is gone at 48 s, and a new test covers a partial request left in the buffer); one
  (`Outcome::Close` ending the connection) cannot be reached until a handler can answer Close, so it waits for 3c.
  Catalogue: `behaviours/server.md` (V1-V7).
- **2026-11-08, Phase 3 slice 3c (topics).** `Fs::remove_dir` (real and simulated: a removed directory takes files
  a cut kept without it), `kafka-storage::topic_dirs` (partition directory names, make, remove, stray clearing),
  `Broker<F: Fs>` (generic, as the server now is, so the broker runs on the simulated disk) with `Metadata`,
  `CreateTopics` and `DeleteTopics` handlers, one file each. The topic configs are a data table
  (`topic_config/table.rs`: key, kind, default) with Kafka's refusal wording from the recordings, and the
  configuration in force is answered on create. `Listener` carries the advertised address into a handler (a loopback
  listener advertises itself, a wildcard `localhost`, port 0 the bound port). The recordings drove the messages; four
  deliberate differences from Kafka are in `behaviours/topics.md` (case-only collisions, replication factor above one,
  `segment.bytes` floor, and nothing else). A power-cut test found a real defect: the data directory was created
  without flushing its parent, so a cut took the directory, the database and every topic with it
  (`create_dirs_durably`). Mutation check (13 mutants): 12 killed, one (partition limit) killed by a new
  boundary test. Catalogue: `behaviours/topics.md` (T1-T9).
- **2026-11-08, Phase 3 slice 3d (Produce, InitProducerId).** `Partitions` (opened on first use, one slot per
  topic id and partition, concurrent opens share one) and a writer task per partition that drains its queue, appends,
  and flushes once for the group if any append asked for it (the producer then waits on the append's ticket); appends
  that did not ask are flushed `fsync_interval` after the first, with a timer armed only while there is something to
  flush. Produce checks follow Kafka's order: request-level first (acks, every partition's first batch, transactional
  without an id), then per partition (`Batch::from_produce` with `ProduceRules`: zstd before v7 and the size limit now
  sit where Kafka puts them, and the two refusals Kafka words are worded). `ProduceError::before_log` tells which
  refusals carry a log start offset. DeleteTopics closes the topic's partitions after removing the registration and
  before the files. Mutation check (22 mutants): 19 killed at first; the survivors were the durable flag (an
  acks=all produce could have waited for the timer: test now sets the timer to an hour), two formatting misses
  re-run and killed, and a timer left armed after a flush (harmless, one extra no-op flush; left). Catalogue:
  `behaviours/produce.md` (P1-P10). Idempotence is accepted without sequence checks, as planned for Phase 6.
- **2026-11-08, Phase 3 slice 3e (Fetch, ListOffsets).** `Batch::record_times` (kafka-wire, fuzzed under
  `fuzz_batch`): offset deltas and timestamps of an uncompressed batch without reading keys and values.
  `LogReader::offset_for_timestamp` and `offset_of_max_timestamp` (no time index: they walk batches from the log
  start and read only the answering batch's records; checked against a list of every record). Fetch: partitions
  resolved and opened once, then rounds of reads under a request-wide byte budget, between rounds a wait on the end
  offsets of the partitions and on a broker-wide count of topic deletions. Mutation check (about 30 mutants over 3d
  and 3e): all killed after tests were added for the survivors. One real defect found by a test: the first waiter
  built its futures afresh on every poll and dropped them, and a dropped `watch` waiter is no longer woken, so a
  fetch slept its whole wait; the waiter now keeps `tokio-stream` `WatchStream`s alive between polls (the one new
  dependency, tiny). A second was found in review of the same test: a deleted topic's closing did not drop the
  watch sender (readers hold it), so the broker counts deletions for waiting fetches instead. Catalogue:
  `behaviours/fetch.md` (F1-F8, L1-L2).
- **2026-11-08, Phase 3 slice 3f part 1 and 3g (validation, codecs, the binary).** `Batch::scan` reads every record
  through its codec as a stream (gzip incl. multi-member, snappy in xerial or plain framing, lz4 frames, zstd with a
  window cap before the decoder allocates), pure Rust, each a cargo feature; keys on compacted topics, timestamps
  against the clock, trailing bytes, compressed numbering, Kafka's texts; the header's newest timestamp is now the
  records' (`Batch::with_max_timestamp`, carried into the stamped header and CRC without copying the body).
  `Broker::shutdown` flushes and closes every partition and writes the clean-stop marker; opens are Vouched when the
  last stop covered them (shown by damage only a full scan would find). The `doze-kafka` binary: clap flags (the Go
  edition's, Go-syntax durations, the oracle's flags hidden and ignored, authentication refused), a data-directory lock,
  two worker threads, SIGTERM/SIGINT clean stop, `listening addr=` readiness line; black-box tests spawn it and speak
  the protocol over a socket, including SIGTERM within ten seconds and a SIGKILL. Mutation checks on both. Deferred and
  recorded: rewriting a batch into the topic's `compression.type`; the descriptor budget (3f part 2). Catalogues:
  `behaviours/produce.md` P11-P14, `behaviours/binary.md`.
- **2026-11-08, Phase 3 oracle run.** `CONFORMANCE_ONLY` added to the Go harness (oracle branch, local). First run
  against the release binary (2.9 MB): 5 of 10 Phase 3 scenarios passed. The differences were real fidelity gaps that
  the broker's own tests had not caught: gzip with trailing bytes, zstd window classes, rewriting a batch into the
  topic's codec (now `Batch::recode`, with encoders for all four codecs), a transactional bit without a producer,
  the text for nonsense after a batch, the value length in a message. After fixing them: 8 pass outright; the other two
  differ only in steps that belong to later phases (idempotent sequence checks, IncrementalAlterConfigs). Table in
  `oracle.md`.
- **2026-11-08, Phase 3 real clients and the weight baseline.** Real clients, by hand against the release binary:
  kcat (librdkafka): metadata, produce to an auto-created topic, consume from the start and from the end, ListOffsets
  (-Q), keys, headers, gzip/snappy/zstd batches, idempotent and acks=0 producers, an 800 KB message;
  franz-go (default idempotent producer, zstd): 2,000 keyed records with headers to a 3-partition topic, consumed back
  by partition, kadm create/list-start/list-end/list-after-time/delete. All passed first time. (librdkafka does not
  compress with lz4 yet: it asks for FindCoordinator to allow it, which is Phase 4. The Java client needs consumer
  groups for its usual flows and no JDK is installed here: Java is checked when the groups exist.) The scale
  measurement (3,000 partitions) found what the Phase 2 audit predicted: every ListOffsets opened its partition, 6,016
  descriptors and 30 MB. Fixed: `--max-open-partitions` (512) closes the least recently used idle partition (flushed,
  and trusted again when reopened), and a ListOffsets for a partition never written to does not open it:
  16 descriptors, 7.7 MB, 58 ms. `docs/port/weight.md` has the baseline against Go: binary 3.0 MB against 28.4,
  idle 4.4 MB against 26.8 and 3 threads against 12, throughput the same or better, and the one number that goes the
  wrong way (resident after a heavy produce: freed large buffers cached by macOS's allocator, plateau, reclaimable;
  Linux not yet measured).

- **2026-11-09, Phase 3 closed.** Audit round 1 (NOT DONE) found a delete/re-create race, a Fetch spin, the clean-stop
  marker vouching for a failed close, descriptor exhaustion. Round 2 (NOT DONE) found the new frame pool deadlocking (two
  locks taken in opposite orders: found by the auditor in a real server under load) and a produce that resolved a topic
  before a delete+re-create opening a second log on the new topic's directory. All fixed with tests that fail on the old
  code (a watchdog thread test for the pool; a stale-topic test that watches for any file operation). Round 3 judged the
  Phase 3 fixes DONE. The frame pool (`kafka-server/src/pool.rs`, 119 lines, safe) took the resident memory after a
  200 MB produce on macOS from 200 MB to 24 MB; `weight.md` has the measurements against the system allocator and
  jemalloc/mimalloc. The librdkafka lz4 note above is closed: FindCoordinator is served, and all four codecs produce and
  read back through kcat.
- **2026-11-09, Phase 4 (classic consumer groups) built, slices 4a to 4g** (`phase4.md`, `behaviours/groups.md`): committed
  offsets and group records in `kafka-meta`; the group as a pure state machine with an actor per group; JoinGroup,
  SyncGroup, Heartbeat, LeaveGroup, FindCoordinator, OffsetCommit/Fetch/Delete, DescribeGroups, ListGroups, DeleteGroups;
  Kafka's answer order ported and checked against 13 recorded orders; offsets retention with a test-control socket for the
  oracle. Nine classic oracle scenarios pass against recorded Apache Kafka 4.3.1; the rest differ only in KIP-848 steps.
  kcat -G (librdkafka) and franz-go (two members, a rebalance, resuming from committed offsets) work. Audit round 1
  (NOT DONE) found a refused or first-contact join pinning a group's offsets for ever, unbounded group actors from clients
  that only ask for ids, and a crash simulation that did not cover the delete paths; fixed by moving issued member ids to
  a bounded table in the registry, writing a group record only for a join that would seat a member, conditional expiry,
  and the simulation now power-cuts every write the groups make. The real clients and the oracle found four more bugs
  that unit tests had not (FindCoordinator v4 encoding, the first joiner leads, an empty instance id is a static member,
  the sweeper forgetting a group mid-handshake); the test rig now encodes every response it is given.

- **2026-11-09, Phase 4 closed.** Five audit rounds (3 MAJOR, then 1, then 1, then none). What kept being found was one
  class: state that a client could make the broker keep for ever (a "live" record from a join that was refused, a group per
  id-only client, a group per made-up id). The fix that ended it was structural, not another special case: issued member
  ids moved out of the group into a bounded table in the registry, a record is written only for a join that would seat a
  member, and the sweeper retires any empty group with no record. Also learned: a real client and the recorded-Kafka oracle
  found four bugs that unit tests did not; the test rig now encodes every response; `cargo clippy` goes before every
  commit (a test function over the length limit was committed once without it). Open and recorded in
  `behaviours/groups.md`: the Java client; KIP-848 and the migration (Phase 5, which needs the owner's go-ahead).
- **2026-11-09, Go clients added; the Java client dropped (owner's call).** `interop/` holds a small Go program and
  `interop/run.sh` (also a CI job): sarama, franz-go and kafka-go each produce, read through a two-member group, and a
  new consumer of the same group resumes from the committed offsets; all three pass. They found one more bug of the
  encode-refused-so-the-connection-closed kind: ListOffsets answered with a leader epoch of 0 below v4, and every
  ListOffsets test had used v7. Fixed, and `tests/versions_data.rs` now asks every served version of Produce, Fetch,
  ListOffsets, Metadata, CreateTopics, DeleteTopics and InitProducerId (the group APIs were done in `versions.rs`).
  Learned about the clients: kafka-go does not retry the first Metadata answer for a topic that does not exist yet (the
  recorded Kafka answers it the same way), and franz-go asks for auto-creation only when told to; neither is a broker
  fault.
- **2026-11-09, mixed-library groups.** `interop/` now has eight scenarios, among them groups whose members are
  different libraries (sarama, franz-go and kafka-go together, with the range and the round-robin assignor, and pairs
  with each as the leader). The group is settled (DescribeGroups through sarama says Stable with all members) before
  records are produced, and every member must read something. All pass: the leader of one library decodes the
  subscriptions of another, the vote picks the common protocol, and each gets its own assignment back. A first run
  looked like a broker fault (a franz-go leader, one member reading everything); franz-go's debug log showed the plan
  was right and the records were all in one partition, because franz-go's producer sticks to a partition per batch.
  Test data now keyed. Lesson: read the client's own log before suspecting the broker.
- **2026-11-09, librdkafka in the set, and a group under churn (owner's asks); CI stays local for now.** The owner wants
  things perfected locally before any GitHub CI, so the `interop` job added earlier is removed (`interop/run.sh` is run by
  hand). librdkafka runs through kcat as a process of its own. New scenario `churn`: four libraries in a group while
  records flow; franz-go leaves, a librdkafka member is killed with SIGKILL (the broker's 6 s session ends it: seen at
  5.5 s), a sarama member joins; every record is read and every partition ends committed. Checked by mutation: a broker
  whose sessions never expire fails it. Found: nothing wrong in the broker; two harness mistakes, both the client's
  ways (kcat `-o beginning` ignores commits; kcat reads no option after a topic).
- **2026-11-09, Phase 5 built (KIP-848 and migration), awaiting its independent audit.** One actor per group owns a
  `Group` that is a classic or a consumer group; a migration is a conversion inside the owner (this replaces the plan's
  "claims registry": no claim race to arbitrate, no lock order). The assignors are pure and property-tested; the consumer
  machine is sans-IO like the classic one, with a random simulation of its ownership invariants (mutation-checked). All the
  recorded KIP-848 and mixed-protocol scenarios pass, as do the classic ones; franz-go and librdkafka run on the new
  protocol, alone and mixed with classic members. Weight 3.62 MB / 107 packages (one dependency added, `regex-lite`, for
  pattern subscriptions). Learned: franz-go's switch to the new protocol is `ServerSideBalancer`; a first "pass" with the
  wrong option was a classic run.
- **2026-11-09, Phase 5 closed** on an independent audit after two rounds (3 MAJOR then 1 MAJOR, the second introduced by a
  fix of the first: a refused join to a group that did not exist answered "not found"). Lesson: a fix that changes
  *when a thing is made* changes the answers of everything that used to be answered by the thing; test it on a group that is
  not there. The test debt the auditor listed is in `phase5.md`. Phase 6 (idempotence and transactions) was started at
  the owner's request while Phase 5 was being audited; 6a (idempotent producers) is committed.
- **2026-11-09, Phase 6 built (idempotence and transactions), awaiting its independent audit.** The partition writer owns the
  producer tracker (last five batches per producer, rebuilt from the last 16 MiB of the log) and the transaction index
  (open transactions, aborted ranges, last stable offset, rebuilt from the log); markers are control batches built in the
  wire crate. The coordinator keeps one record per transactional id in `kafka-meta`, written before the broker acts (the
  metadata crash sweep now writes them), pure rules decide every answer (EndTxn's table is the recording's), and a stop in
  the middle of an end is finished at the next start, before the listener binds. `read_committed` Fetch and ListOffsets use
  the last stable offset. `produce-validation`, `endtxn-decisions` and `txn-empty-id` pass; franz-go and sarama transactional
  producers, a franz-go consume-transform-produce session (200 records, 9 transactions, one aborted and redone, output
  exactly once) and `read_committed` readers (franz-go, sarama, kcat) pass in `interop/`. Weight 3.76 MB / 107 packages /
  4.5 MB idle. Learned: kafka-go's `ReadCommitted` ignores the aborted list and returns markers as messages.
- **2026-11-09, Phase 6 first audit: NOT DONE; the debt pass.** The independent audit of Phase 6 (HEAD 3ed9e1e) found a
  BLOCKER (a transactional Produce to two partitions took the transaction's lock once for each; a writer arriving between
  the two holds deadlocked it, and the sweeper with it), six MAJOR (transactional ids kept for ever; InitProducerId
  ignoring the producer id and epoch a caller holds; recovery untested; catalogue items citing tests that did not check
  them; TxnOffsetCommit ignoring KIP-447 and Kafka's order; read-committed work queued behind the writer and reading the
  whole log inside it) and a list of minors; the commit also had a clippy failure. At the same time the owner said nothing
  known is to be carried past Phase 6, so every older debt was gathered into `debt.md` and cleared the same way. What was
  done, with a test that fails when the behaviour is removed for each: the deadlock (a repro that fails on the old code);
  id expiry; KIP-360; a stop at every operation of an EndTxn (kill and power cut, commit and abort) and one failed
  operation anywhere in it; the describe APIs asserted; the group fence for TxnOffsetCommit; the transaction index shared
  with readers behind a lock that the writer takes before it appends, a durable note on disk saying a log holds
  transactions so that a log that never did is not read for them; Kafka's owner codes, order and clocks, read from its
  source by a research agent (copies kept in the scratchpad) since the recordings do not reach them; OffsetFetch v9's member
  check and `require_stable`; the heartbeat words; a peer address for described members; ids issued for the session timeout
  with a cap for each group; a descriptor budget for open partitions; a read clamp; a cached topic table; the six Phase 5
  mutation tests; a librdkafka transactional producer (a small C program) and a zombie-fencing scenario in `interop/`.
  Lessons: a hold taken twice by one request is a deadlock the moment a writer queues between (tokio's lock is fair); a
  test that cannot tell a lock from a stale copy of what it guards needs the interleaving it is about (here, a reader on its
  own thread asking while a producer writes), and where the schedule rules it out the mutant is written down as surviving,
  with the reason; and Kafka facts that no recording holds are read from its source, by item, and marked as such.
- **Phase 1 test diet (2026-11-09).** Rule 10 applied to `kafka-wire` (debt F1). cargo-mutants 27.1 over the
  whole crate, every test binary run for every mutant (`--no-fail-fast`), the failing tests of each mutant read from its
  log: 719 mutants, 519 caught, 42 timed out, 96 missed, 62 unviable. Kept: every integration suite (recorded ApiVersions
  and exchanges, every offered version, the independent encoder, frame splits, no-panics, the allocation tests), the
  regressions for bugs actually found (frame caps and count limits, the v0 ApiVersions header, stamping byte diffs, one
  batch per partition, the oracle run's gzip trailing bytes, zstd windows, nonsense after a batch and recode), the sole
  catcher of any mutant, and the only check of a catalogue sentence (a zero-length frame, a length over every cap with no
  key, an unknown key, Kafka's check order and refusal classes, multi-member gzip, unreadable codecs, the producer rules).
  Cut: tests that restated the suites (the header parses, response framing, request round trips, floors: all done for every
  API at every version by `every_offered_version`; the ApiVersions answers and client-software names: done by the 31 recorded
  Kafka exchanges), getters, and duplicate refusals. 123 tests became 87 (unit 102 to 67, integration 21 to 20, the
  ignored corpus writer included); `api/tests.rs` is gone. Added where a contract behaviour had no check: one test of the
  zstd window classes (2 MiB never refused, a single segment's content size is its window, past 128 MiB unreadable), a batch
  of exactly `max.message.bytes` is taken, and an empty key is a key on a compacted topic. Proof: every mutant the first run
  caught with a named test still has a catcher (552 of 552, checked against the remaining test list; the other 9 caught
  were timeouts with no test named); the same 719 were run again, and every timeout or difference (81 mutants) rerun at
  two jobs and a 90 s timeout: no mutant the old suite catches escapes the new one. Lesson: at six jobs the machine is loaded
  enough that a test run passes 20 s and is called caught; 13 such "catches" (Display and `AsRef` of ids, `Stamped`
  accessors, the varint's tenth-byte check, the empty-key comparison) were missed by the pre-diet suite too when rerun
  quietly. About 90 mutants survive, on purpose: Display, `AsRef` and getters of the id types and `Stamped` (used and tested
  by `kafka-meta`, `kafka-storage` and the broker), bounds checks the cursor repeats, equivalent boundary forms (a frame of
  length 2, the reservation size, the 14-byte floor, `|` for `^` in zigzag), the test builder, the marker's leader epoch,
  and the varint's tenth-byte check (stricter than Kafka's, which ignores the extra bits). The `before_log` survivors are
  held by `kafka-broker/tests/produce_checks.rs`.

### Phase 7 (2026-11-09): topic admin, configs, retention, compaction
Built in five slices on the debt branch, each with its tests and a commit: 7a retention sweeper and DeleteRecords; 7b the
three config APIs (topic and broker resources, the five dynamic broker configs kept durably, a change reaching open
partitions at once, Kafka's validation order and words, the cross-field compaction-lag rule); 7c CreatePartitions (directories
first, then the count; swept at every operation); 7d compaction (a stored batch spans its offsets; `Batch::filter`; planner,
offset map, transaction tracking, the job, the writer's install with the index removed first; the cleaner task; reads skip
batches with no records when records follow). Catalogue: `behaviours/{admin,compaction}.md`. Oracle: `list-config-append` and
the Phase 7 rows of `version-matrix` pass (`oracle.md`); a full replay is 43 pass, 15 fail, all Phase 8 and 9. Weight: 4.01 MB,
107 packages, 4.5 MB idle (`weight.md`).

Method kept honest this phase: every rule has a test that fails when the rule is put back, and the mutation tool checks that the
mutation applied (two "survivors" were mutations that had not changed the file, one because the formatter had split a line,
one because BSD `sed` has no `0,/re/`). Found by something other than the author's tests: librdkafka against a real compacted
topic (empty batches, G3), the producer-retry test when the fix for it first touched the walk that relearns producers, and the
Random name-loss cut of the stop sweep (the first directory flush in the install cannot be dropped). The audit, when it comes,
gets the property statement of `compaction.md` first.

**Phase 7 audits and closure (2026-11-10): Phases 6 and 7 DONE.** The first independent audit of Phase 7 (HEAD 9eded61) said NOT DONE:
a blocker (aborted records in the compaction offset map lost a committed value), four majors (concurrent config changes lost
acknowledged ones; retention never took an expired active segment and `segment.ms` was not implemented; compaction lag planning
differed from Kafka and was untested; a round held the partition writer for seconds), ten minors and 52 mutation gaps. Every
finding was fixed with a test that fails without the fix (`debt.md` section H), including N3 (a hot key left a file for every segment
it filled: a segment cleaning leaves with only its ending batch is removed, CMP-D1). The 52 gaps were re-run on the fixed code with
seven new mutants for N3 and the audit's list (57 in all): 34 now die to a new test, 8 are written down as equivalent or defensive,
and the work found two real defects the audit had not (a broker number with spaces was validated but not applied; the over-long
config value refusal did not use Kafka's words). The second independent audit (HEAD b394527) said DONE: no blocker or major, it
could not break the segment removal (sweeps at every stop and power cut, failed operations, concurrent readers, DeleteRecords at every
offset, a 400-seed model run), and left five minors and six test gaps, all closed: a transactional producer whose data was trimmed
but whose marker stayed was refused after a reopen (fixed, TX-4); `segment.ms` by wall clock is a decided difference (ADM-D7); four
statements in the docs contradicted the code (corrected, among them that Kafka deletes an empty segment: it merges them by size).
Gates at the closing commit, quiet machine: fmt, clippy `-D warnings`, `cargo test --workspace` 743 passed and 0 failed in 84
binaries (4 ignored on purpose), lint-size (0 over the hard limit), interop 23 of 23 (sarama, franz-go, kafka-go, librdkafka),
oracle replay 43 pass and 15 fail (all Phase 8 and 9), weight 4,029,504 B, 107 packages, 4.8 MB idle. Phase 6 closes with it: the
third Phase 6 audit's fix (A12) was in the Phase 7 audit's scope. Method notes: a test that fixes "the Nth read fails" breaks when a
fix changes the number of reads, so the fault point is swept; a failure that shows up once in a scratch copy is reproduced against
the old code before it is called flaky (here it was the pre-fix defect, deterministic at seed 24).

### Phase 8 (2026-11-12): security, SASL, ACLs, TLS and the allowed-hosts guard (draft entry: built, not closed)
Built in seven slices on parallel branches, each merged to `main` with its gates (`phase8.md` has the ledger and the commit ids). **8a** the `kafka-auth` crate
(RustCrypto `hmac`, `sha2`, `subtle` behind one file; PBKDF2 and base64 in-tree, checked against the RFC vectors and the `pbkdf2` crate), SASL/PLAIN, `Session` and the login
gating (a stranger's body is never decoded; a failed login is answered, flushed, then closed), SEC-1 to SEC-15. **8b** the authorizer (Kafka's decision procedure, checked against a model
transcribed from `StandardAuthorizerData` on 6000 random ACL sets), the ACL table, the three ACL APIs, the Metadata and InitProducerId checks, `--acl-default`, `--acl-super-user`, SEC-16 to SEC-30.
**8c** SCRAM-SHA-256 and -512 (the exchange, the recorded parsers and refusals, constant-time proof check), SEC-40 to SEC-51. **8d** the authorization matrix on every other served handler (11 topic APIs,
13 group APIs, 7 transaction APIs, one source-derived test per row), SEC-60 to SEC-72 and SEC-100 to SEC-123, with 25 and 40 hand-made mutations killed. **8e** TLS as the off-by-default cargo feature `tls`
(rustls 0.23 on ring, PEM only, never a fallback to plaintext, the handshake inside the connection's own task), interop over SASL_SSL, a `weight-tls` CI job, SEC-140 to SEC-154. **8f** the SCRAM
credential APIs 50 and 51 with a stored table, `--sasl-users-file`, `DOZE_KAFKA_SASL_USERS` and `--sasl-allow-empty-users`, SEC-160 to SEC-179. **8g** the allowed-hosts guard (a pure function in
`kafka-auth`, 107 lines, no dependency, fuzzed 11 million runs without a finding, no `.doze` special case), `--console-allow-host` and `--console-unprotected` (parsed and kept; nothing serves a console
before Phase 9), the start-up rule whose inputs exist (SASL or TLS on and a console address beyond loopback is refused, exit 2, unless `--console-unprotected`) and the start-up notes, SEC-180 to SEC-189;
and the documentation closure: the catalogue index, `tools/check-security-catalogue.ts` (a CI step), `oracle.md`, `weight.md`, `debt.md` section I.

Gates at the end of the build: fmt, clippy `-D warnings`, `cargo test --workspace` 1157 passed and 0 failed in 121 binaries (4 ignored on purpose; 1137 at the 8f merge, 20 new in 8g), the `tls` tests of `doze-kafka` and its clippy, lint-size (0 over the hard limit), fuzz-check (17 parsers), the catalogue checker (124 rules, 51
deviations, 252 test names, no problem), interop 23 of 23 plain, 12 of 12 over SASL and 8 of 8 over TLS (recorded at the 8d-2 merge), oracle replay **47 pass and 11 fail** (the eleven are the ten
`telemetry-*` scenarios and `version-matrix` on DescribeCluster and the telemetry rows: Phase 9; `sasl-plain`, `sasl-scram`, `acl-unknown-codes` and `idempotent-write-from-topic-write` pass whole).
Weight, default build: 4,267,504 B (+238,000 B over Phase 7), 123 packages (107 before: the 12 of RustCrypto and `kafka-auth`), idle 4.96 MB; the `tls` build 5.02 MB and 134 packages at 8e (`weight.md`, "Phase 8 close").

Decisions: RustCrypto for SASL (the owner's, D1: the cost is packages, the gate rose from 115 to 126); TLS a feature, not always on (D2); ACL APIs and the authorizer work with SASL off, for `User:ANONYMOUS` (D4,
SEC-D7); SCRAM channel binding as Kafka (D6); the guard is pure and mounted in Phase 9 (D8); 51 deviations written down, each a row of `debt.md` I.1. **What is not known:** about 40 of the 45 matrix rows have no
recording, only Kafka's source, so the one real risk of the phase is a reading that differs from the JVM; the proposal is an `authz-matrix` recording against `apache/kafka:4.3.1` (Docker here has a client and no
daemon). The numbering of the catalogue collided between parallel slices once (SEC-D7 to D11 were used by two slices before a renumbering); the checker now fails on a duplicate, a dangling reference and a quoted test
that does not exist, and found one stale cite (a test file's header) and nothing else. Method notes: slices that touch the same files were built in parallel worktrees and merged, with the gates re-run on each merge; a
mutation run in a copy of the tree needs a target directory of its own (a shared one let the last mutant's build be taken for the worktree's).

**Phase 8 closed (2026-11-12): DONE after four independent audit rounds.** The authentication-half audit (HEAD 909d147) said DONE with one MAJOR design gap: with SASL on and the default `--acl-default allow`, any logged-in user could replace a stored super user's SCRAM credential and escalate; its minors were a world-readable `meta.redb`, 1000 strangers taking the process from 12 to 333 MB, a `--sasl-allow-empty-users` that could never be used, no failed-login delay, plaintext passwords kept with SCRAM only, and an `ECONNRESET` on every failed login on macOS. The authorization-half audit said NOT DONE for one MAJOR: a KIP-848 regex subscription skipped Kafka's DESCRIBE filter, so a member with READ on the group was assigned the partitions of a topic it could not see (an independent transcription of `StandardAuthorizerData` ran 2.13 million checks with no disagreement). Both were fixed with a test that failed first. The re-audit (693da51) said DONE and found one minor of the same CLASS (a static member's replacement inherited the departed member's topic state), so the fix swept every place member state is produced or inherited, including the classic protocol path, and a randomized invariant test covers them. A final focused audit of that round (e6d986b) said DONE: a 630,000-step differential of the group machines with no ACLs against the previous commit found no difference, about 12,000 flapping-ACL liveness scenarios all converged, and an independent invariant generator made about 10 million member checks without a violation; its one narrow minor (a classic-only group not told to rejoin inside the assignment interval) and four test gaps were closed. **Found by the author's own gate, not by any audit:** the audit fix that capped a stranger's frame at 64 KiB turned the recorded real-Kafka step `token-of-64k-authenticate` into a closed connection; the cap is 128 KiB and a test pins the token (`debt.md` I27). Lesson: every hardening change is re-run against the replay, and agents that were told not to run it could not have caught this.

Gates at the closing commit 46d2937 (quiet machine): fmt, clippy `-D warnings` for the default and the `tls` feature, `cargo test --workspace` **1239 passed and 0 failed** in 133 binaries (4 ignored on purpose), 177 passed with the `tls` feature, lint-size (0 over the hard limit), fuzz-check (17 parsers), the catalogue checker (156 rules and 65 deviations defined, 342 quoted test names all exist), interop **23 of 23** plain, **26 of 26** over SASL (four client libraries x PLAIN, SCRAM-SHA-256 and SCRAM-SHA-512, plus runtime user management) and **8 of 8** over TLS, the oracle replay **47 pass and 11 fail** (the eleven are the ten `telemetry-*` scenarios and `version-matrix` on DescribeCluster and the telemetry rows: Phase 9; `sasl-plain`, `sasl-scram`, `acl-unknown-codes` and `idempotent-write-from-topic-write` pass whole). Weight, default build, darwin/arm64: 4,301,200 B (Phase 7 closed at 4,029,504 B, +6.7%), 123 packages (107 before), idle 4.9 MB; the `tls` build 5.02 MB and 134 packages at 8e (`weight.md`).

Sizing, honestly: the plan said 3 to 4 sessions; the phase took the research (three agents), seven build slices (some in parallel worktrees, each merged and gated), four audit rounds and their fixes: about 25 agent runs of one to three hours, i.e. the 7 to 10 sessions of `phase8.md` or a little more. **What is not known:** about 40 of the 45 authorization-matrix rows have no recording, only Kafka's source, so the one real risk of the phase is a source reading that differs from the JVM; the proposal is an `authz-matrix` recording against `apache/kafka:4.3.1` (Docker has a client here and no daemon). The Linux weight of the default and `tls` builds is unmeasured (the estimate says the default build is over the old 5.15 MB gate: Phase 9 raises it by the console's megabyte with headroom, decided by the owner). Out of scope and written down: client certificates and mTLS principals, re-authentication, SASL v0 handshake, OAUTHBEARER/GSSAPI, quotas.

**Real clients at every `--kafka-version` (2026-11-14).** `interop/run.sh -kafka-version N` starts the broker with `--kafka-version N` and
tells the Go program the major (`-kafka-major N`); `interop/run.sh -versions` runs once per major, 1 to 4, each with a broker and data
directory of its own, and prints a summary per major (`-sasl -versions` does the SASL cells). sarama's `cfg.Version` comes from one helper
by major (1.1, 2.8, 3.9, 4.0: the brokers the version table was recorded from); franz-go, kafka-go and librdkafka negotiate from
ApiVersions and need nothing. The program asks the broker for ApiVersions at the start and skips, with the API named, a scenario that
needs one not advertised: no table in the suite. Result: plain 18 passed and 5 skipped at Kafka 1, 2 and 3 (the KIP-848 scenarios:
no ConsumerGroupHeartbeat; 3.9 lists it but its coordinator is classic-only by default, early access), 23 of 23 at Kafka 4; SASL 12
passed and 1 skipped at Kafka 1 (the users step: no SCRAM credential APIs before 2.7), 26 of 26 at 2, 3 and 4. Transactions (all three
producers, the zombie, EOS) and compaction pass at every major, at Kafka 1 over v0 of the transaction APIs. No broker defect was found. One harness defect
was: `mixed` failed about one run in five at Kafka 1 and 2 ("kafka-go read nothing"). The group was Stable with four
members, and kafka-go had an empty assignment. Whenever kcat joined first it led, and librdkafka cannot read kafka-go's
subscription, which is version 1 without its owned partitions. The broker passes those bytes through untouched, as Kafka
does, and `-members librdkafka,kafka-go` fails at every major. The suite now seats the first member before the others
join. The matrix is in `interop/README.md`. CI runs no interop (local by the owner's choice, 2026-11-09), so CI is unchanged.

**Phase 10, slice 10a built (2026-11-14, branch `p10-registry`), not yet audited.** The schema registry's model, its storage in a
`schemas` table of `meta.redb`, every REST route the Go edition serves (Confluent's 29 and the five under `/_doze/`), the host guard,
basic authentication with the SASL users and TLS with the broker's certificate, the wire header, and Avro written in Rust (no
library: `apache-avro` would have added 35 packages and still not given Confluent's reasons). `--schema-registry` is on by default as
in Go, behind a `registry` feature of the binary. The Go oracle passes: every Avro step of the 30 recorded scripts and all 392 Avro
corpus pairs, verdicts and reasons word for word. +233 KB, no third-party package; no limit moved, but 0.2 MB of headroom is left on
the Linux estimate. Protobuf (10b) and JSON Schema (10c) plug into `format::Formats`; the record-value codec is 10d. `phase10.md` has
the weight table, the extension points and the slices; `behaviours/registry.md` the catalogue (REG-1 to REG-27, REG-D1 to REG-D14).
