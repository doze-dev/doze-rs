# doze-aws Rust port, Stage 0 and 1: instruments and risk spikes

Status: **approved 2026-11-09, not started.** Owner steps for the AWS account are in progress.
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
| 0.1 | Recorder account: root MFA, budget, Identity Center, member account, SSO user and permission set, SCP, CLI v2, teardown script | `aws sts get-caller-identity --profile doze-rec` shows the member account id; teardown dry-run lists nothing | owner steps in progress |
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
| S1 | Types and schema tables for all 18 models fit in 3 MiB at fat LTO | stripped size of a use-everything build | cargo feature per service, `opt-level = "z"` on model crates | |
| S2 | `Fs`, redb, child supervision and the `serve` mode work on Windows or have a named substitute | CI on windows x86_64 | named pipe or TCP `serve`, document the gap | |
| S3 | A host-process runtime runs CDK's Node custom-resource handler | one real CDK handler under the Lambda runtime API | D6 stays native-first; built-in custom resources only | |
| S4 | `jlens-core` passes a corpus of ASL/JSONata cases recorded from Step Functions `TestState` | pass rate on the corpus | `seuil`, or an own subset evaluator (S to M) | |
| S5 | The system allocator returns memory after a 1 GiB upload and after a burst of invocations; the frame-pool pattern suffices | RSS before, peak, and 10 s after | `doze-alloc` crate (one `unsafe`), mimalloc on macOS | |
| S6 | redb on `SimFs` with a group-commit flusher meets the write-latency prediction for SQS-shaped load | p50 and p99 per send, fsyncs per second | kafka-style append log for hot queues only | |

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
