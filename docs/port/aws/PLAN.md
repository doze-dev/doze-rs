# doze-aws in Rust: a ground-up implementation

Status: **approved in part, 2026-11-08** (written after four feasibility spikes; the owner answered
the open decisions the same day, see "Decisions"). Nothing here is built yet. Items marked
**PROPOSED** are my recommendation and still need the owner's yes; items marked **SETTLED** come from
the owner's own words. Scope review applied 2026-11-09 (see `SCOPE.md`; decisions D1 to D9 answered, edits E1 to E17 applied below). Still open: the console's build (the direction is chosen: Paper).

Target: this workspace (`~/Code/doze-dev/doze-rs`), beside the doze-kafka port (`../PLAN.md`).
Domain reference: `~/Code/doze-dev/doze-aws` (Go, 17 services, about 138k lines of production Go,
93k lines of Go tests, a 30k-line console). Read for *what* a local AWS must do, never for *how* to
structure Rust. Nothing is translated line by line, and Go's tests are not translated at all.

## Context and settled decisions

- **SETTLED. Ground up, idiomatic Rust.** Typestate, parse-don't-validate newtypes, sans-IO state
  machines, actors over locks, exhaustive enums. No clone-to-appease-the-borrow-checker, no
  `Rc<RefCell>`, no Go patterns hacked into tokio. A copy or clone needs a stated reason.
- **SETTLED. Weight is the headline feature.** The goal is to beat Floci and LocalStack on lightness
  and on what is offered. Numbers may only rise with a written reason (`docs/port/aws/weight.md`).
- **SETTLED. Tests are only the ones truly needed.** The Go suite grew excessive; see "Testing".
- **SETTLED (in principle). redb for metadata**, as in the kafka port.
- **SETTLED. IP and port only.** No doze-names, no name registry, as doze-kafka.
- **SETTLED. Windows and x86 are in scope** (CI matrix from A0; see Risks).
- **SETTLED. Go is frozen; the port is the way forward.** Go doze-aws stays in service until the A17
  cutover but receives no fidelity work; the 951 open findings become the Rust behaviour catalogue.
- **SETTLED. A real AWS account exists** (root only so far; recorder credentials follow, see
  Decisions), so the evidence tags can become `recorded` from A5 on.
- **PROPOSED. Fresh data.** The Rust edition owns its layout and never reads a Go data directory
  (as doze-kafka decided).
- **SETTLED. Protocols by service, not by history.** Query (form body, XML reply) for IAM, STS,
  SNS and CloudFormation, which have no other protocol. SQS: awsJson1_0 only. CloudWatch: awsJson1_0
  *and* Smithy RPC v2 CBOR (boto3 and the CLI send JSON, only the Rust SDK sends CBOR). A client old
  enough to send Query to SQS or CloudWatch gets a clear refusal telling it to upgrade.
