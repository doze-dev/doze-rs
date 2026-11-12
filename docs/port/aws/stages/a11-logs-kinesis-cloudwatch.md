# A11: CloudWatch Logs, Kinesis Data Streams and CloudWatch

Branch `aws-a11-logs-kinesis-cloudwatch`, 2026-11-12. Three new crates (`aws-logs`, `aws-kinesis`, `aws-cloudwatch`), all registered on the
gateway and on the peer bus in `crates/doze-aws/src/run.rs`. This note is for the integrator: it says what was built, what is refused, the
exact scoreboard output, what the recordings taught, what is from memory, and what is open. `PLAN.md`, `STATUS.md`, `scoreboard.md` and
`weight.md` are not edited here.

## Summary

| | Recorded labels exact | Labels in scenarios written ahead of a recording | Built / refused operations |
|---|---|---|---|
| Logs | **28 of 28** | 18, UNVERIFIED | 40 built, 78 refused by name |
| Kinesis | **27 of 30** (the other 3 are the split scenario, which stops at the refused `SplitShard`) | 9, UNVERIFIED | 33 built, 12 refused by name |
| CloudWatch | **28 of 28**, over JSON and again over CBOR | 8, UNVERIFIED | 23 built, 32 refused by name |

The Go edition on the same recordings: Logs 86% exact, Kinesis 50%, CloudWatch 32% (`scoreboard.md`).

## What was built

**Shared pieces (outside the three crates)**

- `doze-store::rows` (`RowTable`, `Rows`, `Db::submit_with_rows`): rows that are never loaded into memory. Log events, Kinesis records and
  CloudWatch samples are read from redb with a range scan and written in the same transaction as the metadata of the call. Without a disk
  (the rules under test) a table is a `BTreeMap`. Two tests.
- The **RPC v2 CBOR protocol**: `aws_wire::cbor` (request routing by path and `smithy-protocol`, a bounded CBOR-to-JSON reader, CBOR errors with
  `__type` and the `x-amzn-query-error` header), `aws_schema::cbor` (an answer tree written as CBOR, driven by the schema: tag 1 for a
  timestamp, byte strings for blobs), and `aws_gateway::cbor` (the same pipeline as JSON and Query). Fuzz target `fuzz_cbor` (2.4 million
  inputs in 60 s, no finding); the RFC 8949 appendix A examples are in the tests.
- `aws_schema::enc`: a double is now written with its point (`3.0`, not `3`). AWS does, and botocore keeps what the wire says, so `3` showed
  as an `int` where AWS's was a `float` (found by the first CloudWatch run; it affects every service with a double in an answer).
- `aws_gateway::pipeline::inflate`: a request body with `Content-Encoding: gzip` is inflated after the signature check, held to the body limit once
  inflated (the CloudWatch model gives `PutMetricData` the `requestCompression` trait, and botocore compresses past 10 KB). Applies to the JSON and
  CBOR paths. Found by reading the model, not by a failing scenario; covered by the socket test. This is what added `flate2` to the binary.
- `conformance/harness/target.py`: `CONFORMANCE_CLOUDWATCH_PROTOCOL=cbor` makes botocore speak CBOR to CloudWatch (see "Protocols").

**Logs (`aws-logs`)**: groups (create, delete, describe, list, class, KMS key, deletion protection, tags by name and by ARN, retention),
streams, `PutLogEvents` (the sequence token is ignored, as AWS now does; batch size 1,048,576 bytes with 26 per event, chronological order,
24-hour span, `tooOld`/`tooNew`/`expired` indexes), `GetLogEvents` (both directions, `f/` and `b/` tokens, time window), `FilterLogEvents`
(across streams in time order, paged), filter patterns of the three forms (terms with `-` and `?`, JSON with `&&`, `||`, parentheses,
`IS NULL`, `EXISTS`, indexes and `[*]`, space-delimited with `...`), retention applied lazily (when a group is written or read, at most once
a minute), `storedBytes` as a counter (message bytes plus 26), metric filters (a matching event becomes a CloudWatch data point through the
peer bus when CloudWatch runs; `TestMetricFilter`), subscription filters (stored; at most 2 per group; **nothing is delivered**), and
stored documents in one table (resource policies, account policies, query definitions, data protection policies).
Refused by name: Insights (`StartQuery`, `GetQueryResults`, ...), Live Tail, export and import tasks, deliveries, destinations, anomaly
detectors, transformers, integrations, scheduled queries, lookup tables, index policies.

