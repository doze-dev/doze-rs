# Gaps: nine open items closed (stage note, 2026-11-14)

Branch `aws-gaps`, on `aws` at `70ce8e3d`. One commit per item. The integrator folds this note into `PLAN.md`, `STATUS.md`, `scoreboard.md`
and `weight.md`; none of those was touched here. Each behaviour carries one call: **AWS-correct**, **Go wrong (fixed)** or **Go weak
(bettered: how)**. Wording not seen recorded is marked `(memory)` in the code beside it.

## 1. Secrets Manager scheduled rotation (`crates/aws-secrets/src/schedule.rs`, `rotate.rs`)

- **The schedule starts rotations.** After every call the actor answers when the soonest rotation window of the region opens; one
  `doze_runtime::Deadlines` timer per region sleeps to it and sends a tick that starts every due rotation exactly as `RotateSecret` with
  no arguments would (pending version, the four steps over the bus, outside the actor). Nothing is due while a rotation is in progress,
  so a failed rotation is not retried in a loop. `Secrets::resume` (called once in `run.rs`, as CloudWatch's, EventBridge's and
  DynamoDB's are) ticks every region with state on disk: what came due while the process was stopped starts at once, and the timer is
  re-armed. With no rotation scheduled there is no task. **Go weak (bettered):** Go stored the schedule and never rotated on it.
- **Windows, as the model documents `RotationRulesType`.** `AutomaticallyAfterDays` and `rate()` count from the last rotation and open
  at midnight UTC; `cron()` names the window start; a manual `PutSecretValue`/`UpdateSecret` that becomes `AWSCURRENT` counts as a
  rotation for `AutomaticallyAfterDays`. AWS rotates "any time during the window"; here at its first instant (a time AWS could choose,
  the same on every run). `Duration` is stored and shown and changes nothing locally. `NextRotationDate` is the window start (it was
  last + N days to the millisecond, and `cron()` showed none). **AWS-correct** (documented; `NextRotationDate`'s exact instant is not
  recorded).
- **Rules AWS refuses are refused:** both `AutomaticallyAfterDays` and `ScheduleExpression`; an expression that is not `rate()`/`cron()`;
  a rate in minutes or under four hours (`InvalidParameterException`, words `(memory)`). **Go weak (bettered):** stored anything.
- **`RotateImmediately: false`** does what the model says: the configuration is stored, the first rotation waits for the window, and
  the configuration is tested by running `testSecret` alone against an `AWSPENDING` version "that is then removed". The version holds a
  copy of the current value (what AWS puts there is not documented: `(memory)`), so a `testSecret` that logs in with it succeeds;
  afterwards `AWSPENDING` is taken off it over the bus (the version is then label-less and dropped after a day, as every deprecated
  version is). It is not a rotation in progress: the next `RotateSecret` is not refused. A secret with no current value is not tested.
  **AWS-correct** (the brief guessed "must not run `testSecret`"; the model's documentation says it does).
- Scheduled rotations act as Secrets Manager in the secret's account and region (caller `secretsmanager.amazonaws.com`).
- Tests: `schedule.rs` (windows, refused rules, the caller), `tests/rotation.rs` (tick before and at the window, nothing due while in
  progress, manual rotation, the test-only run, the timer armed by the call and again by `resume` after a restart).
- Open: a failed rotation is not reattempted by the schedule (AWS says "That rotation will be reattempted"; locally it stays until
  `CancelRotateSecret` or a new `RotateSecret` after the label is cleared). The function's resource policy for
  `secretsmanager.amazonaws.com` is not checked.
