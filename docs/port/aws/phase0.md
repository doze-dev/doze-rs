# doze-aws Rust port, Stage 0 and 1: instruments and risk spikes

Status: **in progress.** 0.1 done 2026-11-09; 0.2 and 0.3 started (SQS recorded and baselined, the rest follow).
This file is the working document for the two stages that come before any service code. `PLAN.md` says what to build;
this says how we find out whether we are building it correctly. Update it, do not rewrite it.

## Method in one paragraph

Fidelity is judged by AWS, not by opinion. Every phase states a prediction before it measures, and every claim comes out of one of
five loops: oracle (record on AWS, replay on doze), limit prober (boundary values sent to AWS, written out as overlays), property
(model-based and fuzz tests plus mutation checks on the sans-IO cores only), weight (`cargo xtask weight`, ceilings gated in CI) and
independent audit (a separate agent diffs doze against AWS on new request sequences; MAJOR / NOT DONE only; three rounds budgeted per
service). Results go on `scoreboard.md`. A claim that did not come out of a loop is tagged `docs` or `unverified`.

## Decisions taken (2026-11-09)

- One cutover at A17. Larder runs against the Rust services from A5 as internal dogfood.
- Recording spend under $10 a month; budget alerts at 50, 80 and 100 percent of actual spend.
- Two to three services in flight at once after the foundation, each in its own worktree.
- Recording happens in a **dedicated member account** (`doze-recorder`) under the Organization, reached through IAM Identity Center
  (organization instance, us-east-1). A service control policy limits it to us-east-1 and denies services we never record. This
  also makes account-wide behaviours (IAM password policy, account settings) recordable.
- Lambda response streaming and API Gateway WebSocket are in 1.0 (SCOPE.md D8).

## Stage 0: instruments before product

| # | Item | Exit evidence | Status |
|---|---|---|---|
| 0.1 | **Done 2026-11-09.** Recorder account: root MFA, budget, Identity Center, member account, SSO user and permission set, SCP, CLI v2, teardown script | `aws sts get-caller-identity --profile doze-rec` shows the member account id; teardown dry-run lists nothing | done: main account `821712915674`, role `doze-recorder-limited`, profile `doze-rec-main`; teardown tested on 19 throwaway resources and left the account clean (see `tools/aws-rec/`). The dedicated member account `doze-recorder` was parked: new member accounts return `OptInRequired` until sign-up completes |
| 0.2 | Record the existing Go conformance suite (`doze-aws/conformance`, about 139 tests) with `CONFORMANCE_RECORD=1` | `snapshots/` populated; spend logged here | waits on 0.1 |
| 0.3 | Run the same suite against the Go doze-aws | **A number**: Go's fidelity against AWS. This replaces 951 anecdotes and is the bar Rust must beat | waits on 0.2 |
| 0.4 | `cargo xtask` gains `weight`, `conformance` (wraps the pytest harness through `CONFORMANCE_ENDPOINT`), `gates` (the pre-commit order), and a mutation baseline file | each command runs and prints a table | not started |
| 0.5 | CI matrix: ubuntu x86_64 and aarch64, macos arm64, windows x86_64 on a hello-world crate | green on all four | not started |
| 0.6 | Limit prober v0 (bun): SQS names and attributes only | generated overlay compared with the docs-based one | not started |

What the repo has today (found 2026-11-09): `xtask` has only `lint-size` and `fuzz-check`; the weight job is inline shell in CI;
mutation testing is a manual method, with no cargo-mutants config; CI runs ubuntu-24.04 and macos-latest only; no Windows, no coverage.
The Go conformance harness is pytest and boto3 (Python is allowed here, as the test subject) and already supports
`CONFORMANCE_ENDPOINT`, `CONFORMANCE_TARGET=aws`, `CONFORMANCE_RECORD=1`; its `snapshots/` directory is empty.

Go/no-go: the Go baseline exists and spend matches the prediction. If recording is blocked, Stages 1 and 2 go ahead on `docs`
evidence and every fidelity claim stays `unverified`.

## Stage 1: risk spikes

Each has a hypothesis, a measurement, and a fallback if it fails. Code stays in the scratchpad; results are written here.

| # | Hypothesis | Measurement | Fallback if false | Result |
|---|---|---|---|---|
| S1 | Types and schema tables for all 18 models fit in 3 MiB at fat LTO | stripped size of a use-everything build | cargo feature per service, `opt-level = "z"` on model crates | **FAIL** (`spikes/S1.md`): typed decode + encode of all 18 is 6.61 MiB at opt 3 (4.21 at opt z); the tables alone are a 2.25 MiB floor. Gating fits about 11 of 18 services in 3 MiB. **S1b done: design C** (`spikes/S1b.md`) |
| S2 | `Fs`, redb, child supervision and the `serve` mode work on Windows or have a named substitute | **Deferred 2026-11-09: no Windows machine.** Until one exists: the Windows and x86 targets must at least compile (`cargo check --target x86_64-pc-windows-msvc` or `-gnu`, no run), and nothing may assume Unix paths, signals or sockets outside a `cfg` gate | named pipe or TCP `serve`, document the gap | Windows is assumed workable, not proven |
| S3 | A host-process runtime runs CDK's Node custom-resource handler | one real CDK handler under the Lambda runtime API | D6 stays native-first; built-in custom resources only | |
| S4 | `jlens-core` passes a corpus of ASL/JSONata cases recorded from Step Functions `TestState` | pass rate on the corpus | `seuil`, or an own subset evaluator (S to M) | |
| S5 | The system allocator returns memory after a 1 GiB upload and after a burst of invocations; the frame-pool pattern suffices | RSS before, peak, and 10 s after | `doze-alloc` crate (one `unsafe`), mimalloc on macOS | **Half true** (`spikes/S5.md`): the upload holds (+0 to 2 MiB); a 10k x 64 KiB burst leaves glibc at 823 MiB and the frame pool does not help. Fix: cap bodies in flight in the gateway (A3), system allocator on macOS and Windows, idle `malloc_trim` in a small `doze-alloc` on Linux; no jemalloc |
| S6 | redb on `SimFs` with a group-commit flusher meets the write-latency prediction for SQS-shaped load | p50 and p99 per send, fsyncs per second | kafka-style append log for hot queues only | **Holds** (`spikes/S6.md`): grouped commits give send p50 about 6.4 ms, p99 9 to 13 ms at 1k and 10k msg/s (durable, macOS), 5 to 14% of a core. `kafka-meta` has no group-commit flusher: write one in A1 (fail every waiter on a failed commit). Open costs 1 sync, clean close 4. The append log is not needed |