**Kinesis (`aws-kinesis`)**: streams, shards (even hash ranges, each with a sequence number range of its own), `PutRecord`/`PutRecords`
(MD5 of the partition key against the shard ranges, `ExplicitHashKey`), `GetShardIterator` (all five types), `GetRecords` (limit, byte cap,
`MillisBehindLatest`, iterator expiry after five minutes), retention (lazy: expired records are removed when the stream is next written
or read, at most once a minute), tags by name and by ARN, encryption and enhanced monitoring (stored, not applied), on-demand mode,
record size, warm throughput, resource policy, consumers (registered, listed, deregistered; `DeleteStream` refuses a stream with consumers
unless `EnforceConsumerDeletion`). **A notification on append**: `Kinesis::appended()` is a `watch::Receiver<u64>` that counts appends
once they are on disk; a Lambda mapping (A13) waits on it. (There is no shared notification API in `aws-peers` yet; SQS has its own
`Notify`. A13 should decide the shape; this is the smallest thing that works.) Shard records keep `ParentShardId`, `AdjacentParentShardId`
and `EndingSequenceNumber`.
Refused by name: `SplitShard`, `MergeShards`, `UpdateShardCount` (PLAN E9), `SubscribeToShard`, channels, the account-settings operations,
`UpdateStreamRecordDistributionStrategy`.

