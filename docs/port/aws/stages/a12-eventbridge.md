# A12: EventBridge

Branch `aws-a12-eventbridge`, crates `crates/aws-eventbridge` and `crates/aws-cron`. Written 2026-11-13. The integrator folds the PLAN, STATUS,
scoreboard and weight lines in from here; nothing in those four files was edited.

## What was built

`aws-eventbridge` serves EventBridge (awsJson1.1, target prefix `AWSEvents`, signing name `events`). Same shape as SNS and CloudWatch: a sans-IO
`Core` behind a `doze_runtime::Keyed<Core>` (one lazy actor per region, home region `us-east-1`), state in `doze_store` redb (`eventbridge.redb`,
six record tables plus a row table for archived events), the answer held until the commit is flushed. `authz.rs` describes each request as
`Need`s (`events:<Op>`, rule and bus ARNs, the bus's own policy as `Need.policy`). Everything is keyed by account inside a region.
`aws-cron` is a new crate: `rate()` and the six-field `cron()` (`?`, `L`, `W`, `LW`, `#`, `nL`, names, steps, years 1970 to 2199), pure
(`Schedule::next_after`), with a fuzz target.

**40 of 56 operations built; 16 refused by name** (501 `NotImplemented`, `X-Doze-Not-Built`, before the body is read).

| Area | Operations |
|---|---|
| Events | PutEvents, TestEventPattern |
| Buses | CreateEventBus, UpdateEventBus, DeleteEventBus, DescribeEventBus, ListEventBuses, PutPermission, RemovePermission |
| Rules | PutRule, DeleteRule, DescribeRule, ListRules, EnableRule, DisableRule |
| Targets | PutTargets, RemoveTargets, ListTargetsByRule, ListRuleNamesByTarget |
| Tags | TagResource, UntagResource, ListTagsForResource (buses and rules) |
| Archives, replay | CreateArchive, DescribeArchive, ListArchives, UpdateArchive, DeleteArchive, StartReplay, DescribeReplay, ListReplays |
| Connections | CreateConnection, UpdateConnection, DeauthorizeConnection, DeleteConnection, DescribeConnection, ListConnections |
| API destinations | CreateApiDestination, UpdateApiDestination, DeleteApiDestination, DescribeApiDestination, ListApiDestinations |

**Refused by name (16):** the ten partner-event-source operations (`ActivateEventSource`, `DeactivateEventSource`, `DescribeEventSource`,
`ListEventSources`, `CreatePartnerEventSource`, `DeletePartnerEventSource`, `DescribePartnerEventSource`, `ListPartnerEventSources`,
`ListPartnerEventSourceAccounts`, `PutPartnerEvents`) and the five global-endpoint operations (`CreateEndpoint`, `DeleteEndpoint`,
`DescribeEndpoint`, `ListEndpoints`, `UpdateEndpoint`): STAY CUT in SCOPE.md, and Go refused them too. `CancelReplay`: SCOPE.md cuts
asynchronous or cancellable replays; a replay here has finished before `StartReplay` answers (it answers `STARTING`, as AWS does, and
`DescribeReplay` says `COMPLETED`), so there is never one to cancel. Go marked it S (a stub) as well.

What the operations do:

- **Patterns** (`pattern/`, own position-tracking JSON reader so refusals carry Jackson's `line`/`column`; fuzz target `fuzz_event_pattern`):
  exact strings, numbers, `true`, `false`, `null`; `prefix`, `suffix` (each also with `equals-ignore-case`), `equals-ignore-case`,
  `wildcard` (`*`, `\*`, `\\`; consecutive stars refused), `anything-but` (value, list, `prefix`, `suffix`, `equals-ignore-case`,
  `wildcard`), `numeric` (`=`, `>`, `>=`, `<`, `<=`, ranges), `exists`, `cidr` (v4 and v6), `$or` at any level; nested objects; an event array
  matches when any element does, including an array of objects. Nesting is capped, and the parser cannot overflow the stack.
- **PutEvents**: entries judged one by one (a bad one fails alone, the call succeeds); `Time`, `Resources`, bus by name or ARN; request size
  over 256 KB refused whole; the event is made in AWS's member order (`version`, `id`, `detail-type`, `source`, `account`, `time`, `region`,
  `resources`, `detail`).
- **Delivery** is in the call, through the peer bus (`aws_peers::Peers::call`, acting as `events`): SQS (with `SqsParameters.MessageGroupId`;
  the queue's policy is asked with principal `events.amazonaws.com`, `aws:SourceArn` = the rule, as SNS does, unless the target has a
  `RoleArn`), SNS (topic policy asked), Kinesis (`PutRecord`, partition key from `KinesisParameters.PartitionKeyPath` or the event id,
  documented), CloudWatch Logs (stream named for the rule), another event bus (same region by an in-process loop with a forward cap, other
  regions handed to a task so two regions never wait on each other), API destinations (below). Step Functions (`StartExecution`) and Lambda
  (`Invoke`, as CloudWatch does) are called over the bus **when those services are registered**; today the bus answers "no service" and the
  target is logged as not delivered. Every other target service (ECS, Batch, SSM, Firehose, ...) is stored, returned as sent and **not
  delivered**: the log says so. No delivery is ever faked.
- **Input**: `Input` (verbatim), `InputPath` (`$.a.b[0]['c']`), `InputTransformer` complete: `InputPathsMap`, `<name>` placeholders, the
  predefined `aws.events.rule-arn`, `rule-name`, `event.ingestion-time`, `event`, `event.json`; values go in raw inside a JSON string, as JSON
  elsewhere; a template of plain text takes text raw; undefined placeholders are refused at `PutTargets` in the failed entry.
- **Retry policy and dead-letter queue**: `RetryPolicy` (`MaximumRetryAttempts`, default 185; `MaximumEventAgeInSeconds`, default 86,400)
  and `DeadLetterConfig` round-trip and act. A failure that may pass (5xx, throttling, no answer) waits 1, 2, 4 ... 60 s and is tried again
  from the deadline queue; one that cannot (a queue that is not there, a policy that refuses) goes straight on. Out of tries or too old, the
  event goes to the dead-letter SQS queue with the documented attributes `RULE_ARN`, `TARGET_ARN`, `ERROR_CODE`, `ERROR_MESSAGE`,
  `EXHAUSTED_RETRY_CONDITION`, `RETRY_ATTEMPTS`; with no dead-letter queue it is logged and dropped. Waiting retries are in memory and a
  restart loses them (SCOPE.md: the 24-hour retry is "not needed", the difference is flagged).
- **Schedules** (`schedule.rs`, `timers.rs`): deadline-driven, no tick. After every call the actor says when anything is next due; one timer
  task per region sleeps to it and ends when nothing is pending (a rule with no target, a disabled rule, a pattern rule, a cron whose last
  year has passed have no deadline). A rate counts from the rule's creation; what a pause or restart missed is not replayed; a service that
  was reaped and reopened for the wake-up still fires the slot it was woken for; `resume()` re-arms regions found on disk at start.
  Scheduled rules only on the default bus (recorded).
- **Archives and replay**: an archive appends the events its pattern wants as rows; retention is enforced when an event is appended (at most
  hourly per archive); counts kept. A replay reads its window and puts each event back through the bus's rules (optionally only the rules in
  `FilterArns`) marked with `replay-name`; replayed events are not archived again.
