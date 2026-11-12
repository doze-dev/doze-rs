# doze-aws in Rust: a ground-up implementation

Status: **approved in part, 2026-11-08** (written after four feasibility spikes; the owner answered
the open decisions the same day, see "Decisions"). Nothing here is built yet. Items marked
**PROPOSED** are my recommendation and still need the owner's yes; items marked **SETTLED** come from
the owner's own words. Scope review applied 2026-11-09 (see `SCOPE.md`; decisions D1 to D9 answered, edits E1 to E17 applied below). Still open: the console's build (the direction is chosen: Paper). How the port is run (five loops, scoreboard, stages with go/no-go) is in `phase0.md` and `scoreboard.md`, approved 2026-11-09.

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

### Rust principles we live by

Source: the learning blog the owner and I wrote the doze principles from (https://learningrust.nerdmenot.in/, read 2026-11-09: "The ten ideas behind most rules" and
"Habits that look like idioms"). The owner's standing instructions, 2026-11-08 and 2026-11-09: no copying unless it is needed, no easy options, Rust done the way it is
intended, and no cop-outs just because we are stuck. Targets are realistic; the standards are not loosened to meet them.

16. **A copy needs a stated reason.** Every `.clone()`, `to_vec()`, `to_owned()` or buffer copy on data of any size carries a one-line reason (a real mutation of shared data, a value
    that must outlive its source, a small `Copy` value). Borrow, slice `Bytes`, build small things on the stack, write from the buffers already held. Parameters borrow when they only read.
    A rising clone or `Rc` count is a design signal, not a fix: stop and change the ownership. Enforced by: the aws crates opt into `clippy::redundant_clone`, `implicit_clone`,
    `unnecessary_to_owned`, `needless_pass_by_value` and `clone_on_ref_ptr` as denies; every audit brief includes "hunt avoidable copies and allocations on hot paths".
17. **Decide with `&self`, then apply with `&mut self`.** Service logic is the sans-IO step function; one writer owns the state (the actor). No `Rc<RefCell<_>>`, no `Arc<Mutex<_>>`
    around service state, no guard held across an `.await` (already a deny). A lock is allowed only on small leaf data with a comment saying why an owner task would be worse.
18. **Wrong states are hard to write.** Parse, don't validate: loose input becomes a checked newtype once, at the edge. Enums with per-variant data, not a struct with a field per state;
    typestate where order is fixed (connect before send). No text where an enum belongs (error codes, operation names, states).
19. **Exhaustive means exhaustive.** No `_` arm in a `match` over our own operation, error or state enums (`clippy::wildcard_enum_match_arm` in the aws crates), so adding a variant
    stops the build. Public error enums are `#[non_exhaustive]`. Library errors are `thiserror` enums that map to an AWS code once, at the boundary; `anyhow` and `Box<dyn Error>`
    only in binaries. No `unwrap`/`expect` outside tests, no `let _ =` on a `Result`, no `catch_unwind` as a try/catch, no `process::exit`.
20. **`unsafe` is the last resort.** Try safe code, then std, then a well-used crate, and only then our own block: small, behind a safe function, with a `// SAFETY:` comment.
    Every crate is `forbid(unsafe_code)` except `doze-alloc` (the Linux `malloc_trim`, spike S5). Never `unsafe impl Send/Sync` to get past the checker.
21. **Nothing happens behind our back.** Conversions are written out (`From`, `TryFrom`, `FromStr` by failure mode). Blocking work never runs on the async threads (`spawn_blocking`
    or a dedicated thread). Async functions that must run are awaited or spawned and tracked (`TaskTracker`), never fire-and-forget.
22. **Tests: own data, own folder, own clock.** Inject the clock and `Fs` (already the pattern). Property tests with a fixed seed so a failure repeats. Recordings are never re-recorded
    without reading the diff. Coverage shows what ran, not what was checked; it is not a target.
23. **Measure release builds, and count.** Weight and speed claims come from release builds, one run is a direction not a result, and hot paths get an allocation-count test (a counting
    global allocator in the test binary) alongside any timing. A target moves only with a measurement and a written reason; it is not relaxed because a build was hard.
