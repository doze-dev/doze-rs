# doze-aws Rust port: where we stopped (2026-11-09 night) and how to resume

Read this first. Then `phase0.md` (method, stages, spikes), `scoreboard.md`, `baseline.md`, `PLAN.md` (phases, targets, rules 16 to 25).

## State in one paragraph

Stage 0 (instruments) is nearly done: the AWS recorder account works (main account `821712915674`, permission set `doze-recorder-limited`,
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
