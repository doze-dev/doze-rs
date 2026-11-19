# logs-ops: the unbuilt CloudWatch Logs operations (stage note, 2026-11-18)

Branch `aws-logs-ops`, on `main` at `f864c8dd`. The integrator folds this note into `PLAN.md`, `STATUS.md` and `scoreboard.md`; none
of those was touched. AWS is the only reference: each behaviour cites the model (`models/cloudwatch-logs.json`) or is marked
`(memory)` in the code beside it. **Nothing here is recorded**: the recorder's SSO session (`doze-rec-main`) had expired, so every new
boto3 scenario is UNVERIFIED (57 labels in `conformance/tests/test_logs_insights.py`). The 51 recorded Logs labels still match.

29 operations built (CloudWatch Logs is now 69 of 118); 2 moved to `not_planned`:
- `GetLogObject`: large log objects exist only for logs ingested through OpenTelemetry (`PutOpenTelemetryLogs`), which this model
  does not have; there is never an object to fetch.
- `GetLogFields`: data sources are named by AWS-vended logs and CloudWatch pipelines, which do not exist locally;
  `GetLogGroupFields` answers the same question for a log group.

## 1. Logs Insights (`insights/`, `query_ops.rs`, `queries.rs`, `fields.rs`)

- **The language, by hand** (`insights/lex.rs`, `parse.rs`, `ast.rs`; fuzzed by `fuzz_insights_query`): commands `fields`,
  `display`, `filter`, `stats ... by`, `sort` (asc default), `limit`, `parse` (glob with `*`, or a regular expression with named
  groups), `dedup`, `unmask`, `#` comments; expressions with `or`/`and`/`not`, comparisons, `like` and `=~` (a regex matches anywhere,
  a string is a substring), `in`/`not in`, `+ - * / % ^`; backtick field names; periods `5m`, `1h`, `1mo`, `1q`, `1y` etc. The
  `SOURCE`, `pattern`, `diff`, `anomaly`, `filterIndex`, `unnest` and `lookup` commands are refused with a sentence. A refusal is
  `MalformedQueryException` worded `... at line L and position P` (memory). Nesting is bounded (64), periods are clamped so the
  calendar arithmetic cannot overflow (the fuzzer found that one: `period_overflow` seed).
- **Functions** (`funcs.rs`): `abs ceil floor log sqrt greatest least concat ltrim rtrim trim strlen toupper tolower substr replace
  strcontains isempty isblank ispresent coalesce bin datefloor dateceil fromMillis toMillis isValidIp isValidIpV4 isValidIpV6
  isIpInSubnet isIpv4InSubnet isIpv6InSubnet`, any case; an unknown one is refused. **Aggregates** (`stats.rs`): `count` (`*`, `()`,
  or a field: present values), `count_distinct`, `sum`, `avg`, `min`, `max`, `stddev` (sample), `pct` (exact, nearest rank; AWS
  estimates), `earliest`, `latest`, `sortsFirst`, `sortsLast`; they may be combined arithmetically (`sum(x)/count(x)`), not nested,
  and not used in `by`. Commands after a `stats` run over its rows, including a second `stats`.
- **Fields** (`record.rs`): `@timestamp @message @logStream @log (account:group) @ingestionTime @ptr`, the members of a JSON event
  flattened with dots (arrays as `a.0`), and in `/aws/lambda/` groups the `START`/`END`/`REPORT` fields (`@type @requestId @duration
  @billedDuration @memorySize @maxMemoryUsed @initDuration`, memory in bytes as Insights shows it) (memory). A transformed event's
  fields are read from the transformer's output; `@message` stays the original (memory).
- **Shown:** results are text; `@timestamp` as `2026-11-18 09:30:00.000`; whole numbers without a point (memory). A query that names
  no field shows `@timestamp` and `@message` (memory); `parse`'s fields are shown; `display` replaces the list; `@ptr` ends each event
  row. With no `sort`, rows come newest first (memory). Rows per query: `StartQuery.limit`, else 10,000 when the query has its own
  `limit`, else 1,000 (memory). `GetQueryResults` pages by `maxItems` (≤ 10,000, the model) with `nextToken`. Statistics:
  `recordsMatched`, `recordsScanned`, `bytesScanned` (stored bytes), `logGroupsScanned`, `resultCount`; the `estimated*Skipped` are 0.