24. **Features only add.** Cargo unions features, so a feature never removes behaviour. Workspace inheritance for version, edition and dependencies; the declared `rust-version` is tested.
25. **When stuck, the design is the problem.** The escape hatches (`clone`, `Rc<RefCell>`, `unsafe`, `unwrap`, a `_` arm, `Box<dyn Error>`, a global lock, a copied buffer) need a
    `// WHY:` comment naming the constraint and the better design that was tried, and the independent audit reviews every one. The compiler's `help:` lines are hints, not a
    diagnosis. If two parts both want to change one value, or a third `clone` appears to quiet an error, write the blocker in the phase doc and redesign before continuing.

## Targets (provisional, set from measurements; re-based at A5)

Rule: a target must be defended by a measurement plus a margin. A number we cannot defend is a hope, not a target, and does not go here.
Rust's floor is what it is (async runtime, TLS-free HTTP, redb, crypto); the goal is the best result inside that floor, not below it.

| | Go doze-aws today | Measured so far (Rust) | Target for 1.0 (all 17 services) | Floci (advertised) |
|---|---|---|---|---|
| Idle RSS, fresh, all services enabled and unused | 27 to 31 MiB | 2.6 MiB skeleton; 3.6 MiB with all 18 models linked (S1) | at most 6 MiB (kafka rose about 60% from skeleton to real code) | about 13 MiB |
| Memory after a burst (10k small requests) and idle | 40 to 46 MiB | Linux: 823 MiB held by glibc without action; with a body cap plus one idle `malloc_trim`, about 3 MiB over baseline (S5). macOS RSS stays high but footprint is 18 MiB with the cap | Linux: back within 16 MiB of baseline within 60 s of idle. macOS: footprint (vmmap), not RSS, within 24 MiB | n/a |
| Idle wake-ups, 30 s, nothing configured | 132 to 583 | 0 | 0; with an alarm, mapping, lifecycle rule and TTL table configured, wake-ups only at their deadlines | n/a |
| Start to first answer | 10 to 13 ms | 0.74 to 2.4 ms skeleton; kafka 55 ms first start (database creation) | at most 25 ms fresh, 10 ms restart | about 24 ms |
| Stripped binary, release, fat LTO (macOS arm64; **Linux x86_64 measured 1.45x larger** on the kafka build: 4.90 MB against 3.39 MB, CI 2026-11-09, so each platform gets its own gate) | 22 to 24 MB | codegen alone: 4.2 MiB (opt z) to 6.6 MiB (opt 3) for typed decode and encode of all 18 (S1); kafka 3.4 MB | at most 16 MiB with all 17 services and the console (12 MiB intended); decode design chosen by spike S1b | n/a |
| Durable send, SQS-shaped load | not measured | p50 about 6.4 ms, p99 9 to 13 ms at 1k and 10k msg/s, 5 to 14% of a core (S6, macOS; disk-bound) | within 1.5x of the S6 numbers; no regression gate on absolute latency (disk-dependent) | n/a |
| Clean build, CI | n/a | codegen binary 5 min 42 s at opt 3, 44 s at opt z | CI uses opt z for the model crates in test builds; release builds are not a per-push job | n/a |

Fidelity targets (the oracle is AWS recordings, see `phase0.md`):

- **A service is done** when every recorded label matches AWS exactly or is listed in `deviations.py` with a reason (timing, randomness, a 60-second rule
  that needs real time, something that cannot exist on a laptop). More than 3% of a service's labels in `deviations.py` means stop and re-plan, not accept.
- Error-message wording is matched like everything else (the recordings contain the text), but reported separately on the scoreboard so a wording miss never hides a behaviour miss.
- Rust must beat Go's measured score on every service it replaces; Go's first measurement is 46% exact over 537 labels (`baseline.md`).
- Behaviour that cannot be recorded stays tagged `docs` or `unverified`; it never counts as matched.