- **Connections and API destinations**: stored; secrets (password, key value, client secret, any `IsValueSecret` parameter) are kept and never
  returned. An API destination target makes one HTTP request (5 s limit, inside the actor): endpoint `*` filled from
  `PathParameterValues`, target and connection header/query parameters (connection wins), Basic and API key credentials, connection body
  parameters joined to an object body, 2xx success, 429/5xx/no answer retried, other statuses not. **OAuth connections and `https://`
  endpoints are stored and not called** (SCOPE.md defers the OAuth token request; there is no TLS client).
- **Permissions**: `PutPermission` builds the bus policy (account principal or `*`, condition, or a whole `Policy`), `RemovePermission` takes
  statements away; `DescribeEventBus` returns `Policy`; the policy goes to the authorizer as `Need.policy`.
- **Default-bus publish hook** (`aws_eventbridge::publish::put`): one function other services call with a source, detail type, detail and
  resources; it is a `PutEvents` over the peer bus acting as the sender. **S3 is wired**: a bucket whose notification configuration has
  `<EventBridgeConfiguration/>` puts `Object Created` (PutObject, POST Object, CopyObject, CompleteMultipartUpload), `Object Deleted`
  (DeleteObject with `deletion-type`, Lifecycle Expiration), `Object Tags Added`, `Object Tags Deleted` on the default bus as `aws.s3`
  (`crates/aws-s3/src/eventbridge.rs`; test `a_bucket_that_asks_for_it_puts_its_events_on_the_default_bus`, S3 to EventBridge to SQS).
  Step Functions, CloudWatch, SSM and Secrets Manager call the same function when they have events.