Note for S6: `PLAN.md` puts a group-commit flusher in `doze-store`; it was not found in `crates/kafka-meta` on 2026-11-09, so it may have to be written,
not extracted.

## Predictions to write down before measuring

Filled in when each item starts, with the date, so a miss is visible. First two (from the skeleton spike and kafka's results):
idle RSS of the Rust host with SQS enabled but unused stays under 6 MiB; adding the SQS service adds under 0.8 MB to the binary.

## Reuse from doze-kafka

`Fs` and `SimFs` (`crates/kafka-storage/src/{fs.rs,sim.rs}`), redb on the seam (`crates/kafka-meta/src/backend.rs`), the record pattern
(`crates/kafka-meta/src/life.rs`), shutdown and server handle (`crates/kafka-server/src/server.rs`), frame pool
(`crates/kafka-server/src/pool.rs`), data-dir lock (`crates/doze-kafka/src/lock.rs`). Extraction into `doze-fs`, `doze-store` and
`doze-runtime` happens at A1 and must leave the kafka gates and weight unchanged.

## Results so far

### 0.1 Recorder (2026-11-09)

- The permission set is limited to `doze-rec-*` names in us-east-1 (KMS: keys tagged `doze-rec=true`). A first draft let the role manage
  the account's Identity Center KMS key; the probe found it and the policy was rewritten before any recording.
- The teardown sweeps 34 kinds. Tested on 16 seeded services-worth of resources: dry run found 19, delete removed them, an independent
  CLI check showed zero. SQS lists lag deletes by minutes, so the residue check waits up to 90 s before calling anything left over.

### 0.2 / 0.3 SQS (2026-11-09)

Recorded `tests/test_sqs.py` against AWS: 182 labels, about 2.5 minutes, under a cent. The oracle contradicted the Go author in
three scenarios, which were corrected: AWS accepts a queue policy statement with no `Principal`; AWS answers a second `CancelMessageMoveTask`;
AWS verifies the AddPermission principal account exists and accepts any label (81 characters, with a space).

**Go doze-aws against that recording (origin/main f8c31a1), 182 labels:**

| | labels | share |
|---|---|---|
| matched exactly | 80 | 44% |
| differ only in the wording of the error message | 53 | 29% |
| behavioural differences | 48 | 26% |
| unverified | 1 | 1% |

Of the 101 differing labels, 34 differ in HTTP status or error code and 20 differ in whether AWS accepts or refuses the call.
This is the bar for Rust SQS: exact match first, with message wording tracked separately.

### 0.2 / 0.3 first batch (2026-11-09)

Ten modules recorded: sqs, ssm, secretsmanager, sns, sts, dynamodb, cloudwatch, kinesis, eventbridge, s3 (537 labels, about 20 minutes,
cents). Every sweep after a module came back clean. The oracle changed several scenarios again: AWS accepts a PutMetricData namespace
beginning `AWS/`; verifies that EventBridge SQS targets in another account carry a RoleArn; may refuse a boto3 presigned PUT (the URL signs
the checksum of an empty body); and a Kinesis ListShards can answer "not found" shortly after the stream is ACTIVE.

Go doze-aws against those recordings: 537 labels, 249 exact (46%), 147 behavioural differences, per module in `baseline.md`.
Still to record: logs (needs the policy to allow `/doze-rec-` log groups), the EventBridge pattern scenario (`events:TestEventPattern`), kms (needs key
tagging in the harness), iam, lambda, cloudformation, apigateway, stepfunctions.

### Windows (owner decision, 2026-11-09)

No Windows machine is available, so S2 and the Windows rows of 0.5 wait. Windows stays in scope as an assumption, not a claim: the design keeps
Unix specifics behind `cfg` and the `Fs` trait, the Windows targets are compile-checked where the toolchain allows, and no release note says Windows
works until it has run on one. Revisit when a machine or a hosted runner is available.

### S1b (queued, 2026-11-09)

S1 failed its 3 MiB target. That ceiling was ours and was set before measuring, and it is retired: PLAN.md now carries a whole-binary budget (at most 16 MiB, 12 intended, against Go's 22 to 24 MB stripped) defended by measurements. S1b measures the two cheaper designs the spike suggested: (a) typed glue only for tier-F operations, everything else decoded and validated on a schema-driven dynamic tree; (b) the fully dynamic validator for all operations. Prediction and the restated whole-binary budget are written before measuring.

## Audit brief, standing items (2026-11-09)

Every independent audit also checks PLAN.md rules 16 to 25: avoidable copies and allocations on hot paths; every escape hatch has a `// WHY:` and a better design was tried; no `_` arms over our own enums; no
`Rc<RefCell>`, `Arc<Mutex>` service state or guards across `.await`; hot-path allocation counts; which tests are redundant and which real failures would now pass silently.