The first honest comparison is taken at the end of A5 (SQS: real service code, real tables); these figures are re-based then.

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
| A0 | **Instruments before product** (detail and status in `phase0.md`; spikes S1 to S6 run beside it). A dedicated recorder account (`doze-recorder`, Identity Center, SCP); record the existing boto3 conformance suite (`doze-aws/conformance`, `CONFORMANCE_RECORD=1`) and run it against Go for a measured baseline; xtask `weight`, `conformance`, `gates` (today xtask has only `lint-size` and `fuzz-check`); CI on Linux x86_64 and aarch64, macOS arm64, Windows x86_64 (today ubuntu and macos only); limit prober v0; `scoreboard.md` | Go baseline fidelity number exists; `cargo xtask weight|conformance|gates` run; CI green on all four targets | 6 to 9 | 0 |
| A1 | **Shared kit.** Extract `doze-fs`, `doze-store`, `doze-runtime` from the kafka crates; decide whether `doze-log` generalises (small spike first). Kafka stays green and its weight unchanged | kafka gates pass unchanged; the lifecycle pattern has the spike's tests | 4 to 6 (**done 2026-11-12, one session; see the log**) | 0 |
| A2 | **Model and codegen.** `xtask codegen` (IR, types, newtypes, `Operation`, schema tables), overlays, the table-driven decoder proved on **SSM and S3 first** (the two largest) | a use-everything build of all 18 models, decode tables included, has a stated size ceiling (within the whole-binary budget under Targets; decode design chosen by spike S1b); `--check` clean; clippy clean under the workspace lints | 5 to 8 (**first half done 2026-11-12**) | 0 |
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