## EventBridge Scheduler: not built

PLAN decision D2 makes Scheduler a fifth service. **`models/` has no Scheduler model** (the Scheduler API is restJson, namespace
`com.amazonaws.scheduler`) and the REST-JSON codec is being built for Lambda, so nothing was invented or half-built. To build it: add
`models/scheduler.json`, run codegen for `aws-model-scheduler`, and use `aws-cron` (which already evaluates `rate()` and `cron()`; `at()`
and time zones are the additions) and the target dispatch in `deliver.rs` (which is not tied to rules). Owner: please supply the model file.

## Parity with Go

Go dispatched 40 operations; **all 40 are built**, plus `PutPermission` and `RemovePermission` (Go refused them). Differences, all stated:

- **Bus deletion**: Go deleted a bus together with its rules; here a bus with rules is refused (`ValidationException`, wording from memory),
  from AWS's documentation ("All rules associated with this event bus need to be deleted first"). AWS is the judge; the wording is unrecorded.
- **API destinations**: Go made real HTTP calls with Basic, API key and OAuth. Here Basic and API key over `http://` are called; OAuth and
  `https://` are not (see above; SCOPE.md defers OAuth). Go retried once after a second; here the target's retry policy applies.
- **Retries**: Go had no dead-letter queue; here it exists. Go's schedule driver ticked every second; here there is no tick.
- **Lambda target**: Go delivered; here it waits for A13 (the call is already written against the bus).
- Go put Kinesis, event-bus and Step Functions targets nowhere; here SQS, SNS, Kinesis, Logs, buses, API destinations are delivered.
- `CancelReplay`: Go answered a stub error; here 501 by name.

## The scoreboard

`UV_BIN=... cargo xtask conformance --cmd "$PWD/target/release-aws/doze-aws --listen {addr} --data-dir {data} --account 111122223333" eventbridge eventbridge_local`

```
module           labels  exact  wording  behaviour  unverified
eventbridge          87     55        0          1          31
eventbridge_local      0      0        0          0           0
total: 87 labels, 63% exact, 63% ignoring wording
```

- **Recorded: 55 of 56 labels exact (98%).** Go on the same snapshots: 73% exact / 88% ignoring wording (56 labels).
- **The one that differs, `test_refusals::delete-default-bus`**: AWS answered `AccessDeniedException` because the recorder's policy does not
  allow `events:DeleteEventBus` on the default bus; doze-aws answers `ValidationException` "Cannot delete event bus default." (wording from
  memory). It cannot be matched until the label is re-recorded under a policy that allows it (owner item, like the SSM and SNS ones).
- **31 labels are UNVERIFIED**: five new scenarios added to `test_eventbridge.py` (schedule expressions valid and refused, target parameter
  round trip, bus policy, input settings reaching a queue: constant, path, JSON and text transformers). They pass against doze-aws through
  boto3 and compare with nothing until recorded. The recorder's policy must allow `events:*` on its prefix, `events:PutPermission`, and an
  SQS queue for the delivery ones.
