# small-ops: SSM resource policies and reviews, CloudWatch log alarms and metric math, EventBridge CancelReplay (stage note, 2026-11-19)

Branch `aws-small-ops`, from `origin/main` at `f5fa6da88`. The integrator folds this note into `PLAN.md`, `STATUS.md` and
`scoreboard.md`; none of those was touched here. AWS is the only reference: each behaviour cites the model documentation
(`models/<service>.json`) or says it is not recorded. Wording not seen recorded is marked `(memory)` in the code beside it. The recorder
SSO session (`doze-rec-main`) had expired, so **nothing was recorded**: every new boto3 scenario is UNVERIFIED.

## 1. SSM resource policies (`crates/aws-ssm/src/resource_policy.rs`, `authz.rs`)

- Built: `PutResourcePolicy`, `GetResourcePolicies` (paged, the model's 50 at most), `DeleteResourcePolicy`.
- What takes one, from the model: a **parameter** (only in the advanced tier; a `SecureString` only under a customer managed key, not
  `alias/aws/ssm`), a **document**, and OpsCenter's `OpsItemGroup`, which doze-aws does not run (refused with
  `ResourcePolicyInvalidParameterException`, `ParameterNames: ["ResourceArn"]`, saying so).
- Concurrency, as the model documents it: a new policy is put with neither `PolicyId` nor `PolicyHash`; a change or a delete names
  both. An id with no hash (or a hash with no id) is `ResourcePolicyInvalidParameterException` naming the missing member; an unknown id
  is `ResourcePolicyNotFoundException`; a hash that is not the current one is `ResourcePolicyConflictException`. The hash is the
  SHA-256 of the policy text (hex) and the id is derived from the resource and the time; both are opaque on AWS (memory).
- Limits: more than 1024 bytes is `ResourcePolicyLimitExceededException` with `Limit` and `LimitType` (the model's number; the
  `LimitType` value is (memory)); text that is not an IAM policy document is `MalformedResourcePolicyDocumentException`.
