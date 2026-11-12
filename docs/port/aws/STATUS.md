# doze-aws Rust port: where we stopped (2026-11-12) and how to resume

**2026-11-12 (latest: A7 is built — IAM, STS, the Query protocol and policy enforcement)** — IAM 36 of 45 recorded labels exact (the other 9 wait on a re-recording), STS 2 of 2. `doze-aws --iam off|soft|enforce` (soft by default); SQS, SSM, Secrets Manager, KMS, IAM and STS ask the authorizer before they act; the simulator runs the same evaluator. The AccessDenied wording, Simulate* answers and trust-policy evaluation are UNVERIFIED (no recording). Waiting on the owner: paste the recorder policy, then record SSM, Secrets Manager, KMS and the IAM user/group/key scenario. Next: the cross-cutting items (region-scoped state, IAM-issued keys under verify), then the next service stage. (Earlier entry:) **2026-11-12 (A6 is built on the API side) — SSM, Secrets Manager and KMS are done; the services call each other over `aws-peers`; SecureStrings and secrets are kept under KMS keys; the Secrets Manager reference path works; KMS has RSA, key import, grants and key pairs, cross-checked against `ring`. 284 recorded labels are 96% exact; 113 more labels in scenarios written ahead of the recording are UNVERIFIED. Waiting on the owner: paste `tools/aws-rec/recorder-permission-set.min.json` (regenerated), then record SSM, Secrets Manager and KMS. Next stage: A7 (IAM and STS, the Query protocol).** (Earlier entry:) **2026-11-12 (A6 under way) — SSM Parameter Store is built: 27 of 31 recorded labels match AWS exactly (87%, Go 52%); the 4 that differ are labels where AWS answered AccessDenied under the recorder's policy. Secrets Manager is built too: 27 of 27 (100%). KMS first part: 44 of 45 (98%). Four services, 284 labels, 96% exact (Go: about 50%). Next in A6: KMS K2 (RSA, grants, import, replicas, data key pairs), then the SSM-to-Secrets read-through and SSM/Secrets sealing through KMS.** **Conformance runs need `--account 111122223333` in the `--cmd`.** (Earlier entry follows.) **2026-11-12 — SQS is built: 177 of 181 recorded labels match AWS exactly (97%), against the Go edition's 44%.** All 23 operations, exhaustive dispatch, binary 1.83 MiB, idle 3.0 MB, all gates green. Long polling and persistence are in (see the last two PLAN entries): state survives an idle reap and a restart; the binary is 2.56 MiB, idle 3.1 MB. SQS is closed at 177 of 181; the four that differ are decided (see the last PLAN entry): two are the opaque receipt handle in AWS's wording, one is the Query protocol that SQS does not serve by decision, one is a sampling quirk not worth copying. **Next:** A6 (SSM, Secrets Manager, KMS: the next three simple JSON services, whose recordings are already in). Still waiting on the owner: the IAM recorder policy paste, Node for spike S3, and the merge and push of `aws`.

**2026-11-12 (later) — a request now runs end to end in Rust.** On branch `aws`: A1 (the shared kit) and A2 (models and codegen, 18 generated crates, checked in CI) are done; A3 is built for the JSON protocols (auth, wire, gateway) and `aws-sqs` is a standard-queue tracer bullet behind the `doze-aws` binary. First Rust SQS score: **61 exact of 116 reached labels, 34% of all 182** (Go: 80, 44%); whole binary 1.63 MiB, idle 3.0 MB. See the last PLAN log entry for what is open. **Next:** the rest of SQS (batches, FIFO, message attributes, tags, permissions, dead letters, move tasks, long polling, persistence) driven by the scoreboard, then A6. Still waiting on the owner: the IAM recorder policy paste, Node for spike S3, and the merge and push of `aws`.

**2026-11-12 update (branch `aws`, worktree `~/Code/doze-dev/doze-rs-aws`; merge into main when a stage closes):** SSO verified, teardown found 0 (and 0 again after the re-record). Baseline re-run over all 17 recorded modules: 850 labels, 423 exact (50%), 225 behavioural, 18 unverified (`baseline.md`). Audit decisions answered by the owner (top of `audit-decisions.md`). The two CloudFormation scenarios are re-recorded (the bad parameter now carries the run prefix). **Stage 0.4 done:** `cargo xtask weight | conformance | gates` exist, CI runs `weight`. **S4 holds** (`spikes/S4.md`, 216 recorded TestState cases; `$eval` must be shadowed). CI: only the nightly `fuzz` job is red (kafka-wire fuzz target lacks `Features.served`; Kafka-side, untouched). **Still waiting on the owner:** (1) paste the regenerated recorder policy (`bun tools/aws-rec/recorder-permission-set.ts`, copy `recorder-permission-set.min.json`) so IAM can be re-recorded; (2) Node for spike S3 (none is installed: `proto` has no pinned version; S3 needs `proto install node` or the Lima VM). Next after those: S3, then A1. The SQS redrive scenarios (StartMessageMoveTask, ListMessageMoveTasks, CancelMessageMoveTask) are owed to the SQS recording.

Read this first. Then `phase0.md` (method, stages, spikes), `scoreboard.md`, `baseline.md`, `PLAN.md` (phases, targets, rules 16 to 25).

## State in one paragraph