**CloudWatch (`aws-cloudwatch`)**: `PutMetricData` (values, statistic sets, `Values`/`Counts`, units, high-resolution), `GetMetricStatistics`,
`GetMetricData` (single `MetricStat` per query, `ScanBy`; an `Expression` query is refused with 501), `ListMetrics` (filters, `RecentlyActive`,
paging); alarms as objects (`PutMetricAlarm`, `PutCompositeAlarm`, `DescribeAlarms`, `DescribeAlarmsForMetric`, `DeleteAlarms`, `SetAlarmState`,
`Enable`/`DisableAlarmActions`, `DescribeAlarmHistory`, tags); dashboards (the body kept verbatim); anomaly detectors (stored).
Samples are kept for 24 hours (3 hours at one-second resolution), a metric is forgotten after 14 days without data, at most 10,000 metrics
are kept (the one updated longest ago goes).
**Alarms are evaluated at their deadlines, not on a tick.** After every call the service computes, per alarm, the first period boundary at
which the verdict with no further data would differ from the state; the earliest of those is the region's `next_due`. One tokio task per
region sleeps to it (`timers.rs`), asks the actor to judge what is due, and is handed the next deadline; with no deadline the task ends and
the service holds no timer. A write of data an alarm watches is committed first and then judges that alarm at once. A forced state
(`SetAlarmState`) stands until the next period boundary. `Cloudwatch::resume()`, called from `run.rs` after the gateway binds, looks for
regions with state on disk and arms their timers, so a restart does not leave an alarm waiting for a request. Test: a one-second-period alarm
goes `OK` to `ALARM` on a write and back to `OK` at its deadline with no request, and the timer is gone afterwards; another test stops the
process and starts a second service on the same directory.
Math alarms (`Metrics`) and composite alarms are stored and returned; their state is `INSUFFICIENT_DATA` with a reason that says they are not
evaluated. **Alarm actions**: an ARN on SNS is carried out with `sns:Publish` and one on Lambda with `lambda:Invoke` through the peer bus *when that service
is registered*; otherwise the alarm's history gets `Failed to execute action <arn>: <service> is not running, so the action is stored and not
delivered locally` (any other ARN: `actions on <service> are stored and not delivered locally`). SNS and Lambda are not in this branch, so
the delivery path is tested only up to the refusal; the message format is from AWS's documentation and memory.
Refused by name: metric streams, Contributor Insights, mute rules, `GetMetricWidgetImage`, OTel enrichment, datasets, resource metrics
configuration, `PutLogAlarm`, `DescribeAlarmContributors`.

## Protocols

Work done from the models and the installed boto3 (1.43.107): Logs and Kinesis are awsJson1.1. **CloudWatch's model offers awsJson1.0, awsQuery
and rpcv2Cbor, and botocore 1.43.107 chooses `json`** (its `PRIORITY_ORDERED_SUPPORTED_PROTOCOLS` lists `json` before `smithy-rpc-v2-cbor`;
`service-2.json` lists `smithy-rpc-v2-cbor` first, which is not what decides). So boto3 did not need CBOR here. CBOR is built because the
model offers it, newer SDKs and the Rust/Java/Go SDKs prefer it, and PLAN A11 asks for both encodings. Query on CloudWatch is refused with
the same readable XML answer as on SQS (the settled decision). Both encodings are scored against the same recording (below); the CBOR path is
also covered by a socket test (`crates/aws-cloudwatch/tests/cbor_wire.rs`) and by the RFC vectors.

## Scoreboard

Run exactly as the SQS and SSM entries did (`--account 111122223333`, `UV_BIN` pointing at a real uv), on a debug build of this branch:

```
cargo xtask conformance --cmd "$PWD/target/debug/doze-aws --listen {addr} --data-dir {data} --account 111122223333" logs kinesis cloudwatch
```

Exact output (release binary), JSON encoding for CloudWatch:

```
module           labels  exact  wording  behaviour  unverified
logs                 46     28        0          0          18
kinesis              36     27        0          0           9
cloudwatch           36     28        0          0           8
total: 118 labels, 70% exact, 70% ignoring wording
```

and with `CONFORMANCE_CLOUDWATCH_PROTOCOL=cbor` (botocore forced to RPC v2 CBOR):

```
module           labels  exact  wording  behaviour  unverified
cloudwatch           36     28        0          0           8
total: 36 labels, 77% exact, 77% ignoring wording
```

Read it as: every label that has a recording matches AWS exactly (Logs 28 of 28, CloudWatch 28 of 28 in both encodings, Kinesis 27 of 27 reached);
**3 recorded Kinesis labels are not reached** (the split scenario stops at the refused `SplitShard`; the harness counts only labels a scenario
reaches, so they are in neither column: 27 of 30 recorded). The unverified labels are in scenarios written ahead of a recording: they run and
pass their own assertions against doze-aws and are compared with nothing. The deviations list is empty. The Go edition on the same
recordings: Logs 86% of 28, Kinesis 50% of 30, CloudWatch 32% of 28 (`scoreboard.md`).

## What the recordings taught

- **Logs:** "already exists" messages for groups and streams have no full stop; "does not exist" ones do. A retention outside the list is refused with
  the full list. `DescribeLogGroups` carries `bearerTokenAuthenticationEnabled: false`, `deletionProtectionEnabled: false`, `logGroupClass`,
  `metricFilterCount` and an `arn` ending `:*` beside `logGroupArn`; `retentionInDays` only when set. A stream with no events has only `arn`,
  `creationTime` and `logStreamName`. `{ $.code = }` is `InvalidParameterException: Invalid filter pattern`.
- **Kinesis:** every shard of a stream starts at a sequence number of its own (the first run failed on this). A `ValidationException` for a
  pattern prints the model's pattern with its anchors (`^(0|([1-9]\d{0,38}))$`), where IAM and STS print theirs without: a service-specific
  wording, so Kinesis overrides `Service::invalid` for patterns. `ResourceNotFoundException` for a stream is
  `Stream <name> under account <account> not found.`; for a shard `Shard <id> in stream <name> under account <account> does not exist` (no full stop).
  Increasing the retention to what it already is is accepted. The split scenario shows AWS closes the parent of a split at once even if it never held a record.
- **CloudWatch:** doubles go out as `3.0`. The validation errors are `ValidationError` (the Query code of `ValidationException`; the CBOR and JSON
  answers carry it in `x-amzn-query-error`). `ComparisonOperator`'s enum set is listed in an order that is neither the model's nor its
  reverse. A bad unit is `InvalidParameterValue` with the set of 27 units in AWS's order, not a validation error. A period of 45 seconds is
  refused as `must be a value in the set [1, 5, 10, 20, 30]`. `SetAlarmState` on an absent alarm is a 404 `ResourceNotFound` with an **empty
  message**. `DescribeAlarms` always has `CompositeAlarms` and `LogAlarms` (empty lists). Deleting an absent alarm is accepted. Reserved
  `AWS/` namespaces are accepted by `PutMetricData`.

## (memory): worded or ruled from memory, not from a recording

Each is marked `(memory)` in a code comment.

- Logs: the filter-pattern term rule (a term matches as a whole word; `*` is a run of non-blanks), the space-delimited forms with `||` and a
  repeated field name, `TestMetricFilter`'s `extractedValues` keys, the `aws/` prefix refusal, `Upload too large`, the 24-hour span message, the
  deletion-protection refusal, `LimitExceededException` for more than 2 subscription or 100 metric filters, the words of every stored-document
  refusal, `rejectedLogEventsInfo` indexes as exclusive end counts, the shape of `eventId`, tokens (`f/<32 hex>/s`), the sequence token (56
  digits that move), the group-name pattern for `ListLogGroups`.
- Kinesis: sequence numbers (about 29 digits, time-ordered: `(arrival ms << 24) + counter`), iterators (base64 of stream, shard, position and
  issue time), `ExpiredIteratorException` wording, the retention refusals other than the recorded minimum, the 500-shard limit, consumer
  refusals, the words of an oversized record, `GetResourcePolicy` on a stream with none (a not-found error: the Go finding G7 says AWS does this).
- CloudWatch: the **whole evaluation rule** (the last N periods aligned to the epoch with the newest holding the present; ALARM when at least
  `DatapointsToAlarm` breach, OK when more than `N - M` do not, otherwise `INSUFFICIENT_DATA`, or unchanged under `ignore`; `TreatMissingData`
  substitutes), the reason and `StateReasonData` text, the SNS message, the percentile definition (nearest rank, from at most 128 raw values
  kept per bucket), the 1,440-datapoint limit text, the timestamp-window refusal, `Period` must be a multiple of 60, the dashboard refusals, the
  anomaly detector shape, and the "Not evaluated locally" reasons. Nothing in the recordings evaluates an alarm or reads a percentile.
- A `Unit` refusal says `MetricData.member.1.Unit` whatever the data point's index and whichever operation sent it (the violation does not
  carry the path).

## Decisions for the owner, and what is not done

1. **Resharding.** The split scenario (3 recorded labels, so Kinesis is 27 of 30) is the only thing keeping Kinesis from 30 of 30. PLAN E9 says
   refuse by name; the shard records already keep lineage. `SplitShard`, `MergeShards` and a uniform `UpdateShardCount` are roughly 150
   lines plus `ChildShards` in `GetRecords`; lifting the refusal is an owner call.
2. **Subscription filters deliver nothing**, to Kinesis too, which now exists. Delivering means gzip and CloudWatch Logs' envelope
   (`messageType: DATA_MESSAGE`, base64) and a compression dependency. Left stored and unflagged on the wire (there is no response-header sibling of
   `X-Doze-Not-Built` in the tree to carry a "stored, not active" flag; the flag is in the data: alarm reasons and history).
3. **Metric streams, Contributor Insights rules and mute rules** (SCOPE tier B, "store and flag") are refused by name, not stored. The
   stored-document table exists (anomaly detectors use it); each family is a handful of shapes. Say if they should be built for 1.0.
4. **The Lambda `AWS/Lambda` producer** is not built (no Lambda). Alarm actions on Lambda are tried through `lambda:Invoke` with the event as a
   base64 payload; the real Lambda service may want another shape.
5. **A pattern with a regular expression (`%...%`) is refused with 501**, as PLAN defers it ("Regex filter patterns DEFER").
6. **The filter-pattern term rule** (whole words) is the largest unverified behaviour in Logs; record scenarios for it before relying on it.
7. **Recordings still to make**, all written ahead and passing against doze-aws: `test_logs.py` (`test_filter_patterns_of_each_form`,
   `test_get_log_events_pages_in_both_directions`, `test_metric_filters_subscription_filters_and_tags_by_arn`), `test_kinesis.py`
   (`test_every_iterator_type`, `test_consumers_policy_and_tags_by_arn`), `test_cloudwatch.py`
   (`test_composite_alarms_dashboards_and_history`). The recorder policy will need `logs:PutMetricFilter` and friends, `kinesis:RegisterStreamConsumer`,
   `cloudwatch:PutCompositeAlarm`, `PutDashboard`.
8. **Not tested against the real SDKs other than boto3.** CBOR is exercised by botocore (forced), by a socket test and by RFC vectors; no Java, Go or Rust SDK ran.
9. **Restart and the first request of a region.** A region the process has never been asked about gets no timer until `resume()` opens it at startup
   or a call arrives. `resume()` opens every region that has CloudWatch files, which costs a database open each until the idle reap.

## Tests

- `aws-logs`: 24 (15 state-machine tests including restart-after-writes and "a read changes nothing on disk", 8 pattern tests, a refusal-by-name test), fuzz target `fuzz_filter_pattern` (850,000 inputs in 60 s, no finding).
- `aws-kinesis`: 21 (iterator types, retention expiry with a manual clock, routing by the recorded keys, restart, consumers, tags, notification flag).
- `aws-cloudwatch`: 22 (metrics read back as the recording, alarm evaluation against deadlines with a manual clock, missing-data settings, M of N, forced states,
  actions queued and refused, history, tags, dashboards, detectors; a real-time timer test; a restart test; a CBOR round trip; the socket test in `tests/cbor_wire.rs`).
- `aws-wire` cbor: 6 (RFC 8949 vectors, bounds, routing, errors); `doze-store` rows: 2.

## Weight

Measured with `cargo xtask weight doze-aws` on darwin/arm64, release (2026-11-12, this branch only, without the other A8 to A13 stages):
**6,300,816 B (6.01 MiB), 210 packages, 3,664 KB idle.** The limit file line is now `doze-aws 8600000 213 6144`; the byte limit is the macOS size
times 1.3 plus 5% (8,600,617), under the owner's 10 MB ceiling. The previous line was 5,097,200 B, 198 packages, 3.3 MB idle.

Ready to paste into `weight.md`: *A11, CloudWatch Logs, Kinesis and CloudWatch: the whole binary goes from 5,097,200 B to 6,300,816 B (+1,203,616 B, +12
packages) and idles at 3,664 KB. The cost is the three models' tables (Logs 118 operations, Kinesis 45, CloudWatch 55), the three services, the
filter-pattern parser, the CBOR codec, and `flate2` with its pure-Rust backend (`flate2`, `miniz_oxide`, `adler2`,
`crc32fast`, `simd-adler32`) so that a gzip request body, which botocore sends for `PutMetricData` past 10 KB, can be read. Nothing runs until a
request: no task, no timer. The CloudWatch alarm timer is one task per region that holds an alarm with a pending deadline and ends when none is
pending; idle with a configured alarm and nothing pending is the same as idle without. Events, records and samples are never held in memory: they
are read from redb by range. Limit: 8,600,000 B, 213 packages.* Per-piece sizes were not isolated; the +12 packages are the 3 crates, the 5 named above and
4 that I did not trace. The idle-wake-up gate for "one alarm configured" (A4's weight case) was not run: a configured alarm with nothing pending
holds no timer by construction and by test (`the_timer_sleeps_to_the_deadline_and_is_gone_when_there_is_none`), and an alarm with a pending deadline
wakes once at that deadline.