- **PROPOSED. Refused, not built:** SigV2, SigV4a, `Date`-header auth, HTTP/2. A refused operation
  answers HTTP 501 `NotImplemented` with `X-Doze-Not-Built` (the Go plan's D5).
- **PROPOSED. AWS facts carry evidence tags** (`model`, `recorded`, `doc`, `memory`), as in the Go
  plan; nothing is fixed on `memory`. Until recordings exist, everything is unverified.
- Go stays the reference until parity; fidelity fixes that are pure AWS knowledge are written down
  once (in the behaviour catalogue) and apply to both.

## Prior art

| Project | What it is | Take from it |
|---|---|---|
| LocalStack Community | Python, broad, about 143 MiB idle, about 3.3 s start, about 1 GB image (its own README); one source reports the community edition needs an auth token since March 2026 | breadth of services is its strength; weight and licence are the opening |
| Floci | Java/Quarkus native, MIT, advertises about 13 MiB idle, 24 ms start (one tester measured about 130 ms), 26 to 83 services depending on source | proof that "light" sells; memory-only mode loses data on restart |
| moto | Python mock library | behaviour catalogue ideas only |
| `s3s` 0.17 | Rust S3 adapter generated from the S3 model, experimental | reference for model-driven S3; not adopted (S3 only, no constraint checks, default trait methods make dispatch non-exhaustive) |
| smithy-rs server | AWS's Rust server generator | its constraint design (RFC-0025) is the same idea as ours; not adopted (needs a JVM, no awsQuery) |
| doze-aws (Go) | the domain reference, 951 open fidelity findings in `plan/services/*.md` | the behaviour catalogue's raw material |

## What the spikes found (measured 2026-11-08; macOS arm64 and a Linux aarch64 VM; shared machine)

Spike code lives in the session scratchpad (see Open decisions about archiving it). The findings:

| Question | Result |
|---|---|
| Codegen from the Smithy models | Works for all 18 model files (1,372 operations). **Serde-derived decoders are the size trap**: a use-everything binary was 13.3 MiB and took 99 s to compile; about 12 KB of binary per data type. Generated types alone are cheap |
| Models as the source of limits | They under-constrain. `sqs.json` has no length, range or pattern traits at all; 14 of 310 distinct patterns cannot compile in `regex` (look-arounds, `\p{Print}`), 52 newtypes would lose their check. Hand-written overlays are needed |
| Wire layer | JSON 1.0/1.1, Query, CBOR, REST-XML and REST-JSON all pass the Rust SDK, boto3 and the AWS CLI. Whole server 0.87 MB stripped, 2.2 MiB idle, a 1 GiB streamed upload peaks at about 10 MB (flat at 4 GiB) |
| SigV4 | Hand-written verification (about 600 lines, +1.7 KB). `aws-sigv4` is a signer (+81 KB, five workarounds); keep it as a test oracle. Current SDKs send hex, `STREAMING-AWS4-HMAC-SHA256-PAYLOAD-TRAILER` and `STREAMING-UNSIGNED-PAYLOAD-TRAILER` (HTTPS only) |
| Weight, skeleton with 16 lazy redb services | Idle 2.6 / 2.4 MiB (Go: 31.4 / 26.8), **0** wake-ups in 30 s (Go: 583 / 132), start 2.4 / 0.74 ms (Go: 12.9 / 9.7). After 1,000 requests and reaping: 6.9 / 5.3 MiB (Go: 46.5 / 40.3) |
| Binary | Whole dependency stack 3.86 MB (4.13 MB Linux). Fat LTO with `opt-level` s or z saves 20 to 27% |
| Storage | redb adds 574 KB; one commit 3.1 ms against bbolt 6.0 ms; 16 open databases idle at about 4 MiB against 7.6 for bbolt. Holding a write transaction across an `.await` on a current-thread runtime **deadlocks**; use a group-commit flusher |
| Timers | `DelayQueue` costs 56 to 74 bytes an entry and zero idle wake-ups, but an empty queue spins unless wrapped, and `remove` panics on a stale key |
| JSONata | `jlens-core` passes 1,685 of 1,689 official cases; `seuil` (my earlier pick) passes 70% and crashes on hostile input |
| Parsers | `winnow` produces DynamoDB-shaped errors; every recursive parser overflows a 2 MiB stack at about 2,000 nested parentheses, so a nesting cap (300) is mandatory |
| Crypto | RustCrypto covers all of KMS (RSA, ECDSA P-256/384/521, secp256k1, Ed25519, AES-GCM, HMAC) and agrees with ring and aws-lc-rs; `rsa` exists only as `0.10.0-rc.22`. RSA-4096 key generation takes about 0.5 s |
| IAM, cron | No crate gets either right (best IAM crate: 48 of 53 cases). Write both |

Not verified anywhere: behaviour against real AWS (no account), x86, musl, Windows, and the speed cost
of `opt-level = "z"` on CPU-heavy paths.

## Design: what Rust makes natural (and what it replaces)

| Concern | Go design | Rust-native design |
|---|---|---|
| "Accepts what AWS refuses" (about 60 bugs found by boto3) | validation sprinkled through handlers | **Checked newtypes** (`QueueName`, `Arn`, `BucketName`...) built only through constructors that enforce the limits. Handlers cannot receive an unchecked value |
| Operation dispatch | route tables plus a test that every route has a handler | one exhaustive `match` over a generated `Operation` enum per service; an operation is built or refused, or it does not compile |
| Request decoding | per-service hand code plus an SDK-shaped table | **generated types and schema tables, one hand-written table-driven decoder per protocol**. No serde derive on wire types (size) |
| Caller identity | values on the request context (the chi trap, SQS-30) | an `Authenticated` request type that cannot be constructed without a verified `Principal`; account and region are explicit parameters |
| Service state | mutexes plus bbolt | **one actor task per service owns its state** (`&mut self`); requests arrive over an `mpsc`, replies over a `oneshot`. No lock order exists |
| Business logic | handlers that read the clock and write the disk | **sans-IO**: `fn step(&mut State, Op, Ctx, now) -> Result<(Output, Vec<Effect>)>`; the actor performs the effects (persist, arm a timer, tell a peer). Tests drive it with synthetic time and `proptest` |
| Service lifecycle | `lazybolt` plus a supervisor | a service is an 8-byte handle until its first request; it spawns, opens its redb file, serves, and after an idle timeout closes and frees itself. Nothing polls |
| Timers | `time.AfterFunc` plus generation counters | `tokio_util::time::DelayQueue` behind an `Expiring` wrapper; paused-time tests |
| Cross-service calls (SNS to SQS, S3 to Lambda, EventBridge targets) | `internal/peercall` over loopback URLs | typed messages on an in-process bus, carrying the caller's `Principal` explicitly. No loopback HTTP, no URL parsing |
| Storage | bbolt per service, files for blobs | redb per service (postcard values with a version byte, one writer, group commit); blobs (S3 objects, Lambda zips) as files written temp-then-rename-then-fsync through the shared `Fs` trait; append-only streams (Logs, Kinesis, DynamoDB streams) on the shared partition log if it generalises (see Risks) |
| IAM | `iampolicy` plus `iamguard` middleware | a typed policy tree parsed once and a **pure evaluator** with identity, boundary, session, SCP and resource layers, policy variables and the full condition-operator set; a tower layer calls it |
| Errors | error values and sentinels | one `thiserror` enum per crate; each maps to an AWS error code and status once, at the protocol boundary; `anyhow` only in the binary |
| Config | TOML plus flags with manual precedence | `clap` derive plus `serde` (`deny_unknown_fields`) into a raw layer, then `TryFrom` into a validated `Config`; secrets are `secrecy` types |
| Shutdown | context cancel and wait | `CancellationToken` plus `TaskTracker`; `Running` handle with `local_addr()` and `shutdown().await` |
| Console HTML | Go templates, 29 templates, 371 routes | `maud` components over typed view-models; htmx and the assets embedded with `rust-embed`; route list exported for a route gate. Target is the dream-design boards, not the Go console's present shape |

## Quality rules

Inherits all nine rules in `../PLAN.md` (file sizes 300/450, small functions, no panics, no `unsafe`,
layering by crate, typed domain, ownership over locks, `foo.rs` + `foo/`, evidence-based phases).
Additions for this port:

10. **Limits come from evidence.** A limit in code cites its source tag (`model`, `doc`, `recorded`).
    A limit from `memory` is a test marked ignored plus an entry in the recording queue.
11. **Generated code is committed, reviewable and checked.** `cargo xtask codegen --check` fails CI if
    the output differs from the model plus overlays. The generator is a plain binary, not `build.rs`.
12. **Build lazily.** No `LazyLock`-less global tables, no regex or validation maps built at load; a
    table is built on first use. A weight gate catches a miss.
13. **No wire type derives serde.** Types are plain structs and enums; the schema tables and the
    per-protocol decoder do the work. (Size: spike A.)
14. **A refused operation is a deliberate act:** it appears in the exhaustive `match` with its 501.
15. **Every parser has a fuzz target and a nesting or length cap** (CI checks the pairing).

## Weight budget (targets, not measurements; spike C's skeleton had no real service code)

| | Go doze-aws today | Target (Rust, all 17 services) | Floci (advertised) |
|---|---|---|---|
| Idle RSS, fresh | 27 to 31 MiB | at most 4 MiB | about 13 MiB |
| Idle RSS after traffic and reaping | 40 to 46 MiB | at most 12 MiB | n/a |
| Idle wake-ups, 30 s | 132 to 583 | 0 | n/a |
| Start to first answer | 10 to 13 ms | at most 10 ms | about 24 ms |
| Stripped binary | 22 to 24 MB | at most 10 MB (stretch 6) | n/a |

The first honest comparison is taken at the end of A5 (SQS: real service code, real tables).

## Workspace and crates

Shared crates are extracted from the kafka port at A1 (not before: the second user shows what the
abstraction needs). AWS crates are new.

```
crates/
  doze-fs/         Fs trait, RealFs, SimFs (crash points, torn writes, ENOSPC). From kafka-storage.
  doze-store/      redb wrapper: atomic create, lazy open, group-commit flusher, versioned postcard.
  doze-runtime/    service host (lazy actor, idle reap), Expiring timers, Clock, shutdown.
  doze-log/        (only if kafka's partition log generalises) record-agnostic append log.
  aws-core/        Arn, AccountId, Region, RequestId, Principal, service ids, error taxonomy.
  aws-schema/      runtime for generated tables: ValidationError, Pattern (regex-lite), Blob, Timestamp.
  aws-model-<svc>/ GENERATED, one per service: types, newtypes, Operation enum, schema tables.
                   Overlays live in xtask/overlays/<svc>.json.
  aws-wire/        sans-IO codecs: json, query, xml, cbor, rest; table-driven decoders and encoders.
  aws-auth/        SigV4 verify (headers, presigned, chunked, trailers), credentials trait, Principal.
  aws-iam-policy/  typed policy tree and the pure evaluator.
  aws-gateway/     hyper listener, router, dispatch by protocol, limits, Expect handling, middleware.
  aws-peers/       typed in-process bus between services; carries the Principal.
  aws-expr/        winnow parsers: DynamoDB expressions, JSONPath, intrinsics, event patterns, Fn::Sub.
  aws-cron/        cron() and rate().
  aws-asl/         Amazon States Language interpreter on jlens-core.
  aws-<svc>/       one per service: sqs sns s3 dynamodb lambda iam (with sts) kms ssm secrets
                   logs kinesis cloudwatch eventbridge sfn apigateway cfn.
  aws-console/     kit (shell, components, htmx) plus one module per service behind a feature.
  aws-testkit/     fake clock, in-memory streams, SDK test clients, proptest strategies.
  doze-aws/        the binary: clap, config, tracing, wiring, serve mode.
xtask/             codegen, lint-size, weight, conformance, route-gate, fuzz-check.
```

A service crate looks like this (SQS shown; the same shape everywhere):

```
aws-sqs/src/
  lib.rs            public surface: Service, Config
  actor.rs          the task: inbox loop, effects executor, idle reap
  state.rs          State: queues, messages (owned data)
  machine.rs        the pure step function
  machine/          one file per concern: send.rs receive.rs visibility.rs fifo.rs redrive.rs ...
  handlers/         one file per operation: parse -> step -> encode
  store.rs          redb tables and versioned records
  console.rs        view-models for the console (pages live in aws-console)
  tests/            behaviour tests per catalogue entry
```

## Testing strategy (lean; Go's 93k lines are not translated)

What is needed, and why each earns its place:

1. **boto3 scenarios, one per behaviour that matters.** The language-neutral oracle. They found about
   35 real bugs that Go's own tests did not. The Go repo's scenarios (`conformance/`) are kept and run
   against the Rust binary. When the AWS account exists, they record against real AWS.
2. **Pure state machines under paused time**, with `proptest` over event sequences and stated
   invariants (SQS: a message is delivered at most once per visibility window; Step Functions waits;
   CloudFormation rollback; DynamoDB condition results).
3. **A fuzz target for every parser** (five wire formats, expression languages, SigV4 headers, chunk
   framing) with its input caps.
4. **The weight gate:** binary size, idle RSS, idle wake-ups, start time, against `weight.md`.
5. **A thin browser suite:** one main path per service console, plus the exported route list.
6. **Behaviour catalogue** (`behaviours/<svc>.md`): distilled from the Go plan's `plan/services/*.md`
   (951 findings) and the model documentation, each item tagged with its evidence and linked to the
   Rust test that covers it. Traceability instead of translation; it is what the audit checks.

Not planned: per-field validation tests (the types make them redundant), tests that restate the
model, leak tests (task trackers and typestate), per-route console specs. `cargo-mutants` runs
occasionally on the core crates, not as a gate. The Rust SDK is a test-only client alongside boto3.

## Dependencies (pinned from the spikes; each justified against the weight baseline)

Runtime and web: `tokio` 1.54 (`rt`, `net`, `time`, `sync`, `signal`; `current_thread` by default),
`tokio-util` 0.7.20, `hyper` 1.12, `hyper-util` 0.1.21, `http-body-util` 0.1.5, `bytes` 1.12. No axum
and no tower for the API (spike B: +78 KB, and our dispatch keys on header, `Action`, `X-Amz-Target`,
Host and signing scope, not path alone). The console is its own `axum` router (owner's choice: more
readable), mounted behind the gateway as a tower service; the kafka console keeps its own.

Formats: `serde_json` 1.0.154, `quick-xml` 0.42, `minicbor` 2.3, `form_urlencoded` 1.2, `postcard`
1.1.3 (storage only).

Storage and time: `redb` 4.5, `arc-swap` 1.9, our own UTC time formatting (about 150 lines; `jiff`
costs +150 KB), `crc-fast` or the 3-crate CRC set, `md-5`, `sha1`, `sha2` 0.11, `base64` 0.23.

Crypto: `aes-gcm` 0.11, `hmac` 0.13, `p256`/`p384`/`p521`/`k256` 0.14, `ed25519-dalek` 3, `aes-kw`
0.3, `rsa =0.10.0-rc.22` (release candidate, pinned exactly; RUSTSEC-2023-0071 noted, acceptable for
local use; normalise high-S for secp256k1; per-package `opt-level = 3` for the hash and CRC crates).

Language tools: `winnow` 1.0.5, `jlens-core =0.31.0` (young and single-author; pinned, forkable, MIT),
`regex-lite` 0.1.9 (Java-compatible ASCII `\w`; hand predicates where a pattern cannot compile).

Application: `clap` 4.6 (builder if size matters), `serde` plus `toml` for config only, `thiserror` 2,
`anyhow` (binary), `tracing` 0.1 (no env-filter), `secrecy`, `maud` 0.27, `rust-embed` 8.13.

Test only: `proptest` 1.12, `insta` 1.49, `cargo-fuzz`, `aws-sdk-*` (Rust SDK), `aws-sigv4` (oracle),
`tokio` test-util.

Build profile to start from: `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`,
`opt-level` s or z for the generated crates (decide on measured speed in A2).

## Phases

Each phase begins with its behaviour catalogue and ends with its gates green plus an independent
audit (a separate agent, not the author). Sessions are working sessions, honest estimates, not
calendar time. "Console" is that service's pages built to the dream-design boards.

| # | Phase | Gate | API | Console |
|---|---|---|---|---|
| A0 | **Rails.** `docs/port/aws/` (this plan, `weight.md`, `oracle.md`), xtask additions (`codegen`, `weight`, `conformance`), CI for the new crates on Linux x86_64 and aarch64, macOS arm64 and Windows x86_64, Go-side `CONFORMANCE_BIN` use for doze-aws scenarios | conformance harness runs the boto3 scenarios against the **Go** doze-aws binary through the process target; CI green on all four targets | 2 to 3 | 0 |
| A1 | **Shared kit.** Extract `doze-fs`, `doze-store`, `doze-runtime` from the kafka crates; decide whether `doze-log` generalises (small spike first). Kafka stays green and its weight unchanged | kafka gates pass unchanged; the lifecycle pattern has the spike's tests | 4 to 6 | 0 |
| A2 | **Model and codegen.** `xtask codegen` (IR, types, newtypes, `Operation`, schema tables), overlays, the table-driven decoder proved on **SSM and S3 first** (the two largest) | a use-everything build of all 18 models, decode tables included, has a stated size ceiling (target 3 MiB at fat LTO); `--check` clean; clippy clean under the workspace lints | 5 to 8 | 0 |
| A3 | **Wire and auth.** `aws-wire` (five codecs, fuzzed), `aws-auth` (SigV4 headers, presigned, chunked and trailer payloads, clock skew, typed `Principal`), `aws-gateway` (router, dispatch, limits, `Expect: 100-continue`) | spike B's client matrix (Rust SDK, boto3, CLI) passes against stub services; fuzz targets clean; 1 GiB upload peak RSS flat | 8 to 12 | 0 |
| A4 | **Core domain.** `aws-core` types, `aws-iam-policy` evaluator (identity, boundary and resource layers, plus empty session-policy and SCP slots; policy variables, `BinaryEquals`, three-state context keys, decision trace, AWS-shaped AccessDenied text; all condition operators), `aws-peers`, config, the service host. **Also: the generic accept-and-store path** (a stored-only record: kind, id, raw document, `active: false`) and a response-header sibling of `X-Doze-Not-Built`, which A3 carries on the wire; **`Expiring` is the only timer mechanism** | evaluator passes its case table (start from spike D's 53, add the Go gaps); host passes lifecycle tests; weight gate adds "30 s wake-ups with one alarm, one mapping, one lifecycle rule and one TTL table configured" next to the idle case | 7 to 10 | 0 |
| A5 | **SQS** (the calibration). JSON only. Visibility, delay, FIFO, redrive, move tasks, DLQ, long polling; publishes an `aws-peers` notification on append so pollers await it, with lazy built-in metrics (the three Approximate* gauges plus Sent, Received, Deleted, materialised only when an alarm, dashboard or metric read names the namespace; decision D4). Console **kit** (shell, components, htmx, route export) built here with SQS's pages, **after the console redesign is approved** (see Open decisions) | boto3 scenarios; machine properties; weight table vs Go (first honest comparison); **re-estimate everything from the actual sessions** | 5 to 7 | 5 to 7 |
| A6 | **SSM, Secrets Manager, KMS.** Simple JSON services; KMS brings the crypto and key import. Parameter Store: limits, paging and `AllowedPattern`, the SSM fleet half refused by name before parsing (no validation tables for it). KMS: lazy key generation, stored grants, `mrk-` ids. Secrets Manager: `/aws/reference/secretsmanager/*` read-through | scenarios; known-answer crypto tests; ring/aws-lc cross-checks kept | 8 to 11 | 4 to 6 |
| A7 | **IAM and STS.** Query protocol, users/roles/policies, STS tokens, enforcement across all existing services. Real role sessions: stateless signed session tokens, a `GetCallerIdentity` exemption, permissions boundary at create and returned, access-key rules in one case-folded store, OIDC/SAML/password-policy records, `aws:PrincipalTag` from IAM tags. Not built: credential report, access advisor, `AssumeRoot`, strict-time setting, async simulations | second-identity pass: every service under a non-default account and region | 6 to 9 | 3 to 4 |
| A8 | **SNS.** Fan-out to SQS through the bus, filter policies including `FilterPolicyScope: MessageBody`, raw delivery. SMS publish and mobile-push platform applications and endpoints are accepted and stored, nothing is sent (decision D1; an optional recorded outbox) | scenarios incl. SNS to SQS under non-default identity | 3 to 5 | 2 to 3 |
| A9 | **S3.** Buckets, objects, versioning, multipart, streaming checksums, presigned URLs, notifications (EventBridge notification config, per-key DeleteObjects events, delete-marker events), virtual-host addressing. A lifecycle janitor armed only on configuration, with tag, And and size filters correct before enforcement; ACL and archive-class reads refused by name | scenarios; flat memory on large objects; blob crash tests via `SimFs` | 8 to 12 | 4 to 6 |
| A10 | **DynamoDB.** Expressions (`aws-expr`), indexes, transactions, TTL, streams (`UpdateTable` stream spec; streams publish a notification on append), PITR true round trip, legacy-parameter refusal, DynamoDB-to-Kinesis destination, opt-in fault injectors | scenarios; expression fuzz; error-text catalogue | 9 to 13 | 4 to 6 |
| A11 | **Logs, Kinesis, CloudWatch.** Append-log storage; JSON and CBOR. CloudWatch scope is SCOPE.md section 7 tiers A, B, C: metrics and single-metric alarms built (evaluation is deadline-driven, no tick), `SetAlarmState` firing SNS and Lambda with AWS's payload, lazy `AWS/Lambda` producer, math and composite alarms stored and flagged, no dashboard rendering and no metric-math evaluator for now. Logs: bounded store with the full read API, lazy retention, metric and subscription filters, no Insights. One generic stored-document family. Kinesis: shard records keep `ParentShardId`, Split/Merge refused by name | scenarios for both CloudWatch encodings | 9 to 13 | 5 to 7 |
| A12 | **EventBridge.** Patterns, scheduler (`aws-cron`), targets through the bus (local targets: Step Functions, Kinesis, EventBus, API Gateway; `SqsParameters`, complete input transformer, DLQ and retry round trip), archives and replay; one default-bus publish hook for S3, Step Functions, CloudWatch, SSM and Secrets events. **EventBridge Scheduler is built as a fifth service** (decision D2: one-time `at()`, time zones, flexible windows, `AWS::Scheduler::*`), reusing the cron parser and target dispatch | scenarios; schedule tests under paused time | 5 to 7 | 3 to 4 |
| A13 | **Lambda.** Runtime API, process supervision, layers, versions and aliases, event source mappings (SQS, Kinesis, DynamoDB streams) with real semantics (batch-item failure, filters, windows; notify-driven pollers), run-as-role, an accept bundle (container-image functions, foreign layer ARNs, provisioned concurrency, code signing, config blocks; decision D3: stored and flagged, an invoke returns a clear error), `_local_` reload on change. **Response streaming is in 1.0** (decision D8) | scenarios; no leaked child processes (tracked tasks) | 11 to 17 | 3 to 5 |
| A14 | **Step Functions.** `aws-asl` on `jlens-core`, service integrations (unserved ones fail the state catchably unless mocked; decision D3), Map/Parallel, execution history bounded by count with an Express ring; Wait scaling, mock config, `TestState` completeness, HTTP Task, wider `aws-sdk:`; the driver sleeps to the next deadline | scenarios; interpreter bounded (depth, steps, time) | 8 to 12 | 4 to 6 |
| A15 | **API Gateway** v1 and v2. Routes, integrations, authorisers (including lite JWT), stages (settings round trip), REST Update* operations, an OpenAPI import subset, gateway responses. **WebSocket APIs are in 1.0** (decision D8: connections, route selection, `@connections` management API) | scenarios; `execute-api:Invoke` evaluated by IAM | 6 to 9 | 3 to 5 |
| A16 | **CloudFormation.** Templates, intrinsics, creation order, rollback, deploy-tool smokes (SAM, CDK, Terraform, Serverless). Resource-type policy with a config switch: MAP, SKIP+WARN for opaque infrastructure, FAIL for behaviour-bearing types (decision D5); the report-on-resource rule keeps it honest. Custom resources native-first with a generic Lambda-backed runner (D6); dynamic references, IAM mapping, LanguageExtensions, `DefinitionUri`, Kinesis stream creation, SAM `S3`/`DynamoDB`/`Kinesis`/`CloudWatchLogs` events; Application Auto Scaling resources SKIP+WARN (D7). The Go doc/code mismatches are not ported | stacks across all services; rollback and retention tests | 8 to 12 | 4 to 6 |
| A17 | **Release.** Integration pass, weight and API gates, `serve` mode so the doze module can start it, docs for the Rust edition, `not-built.md` regenerated from the four classes (BUILD, ACCEPT-AND-FLAG, STAY CUT, NOT NEEDED; OpenAPI import reclassified as deployment; the SNS filter-scope statement corrected), cutover decision | doze runs Larder against the Rust edition; weight table meets the budget | 5 to 8 | 0 |

**Total: about 113 to 168 API sessions plus 47 to 69 console sessions before the scope review (160 to 235 in all). The scope review (2026-11-09) adds roughly 20 to 38 API sessions (the accept-and-store path and deadline timers in A4, Scheduler, Lambda response streaming, API Gateway WebSocket, richer mappings and CloudFormation policy) and 4 to 8 console sessions, so about 185 to 285 in all.** The additions are guesses, sized by section, and A5 re-bases them.
That is higher than the 125 to 175 I quoted earlier; the earlier figure lumped the console and the
shared kit. It has low confidence and A5 re-estimates it. For scale, the kafka port is planned at 50
to 75 sessions for one protocol; my past doze-aws estimates have run low by a step.

Order rationale: the foundation (A0 to A4) comes before any service because every service rests on
it. SQS first because it is the deepest behavioural service, JSON only, and its answer tells us
whether the estimate holds. Services then follow their dependencies: SNS needs SQS; S3 notifications
need SQS, SNS and later Lambda; EventBridge targets need SQS, SNS and Lambda; Lambda triggers need
SQS, Kinesis and DynamoDB streams; Step Functions calls everything; CloudFormation creates resources
in every service, so it is last and doubles as the integration test.

## Risks

- **Decoder size (A2).** The table-driven decoder is unproven at scale; if all 18 models exceed the
  ceiling, fall back to cargo features per service and `opt-level = "z"` on the model crates.
- **Overlays are the hidden work.** Models under-constrain; the overlay for each service is real
  research, tagged by evidence. The 52 uncompilable patterns need hand predicates.
- **The partition log may not generalise (A1).** Kafka's log stores Kafka record batches. If a
  record-agnostic layer costs the kafka port anything, Logs and Kinesis get their own small log.
- **jlens-core is young** (single author, pre-1.0). Pinned, forkable; behaviour confirmed only against
  jsonata-js, not against real Step Functions.
- **`rsa` is a release candidate** and carries a known timing advisory (acceptable for local use).
- **Allocator.** The macOS system allocator never returns freed memory; glibc needs `malloc_trim`
  after a reap (one `unsafe` call, against the workspace's `forbid(unsafe_code)`); mimalloc returns
  memory on macOS but is a poor fit on Linux. See Open decisions.
- **Real-service weight.** The skeleton had no route tables, IAM action tables or validation tables;
  these must be built lazily or the budget is missed.
- **Fidelity cannot be proven without the AWS account.** Auth error codes, DynamoDB error text,
  JSONata edge cases and IAM expectations are unverified.
- **Windows and x86 (now in scope).** Nothing in the spikes ran on Windows. Known exposure: file
  sync and rename semantics behind `Fs`, child-process supervision for Lambda, the unix-socket `serve`
  mode doze uses, and `rsa`/crypto speed on x86. Rough extra cost 4 to 8 sessions, a guess to be
  re-based in A1 when `Fs` is extracted.
- **redb close cost.** Closing a database costs two to three fsyncs (about 12 ms on this Mac) on each
  reap; it is off the request path and accepted.
- **Scope.** Longest project yet. A5 is the checkpoint; A0 to A4 must not grow.

## Decisions (2026-11-08) and what is still open

Decided by the owner: (1) IP and port only, no doze-names. (2) The protocol cuts. (3) Two separate
routers: the API gateway on raw hyper, the AWS console on `axum`. (5) Windows and x86 are in scope.
(6) Go is frozen; the port is the way forward. (7) An AWS account exists (root only); the owner will
set up recorder keys later this week, with help (least-privilege IAM user or Identity Center, MFA on
root, no root access keys, a budget alert, a `doze-rec-` naming prefix and a teardown script).
(8) The spike code stays in the scratchpad (temporary; the findings are in this plan and in memory).

(9) Allocator: left to my judgement. Allow a tiny `doze-alloc` crate holding the one `unsafe` call
(`malloc_trim` on glibc); default to the system allocator; decide macOS (mimalloc) and Windows on real
service numbers at A5.

Console requirement added 2026-11-08: when run by doze, the console shows **both** the `.doze`
endpoint and the plain IP-and-port endpoint for every service, copyable, with SDK and CLI snippets for
each. Standalone (and on Windows, where `.doze` setup is Unix-only) it shows the IP form. Doze tells
the service its names at start; doze-aws itself no longer owns names. Virtual-hosted S3 and
region-qualified service hostnames work only on a `.doze` name, so the console says so.

(10) Scope review answers (2026-11-09), full text in `SCOPE.md` section 8: D1 accept SMS publish and mobile push as stored; D2 build EventBridge Scheduler; D3 accept and flag container-image Lambdas, unserved Step Functions integrations and foreign layer ARNs; D4 SQS built-in metrics, lazily; D5 CloudFormation default SKIP+WARN for opaque types and FAIL for behaviour-bearing ones; D6 custom resources native-first with a Lambda-backed runner; D7 Application Auto Scaling as SKIP+WARN in CloudFormation only; **D8 Lambda response streaming and API Gateway WebSocket are part of 1.0 (the owner's change)**; D9 record the AWS-managed policy documents once recorder keys exist.

Console direction (2026-11-08): **Paper** chosen. 380 screens across 17 services are drawn, audited and published on the direction canvas; they are the spec for the A5 console kit. The kit rule set (lineage, real operations only, scope states) lives in the scratchpad until a repo location is chosen.

Still open:

- **Owner decisions from the screen audits** (engine gaps versus screens), collected in one list for the owner.
- **AWS recorder keys** (D9 and the `recorded` evidence tag wait on them).

## Verification

- Per phase: the scenarios, the phase's own machine properties and fuzz targets, the weight gate, and an
  independent audit against the behaviour catalogue and the rules above.
- A5: first weight comparison against Go and Floci's advertised numbers; the estimate is re-based.
- A7 and A16: the second-identity pass (every service under a non-default account and region).
- A17: Larder on the Rust edition via doze.

## Critical files

Rust (to create): `crates/doze-*`, `crates/aws-*`, `xtask/`, `docs/port/aws/`.
Go reference (domain knowledge only): `doze-aws/plan/` (service docs, protocol, lessons, shell
fidelity), `doze-aws/.audit-models/*.json`, `internal/{awsjson,awsquery,rpcv2cbor,sigparse,awschunk,
awshttp,restroute,iampolicy,iamguard,peercall,awscron,eventpattern,asl}`, `<svc>/` for each service,
`conformance/` (scenarios, `deviations.py`), `testdata/lightness.json`, `docs/performance.md`.
Spike reports: `scratchpad/spike-{a-codegen,b-wire,c-weight,d-libs}` (see Open decisions).

## Revision log

- **2026-11-08, draft.** Written after the four spikes; supersedes the earlier lump estimate
  (125 to 175 sessions) with the itemised 160 to 235. Spike A changed the design (no serde on wire
  types); spike B settled hand-written SigV4 and raw hyper; spike C settled the runtime and lifecycle
  pattern; spike D settled the language libraries and reversed the JSONata pick (`seuil` to
  `jlens-core`).
- **2026-11-08, decisions.** Owner answered the open decisions: IP and port only, protocol cuts accepted, console on its own axum router, Windows and x86 in scope (+4 to 8 sessions, a guess), Go frozen, AWS account available (keys later this week), spikes stay in the scratchpad. The allocator and the console redesign remain open.
- **2026-11-09, scope review.** `SCOPE.md` consolidated six relevance reviews. The owner answered D1 to D9 with the recommended lean, except D8 (Lambda response streaming and API Gateway WebSocket move into 1.0). E1 to E17 applied to the phase table; sizes revised upward (about 185 to 285 sessions, low confidence). Console direction: Paper.