- **Evaluated:** `needs` is now a `Core` method and hands the authorizer the policy on the parameter or document a request names,
  as SQS, SNS, Secrets Manager and S3 do. IAM reads it as the resource-based layer: a statement naming a principal grants what its
  identity policies do not, an explicit deny refuses what they allow. A resource with several policies is handed to the evaluator as
  one document holding all their statements. A parameter named by ARN is now its ARN in the need (the account in the ARN, not the
  caller's), and its selector (`:3`, `:label`) is dropped: the old code cut an ARN at its first colon.
- Kept in a table of its own (`resource_policies`), so the parameter and document records keep their on-disk shape. Deleting a
  parameter (also by expiry, `DeleteParameters`) or a document takes its policies with it; `doze-aws data reset` clears them.
- Difference (compatibility notes): one account locally, so cross-account sharing, which is what these policies are for on AWS, is
  not exercised, and no Resource Access Manager share is made (so `DescribeParameters --shared` has nothing to show).
- Tests: `aws-ssm/src/tests/resource_policies.rs`, including the real evaluator (`aws_iam_policy::evaluate`) reading the policy SSM
  hands it: a user with no identity policy reads the parameter by its grant, and its deny stops a delete.

## 2. SSM change-template reviews (`crates/aws-ssm/src/doc_review.rs`)

- Built: `ListDocumentMetadataHistory` (`Metadata=DocumentReviews`, paged). `UpdateDocumentMetadata` existed and only set a status;
  it moved here and now keeps each action as a reviewer response (create and update time, status, comment, reviewer), and who sent the
  version for review (`Author`).
- From the model's `ReviewStatus`: only one version is `APPROVED` at a time, approving one makes the one approved before it
  `REJECTED`; only one is `PENDING`, so sending a second version for review while one is in review is refused
  (`InvalidDocumentOperation`, (memory) whether AWS refuses or replaces). `UpdateReview` changes the comment of the last response and
  its `UpdatedTime`; with no review it is refused (memory). A non-change-template is refused as before.
- Author and reviewer are the account id (the root user's id is the account id); doze-aws does not ask IAM for the user name here
  (compatibility notes). Reviews are a table of their own (`reviews`); deleting a version or a document removes its reviews.
- Tests: `aws-ssm/src/tests/reviews.rs`.

## 3. CloudWatch

- **Metric math** (`crates/aws-cloudwatch/src/math/`: `lex.rs`, `parse.rs`, `search.rs`, `value.rs`, `eval.rs`, `funcs.rs`,
  `series_fns.rs`, `mod.rs`): a hand-written precedence parser (depth capped at 64), fuzzed (`fuzz_metric_math`, marker in
  `parse.rs`, target and seeds in `crates/aws-cloudwatch/fuzz/`; `cargo xtask fuzz-check` passes; about 1.6M runs on nightly found
  no crash). Supported: `+ - * / ^`, comparisons, `AND OR NOT`, lists; `ABS CEIL FLOOR LOG LOG10`, `AVG SUM MIN MAX STDDEV`,
  `DATAPOINT_COUNT PERIOD TIME_SERIES IF`, `METRICS()`, `REMOVE_EMPTY FIRST LAST SORT SLICE`, `FILL` (value, series, `REPEAT`,
  `LINEAR`), `RATE DIFF DIFF_TIME RUNNING_SUM`, `ANOMALY_DETECTION_BAND`, `EPOCH MINUTE HOUR DAY DATE MONTH YEAR`, and `SEARCH` over
  local metrics (schema braces, `Name=value`, quoted and partial terms, `AND OR NOT`, at most 500 series). Metrics Insights `SELECT`,
  `INSIGHT_RULE_METRIC`, `SERVICE_QUOTA`, `LAMBDA` and `DB_PERF_INSIGHTS` are refused with a sentence.
- **GetMetricData** (`metric_data.rs`, moved out of `read.rs`): MetricStat or Expression per query; ids unique; at most 500 queries;
  `ReturnData: false` left out; an array result is one result per series.
- **Math alarms** (`math_alarm.rs`, `judge.rs`, `eval.rs`, `transition.rs`): `PutMetricAlarm` with `Metrics` is now evaluated on
  the existing deadline mechanism. The model's rules are kept: at most 20 queries, exactly one with `ReturnData`, a single series;
  `ThresholdMetricId` must name an `ANOMALY_DETECTION_BAND`. `SEARCH` in an alarm is refused (memory).
- **PutLogAlarm** (`log_alarm.rs`): built **store-only**. It is checked as the model documents it (result counts, comparisons,
  `ActionLogLineCount` and the role it needs, a `rate()` schedule, offsets, `TreatMissingData`). It stays `INSUFFICIENT_DATA` with a
  reason saying it is not evaluated: main's CloudWatch Logs has no Logs Insights engine (`StartQuery` is not built). The aws-logs-ops
  branch adds one, and once that lands it can be wired through aws-peers. It is described only when `AlarmTypes` names `LogAlarm`
  (model). `Kind::Log` is the last enum variant, so alarms already stored still read.
- Differences (compatibility notes): `ANOMALY_DETECTION_BAND` is mean ± k standard deviations of the data read, not AWS's trained
  model. `SEARCH` partial terms are case-insensitive substrings. Log alarms and composite alarms are kept and not evaluated.
- Marked (memory): every refusal's wording; series paired by timestamp; division by zero giving no point; STDDEV as population
  standard deviation; `METRICS('x')` filtering by id; FILL leaving leading gaps; `DAY` with Monday as 1; the labels; a scalar shown at
  the metrics' timestamps; one period for every metric of a math alarm; the log alarm's `QueryARN` shape.
- Tests: `aws-cloudwatch/src/tests/math.rs`. The old tests that said math was refused or not evaluated were replaced.
- Open: evaluating log alarms; `MaxDatapoints`/`NextToken` paging and `Messages` in GetMetricData; one grid per request (no
  per-expression period); Metrics Insights. Also, behaviour not changed here: `DescribeAlarms` with no `AlarmTypes` still returns
  composite alarms, where the model says metric alarms only.

## 4. EventBridge CancelReplay (`crates/aws-eventbridge/src/replays.rs`, `replay_run.rs`)

- **Replays did finish instantly**: `StartReplay` put the whole window back before it answered, so nothing could ever be cancelled.
  A replay now runs over time, on the region's existing deadline timer (no new task or thread; with no replay going there is no
  deadline): `STARTING` for one second, then `RUNNING`, putting back 100 events every 250 ms, then `COMPLETED`. The pace is ours (AWS
  gives none and takes minutes to start, (memory)); it is a difference in the compatibility notes.
- `CancelReplay` (built): a `STARTING` or `RUNNING` replay answers `CANCELLING` and is `CANCELLED` at its next step (made due at
  once), with a `StateReason`; the events it had not yet put back are not sent. Any other state is `IllegalStatusException` (the model:
  "a replay can be canceled only when the state is Running or Starting"; the sentence is (memory)); an unknown replay is
  `ResourceNotFoundException`. A replay whose archive is deleted under it is `FAILED` (memory).
- `DescribeReplay` and `ListReplays` report the real state (the `State` filter works on every state), `StateReason`, and
  `ReplayEndTime` only once the replay has stopped. A running replay survives a stop and a restart, and carries on from its cursor.
- Storage: a replay that has not completed is a `Run` in a table of its own (`replay_runs`), so completed replays already on disk read
  as they are; a completed run moves to `replays`. `doze-aws data reset` clears both.
- Tests: `aws-eventbridge/src/tests/replays.rs` (cancelled while starting, stopped mid-way after one batch of 250 events, refusals,
  archive deleted), and the archive and persistence tests now drive the replay with the timer.

## 5. Scenarios (UNVERIFIED: the SSO session had expired)

- `test_ssm.py`: `test_resource_policy_on_a_parameter`, `test_resource_policy_refusals`, `test_change_template_review_history`.
- `test_eventbridge.py`: `test_a_replay_is_cancelled_while_it_starts`.
- `test_cloudwatch.py`: `test_metric_math_in_get_metric_data`, `test_metric_math_refusals`, `test_search_expression`,
  `test_math_alarm`, `test_log_alarm`. On AWS, the last one needs a real role `dzc-log-alarm` (the test does not create it) and runs
  a scheduled query while the alarm lives.
- All pass against doze-aws (boto3 drives every call). The advanced parameter they make is billed monthly, prorated: a fraction of a
  cent. The recorder permission set may need `ssm:PutResourcePolicy`, `ssm:GetResourcePolicies`, `ssm:DeleteResourcePolicy`,
  `ssm:ListDocumentMetadataHistory`, `ssm:UpdateDocumentMetadata` and `events:CancelReplay`; not checked, not asked for.

## Open

- Recording every scenario above, and every (memory) wording: the policy id and hash shapes, the refusals' sentences, whether a second
  version sent for review is refused, the replay's `StateReason` text and `IllegalStatusException` sentence.
- `DescribeParameters` with `Shared` and RAM promotion; `DeletionMode` on a document's policy is accepted and ignored.
- `doze-aws data` does not list resource policies or reviews as items of their own (they are counted with nothing; they are small).
