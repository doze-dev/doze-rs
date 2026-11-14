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
