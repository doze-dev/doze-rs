# verify-c: dynamodb, eventbridge, scheduler, lambda against the 2026-11-15 recording

Branch `aws-vc` (from `aws` at `6ee5c7ca`). AWS is the judge: where a scenario guessed AWS wrong it was fixed and only that
scenario re-recorded (account 821712915674, profile `doze-rec-main`); where doze-aws answered differently from a recording,
doze-aws was fixed.

## Scores (labels / exact / wording / behaviour / unverified)

| module | before | after |
|---|---|---|
| dynamodb | 116 / 92 / 10 / 2 / 12 | 116 / 107 / 0 / 1 / 8 |
| eventbridge | 87 / 75 / 0 / 2 / 10 | 88 / 87 / 0 / 1 / 0 |
| scheduler | 27 / 7 / 0 / 2 / 18 | 31 / 30 / 0 / 1 / 0 |
| lambda | 58 / 53 / 0 / 3 / 2 | 67 / 66 / 0 / 1 / 0 |

Every remaining difference is a label whose recording is AWS refusing the *recorder* (AccessDenied: the permission list below), or
the one decided deviation (DynamoDB stream shards). None is doze-aws answering a developer wrongly.

## What AWS taught us (and what changed)

### DynamoDB (all doze-aws, all wording unless marked)
- `BatchWriteItem` with 26 requests: AWS no longer quotes the request in Java `toString` form (the 2026-11-09 recording did). Now
  `1 validation error detected: Value at 'RequestItems.<table>.member' failed to satisfy constraint: Member must have length less than
  or equal to 25`. **Go wrong (fixed)**: the Java printer is gone (`aws-dynamodb/src/invalid.rs`).
- `UpdateTable` with a stream already on: `Table already has an enabled stream: TableName: <t>` (not the stream ARN).
- `UpdateTable` with nothing to change: the longer list of members AWS names now (MultiAccountReplicaReady ... TableClass).
- Overlapping update paths (`SET m.k`, `m`): AWS says "overlap" for one path inside another; the Go edition's "conflict" was wrong.
- `Scan` with `Segment >= TotalSegments`: `The Segment parameter is zero-based and must be less than parameter TotalSegments` alone.
- Transaction token reused with other parameters: `Specified idempotent token was used with different request parameters within the
  idempotency window`.
- A set with a repeated member: `1 validation error detected: One or more parameter values were invalid: Input collection ["a", "a"]
  contains duplicates.` (members quoted, no type named).
- A number that is not one: `1 validation error detected: The parameter cannot be converted to a numeric value: 12abc`.
- PartiQL `SELEKT ...`: `Statement wasn't well formed, can't be processed: Expected data manipulation`.
- Streams `GetShardIterator` on an unknown shard: `Requested resource not found: Shard does not exist` (no id).
- A reserved tag key: `Tag Key cannot be prefixed with aws:, Key: aws:x`.
- **Behaviour**: a PartiQL write `ExecuteTransaction` answers `"Responses": []`, not an empty object. **Go weak (fixed)**.