- **`test_eventbridge_local.py`** (new, four tests, in no score, skipped against AWS): archive and replay, a bus as a target, dead letter,
  connection secrets with a destination that follows its connection.
- All other modules re-run at the end, unchanged: sqs 177 exact of 181, ssm 29, secretsmanager 27, kms 44, iam 36, sts 2, sns 38,
  dynamodb 53, s3 84, logs 28, kinesis 27, cloudwatch 28.

## What the recording taught

- A rule's `EventPattern` comes back **verbatim** (`{"source": ["x"]}` with its space), where SNS compacts a filter policy.
- `DescribeRule` of an event-pattern rule has `CreatedBy` (the account) and `EventBusName`; `ListRules` has neither `CreatedBy` nor
  `RoleArn`; `DescribeEventBus` of a custom bus has only `Arn`, `CreationTime`, `LastModifiedTime`, `Name`.
- Rule ARNs on a custom bus are `rule/<bus>/<rule>`. Deleting a bus that is not there is 200. Removing a target id that is not there is not
  a failure. A rule with targets cannot be deleted (`ValidationException`, "Rule can't be deleted since it has targets.").
- Pattern refusals are Jackson's, with a position: `Unexpected character ('n' (code 110)): was expecting double-quote to start field name`
  (column of the character) and `"source" must be an object or an array` (column one past the start of the value).
- A schedule on a custom bus is refused ("ScheduleExpression is supported only on the default event bus."); `rate(1 minutes)` and
  `rate(5 minute)` are both "Parameter ScheduleExpression is not valid." (one sentence for every refusal).
- `PutEvents` entry failures: `InvalidArgument` "Parameter Source is not valid. Reason: Source is a required argument." and `MalformedDetail`
  "Detail is malformed."; the call itself answers 200 with `FailedEntryCount`.
- The delivered event body: `version` "0", `id`, `account`, `region`, `resources`, `source`, `time`, `detail-type`, `detail`; the transformed
  body is the template with the values put in, byte for byte.

## (memory) items: wording or behaviour not checked against a recording

Marked `(memory)` in the code where they sit, or listed here:

- Every refusal not listed above: the default-bus delete refusal, `Statement already exists.` / `Statement with the provided id does not
  exist.`, the five-target limit's wording (the limit itself is AWS's default quota), `Total size of the entries in the request is over the
  limit.`, `Parameter Event is not valid.`, the archive and replay refusals, the connection and API destination refusals, the
  `FailedEntries` wording for `Input`/`InputPath`/`InputTransformer` mistakes, a bus delete with rules.
- That a bus missing from a `PutEvents` entry fails the whole call with `ResourceNotFoundException` (not an entry error).
- Which pattern reasons carry which column (only the two above are recorded), the words of every other pattern reason, and the
  array-with-`anything-but` rule (matches when any element qualifies), absent parents matching `exists: false`, null with `anything-but`.
- Transformer details beyond the recorded one: the quoting rules, the predefined names' exact values (`aws.events.event` as text),
  missing paths (empty inside a string, `null` outside), an `InputPath` that finds nothing (nothing is sent, logged).
- `PutRule` replaces description, state and role (targets and tags stay); an absent `State` is `ENABLED`; deleting a rule that is not there
  is 200; tags on buses and rules only.