- **The bounded worker** (`insights/job.rs`): `StartQuery` parses, checks (exactly one of the three group parameters, at most 50
  groups, each existing, `endTime ≥ startTime`, CWLI only; 100 live queries per account, the model's number), and takes a **snapshot**
  of the event table (`doze_store::rows::RowSnapshot`, new: a redb read transaction that can be held on another thread; under test a
  copy). The run is on the blocking pool, at most **2 at once** (a semaphore); the rest wait as `Scheduled`. Streams are read newest
  first in chunks and merged by time, so a `limit` with no `sort` stops the read early; a `sort` followed only by `limit`/`fields`
  /`display` keeps only the rows the answer can show; otherwise a sort or a stats holds at most 1,000,000 rows or groups and fails
  saying so. `StopQuery` sets a flag the reader checks between events; an ended query is `InvalidParameterException` (memory). Time
  range in seconds, both ends included; the range is moved past what retention and the local 7-day bound have removed.
- **Kept outside the actor** (an idle actor is reaped; a query outlives it), in memory: the newest 200 queries and at most 1,000,000
  result rows. A restart forgets them (difference).
- `GetLogRecord`: `@ptr` is base64 of region, group and row key (AWS's is its own opaque text); the record has every field, with
  `@timestamp` and `@ingestionTime` as milliseconds (memory). `ListLogGroupsForQuery` gives group ARNs. `DescribeQueries` filters by
  group, status and language, newest first. `GetLogGroupFields`: the model's windows (±8 minutes around `time`, or the last 15),
  up to 10,000 events a stream, percent rounded, sorted by share.

## 2. Live Tail (`tail.rs`, gateway, `aws-wire/src/eventstream.rs`)

- **Gateway:** a JSON-protocol service can now answer an event stream: `Service::streams(op)` and `Service::call_stream(call)`, and
  `pipeline.rs` sends the framed body as it comes (`application/vnd.amazon.eventstream`). The framing moved from Lambda to
  `aws_wire::eventstream` (Lambda's `InvokeWithResponseStream` uses it unchanged), with an `exception` frame added.
- **Session:** `initial-response` first (botocore's JSON parser reads it before the members: memory), `sessionStart`, then a
  `sessionUpdate` every second (empty when nothing came), at most 500 events each, sampled evenly past that with `sampled: true`,
  5,000 buffered and the oldest dropped (all the model's numbers), `SessionTimeoutException` after three hours. Refusals: identifiers
  that are not ARNs or end in `*` (the model), stream names and prefixes together or with more than one group, an absent group, a bad
  pattern; at most 15 sessions (memory). The actor hands each open session the events of each `PutLogEvents` it selects (the filter
  pattern language of `FilterLogEvents`); a task per open session writes frames and ends when the reader goes away. No timer while no
  session is open. Seen working through boto3 `start_live_tail` (with `inject_host_prefix` off against doze-aws, as Step Functions'
  sync endpoint already is).

## 3. Export tasks (`export.rs`)

`CreateExportTask` makes the task `PENDING`; before the call is answered it runs (`RUNNING`) and ends `COMPLETED`, or `FAILED` with
S3's reason. Objects through the peer bus: `<prefix>/aws-logs-write-test` (`Permission Check Successful`), then
`<prefix>/<taskId>/<stream>/000000.gz`, one line per event, ISO time with milliseconds, a space, the message (memory: AWS's layout);
prefix default `exportedlogs` (model); `logStreamNamePrefix` and the inclusive range honoured. A bucket S3 cannot write refuses the
call and the task is removed (AWS checks the bucket at create; wording memory). `CancelExportTask` of an ended task is
`InvalidOperationException` (memory). `DescribeExportTasks` filters by id and status, newest first.

## 4. Destinations (`destinations.rs`)

`PutDestination` (Kinesis or Firehose target only, memory), `PutDestinationPolicy` (JSON object up to 5,120 bytes, the model; an
absent destination is `InvalidParameterException` because the model gives this operation no `ResourceNotFoundException`),
`DeleteDestination`, `DescribeDestinations` (`DestinationNamePrefix`, ASCII order, the model). Tags through `TagResource`,
`UntagResource`, `ListTagsForResource`. **A subscription filter whose `destinationArn` is a destination of the same region delivers to
the destination's Kinesis stream** through the existing delivery (envelope, control message and all). The access policy is kept, not
evaluated (one account).

## 5. Transformers (`transform/`, `transformers.rs`)

Built by a fork of this agent in `transform/` (`procs.rs` reads and checks, `apply.rs` runs, `grok.rs`, `datetime.rs`, `path.rs`);
fuzzed by `fuzz_transformer` (grok compiler and date pattern reader carry the marker). 19 processors: `addKeys copyValue csv
dateTimeConverter deleteKeys grok listToMap lowerCaseString moveKeys parseJSON parseKeyValue parseVPC parseWAF renameKeys splitString
substituteString trimString typeConverter upperCaseString`; `copyValue` reads `@logGroupName @logGroupStream @logStream @accountId
@regionName @message`. Refused: `parseCloudfront parseRoute53 parsePostgres parseToOCSF`. The model's rules: 1–20 processors, one
member each, vended parsers first and only on `@message`, `flattenedElement` with `flatten`. **Applied at ingestion:** each event is
kept with what its group's transformer made of it (event record version 2, `transformed`; version 1 records read as before). Insights
reads the transformed fields; metric and subscription filters with `applyOnTransformedLogs` match and send it; `GetLogEvents`,
`FilterLogEvents` and Live Tail give the original (memory). `TestTransformer` shows an event the transformer makes nothing of as it
came (memory). (memory) in the processors: the starting event is the message when it is a JSON object, else empty; a processor that
cannot apply leaves the event; parseJSON and csv overwrite at the root; csv's extra columns are `column_N`; parseKeyValue's
`nonMatchValue`; renameKeys keeps the parent; listToMap without flatten gathers lists; integer conversion of `1.5` leaves it; grok's
pattern table is Logstash's without look-around (IPV6 loose; types `int` and `float`); dateTimeConverter is a Java pattern subset
with English names, no `epoch`, a skipped or doubled local time takes the earlier instant; the VPC v2 field order; all wording.

## 6. Field index policies (`indexes.rs`)

`PutIndexPolicy` checks the document as the model describes it (`Fields` and/or `FieldsV2` with `FIELD_INDEX`/`FACET`, mutually
exclusive names, at least one, at most 20, 100 characters each; Standard class only); `DeleteIndexPolicy`; `DescribeIndexPolicies`
(the group's own, or an account `FIELD_INDEX_POLICY` whose `LogGroupNamePrefix` selection takes it, `source: ACCOUNT`);
`DescribeFieldIndexes` (DEFAULT: the model's `@logStream @aws.region @aws.account @source.log traceId`; CUSTOM from the policy;
categories filter, default without AUTO) with `firstEventTime`/`lastEventTime` from the group's newest 10,000 events a stream. An
index changes no answer locally (difference).

## 7. Scheduled queries (`scheduled.rs`)

Create, get, update (PUT semantics, keeps history and tags), delete, list (state, schedule type), history (time range, statuses).
`cron(...)`/`rate(...)` by `aws-cron` in the query's time zone (`Zone::named`); `scheduleStartTime`/`EndTime` honoured (read as
seconds, or milliseconds when that large: memory); the window is `now - startTimeOffset .. now - endTimeOffset`. **They run:** each
region has one `doze_runtime::Deadlines` timer (the mechanism Secrets Manager's rotations use), asleep until the next enabled query
is due and gone while none is; `Logs::resume` starts it after a restart (wired in `doze-aws/src/wire.rs`). A run is an ordinary
query (visible to `DescribeQueries`, `GetQueryResults`); the answer goes to the S3 destination as JSON rows at
`<uri path>/<name>/<queryId>.json` (memory). History keeps the newest 100 runs; a run's status and the delivery's (`IN_PROGRESS`,
`COMPLETE`, `FAILED`, `processedIdentifier`) are read from the run when asked. Lookup-table destinations are refused (lookup tables
are not planned). Errors are this API's own (`ValidationException`, `ConflictException`); wording (memory).

## 8. Weight and storage

New dependencies: `aws-cron` and `bytes` in `aws-logs` (both already in the binary), `crc32fast` in `aws-wire` (already linked),
tokio features `rt`, `time`, `macros` for `aws-logs` (already in the binary). Nothing runs while idle: no query, no session, no
scheduled query means no task and no timer. The new documents (destinations, transformers, index policies, export tasks, scheduled
queries) live in the existing `docs` table, so `doze-aws data` resets them with the rest.

## Checked on 2026-11-19 (takeover)

The first agent stopped with the scheduled-query split (`scheduled_run.rs`, for the file-size gate), the destination and Live Tail
authorization, the boto3 file and this note uncommitted; they were kept as they were. On `origin/main` (nothing new since). Clippy
(`aws-logs aws-gateway aws-wire doze-store aws-lambda doze-aws`, all targets), `cargo fmt --check`, `lint-size` (0 over hard) and
`fuzz-check` (0 without a target) are clean; the crate tests pass (72 in `aws-logs`). Conformance against this branch's doze-aws:
`test_logs.py` + `test_logs_insights.py` 51 exact, 0 different, 57 unverified; Lambda's `test_response_streaming` (the event-stream
framing moved to `aws-wire`) still exact. Weight under `release-aws`: 9,059,120 B, 277 packages, 5,088 KB idle, within the limits
file (no line changed). `cargo xtask weight` reads `target/` and ignores `CARGO_TARGET_DIR` (worked around with a link, not changed).
The recorder's SSO session was still expired, so nothing is recorded.

## Open

- Record `test_logs_insights.py` (8 scenarios, 57 labels) when the recorder's session is renewed; every (memory) above waits on it,
  most of all: the default fields and order of a query with no `fields`/`sort`, the default row count, number formatting, whether
  `@ptr`/`GetLogRecord` timestamps are milliseconds, `like` with a string, Live Tail's identifiers by name, export object layout,
  destination target validation, and every refusal's wording. `test_destinations` uses a stream that is never made (a Kinesis stream
  bills by the hour); whether AWS accepts it is what `outcome` records.
- `tools/aws-rec/permission-audit.ts` finds every call of the new scenarios covered by the recorder permission set (the seven
  calls it lists as uncovered are other services' and were so before).
- `GetQueryResults` while `Running` answers no partial rows; scheduled-query `Timeout`; KMS on query results (`encryptionKey`).
