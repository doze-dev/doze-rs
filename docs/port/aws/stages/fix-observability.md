# Fix: CloudWatch Logs, CloudWatch and Lambda's metrics against the Go edition

Branch `aws-fix-observability`, 2026-11-13. Closes the behaviour gaps an audit found against the Go edition (`doze-aws/docs/SUPPORT.md`,
CloudWatch and Logs sections) in `aws-logs`, `aws-cloudwatch`, and the `AWS/Lambda` producer in `aws-lambda`. AWS is the judge: where Go
differed from AWS (Logs retention defaulting to a day), AWS's behaviour is kept and the Go behaviour became a local bound.

## What changed

| # | Gap | Now | Evidence |
|---|---|---|---|
| 1 | `{ $.f = * }` compared `*` as a string, so a number or boolean never matched and `!= *` matched them all | An unquoted `*` after `=` or `!=` is an existence test, whatever the field's type (null counts as there); `"*"` quoted stays a string pattern; `> *` is invalid | `pattern::tests::an_unquoted_star_tests_that_a_field_is_there_whatever_its_type` (strings, numbers, booleans, null, objects, missing, non-JSON) |
| 2 | Metric filter `defaultValue` stored, never used | A batch with events and no match reports the default once a minute per filter (AWS: "the value reported ... during a period when logs are ingested but no matching logs are found"); none for a filter with dimensions `(memory)` | `a_batch_with_no_match_reports_the_default_value_once_a_minute`, `a_filter_with_dimensions_reports_no_default` |
| 3 | Groups without a retention grew without bound | `retentionInDays` is still absent until set (AWS keeps forever). A local bound, applied lazily on write and read: `--logs-retention-days` (default 7, 0 keeps) and `--logs-max-events` per group (default 100,000, 0 no cap; past it the oldest streams' oldest events go until the group is 10% under). Held counts are a new `held` table; a stream from before it is counted once | `the_local_bound_removes_old_events_but_the_group_reports_no_retention`, `a_group_past_its_event_cap_loses_its_oldest_events`, `the_event_cap_holds_on_disk_and_across_a_restart` |
| 4 | No `AWS/Lambda` metrics | `Invocations`, `Errors` (0 or 1 per invocation), `Throttles`, `Duration` by `FunctionName`; `FunctionName`+`Resource`; `+ExecutedVersion` for an alias; and no dimensions. Held in memory per minute (ring of 4,096 minute counts) and **taken by CloudWatch before a read** (`ListMetrics`, `GetMetricStatistics`, `GetMetricData`, `DescribeAlarms*`, `PutMetricAlarm`) through a producer hook on the bus; **while a metric alarm in the region watches `AWS/Lambda`**, each invocation sends at once so the alarm judges it as it comes. Nothing is sent when nothing reads. `ConcurrentExecutions` not built (needs hooks in `rt`, which another agent edits) | `metrics::tests::invocations_reach_cloudwatch_when_it_reads_and_at_once_while_an_alarm_watches` (real `provided` function), two unit tests; `aws-cloudwatch` `producers` test |
| 5 | No EMF | A JSON event with `_aws.CloudWatchMetrics` that Lambda writes becomes data points (per metric, per dimension set, arrays of values, `Unit`, `StorageResolution`); an invalid directive leaves a plain log line. A client's own `PutLogEvents` is not read: AWS needs `x-amzn-logs-format: json/emf`, which the gateway does not hand a service | `emf::tests` (2), `embedded_metric_format_lines_lambda_writes_become_data_points` |
| 6 | Percentiles from the first 128 values per minute | Exact up to 1,024 observations per series per minute; past that a uniform reservoir sample (Algorithm R; a large `Counts` entry replaces its expected number of entries), and buckets merge by the observations each value stands for. Rank error about `sqrt(p(1-p)/1024)`: 0.3 points at p99, 1.6 at p50. A bucket is record version 2 (`seen`); version 1 is still read. Values are only gathered when a percentile is asked for | `stats::tests` (4): exact at 1,024, within bounds at 100,000 with a skewed head, weighted merge, v1 read |
| 7 | Alarm judged the period still filling | AWS (alarm-evaluation docs, read 2026-11-13): evaluated every minute (every 10 s for 10/20/30 s periods), a sliding window not aligned to the clock, ending at the last minute that is over. Implemented as such: the step still filling is never judged, and deadlines are the instants a step of data enters, moves between periods or leaves. Not matched: multi-day alarms (AWS: hourly, on data to the hour) | `an_alarm_follows_its_data...` (rewritten), `a_long_period_slides_by_the_minute_and_is_not_aligned_to_the_clock`, the two real-time timer tests |
| 8 | Metric data fixed at 24 h | `--cloudwatch-retention-hours` (default 24, at least 1); one-second data at most 3 h | `the_retention_setting_bounds_what_is_kept` |
| 9 | Stale doc comments | `aws-logs` crate doc and `Subscription` said subscription filters were stored only; CloudWatch's crate doc said data judged at once | crate docs |

## `(memory)` items

- The `= *` reading for non-string fields and null (pattern/json.rs).
- `defaultValue`: reported at ingestion time, once per minute, not for a filter with dimensions (events.rs).
- EMF rules: an empty or absent `Dimensions` is one point with none; a missing or non-string dimension member drops the directive; a
  missing or non-numeric metric member drops that metric (emf.rs).
- `AWS/Lambda`: the dimension sets, `Resource` naming an unqualified or `$LATEST` invocation by the function name, `Errors` as 0/1,
  `Throttles` apart from `Invocations` (metrics.rs).
- Percentile by nearest rank (stats.rs).

No new deviation; the deviations list stays empty.

## Files outside the three crates

- `crates/aws-peers/src/metrics.rs` (new) and `lib.rs`: the producer hook (`add_producer`, `take_metrics`) and the watched namespaces
  (`watch_namespaces`, `watched`). The bus is the one place both Lambda and CloudWatch already hold.
- `crates/doze-aws/src/cli.rs`, `run.rs`: three flags, passed to `Logs::with_settings` and `Cloudwatch::with_settings`.
- `crates/aws-lambda`: `metrics.rs` (new), three lines in `lib.rs` (module, field, install), `invoke.rs` (a `metric` target on `Started`,
  the count at the end of `drive` and at a throttle or failed start), and `aws-cloudwatch` as a dev-dependency for the end-to-end test.

## Gates

Scoreboard (release-aws, account 111122223333): logs 28 exact of 46, cloudwatch 28 of 36, lambda 52 of 58 (3 behaviour, as before):
unchanged. Weight: 4,950,688 B on darwin/arm64 (+83 KB over 4,867,344 B), 233 packages (no new package; `tracing` was already in the
tree and is now a direct dependency of `aws-cloudwatch`), idle 4,608 KB.
