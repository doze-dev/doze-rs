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

## 2. SNS dead-letter queues (`crates/aws-sns/src/dead_letter.rs`, `deliver.rs`, `webhook.rs`)

- **A subscription's `RedrivePolicy` now receives what finally failed**, over the peer bus to SQS. When: an `sqs` or `lambda`
  subscription's client-side failure (the queue or function is gone, the queue policy refuses the topic under `--iam enforce`) moves the
  message at once, because AWS does not retry client-side errors; an `http` subscription's message moves when the delivery policy's
  retries are spent on a 5xx, a 429 or no answer, or at once on another 4xx. An `https` subscription is not called (no TLS client) and
  that is not a failed delivery: nothing is dead-lettered. **Go weak (bettered):** Go stored the policy and dropped the message.
- **What is sent:** the body the subscription would have been given (the JSON envelope; the raw message and its attributes for a raw
  subscription), FIFO group and deduplication id for a FIFO topic, plus the string attributes `ERROR_CODE`, `ERROR_MESSAGE`, `RequestID`
  and `TopicArn` (the message's own attributes give way to keep SQS's ten). The attribute names and the codes in them (the SQS or Lambda
  error code; `EndpointError`/`EndpointUnavailable` for http) are `(memory)`: not recorded. The dead-letter queue's own policy must let
  the topic send, as for an `sqs` subscription (refused under enforce, logged under soft). **AWS-correct** in where and when (the SNS
  Developer Guide's dead-letter pages); wording unrecorded.
- Tests: `tests/delivery.rs` `a_message_that_cannot_be_delivered_goes_to_the_dead_letter_queue_with_why` (a real SQS beside it).
- Open: AWS retries server-side errors to SQS/Lambda for up to 23 days before dead-lettering; locally a peer call has no server-side
  failure worth retrying, so none is retried.

## 3. CloudWatch Logs regular-expression filter patterns (`crates/aws-logs/src/pattern/regex.rs`)

- **`%...%` is built in every form**, with `regex-lite` (already in the tree; no new package): alone among the terms of an unstructured
  pattern (`%ERROR|WARN%`, combinable with `-term` and `?term`), as the value of a JSON comparison (`{ $.path = %^/api/v[12]/% }`, `=`
  and `!=` only; a number field is matched as written), and as the condition of a space-delimited field (`[ip, method = %GET|PUT%,
  ...]`, `=` and `!=`; a `,` inside `{m,n}` belongs to the expression). It searches (anchored only by `^`/`$`). Used by
  `FilterLogEvents`, `TestMetricFilter`, metric filters and subscription filters alike (one parser). A `%` inside a quoted JSON
  string is now a plain `%` (it was refused as a regex). **Go weak (bettered):** Go refused them; AWS supports them.
- **Limits:** two expressions in one pattern and five across a log group's metric and subscription filters (the model's documentation
  of `PutMetricFilter`/`PutSubscriptionFilter`; replacing a filter counts what it will hold) are `LimitExceededException`, words
  `(memory)`. **25 characters per expression** and the syntax (characters, `.`, `*`, `+`, `?`, `|`, groups, classes with ranges and
  `^`, `{m,n}`, anchors, `\d \D \s \S \w \W`, `\` before a symbol; refused: `(?...)` lookaround/flags/named groups, back references,
  `\b`, `\p{..}`) are **(memory)** of the Logs User Guide's regex page: the first thing to check when Logs is next recorded. Anything
  outside is `InvalidParameterException` "Invalid filter pattern", as for any other bad pattern.
- Tests: `pattern/regex.rs` (syntax, limits, reading to the closing `%`), `pattern/tests.rs` (each form, anchors, exclusion, limits),
  `tests/groups.rs` (the group quota, `TestMetricFilter`, `FilterLogEvents`). Conformance: `test_regular_expressions_in_filter_patterns`
  (UNVERIFIED until recorded).

## 4. Tier-B families stored, not refused (`crates/aws-cloudwatch/src/stored.rs`, `stored_rules.rs`)

- **Metric streams** (`PutMetricStream`, `GetMetricStream`, `ListMetricStreams`, `DeleteMetricStream`, `Start`/`StopMetricStreams`),
  **Contributor Insights rules** (`PutInsightRule`, `DescribeInsightRules`, `DeleteInsightRules`, `Enable`/`DisableInsightRules`,
  `GetInsightRuleReport`) and **alarm mute rules** (`PutAlarmMuteRule`, `GetAlarmMuteRule`, `ListAlarmMuteRules`,
  `DeleteAlarmMuteRule`) are stored in CloudWatch's document table, returned in their model shapes, paged, and tagged by ARN
  (`TagResource`/`ListTagsForResource` on `metric-stream/`, `insight-rule/`, `alarm-mute-rule:`). **Stored, not active locally:** no
  metric is streamed, no contributor counted (`GetInsightRuleReport` answers AWS's shape with the rule's `Contribution.Keys` as
  `KeyLabels` and no contributors), no alarm action muted. **Go weak (bettered):** Go refused them by name, which fails a CDK or
  Terraform deployment that declares one; SCOPE tier B says store and flag.
- **The flag:** the state fields keep AWS's own values (`running`/`stopped`, `ENABLED`/`DISABLED`, mute `Status` computed from
  `StartDate`/`ExpireDate` as `SCHEDULED`/`ACTIVE`/`EXPIRED`, `MuteType` `ONE_TIME` for `at()` else `RECURRING`): a value AWS never sends
  would stall a tool that waits on it (Terraform waits for a metric stream to be `running`). So the flag is the log line every Put
  writes ("stored, not active locally") and this note. This departs from the brief's "state fields" suggestion on purpose.
- Checks: `IncludeFilters` with `ExcludeFilters` is `InvalidParameterCombinationException`; a rule definition that is not a JSON object,
  and a mute schedule that is not `cron()`/`at()`, are `InvalidParameterValueException`; Get of an absent one is
  `ResourceNotFoundException`; Enable/Disable/Delete of absent rules answer `Failures` entries; Delete of an absent stream or mute rule
  succeeds (the model lists no not-found error for them). All wording `(memory)`. `Schema` of a rule is the model's documented
  `{"Name": "CloudWatchLogRule", "Version": 1}`.
- Still refused by name: managed Contributor Insights rules (`PutManagedInsightRules`, `ListManagedInsightRules`: they belong to other
  services' resources), `GetMetricWidgetImage`, OTel enrichment.
- Tests: `aws-cloudwatch/src/tests/stored.rs` (each family round-trips, states, tags, filters, refusals).

## 5. CloudWatch `ConcurrentExecutions` for `AWS/Lambda` (`crates/aws-lambda/src/metrics.rs`, `rt/mod.rs`)

- **Built** on the producer hook of fix-observability: as each invocation begins, the meter notes the invocations in flight for the
  function and for the whole region (`Runtime::concurrency`, read from the in-flight counts reserved concurrency already keeps); each
  minute keeps the most seen, published as `ConcurrentExecutions` (unit `Count`, one value per minute) by `FunctionName` and with no
  dimensions, held until CloudWatch reads (or sent at once while an alarm watches `AWS/Lambda`), like the other four. Read it by
  `Maximum`, as AWS documents. **Go weak (bettered):** Go had no `AWS/Lambda` metrics at all; fix-observability built four and left
  this one.
- **Approximation:** sampled when an invocation begins, so a minute in which none begins but one is still running has no point (AWS
  reports it). The dimension sets (`FunctionName` and none; not `Resource`) are `(memory)`.
- Tests: `metrics/tests.rs` (most-in-flight per minute and region, nothing else counted) and the end-to-end test with a real
  `provided` function now reads `ConcurrentExecutions` `Maximum` 1 back from CloudWatch.

## 6. Event source mapping concurrency (`crates/aws-lambda/src/poll/parallel.rs`, `poll/sqs.rs`)

- **SQS `ScalingConfig.MaximumConcurrency`:** a semaphore per mapping; a batch is read only when a slot is free (so nothing sits
  hidden in the queue waiting), and each batch runs in its own task holding its slot. With no `MaximumConcurrency`, five batches at
  once: AWS starts an SQS mapping at five concurrent batches and scales up (memory); locally five is the ceiling. A FIFO queue keeps
  each message group in order by itself (SQS does not hand out a group's next message while one is in flight). **Go weak
  (bettered):** Go, and this port until now, ran one batch at a time per mapping.
- **Kinesis and DynamoDB streams `ParallelizationFactor` (1 to 10):** the poller reads up to `BatchSize × factor` records of a shard,
  deals them into `factor` lanes by a stable hash (FNV-1a) of the partition key (Kinesis) or the item's key (DynamoDB), delivers each
  lane in batches of `BatchSize` in its own task with the shared retry, bisect, record-age and failure-destination semantics, and
  saves the shard's position past the whole read only when every lane is over (a restart reads again what was not finished:
  at-least-once, as on AWS). Records of one key stay in order. Factor 1 is the old path unchanged. **AWS-correct** (Lambda Developer
  Guide, "parallelization factor"); **Go weak (bettered):** Go read shards one batch at a time.
- Tests: `poll/parallel.rs` (a key keeps its lane and order), `tests/concurrency.rs` with real Python functions: six SQS batches under
  `MaximumConcurrency: 2` are seen two at a time and never more; thirty Kinesis records over five keys with factor 3 arrive each once,
  every key in order.

## 7. SQS `resume()` at startup: decided against, measured

- **What a resume would do.** SQS has no delay or visibility timer to re-arm: a delayed or in-flight message becomes visible by the
  clock, computed at each read (`ReceiveMessage`, the counts, `DozePeek`); a long poll asks again every quarter second; a Lambda
  mapping's poller (which Lambda's own `resume` restarts) reads the queue itself and looks again each second while messages are hidden.
  The only SQS timer is retention expiry, and an expired message is already counted out by the clock at once; the timer only removes it
  from disk. So a resume would change nothing any API caller can see; it would only free disk sooner for queues nobody touches.
- **What it costs, measured** (release binary, macOS, RSS 3 s after start, three starts each; a data dir of 4 regions × 5 queues ×
  200 delayed 512-byte messages, 4 MB on disk): **without resume 5,184–5,200 KB; with a resume that ticks every region 13,760–13,824
  KB** (+8.6 MB). With `--idle-secs 3` the figure stayed at 13,936 KB after the regions were reaped: the allocator keeps the pages. An
  empty data dir costs nothing either way.
- **Decision: no SQS resume.** Zero idle wake-ups stays true either way (a resumed timer would sleep in one-day pieces); the deciding
  facts are no visible benefit against +8.6 MB resident for a modest data set, growing with queued messages. **AWS-correct** as is (what
  AWS shows is reproduced by the clock); the experiment was not committed. The fix-sqs-dynamodb note's guess ("not worth the idle
  cost") is now a measurement.

## 8. DynamoDB transitional windows as flags (`crates/doze-aws/src/cli.rs`, `crates/aws-dynamodb/src/core.rs`)

- `doze-aws --dynamodb-create-ms MS`, `--dynamodb-update-ms MS`, `--dynamodb-delete-ms MS` (documented in `--help`: what each state is,
  that AWS takes seconds to minutes, that the default 0 lets a waiter pass on its first probe). Each falls back to its environment
  variable `DOZE_DDB_CREATE_MS` / `_UPDATE_MS` / `_DELETE_MS` (the older spelling, kept as an alias), then to 0. The values reach each
  region's actor through `Dynamodb::with_windows` (a host context carrying the bus and the windows, as CloudWatch's settings do); a
  host that passes none still reads the environment, so nothing else changes. **Go weak (bettered):** Go's (and this port's)
  windows were environment-only and undocumented. **AWS-correct** default unchanged (states exist; their length is local).
- Test: `tests/expiry.rs` `the_host_s_windows_reach_the_actor` (a table made through the service stays `CREATING` for the host's
  window, then is `ACTIVE`).

## 9. SQS 60-second windows: AWS's by default, `--sqs-skip-60s-waits` for test loops (`crates/aws-sqs/src/queues.rs`)

- **Default unchanged and AWS-correct:** re-creating a deleted queue's name within 60 s is `QueueDeletedRecently`, a second
  `PurgeQueue` within 60 s is `PurgeQueueInProgress`. **Go wrong (fixed earlier, kept):** Go made them opt-in (`--sqs-strict`).
- **`doze-aws --sqs-skip-60s-waits`** frees a deleted queue's name at once and lets a queue be purged again at once, for local test
  loops that make and drop the same queue (it reaches each region's actor through `Sqs::with_settings`). The name says exactly what
  it does; it is documented in `--help` with AWS's behaviour beside it.
- **The refusals name it.** AWS's recorded words are unchanged, and one sentence follows them: "... with the same name. doze-aws: start
  it with --sqs-skip-60s-waits to skip this wait in local test loops." (`SKIP_HINT`). So the recorded labels still match exactly, the
  conformance harness takes a trailing ` doze-aws: ...` sentence off a refusal's message before comparing (`DOZE_HINT` in
  `conformance/harness/snapshot.py`, with a selftest proving AWS's words before it are still compared). That is a harness change the
  integrator should know of: it is the general way for doze to add one hint after AWS's words without losing exactness. **Go weak
  (bettered):** the refusal now tells a developer what to do next.
- Tests: `aws-sqs` `tests/waits.rs`; `selftest/test_harness.py`
  `test_a_doze_hint_after_aws_words_is_set_aside_and_aws_words_still_compared`.

## Checks (2026-11-14)

- `cargo xtask gates`: exit 0 (fmt, clippy, codegen, lint-size, fuzz-check, test). `aws-sqs/src/tests.rs` would have passed the hard
  size limit, so the new SQS test lives in `tests/waits.rs`; CloudWatch's CBOR wire test named `PutMetricStream` as a refused
  operation and now names `GetMetricWidgetImage` (item 4 built the former).
- Conformance (release binary, `--account 111122223333`), exact counts unchanged: sqs 177 of 181, secretsmanager 27 of 65, sns 38 of
  104, logs 28 of 51 (the new regex scenario's labels unverified), cloudwatch 28 of 36, lambda 52 of 58, dynamodb 53 of 116.
- Weight (`cargo xtask weight doze-aws`, macOS): 7,494,304 B, 258 packages, 5,312 KB idle: ok. No new package (`aws-cron` and
  `regex-lite` were already linked); Linux size not measured.