- The retry backoff (1 s doubling to 60 s), which failures count as unretryable, the FIFO deduplication id (the event id), the Logs stream
  name (the rule's), the Kinesis default partition key (documented) and the dead-letter attribute names (documented).
- Connection states in answers (`CREATING`, `UPDATING`, `DEAUTHORIZING`, `DELETING`), the default invocation rate 300, the secret ARN
  shape, the S3 event field names and `reason` values (AWS documentation as remembered, no recording).
- `PutPermission` statement shape (account principal `arn:aws:iam::<id>:root`, `Resource` the bus ARN).
- Cross-account same-region buses are routed in-process **without evaluating the bus policy** (the policy is stored, returned and handed to
  the authorizer for `PutEvents`, but a cross-account delivery is not judged by it).
- `--iam enforce` was exercised through a fake authorizer in a unit test (what each request asks, a refusal does nothing), not end to end
  with an IAM user and boto3.

## Open decisions (for the owner)

1. **Scheduler**: supply `models/scheduler.json` (see above), then it is about the work D2 estimated (M) on top of `aws-cron` and `deliver.rs`.
2. **Bus deletion with rules**: refused (AWS documentation) where Go cascaded. Keep, or cascade for local convenience?
3. **TLS for API destinations** (and SNS HTTP/S): `https://` is not called. A TLS client is a size decision, the same one SNS left open.
4. **Re-record** `delete-default-bus` under a policy that allows it, then record the 31 new labels (`CONFORMANCE_TARGET=aws
   CONFORMANCE_RECORD=1`). Read each diff; where AWS contradicts the build, AWS wins.
5. Retries wait in memory; persisting them across a restart is small but SCOPE.md called the long retry "not needed".
6. Delivery to Lambda and Step Functions needs only the registration of those services (A13, A14); check the request shapes then.
7. Cross-account bus policy evaluation (see `(memory)`).

## Tests

86 in `aws-eventbridge` (patterns: the 17 recorded cases, every operator, the two recorded refusals to the character, stack safety;
transformer and `InputPath`; names and event layout; buses, rules, targets, tags, permissions with the recorded answers; PutEvents entry
rules and size; delivery to SQS, FIFO, SNS, Kinesis, Logs, buses and a self-loop; the queue policy under soft and enforce; retry and
dead-letter with a manual clock; schedules under a manual clock, a long pause, a reaped service, and the timer task under paused time
including its end; archives, retention, replay with a rule filter; connections without secrets; API destination requests against a local
server; persistence across closing the database; what each request asks the authorizer and a refusal doing nothing), 11 in `aws-cron`
(rates, every cron form, leap days, the nth and last weekday, `W`, a schedule that never comes), 1 added to `aws-s3` (S3 to EventBridge to
SQS), two fuzz targets (`fuzz_event_pattern`, `fuzz_schedule`; `fuzz-check` pairs them). Clippy is clean at the workspace lints.

## Weight (ready to paste)

2026-11-13, with EventBridge (A12): `doze-aws` is 5,719,744 B (release-aws, darwin/arm64) and 224 packages by the tool, 3,728 KB idle. That is
+347,872 B and +4 packages over the 5,371,872 B / 220 of the eleven-service build. New packages: `aws-eventbridge`, `aws-cron` and
`aws-model-eventbridge` (first party), and the tool counts one more as in earlier stages. **No third-party package was added**: `serde`,
`serde_json`, `tracing`, `tokio` (features `net` and `io-util` for the HTTP request were already linked), `aws-iam-policy` and `doze-*` were
all in the build. The package limit (224) is now met exactly; the byte limit has 1.08 MB of room. Idle memory is unchanged in kind: the
EventBridge actor, its tables, its compiled patterns and its timer exist only after the first request in a region, and a region with
nothing scheduled holds no timer. `xtask/weight-limits.txt` was not edited; the next stage that adds a package needs the limit moved.

## Outside the crate

New crates `crates/aws-cron` and `crates/aws-eventbridge`. `Cargo.lock` (two packages), `crates/doze-aws/Cargo.toml` and `src/run.rs`
(registered on the gateway and the bus; `resume()` after bind; one `use`), `crates/aws-s3/Cargo.toml` (dependency on `aws-eventbridge` for
`publish`), `crates/aws-s3/src/lib.rs`, `events.rs` (the hook, a doc paragraph), new `eventbridge.rs`, and `tests/notify.rs` (one test),
`conformance/tests/test_eventbridge.py` (five scenarios added; the recorded ones untouched) and `conformance/tests/test_eventbridge_local.py`
(new). Nothing in `aws-wire`, `aws-gateway`, `aws-schema` or `doze-store`.
