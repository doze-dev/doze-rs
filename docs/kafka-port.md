# Porting doze-kafka to Rust

doze-kafka is ported first: the doze family's Rust future starts with the product
that stands most on its own. Slow and steady. A piece is ported when doze-kafka's
existing black-box suites pass against the Rust binary, not when its code is
translated.

## What there is to port

doze-kafka (`origin/main`, 2026-11-08):

- **Size:** about 138,000 lines of Go, plus 92,000 lines of tests.
- **Biggest areas:**
  - `console/`: 18,000 lines of Go (view models and handlers), 14 `html/template` files (2,800 lines), and static assets (htmx, Alpine, CodeMirror, CSS, fonts);
  - `internal/broker/`: 162 files;
  - `internal/storage/log`;
  - `internal/protocol`;
  - `schemaregistry/`;
  - `cmd/doze-kafka`.
- **Kafka APIs:** about 100 request/response types, encoded with franz-go's `kmsg`. A `--kafka-version` flag (1–4) chooses which Kafka major's version ranges to advertise.
- **Features:**
  - produce and fetch;
  - compression (gzip, snappy, lz4, zstd);
  - retention and compaction;
  - consumer groups, classic and KIP-848;
  - idempotence and transactions;
  - SASL, TLS and ACLs;
  - a schema registry (Avro, Protobuf, JSON Schema);
  - observability;
  - the web console;
  - the doze module;
  - `.doze` names.
- **Storage:** segment files, plus a bbolt database for metadata (`docs/storage-and-durability.md`).

## The oracle: the suites that already exist

These run from the outside and don't care what the broker is written in:

- **`conformance/`** is differential: it runs the same scenarios against Apache Kafka in Docker and compares what clients see (recordings, claims).
- **`e2e/`** is the Playwright console suite. It already has an external mode (`E2E_KAFKA_ADDR`, `E2E_CONSOLE_URL`), so it can point at the Rust binary.
- **`docs/*_test.go`** checks that the documentation's commands and snippets work.

Phase 0 makes all three runnable against a Rust binary. Every phase after that is done when its part of them passes.

## The console: htmx and Alpine come across unchanged

htmx and Alpine run in the browser, so the server's language doesn't matter to them:

- **Static assets** (`console/static/`): copied as they are and embedded in the binary (`rust-embed`).
- **Templates** (14 files): translated from Go's `html/template` to **minijinja**. Its blocks and inheritance map onto `{{define}}`/`{{template}}`, and it's quick to iterate on; askama, which checks templates at compile time, is the alternative.
  - **Escaping is the one trap.** Go escapes by context: in HTML, in attributes, in URLs and in JS. minijinja escapes HTML only. Every value that goes into an Alpine `x-data`, an `hx-vals` or a URL gets `|tojson` or `|urlencode` explicitly. The Go templates are the list of places to check.
- **Handlers and view models** (18,000 lines): become axum handlers with serde structs. The live parts (the traffic feed, the tape) use the same SSE/polling as today.
- **Done when:** the Playwright suite passes unchanged against the Rust console.

## Crates (prior art)

| Need | Crate |
|---|---|
| async runtime, TCP | `tokio`, `bytes` |
| Kafka wire format | `kafka-protocol`, generated from Kafka's JSON message specs, like `kmsg` |
| HTTP, console, registry | `axum`, `tower` |
| templates, assets | `minijinja`, `rust-embed` |
| TLS | `rustls` |
| compression | `flate2`, `snap`, `lz4_flex`, `zstd` |
| checksums | `crc32c` |
| metadata store | `redb` (bbolt has no mature Rust reader; see decision 2) |
| schema registry | `apache-avro`, `protox` (Protobuf parsing), `jsonschema` |
| CLI, logs | `clap`, `tracing` |
| doze module protocol | `tonic` (gRPC, from doze-sdk's proto) |

Worth reading first: **Tansu**, a Kafka-compatible broker in Rust, for how it uses `kafka-protocol` and lays out its storage.

## Phases

| Phase | What | Done when | Size |
|---|---|---|---|
| 0 | Harness: the conformance, e2e and docs suites run against any binary (a few changes in doze-kafka to take a binary path or address); CI for this workspace | the suites run against the Go binary through the new path, and fail cleanly against the Rust stub | 1–2 sessions |
| 1 | Wire: TCP, framing, ApiVersions, Metadata, `--kafka-version` | the apiversions scenarios pass | 2–3 sessions |
| 2 | Storage: segments, index, metadata store, recovery after a crash | storage and bad-day scenarios pass | 4–6 sessions |
| 3 | Produce, fetch, compression, retention, compaction | produce/consume scenarios, real clients (franz-go, librdkafka, Java) | 4–6 sessions |
| 4 | Consumer groups (classic and KIP-848), committed offsets | the consumer-group scenarios | 4–6 sessions |
| 5 | Idempotence and transactions | the transaction scenarios | 3–5 sessions |
| 6 | SASL, TLS, ACLs | auth and authorization scenarios | 3–4 sessions |
| 7 | Schema registry | the registry suite | 3–5 sessions |
| 8 | Console | the Playwright suite, unchanged | 5–8 sessions |
| 9 | CLI parity, observability, the doze module, `.doze` names | docs suite; doze runs it as a module | 3–4 sessions |
| 10 | Parity and weight: startup, idle memory, per-request cost against the Go build | numbers recorded; no suite red | 1–2 sessions |

That's about 33–52 working sessions. The suites, not the code, set the pace: each phase is done when its scenarios are green. Phases 1–3 come first and are the ones that tell us whether the port pays off.

## Decisions for the owner

1. **Embedding.** doze-kafka is also a Go library: tests start it in-process (`server.New`, `docs/embedding.md`). A Rust binary can't be that. The options:
   - keep the Go library for embedding;
   - ship a small Go package that starts the Rust binary for a test;
   - drop it.
2. **The data directory.**
   - Should the Rust broker read today's data directories (segments, and the bbolt metadata)?
   - Or import them once?
   - Or start fresh? Reading bbolt from Rust means writing a reader.
3. **Releases.**
   - Does the Rust build replace the Go one under the same name (a major version) when it reaches parity?
   - Or ship beside it until then?

## Rules

- No phase is done on translated code alone; it's done when its scenarios pass.
- The Go repo stays the reference until parity. Behaviour changes are made there first, and in the port to match.
- Measure, don't assume: startup and memory are compared with the Go binary at every phase from 3 on.