Stage 0 (instruments) is nearly done: the AWS recorder account works (main account (id in `DOZE_REC_ACCOUNT`, kept out of the repository), permission set `doze-recorder-limited`,
profile `doze-rec-main`, no keys), teardown is tested, the Go conformance harness is forked to `conformance/` and recordings from real AWS exist for the modules
listed in `baseline.md`. Go doze-aws was measured against them (first batch: 46% exact over 537 labels). Stage 1 spikes: S1 failed (3 MiB ceiling retired, targets restated),
S5 half true, S6 holds, S1b (decode design) was running when we stopped, S2 (Windows) now runs through GitHub Actions, S3 (Lambda handlers) and S4 (ASL) not started.
No Rust for doze-aws exists yet; the foundation (A1 to A4) has not started.

## Repo and CI

- `github.com/doze-dev/doze-rs` (private), branch `main`, commits go straight to main, no co-author trailer.
- CI jobs: check (ubuntu, macos), windows (`cargo check`), weight (Linux limit 5.15 MB from its own measurement), supply-chain, fuzz (nightly, corpus dirs created).
- The kafka session works in the same checkout; check `git status` before committing and add only your paths.

## Resume checklist (in order)

1. `aws sso login --profile doze-rec-main` (8-hour session; the owner runs this). Verify: `/opt/homebrew/bin/aws sts get-caller-identity --profile doze-rec-main`.
   Use `/opt/homebrew/bin/aws` (CLI v2); plain `aws` is v1 on this Mac.
2. `bun tools/aws-rec/teardown.ts` (dry run). It must say `found: 0`. If not, `--delete`.
3. Read the CI state: `gh run list --limit 5`.
4. Finish batch 2 recordings (see "Recording status" below) and re-run `GO_BIN=... bun tools/aws-rec/baseline.ts` (needs the Go binary: build from `doze-aws` origin/main
   into the scratchpad with `GOWORK=off go build ./cmd/doze-aws`; the previous build lived in the session scratchpad and may be gone).
5. Fold S1b into `PLAN.md` (A2 row and risks) once its verdict is read in `spikes/S1b.md`.
6. Decide with the owner: the aws git worktree (kafka session collisions) and the audit decision list (now `audit-decisions.md` in this directory (original:
   `.../scratchpad/aws-console-direction/DECISIONS.md`; the console kit and screen sources are archived in `~/Code/doze-dev/_backup/scratchpad-2026-11-09.tar.gz`).
7. Then Stage 0.4 (xtask `weight`, `conformance`, `gates`), spikes S3 and S4, and A1.

## Recording status

Recorded (2026-11-09): sqs, ssm, secretsmanager, sns, sts, dynamodb, cloudwatch, kinesis, eventbridge, s3, logs, kms, iam, stepfunctions, apigateway, cloudformation, lambda.
**Baseline.md covers the first eleven only**: re-run `GO_BIN=... bun tools/aws-rec/baseline.ts` (all modules) to add kms, iam, stepfunctions, apigateway, cloudformation, lambda.

Scenarios that still need work (the oracle contradicted them or the policy did):

- `iam::test_user_group_and_access_key` was denied: it creates users under the path `/apps/`, and the policy matched only `user/doze-rec-*`. The generator now also allows `kind/*/doze-rec-*`; **the owner must paste the regenerated policy (`bun tools/aws-rec/recorder-permission-set.ts`, then copy `recorder-permission-set.min.json`) before IAM is re-recorded**.
- `cloudformation::test_a_failed_update_goes_back_to_the_previous_template`: the scenario asserts the stack is out of an `*_IN_PROGRESS` state, but AWS reports
  `UPDATE_ROLLBACK_COMPLETE_CLEANUP_IN_PROGRESS`; it needs a wait.
- `cloudformation::test_a_failed_create_can_be_left_or_deleted` errored at cleanup, and its template names an SSM parameter `/dzc/has space` outside the `doze-rec-` prefix, so the role
  could not delete it (the stack ended `DELETE_FAILED`). Rename it with the prefix. The teardown now retains resources that refuse to delete and finishes the stack.
- Do not trust the snapshots of those two CloudFormation tests until they are re-recorded.

See the table in `baseline.md` for what is recorded. Policy lives in `tools/aws-rec/recorder-permission-set.ts` (`bun` regenerates `recorder-permission-set.min.json`;
the owner pastes it into the permission set in IAM Identity Center when it changes). A scenario that AWS contradicts is changed to `snapshot.outcome(...)`
with a dated comment; never re-record without reading the diff.

## Owner decisions made (2026-11-09)

Single cutover at A17; recording spend under $10/month; 2 to 3 services in flight after the foundation; Lambda streaming and API Gateway WebSocket in 1.0;
D1 to D7 and D9 as recommended; recorder in the main account (member account parked: new member accounts return OptInRequired until sign-up completes);
Windows built on GitHub runners; targets realistic and measured, standards not loosened; Rust principles in PLAN.md rules 16 to 25 are binding.

## Open

- S1b verdict and the decode design; whole-binary budget re-based at A5.
- Optional, owner idea (2026-11-09): draw one console board in AWS-style colours (navy and orange) with the same CSS tokens swapped, next to Paper, to compare. Paper stays the direction unless it clearly wins. Kit: `scratchpad/aws-console-direction/kit/paper.css`, backed up in `~/Code/doze-dev/_backup/`.
- Audit decision list (60 items) for the owner; cross-service console links, the Access explainer, menu and filter components.
- Policy gaps found by batch 2 (if any) need one more paste.
- The Identity Center KMS key in the account is not ours; the policy denies touching it.