Scenarios fixed (they guessed AWS wrong): `test_tags` (AWS paces tag changes on a table: an untag straight after a tag is
`LimitExceededException: Subscriber limit exceeded: Table tags are being updated`; the scenario now waits it out),
`test_index_added_and_removed_later` (a new GSI over three items stayed `CREATING` for more than 15 minutes and was `ACTIVE`
within about 50; the scenario now waits up to an hour, and its re-recording is the one still running when this note was written:
see the report), `test_streams_api` (reads every shard: a key's records are in one of several). `test_tags` also paces its
cleanup's `DeleteTable`: its table stayed behind after the recording.

### EventBridge
- `PutPermission` with a statement id already present is **accepted** and replaces the statement (the Go edition refused it:
  **Go wrong, fixed**). The `Principal` account id is kept as given (`{"AWS": "444455556666"}`), not turned into a root ARN (fixed).
- `PutTargets` with a bad input setting refuses the **whole call** (`ValidationException`, 400): `Only one of Input, InputPath, or
  InputTransformer must be provided for target <id>.` The Go edition put it in `FailedEntries` (**Go wrong, fixed**).
- An `InputTemplate` must be JSON once each placeholder is `null`; a text template is a JSON string with its quotes. `hello <who>` is
  `Invalid InputTemplate for target queue : [Source: (String)"hello null"; line: 1, column: 1].` (new check). `"hello <who>"`
  delivers `"hello ada"`.
- Scenario fixed: `test_input_settings_reach_the_queue` changed one target's settings between events, and AWS applied the *old*
  setting to the next three events (a target change takes time to reach delivery). Now one rule, queue and detail type per setting.

### Scheduler
- `rate(5 minute)` is **accepted** by Scheduler (EventBridge rules refuse it). Scheduler no longer holds a unit to its number;
  `rate(1 minutes)` recorded accepted too. `at(2030-02-30T00:00:00)` is accepted (a day is held to 1..31, not its month; doze reads
  it as March 2, `(memory)`).
- Not-found answers are **404** (the model's status): the restJson1 error writer now asks the model for a modelled error's status
  (`aws-wire/src/rest/jsonout.rs`; only Scheduler raises modelled errors on that path). **Go wrong (fixed)**.
- `GetScheduleGroup default` answers only `Name` and `State` (no ARN, no dates).
- Wording: `Invalid timezone X.`; an `EndDate` in the past with no `StartDate` is `The StartDate you specify must come before the
  EndDate.`; `MaximumWindowInMinutes must be provided when FlexibleTimeWindowMode is set to FLEXIBLE.`; a universal target with no
  `Input` is `Invalid RequestJson provided. Reason You must provide an input to set this target for your schedule.`, and with one, a
  read-only action is `Invalid RequestJson provided. Reason getQueueUrl API is not supported.`
- Scenario fixed: the CloudFormation stack asked `Fn::GetAtt Nightly.Arn` for a schedule in a custom group, and **AWS's own
  handler fails that** ("Unable to retrieve Arn attribute for AWS::Scheduler::Schedule ... was not found") and rolls the stack back.
  The scenario no longer asks for it. doze-aws answers the attribute; replicating AWS's failure is left for the owner to decide.

### Lambda
- **Function URLs with `AuthType NONE`** (the correctness stream's open item, built the AWS way): since October 2025 AWS needs two
  grants to `Principal "*"`: `lambda:InvokeFunctionUrl` (with `lambda:FunctionUrlAuthType NONE`) and `lambda:InvokeFunction` (with
  `lambda:InvokedViaFunctionUrl true`). Recorded: with the first alone AWS answers 403 `Forbidden. Invoking a Function URL requires
  both the lambda:InvokeFunctionUrl and lambda:InvokeFunction permissions. For troubleshooting, see: ...` for 20 s; with both, 201.
  doze-aws: `--iam enforce` answers that 403 (AWS's words, then a `doze-aws:` hint with the two `add-permission` commands);
  `--iam soft` logs the same and runs the function; `--iam off` runs it. `AddPermission --invoked-via-function-url` writes
  `"Condition": {"Bool": {"lambda:InvokedViaFunctionUrl": "true"}}` (recorded). **Go wrong (fixed)**: Go checked nothing.
- The URL event's body is base64 by **content type**, not by whether it is UTF-8: text for `text/*`, `application/json`,
  `application/xml`; base64 for `application/octet-stream`, `application/x-www-form-urlencoded`, `image/png` and no type (all
  recorded). **Go wrong (fixed)**.
- `CreateFunctionUrlConfig` without `InvokeMode` does not name one in its answer.
- `UpdateEventSourceMapping` that leaves the function alone is not held to the queue's visibility timeout again (AWS accepted
  `BatchSize` after the function's timeout had passed the queue's). **Go wrong (fixed)**.
- Scenarios fixed: an alias moved is eventually consistent on AWS (the old version ran straight after `UpdateAlias`; the scenario
  waits); an event source mapping is updated only once `Enabled` (AWS refused with `ResourceInUseException` while creating); reserved
  concurrency: the recording account's limit is 10, so AWS refuses any reservation there; the scenario now records the refusal of
  reserving the whole limit (the minimum in the words normalised) and round-trips 1 only where the limit allows.

## Decided deviation (owner to see)
- **DynamoDB Streams shards**: AWS gives a new on-demand table's stream **4 shards** (recorded), a key's records in one of them.
  doze-aws keeps **one shard** per stream: every reader sees all changes in order, and the Lambda poller, the Streams API and the
  stored sequence numbers are built on it. Matching AWS means hashing keys over N shards in `aws-dynamodb/src/stream.rs`, the
  Streams API and `aws-lambda/src/poll/streams.rs`: a stage of its own, not a verification fix. The `describe` label stays one
  behaviour difference until then.

## Permissions the recorder lacks (the integrator asks the owner)
Each made AWS refuse the recorder, so the label recorded an AccessDenied and differs from doze-aws until re-recorded:
- `events:DeleteEventBus` on `arn:aws:events:us-east-1:821712915674:event-bus/default` (`test_eventbridge::test_refusals::delete-default-bus`).
- `scheduler:DeleteScheduleGroup` on `arn:aws:scheduler:us-east-1:821712915674:schedule-group/default` (`test_scheduler::test_groups_and_tags::default-cannot-go`).
- `lambda:GetEventSourceMapping` on `*` (an absent UUID is asked about resource `*`) (`test_lambda::test_refusals::absent-mapping`).
- `iam:CreateServiceLinkedRole` for DynamoDB Contributor Insights (`contributorinsights.dynamodb.amazonaws.com`): without it AWS sets
  the table's insights to `FAILED` ("Not authorized to perform iam:CreateServiceLinkedRole", seen by a probe) and
  `test_dynamodb::test_contributor_insights_settings` cannot record.
All four are refusals AWS gives anyway (the default bus and group cannot be deleted; the mapping does not exist), so granting them
changes nothing in the account.

## Left in the account
Nothing of this group's. (Two tables of the dynamodb re-recording, `doze-rec-9a975dfe-1-table` (test_tags) and
`doze-rec-9a975dfe-2-table` (the index still building blocked its delete), were deleted by hand. A schedule `doze-rec-fdf57144-2-refused` that an accepted `at(2030-02-30...)` made before the scenario cleaned up after
itself was deleted by hand; a probe table `doze-rec-vc-cci-probe` was deleted.)
