# final-ops: Kinesis SubscribeToShard, log and composite alarms evaluated, DescribeAlarms default (stage note, 2026-11-19)

Branch `aws-final-ops`, from `origin/main` at `71ec261a5` (PRs #83 Logs Insights, #89 small-ops and #88 Lambda durable merged). The
integrator folds this note into `PLAN.md`, `STATUS.md` and `scoreboard.md`; none of those, nor `COMPATIBILITY.md` or `results.tsv`,
was touched. AWS is the only reference: each behaviour cites the model (`models/kinesis.json`, `models/cloudwatch.json`) or is
marked `(memory)` in the code beside it. **Nothing here is recorded**: the recorder's SSO session (`doze-rec-main`) had expired
(`aws sts get-caller-identity` refused), so every new boto3 scenario is UNVERIFIED.

## 1. Kinesis `SubscribeToShard` (`fanout.rs`, `subscribe.rs`)

- **Wire:** Kinesis now answers `SubscribeToShard` as an event stream through the gateway's JSON-protocol streaming path
  (`Service::streams`/`call_stream`, as Logs' `StartLiveTail`), framed by `aws_wire::eventstream`: `initial-response` first (botocore's
  JSON parser needs it), then `SubscribeToShardEvent` events. Seen working through boto3 `subscribe_to_shard` (no host-prefix
  workaround needed: the operation has none).
- **Opening** (in the actor, authorized as `kinesis:SubscribeToShard` on the consumer ARN): the consumer must be registered, matched by
  name *and* the creation second in its ARN (a re-registered consumer of the same name is another consumer); the shard must exist;
  `StartingPosition` `TRIM_HORIZON`, `LATEST`, `AT_SEQUENCE_NUMBER`/`AFTER_SEQUENCE_NUMBER` (with `SequenceNumber`) and `AT_TIMESTAMP`
  (with `Timestamp`), else `InvalidArgumentException` (wording memory). `DryRun: true` answers `DryRunOperationException` (the model
  lists it; wording memory).
- **The model's rules kept:** events "for up to 5 minutes", then the stream ends; a second call for the same consumer and shard
  "within 5 seconds of a successful call" is `ResourceInUseException`; one 5 seconds or more later "takes over", and the older
  subscription ends with a `ResourceInUseException` frame. (The brief said "ResourceInUseException on a second"; the model is more
  precise, and that is what is built.)
- **Events:** `Records` as `GetRecords` shows them (shared `record_json`), `MillisBehindLatest`, `ContinuationSequenceNumber` (the last
  record read, or, with nothing read, the number just before the next read, so `AFTER_SEQUENCE_NUMBER` resumes at the same place). At
  the end of a closed shard the event carries `ChildShards` and no continuation, and the subscription ends (memory: the KCL reads a
  missing continuation as the shard's end). A consumer deregistered, or a stream deleted, under a subscription ends it with a
  `ResourceNotFoundException` frame at its next read.
- **Timing:** an event at once, then one as soon as records are written (the task waits on the service's existing append counter, not
  a poll), and an empty one at least every five seconds otherwise (memory: AWS's heartbeat). The heartbeat and the five-minute end
  are one `doze_runtime::Expiring` in the subscription's task; there is a task only while a subscription is open, and nothing at all
  otherwise. The claim on a consumer's shard is released when the task ends.
- **Tests:** `aws-kinesis/src/tests/fanout.rs` (records already there and arriving, every starting position, the five-second rule and
  takeover, refusals, a closed shard's children, a deregistered consumer, and the five-minute life with its heartbeat under paused
  time).
- Difference (compatibility notes): consumers are `ACTIVE` at once; no per-consumer throughput limit.

## 2. CloudWatch log alarms, evaluated (`log_eval.rs`, `log_query.rs`, `timers.rs`)

- The alarm's schedule is its `rate()`: runs fall on a grid a rate apart from when it was put; a run missed while doze-aws was stopped
  is made once, not caught up on. The region's existing alarm timer (`timers.rs`) wakes at the next run; the actor hands back a
  `LogRun` (query, groups, window `[run - StartTimeOffset, run - EndTimeOffset]`) and **the timer runs it outside the actor**, through
  the peer bus: Logs Insights `StartQuery` with `<QueryString> | stats <AggregationExpression>` (a `sort` in the aggregation is left
  out: it only orders contributors), then `GetQueryResults` until it ends. Running it outside the actor matters: Logs calls CloudWatch
  from inside its own actor (metric filters), so CloudWatch must never wait on Logs from inside its own.
- `GetQueryResults` is the only way to learn a query ended, so the run's task asks again with a growing pause (10 ms to 1 s) and gives
  up after a minute. This is the one wait-by-sleeping in the stage; it exists only while a run is in flight.
- The answer comes back to the actor (`Request::LogResult`) and is judged with the metric alarm judge (`judge.rs`): one value per run,
  `QueryResultsToAlarm` out of `QueryResultsToEvaluate`, `TreatMissingData` for runs that found no rows. With several contributors
  (`... by host`) a run takes the highest value for a greater-than comparison, the lowest for a less-than one (difference: AWS judges
  contributors). The value is read from the aggregate's column (an `as` name, or the expression as written, which is how Insights
  names it), else the first number that is not a `by` field. The newest 100 results are kept per alarm in the `docs` table.
- A failed query (no group, Logs not running) makes the alarm `INSUFFICIENT_DATA` with the failure as its reason and
  `EvaluationState: EVALUATION_ERROR` in `DescribeAlarms` (memory). An answer for an alarm put again since is dropped; a put starts its
  results afresh.
- The "stored, not evaluated" reason is gone: every alarm kind starts with `Unchecked: Initial alarm creation`.
- **Tests:** `aws-cloudwatch/src/tests/kinds.rs`, including one through a real bus with CloudWatch Logs registered: log events
  written, the clock moved a minute, and the alarm reaches `ALARM` on Insights' `count(*)`.

## 3. Composite alarms, evaluated (`rule.rs`, `composite.rs`)

- `AlarmRule` is read by a hand-written parser (fuzzed: `fuzz_alarm_rule`, seeds in `crates/aws-cloudwatch/fuzz/seeds/`) to the
  model's grammar: `ALARM`/`OK`/`INSUFFICIENT_DATA(name or ARN, quoted or not)`, `TRUE`, `FALSE`, `AND`, `OR`, `NOT`, parentheses; at
  most 100 alarms and 500 elements (the model); nesting depth 64 (ours). `NOT` binds tighter than `AND`, `AND` than `OR` (memory). A
  rule that does not read, or names its own alarm, is `ValidationException` (wording memory).
- As the model says, a composite is evaluated as soon as it is made (`OK` or `ALARM`; `INSUFFICIENT_DATA` only before that), and again
  whenever an alarm it names changes state, which may move composites above it in turn; each composite is judged at most once per
  change, so a cycle ends. The reason names the child's ARN and when it changed, with `triggeringAlarms` in `StateReasonData`
  (memory: shape and wording).
- Not built: `ActionsSuppressor` (kept, never suppresses); refusing to delete an alarm a composite names (the model implies AWS
  refuses; not built, the composite then finds it in no state). Both are differences in the compatibility notes.

## 4. `DescribeAlarms` default

With no `AlarmTypes`, only metric alarms (metric and metric math) are returned, as the model says; composite alarms were returned
before. Explicit `AlarmTypes` are unchanged.

## 5. Also fixed

`alarms::iso` printed milliseconds twice (`22:13:00.000.000+0000`) in alarm history data; it now prints `22:13:00.000+0000`.

## Scenarios (UNVERIFIED: the SSO session had expired)

- `test_kinesis.py::test_subscribe_to_shard` (5 labels). On AWS enhanced fan-out bills per consumer-shard hour: a fraction of a cent.
- `test_cloudwatch.py::test_describe_alarms_shows_metric_alarms_unless_asked`, `test_a_composite_alarm_follows_its_rule`,
  `test_a_log_alarm_is_evaluated_on_its_schedule` (the last needs the role `dzc-log-alarm` on AWS, not made by the test; it waits for
  the first run, about a minute against doze-aws).
- Against this branch's debug doze-aws: `cloudwatch` 63 labels, 40 exact, 0 different, 23 unverified; `kinesis` 63 / 58 / 0 / 5;
  `logs` 51 / 51 / 0 / 0; `logs_insights` 57 unverified (unchanged). Every scenario passes.

## Weight

No new package: `bytes` (already in the binary) and tokio's `rt`/`time`/`macros` features (already in the binary) for `aws-kinesis`;
`aws-logs` is only a dev-dependency of `aws-cloudwatch`. `release-aws` doze-aws on darwin/arm64: 9,575,392 B, far under the
15,562,000 B line (not compared with main's own build here; `cargo xtask weight` was not run, it reads `./target`). Idle: no task or
timer unless a subscription is open, a log alarm exists, or a log query is in flight.

## Open

- Record the four scenarios; every (memory) above waits on it, most of all: the heartbeat interval, the continuation at a shard's end,
  the refusal wording, whether AWS's first log-alarm run is a rate after the put, the composite reason's shape.
- Recorder permission set may need `kinesis:SubscribeToShard` and `cloudwatch:PutLogAlarm` with Logs scheduled-query permissions;
  not checked, not asked for.
- Log alarm `WarmUpConfiguration` is kept, not applied; `ActionLogLineCount` adds no log lines to notifications.