- **Decoder size (A2). Measured 2026-11-09 (spike S1): FAIL.** Typed decode and encode for all 18 models is 6.61 MiB at opt 3 against a 3 MiB ceiling; tables alone are 2.25 MiB. Cargo features fit about 11 of 18 in 3 MiB. Spike S1b (2026-11-09) chose **design C**: generated packed tables (0.71 MiB for all 18 models) and one generic zero-copy decoder, with every unimplemented operation answering 501 by name from the exhaustive `Operation` enum. All 18 models: 1.11 MiB stripped at opt 3 (typed glue was 6.61); SQS `SendMessage` decodes with zero allocations once the arena is reused; whole-binary projection 8.7 MiB (6.5 to 10.5), inside the 16 MiB limit and the 12 MiB intention. The overlay must carry a hand-predicate registry (22 types on implemented operations need one; the Unicode-category family needs a table `std` lacks), cross-field rules as data, limits the models omit (tagged `recorded`), a violation-to-AWS-error map per service, and recorded decisions on duplicate JSON keys. Query/REST/XML decoding over packed tables is inferred, not built. Compiling every constraint regex eagerly costs 23 MiB, so patterns stay lazy.
- **Memory after load (spike S5).** The gateway caps request bodies in flight (A3); a burst of small requests otherwise leaves glibc holding hundreds of MiB. Linux gets an idle `malloc_trim` in `doze-alloc`; macOS and Windows keep the system allocator.
- **Store (spike S6).** redb needs a group-commit flusher, which kafka-meta does not have (write it in A1); without grouping a durable send path collapses to about 240 ops/s.
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
- **Windows is deferred (2026-11-09): no machine to test on.** It stays in scope as an assumption; the port is compile-checked for it where possible and nothing claims it works until it has run there. Original note follows.
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
- **2026-11-12, A1 (shared kit).** `doze-fs` (the `Fs` seam, `RealFs`, `SimFs`: moved whole out of `kafka-storage`, which re-exports it so no kafka path changed), `doze-store` (redb on `Fs`, atomic create, versioned records, and the **group-commit writer** that S6 said had to be written: one thread, one transaction and one flush per batch, a failed commit fails the batch and every later write, tested with power cuts) and `doze-runtime` (`Lazy` and `Service` from spike C with a typed open error and tracked, stoppable actors; `Expiring`, the only timer mechanism, with the two panics of `DelayQueue` removed and `reset` made safe; `Clock`; `Tasks`; `DataDirLock`). `kafka-meta` and `doze-kafka` use them. Kafka's tests pass; weight +944 B (see `weight.md`). **`doze-log` is not extracted:** S6 showed redb with group commit meets the SQS latency target, so no service needs a record-agnostic append log yet; revisit if S3 object storage or Kinesis shows otherwise. The estimate was 4 to 6 sessions; this took one.
- **2026-11-12, A2 (model and codegen), first half.** `models/` holds the 18 Smithy models verbatim (checksums in `models/PROVENANCE.md`; the upstream revision was not recorded when they were fetched). `codegen` (a build tool, its own workspace member, run as `cargo xtask codegen [--check]`) turns each into `crates/aws-model-<name>`: the packed tables (one `u32` word array and one string pool, no pointers), typed member keys and the exhaustive `Operation` enum. CI and `cargo xtask gates` run `--check`, so a stale generated file fails the build. Ported from the S1b spike, and the tables are **byte-identical to the spike's** (checked against five models before the IR was pruned). `aws-schema` is the runtime: the arena, the JSON decoder that validates in the same pass, the JSON encoder, typed views, lazy patterns. The tables also expose each operation's HTTP binding and its errors, for `aws-wire`. **Tested by `aws-modelcheck`** four ways: every index in every table is in range (the invariant the table reader's no-panic design rests on); the decoder's contract table on SQS and DynamoDB; for 588 operations of the ten JSON-protocol services a valid body is built from the schema alone and accepted, and 1,686 single faults (missing required member, empty where non-empty, bad enum, out-of-range number, wrong type) are each refused with the right violation; and all 4,152 structures and unions agree with the raw Smithy JSON on members, required flags and kinds, read by code that shares nothing with the generator. **Size:** a binary that links all 18 models, the tables and the decoder is **1,186,144 B (1.13 MiB)** in a release build with the repo's thin LTO (S1b predicted 1.11 MiB at fat LTO): that is the model layer's ceiling. **Not yet in A2:** the Query, REST-XML and CBOR decoders (A3, with `aws-wire`; S3 is REST-XML, so the plan's "SSM and S3 first" is half done: SSM is proven, S3's decode waits for the XML codec), and per-service overlays beyond SQS's (each lands with its service). **Found:** the JSON decoder checks `required` for every member, including REST members bound to the path, a header or the query; `aws-wire` must either satisfy those before decoding the body or the decoder must skip non-body members (a decision for A3). 29 patterns across the models cannot be compiled by `regex-lite`; one has a hand predicate (not-blank) and the rest accept; the list, per service, is printed by `cargo xtask codegen` and each needs a predicate or a recorded reason before its service is called done.
- **2026-11-12, A3 and a tracer bullet (the first Rust request, end to end).** Built: `aws-core` (hex, URI encoding, calendar arithmetic), `aws-auth` (sans-IO SigV4: the claim, the canonical request, a two-step verify, `begin` then `Pending::finish`, that never touches the body and yields an unforgeable `Verified`; the aws-chunked decoder; the S3 documentation's known answers and a re-sign with the `aws-sigv4` crate as oracle), `aws-wire` (awsJson routing by `X-Amz-Target`, answers, and **errors worded from the model**: a service names an error shape and the model supplies the wire code, status, fault and the query-compat header; per-family authentication errors; the service contract), `aws-gateway` (hyper front: routes by credential scope, `Trust` or `Verify` auth, reads the body to a limit, decodes against the model, hands the service an owned `Call`, encodes the answer), a typed response builder in `aws-schema`, per-model `ServiceMeta`, and `aws-sqs` (standard queues: create, delete, list, look up, attributes, send, receive, delete, change visibility, purge; a sans-IO core behind a lazy actor; every operation named in an exhaustive match, the unbuilt ones answering 501) with `doze-aws`, the binary. Three fuzz targets (the JSON decoder, the claim parser, the chunk decoder) ran 7 million inputs without a finding.

  **The first Rust fidelity number (SQS, 2026-11-12, standard-queue core only): 61 of 116 compared labels match AWS exactly, 3 differ only in wording, 51 differ, 1 unverified.** Read it with its caveat: the harness counts only the labels a scenario reaches, and a scenario stops at its first unbuilt operation, so against all 182 recorded labels this is 61 exact (34%); the Go edition is at 80 (44%). The differences are the unbuilt surface (batches, FIFO, message attributes, tags, permissions, dead letters, move tasks, long polling), not wrong answers: every refusal that is built (no such queue, bad name, different attributes, bad receipt handle, attribute ranges, receive ranges) matches the recordings. **Weight, macOS arm64 release: the whole binary with SQS is 1,713,536 B (1.63 MiB) and idles at 3.0 MB resident** (the Go edition idles at 31.4 MB); the S6 prediction was under 6 MiB idle. Limits are in `xtask/weight-limits.txt` and CI runs them.

  **Open, found while building:** (1) the JSON decoder checks `required` for REST members bound to the path, a header or the query: `aws-wire` needs to satisfy those before decoding a restJson body, or the decoder must skip non-body members. (2) Long polling needs waiters and `Expiring`; today a receive answers at once, which the harness tolerates (it polls) but a client relying on `WaitTimeSeconds` would not. (3) Persistence: until services keep state on disk the idle reap is a year, because closing a service would lose its queues. (4) The authentication error table is unverified against AWS (a recording scenario for each failure, in each family, is owed). (5) `Trust` mode is the default and accepts any access key, as a local tool must; `Verify` mode is tested over a real socket.
- **2026-11-12, SQS built (A5, the calibration), against the recordings.** Every one of the model's 23 operations is built and the dispatch is an exhaustive match. Added on top of the tracer bullet: message attributes with AWS's own MD5 (checked against the recorded values), system attributes, the three batch operations with AWS's batch rules, tags and their limits, FIFO queues (name rules per kind, group and deduplication rules, the five-minute window answering with the original's id and sequence number, group-ordered receive, `ReceiveRequestAttemptId`), queue policies, `AddPermission` and `RemovePermission`, redrive and redrive-allow policies checked as AWS checks them (including its Java-style object print and its curly apostrophe), dead-letter moves, `ListDeadLetterSourceQueues`, move tasks, and the 60-second windows for a deleted name and a purge. The Go edition's rule holds: the deviations list is empty and `doze-aws` is no more forgiving than AWS.

  **Score, SQS, 2026-11-12: 177 of 181 labels match AWS exactly (97%); 2 differ only in wording; 2 in behaviour. The Go edition on the same recordings: 80 of 182 (44%).** One recorded label is not reached (the Query-protocol scenario). The four that differ: (1) and (2), `not-in-flight` and `change-visibility-with-the-old-handle`: AWS quotes the opaque receipt handle in its message, which no implementation can reproduce (counted as wording); (3) `test_redrive_receive_count`: AWS returned the same dead-lettered message ten times in one receive (counts 2 to 11) where we return it once, a quirk the recordings do not explain (the call was `VisibilityTimeout=0`, `MaxNumberOfMessages=10`, `WaitTimeSeconds=1`, `MessageSystemAttributeNames=All`; a similar call elsewhere returned one); (4) `test_query_batch_with_a_gap`: needs the Query protocol, which SQS also speaks and the gateway does not yet.

  **Weight, macOS arm64 release:** the whole binary is 1,914,240 B (1.83 MiB), 109 lines in the package count, idle 3,072 KB. The full SQS and its policy parser cost 200 KB over the tracer bullet (`serde_json` is most of it, and the IAM evaluator will need it anyway).

  **Modelled from the recordings rather than documented** (each marked in the code): a moved message's receive count is reset; the first message of a move task moves at once; AWS reported a quickly finished task's moved count as 0, so a finished task's count is held back five seconds unless it was cancelled; the FIFO sequence number is an increasing 18-digit decimal. **Known deviation:** `AddPermission` accepts any 12-digit account id, where AWS checks the account exists (it refused a well-formed id nobody held). **Unrecorded wording**, on the queue: the statement-limit message, the `FifoThroughputLimit` messages on a standard queue, the dead-letter-target-missing and malformed-redrive-policy messages, the message-attribute name and value messages.

  **Still open for SQS:** persistence (state is in memory, so a restart empties every queue), the Query protocol on the gateway, and the recorded ten-copies quirk.
- **2026-11-12, SQS long polling.** A receive that finds nothing and may wait (`WaitTimeSeconds`, or the queue's default) is answered at once by the actor with how long the caller will wait; the service wrapper then waits for a send (or any call that could give it a message) or for a quarter second, asks again, and answers empty at the deadline. The actor is never blocked, so the send that ends the wait can be served. Three tests with a paused clock pin it: a waiting receive returns when a message arrives, waits the full time on an empty queue, and uses the queue default unless the request says otherwise. The score is unchanged (177 of 181). **Open for SQS now:** persistence, the Query protocol on the gateway, and the recorded ten-copies quirk.
- **2026-11-12, SQS on disk.** Queues and messages survive an idle reap and a restart. One redb file (`sqs.redb`) with four tables: queue headers, FIFO bookkeeping, **one row per message** (keyed by queue and an increasing `ord`), and the meta (recently deleted names, move tasks). The state machine still knows nothing of the disk. After each call `Core::flush` finds what changed and hands the new rows to the group-commit writer, and the reply is held back until that commit is flushed, so an acknowledged send survives a crash. Messages record their own changes (the list is a wrapper whose only mutators note the row), and the header, FIFO state and meta are compared with the bytes last written, so a change cannot be made and forgotten. **How it is tested:** a scenario of 27 calls over a standard queue, a FIFO queue, a dead-letter queue with a move task, purge, delete and the held name stops and starts the service after every call and demands that what it loads equals what it had (removing the touched-queue marking makes it fail, which was checked); a call writes one row per message it changes and none for a read; messages in flight stay in flight, and a receipt handle from before a restart still deletes. A real process was sent a queue, stopped with SIGTERM and started again: attributes, tags, the in-flight message and the delete all survived. The conformance score is unchanged (177 of 181). `--idle-secs` (300 by default) now closes the service and its file after five quiet minutes; it was a year while state was only in memory.

  **Cost, written down:** the binary grew from 1.83 to 2.56 MiB (2,687,088 B) and the package count from 109 to 118; `redb` is 507 KiB of it and every later service shares it. The Linux limits moved to 3,700,000 B and 120 packages (`docs/port/weight.md`). Idle is unchanged at 3.1 MB.

  **Known limits:** a FIFO send rewrites that queue's FIFO row (sequence plus the five-minute deduplication window), so a FIFO queue taking thousands of sends inside five minutes writes a few hundred KiB per send; if it matters, the window becomes a row per entry. A message's `ord` is one past the last message on load, so an ord freed by a delete may be given again (its row is gone, so nothing collides). The conformance runner needs `UV_BIN` set to a real uv (`~/.proto/tools/uv/<version>/uv`) here: the proto shim has no version pinned in `conformance/` and the run scores 0 labels without saying why. **Open for SQS now:** the Query protocol on the gateway, and the recorded ten-copies quirk.
- **2026-11-12, SQS closed at 177 of 181 (the four that differ are decided, not open).** (1) and (2), `not-in-flight` and `change-visibility-with-the-old-handle`: AWS quotes the opaque receipt handle in its message; wording only, unfixable. (3) `test_query_batch_with_a_gap`: AWS accepts the Query protocol on SQS and answers a gap in the entry numbers with 400 `MissingParameter`; doze-aws serves SQS over JSON only, by the settled decision in this file ("Protocols by service"), so a Query client gets 400 `UnsupportedProtocol` as XML (an old SDK can read XML; a JSON error would surface as its own parse failure) saying to upgrade. The Query codec is built with the first service that has no other protocol (IAM, STS, SNS, CloudFormation), not for SQS. (4) `test_redrive_receive_count`: AWS returned one dead-lettered message ten times in a single receive (counts 2 to 11) for `VisibilityTimeout=0`, `MaxNumberOfMessages=10`; another recording of a similar call returned it once, so this is AWS sampling, not a rule, and it is **not** built (building it would copy an accident). Nothing is open for SQS but the re-estimate the A5 row asks for: the checkpoint's score test is passed (97% against Go's 44%), but sessions were not counted as they were worked, so the stop-at-7 test cannot be shown either way and the re-estimate of later stages is made from the work done (above), not from a session tally. The console kit and SQS pages wait for the console redesign to be approved.
- **2026-11-12, A6 begun: SSM Parameter Store at 27 of 31 labels exact (87%; Go 52%).** `aws-ssm`: the 13 Parameter Store operations (put, get, get many, by path, history, delete, delete many, describe with filters, label, unlabel, add, remove and list tags), sans-IO `Core` behind the `Lazy` actor on disk, `SecureString` sealed with AES-256-GCM under a key the service makes once per data directory. **The other 133 operations of SSM (Run Command, Automation, Session Manager, associations, inventory, patching, maintenance windows, documents) are refused by name before the request is read:** a new `Service::serves(op)` answers 501 `NotImplemented` with `X-Doze-Not-Built: 1` and the operation's name, so no validation work is done for them (gateway test added). **New shared pieces, built for the three services of A6 and reused by every later one:** `doze_store::records::Records<T>` (a keyed collection in one redb table where the only way to a `&mut T` notes the key, and `stage()` returns just the rows that differ from the bytes last written, so a change cannot be made and forgotten) and `doze_store::Db` (the file, its tables and its group-commit writer as one value); `aws-testkit` (a request as JSON decoded against the model the way the gateway would, an answer read back as JSON, a temp dir); `aws_core::time::parse_rfc3339` (with a fuzz target). **Validation errors now carry the offending value,** and `default_invalid` words them as AWS did when the SSM scenario was recorded (`1 validation error detected: Value 'Sentence' at 'type' failed to satisfy constraint: Member must satisfy enum value set: [SecureString, StringList, String]`); that one case is the evidence, including the enum set being listed in the reverse of the model's order, and the length, range and pattern sentences follow the same convention from memory until a scenario reaches them. **What AWS taught that Go got wrong (recorded):** an empty message on `ParameterNotFound`; a type change on overwrite is accepted (String to StringList); a character the name rules forbid is answered with the "can't be prefixed with ssm" text; `/x/aws-inside` is accepted (the reserved word only counts as the first segment). **Wording not recorded** is marked `(memory)` in `write.rs`: the value-too-long messages, the data-type message, the tags-with-overwrite message, the pattern mismatch message, the max-versions message. **Open:** (1) the four `AccessDenied` labels need a policy that lets the recorder write `/aws/...`, `ssm-...` and `/awsome-...` and read a path with no leading slash, or the scenarios become `snapshot.outcome` with the denial dated; this is an owner item with the policy paste. (2) `SecureString` should call doze KMS once KMS exists (AWS encrypts with `alias/aws/ssm` or the key the caller names, and `Decrypt` of the ciphertext works there); until then the value is sealed under the service's own key and `KeyId` is reported. (3) A parameter's `Expiration` policy is checked on every call by a cheap sweep (nothing wakes on its own); the `Expiring` timer is the other way and is not needed until a service must act at a time with no request. **Cost:** +399 KB and +21 packages, limits re-based in `weight.md`. **Tests:** 22 in the crate, including the restart-after-every-call scenario (it passes) and a count of rows written per call (one parameter changed writes one row, a read writes none).
- **2026-11-12, A6: Secrets Manager at 27 of 27 labels exact (100%; Go 48%).** `aws-secrets`: 18 of the 23 operations (create, get, batch get, put value, update, describe, list with filters, sort and paging, list versions, delete with recovery window, restore, move a staging label, tag, untag, random password, resource policy put/get/delete/validate). Same shape as SSM: sans-IO `Core` behind the `Lazy` actor, `Records<Secret>` on disk, values sealed. **Refused by name:** `RotateSecret` and `CancelRotateSecret` (rotation drives a Lambda function: after A13), and the three replication operations (real replicas are not built). `aws-seal` is new: the sealing, `uuid_v4`, unbiased random numbers and random alphanumerics that SSM, Secrets Manager and KMS share, so `aws-core` stays dependency-free. `aws_wire::handler` now holds `request`, `reply` and `empty` for every new service (SQS still has its own copies; fold them in when SQS is next touched). **What the recording showed:** `LastAccessedDate` is the date and not the time (kept as the start of the UTC day), and a secret never read has none, also not on a deleted one; `Tags` is absent when there are none; `KmsKeyIds` of a version is `DefaultEncryptionKey` with no key set; a bad name is `ValidationException` (Go said `InvalidRequestException`); `PutSecretValue` and everything else on a secret marked for deletion say the recorded `InvalidRequestException` text. **Wording not recorded** is marked `(memory)`: the client-token conflict, a create over a name scheduled for deletion, a put with no value, the empty password character set; the refusals of `UpdateSecretVersionStage` and the `ListSecrets` filter semantics (prefix, case-insensitive, a leading `!` negates) are unrecorded behaviour. A version with no label is dropped after a day; a secret keeps at most 100. **Harness change:** the conformance account for doze is now `111122223333` (it was `000000000000`), because a scenario sends the all-zero UUID `00000000-0000-4000-8000-000000000000` and the normaliser rewrote its last twelve zeros to `<account>`; `cargo xtask conformance` must be given `--account 111122223333` in its `--cmd` (the scoreboard lines above were made that way). **Cost:** +234 KB and +4 packages. **Tests:** 17 in the crate, including the restart-after-every-call scenario and rows-per-call counts.
