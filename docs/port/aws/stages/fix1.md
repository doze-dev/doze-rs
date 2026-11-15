# Stage: AWS-faithful fixes (`fix1`)

Branch `aws-fix1`, from `78ba6190`. Five changes, each judged against AWS alone: EventBridge delivers after `PutEvents` answers;
CloudFormation no longer fails a deploy for a type of a service doze-aws does not run; Lambda loses the `_local_` code bucket and the
regionless function URL path, and a function URL with `AWS_IAM` checks its SigV4 signature; CloudWatch refuses alarm periods AWS
refuses; the remaining "Go edition" mentions in those crates are gone.

## 1. EventBridge `PutEvents` delivers asynchronously

**AWS.** `PutEvents` answers once its entries are accepted (each entry gets an `EventId` or an error); targets are invoked afterwards,
retried by the target's `RetryPolicy` (`MaximumRetryAttempts`, default 185; `MaximumEventAgeInSeconds`, default 86,400, both in
`models/eventbridge.json`) and then sent to its dead-letter queue. Before this stage doze-aws delivered inside the call.

**Now.**
- `PutEvents` (`events.rs`, `route.rs`) checks each entry as before, then writes the event to a new `pending` table and answers. The
  call's answer is held until that write is on disk, so an accepted event outlives a stop or a crash.
- The answer tells the region's timer that work is due **now** (`schedule.rs`: `next_due` includes `next_pending`). The timer task
  wakes at once, asks the actor for a `Tick`, and the actor runs every pending event through its bus in the order it was accepted
  (`deliver_pending`), then fires schedules and due retries. With nothing pending, no schedule and no retry, there is no deadline,
  no timer and no task: **no idle wake-ups**.
- **Retries are now on disk too** (`retries` table; `retry.rs`). Before, a waiting retry lived in memory and a restart lost it; now
  `resume()` (run at start) finds pending events and retries, delivers the pending ones at once, and waits for the retries' due times.
- **A race in the timer is fixed** (`timers.rs`). A call's deadline and a wake-up's deadline could arrive out of order (a call
  answered while a wake-up waited in the actor's queue), and the stale "nothing due" of the wake-up could end the timer with an event
  pending. Every answer now carries a process-wide `generation`; the timer keeps the deadline of the latest answer.
- Delivery is at least once: an event whose delivery ran but whose removal was not yet written (a crash mid-tick) is delivered
  again on the next start, as AWS's at-least-once delivery allows.

**Callers that relied on synchronous delivery.**
- S3 → EventBridge (`aws-s3/tests/notify.rs`): the test waits for the expected count (up to two seconds) instead of three quick reads.
- Scheduler (`aws-scheduler/src/deliver.rs`), Step Functions `events:putEvents` (`aws-sfn`), API Gateway's AWS integration
  (`aws-apigateway/src/plane/aws.rs`), Lambda destinations (`aws-lambda/src/async_invoke.rs`): all use only the `PutEvents` answer
  (accepted or not), which is unchanged. Their tests pass unchanged.
- CloudFormation custom resources go through Lambda or SNS, not EventBridge; nothing there waited on delivery.
- EventBridge's own tests: `World::put` now delivers explicitly (`World::deliver`, as the timer would); the tests that called
  `PutEvents` directly call `deliver()`. `a_matching_event_reaches_the_queue_as_the_event_aws_recorded` now also asserts the queue is
  empty and two events pending after the answer.
- Conformance `test_eventbridge_local.py`: waits for messages (`WaitTimeSeconds=5`) and for the archive count (`eventually`).

**New tests.** `persistence::an_accepted_event_and_a_waiting_retry_outlive_a_stop_and_are_delivered_after_it`,
`schedules::put_events_answers_first_and_the_timer_delivers_then_ends` (the real service and timer on paused time),
`timers::tests::a_deadline_from_an_older_answer_does_not_replace_a_newer_one`. boto3:
`test_eventbridge.py::test_put_events_answers_before_the_target_is_invoked` (UNVERIFIED until recorded).

**Open.** `StartReplay` still replays inside the call (SCOPE.md: replays finish as they start); on AWS a replay is asynchronous
(`STARTING` → `RUNNING` → `COMPLETED`). Cross-region forwarding is still a task (not on disk): a stop between accepting and forwarding
an event to another region loses it.

## 2. CloudFormation never fails a deploy for a service doze-aws does not run

**The owner:** "It should not fail. Should possibly just log unsupported and move on. Some of these services we may never support."

**Now** (`res/policy.rs`):
- **Every type doze-aws does not make is skipped by default**: EC2 (all of it, `Instance` included), RDS (`DBInstance`,
  `DBCluster` included), Cognito, ECS, AppSync, Firehose, SES, Pipes, ELBv2, and any family not listed anywhere. The resource reaches
  `CREATE_COMPLETE` with the status reason `Not created: doze-aws does not run <Family>. CloudFormation accepted the resource without
  making it; Ref and Fn::GetAtt give placeholders.` `Ref`/`GetAtt` give the stable placeholders skipped types already had
  (`res/skip.rs`: ARN-shaped, `*.invalid` hosts). A warning is logged once per type per deploy (`engine/forward.rs`):
  `cloudformation: stack <s>: <Type> (<Logical>) not created: doze-aws does not run <Family>; the deploy goes on`.
- Types of a served family that doze-aws does not make say so: `doze-aws runs SSM but not SSM documents`, `doze-aws runs Lambda but
  does not make AWS::Lambda::X yet`.
- **Strict mode** is the existing setting, inverted: `--cfn-unsupported skip|fail` (default `skip`; the old `default` value is
  gone). `fail` refuses every type that would be skipped before anything is made, naming each resource and the flag that turns it
  off. `--cfn-skip TYPE` skips a type even in strict mode; `--cfn-fail TYPE` refuses one in the default mode.
- **A near miss of a type doze-aws makes is refused, as AWS refuses an unknown type** (`Template format error: Unrecognized resource
  types: [AWS::Lambda::Funtion]`, AWS's words (memory), then doze-aws's `(AWS::Lambda::Funtion is probably AWS::Lambda::Function)`).
  Near: the same family (or one a letter away, families over two letters) and a type name one letter away (two for names of five
  letters or more), or equal but for case (`res/more.rs::near_miss`).

**The judgement asked for: a type in a family doze-aws runs that it cannot make.** Two cases, two answers:
- *A typo* (`AWS::Lambda::Funtion`, `AWS::Lamda::Function`, `AWS::SQS::queue`): **refused**, as AWS refuses it. Skipping it would
  deploy a stack without the function or queue the developer meant, and the failure would surface later (a `Ref` to a placeholder, a
  queue that is not there) far from its cause. AWS refuses it at validation, before anything is made, so refusing is both faithful
  and the faster feedback, and the refusal names the type it probably is.
- *A real type doze-aws has not built* (`AWS::SSM::Document`, `AWS::ApiGatewayV2::Model`): **skipped**, like any type of a service
  doze-aws does not run. It is not a mistake of the developer's; failing would block a template AWS deploys, which is what the owner
  ruled out. The status reason and the log name it, so the gap is visible.
- Limit: a typo that is not near a type doze-aws makes (a family nobody has heard of, `AWS::Nope::Thing`) is skipped, because without
  the full CloudFormation registry a real but unlisted family cannot be told from a misspelt one; the log line names it.

Unchanged and AWS-like: a type outside `AWS::`/`Alexa::`/`Custom::` or not of three parts is `Unrecognized resource types`;
`AWS::Serverless::*` without the transform is refused.

**Open.** A mapped type whose service is switched off in this process (`--services` without `states`) still fails at create with
`no service states is running`; it is a deliberate choice of the person running doze-aws, and the message says what is missing. The
help text is updated (`keys.rs`); `stages/a16-cloudformation.md` carries a note that its FAIL list is superseded.

## 3. Lambda: `_local_`, the regionless URL path, and `AWS_IAM` function URLs

**`_local_` is gone** (`code.rs`, `updates.rs`, `lib.rs`, `invoke.rs`, `aws-cloudformation/src/transform/sam.rs`). AWS reads code from
an inline zip, an S3 object or an image; a bucket named `_local_` is now an S3 bucket like any other. The in-place machinery went
with it (the tree fingerprint, the per-invoke stamp that retired processes when the directory changed, the copy a published
version froze). The `Code.local` member stays in the record, never set, so a data directory written before 2026-11-15 still reads;
such a function still runs from its path. SAM: a local `CodeUri`/`ContentUri`/`DefinitionUri` is refused as the SAM translator
refuses it (`'CodeUri' is not a valid S3 Uri of the form 's3://bucket/key' ...`, memory), with doze-aws's hint to upload first
(`sam deploy`, `sam package`, `aws cloudformation package` against doze-aws).

**Function URL addresses** (`web.rs`). AWS's form, `<id>.lambda-url.<region>.<host>`, is kept. The regionless path
`/_aws/lambda-url/<id>/` is removed (404). `/_aws/lambda-url/<region>/<id>/<path>` stays, documented as **doze's local address** for
clients that cannot set a host or resolve `*.localhost`; it is not an AWS address.

**`AuthType` `AWS_IAM` verifies SigV4.** Before, a request with any `Authorization` header was let through unchecked (and an unsigned
one only refused under `--iam enforce`).
- The gateway checks the signature of a request to an address a service owns when the service names a signing name
  (`Web::signing_name`, new; Lambda answers `lambda` for URLs). The result reaches the service as `WebRequest::signature`
  (`WebSignature::Unsigned | Signed{..} | Refused(words)`), the same check `begin`/`finish` run for API calls
  (`aws-gateway/src/websig.rs`).
- **`--auth verify`**: every key is checked against the configured key and the keys IAM issued.
- **`--auth trust`** (the judgement): AWS always checks the signature; locally, under trust, the gateway takes every API call's key on
  trust because it does not know the secret. For function URLs it now checks whenever it does know the secret: the configured
  `--access-key`/`--secret-key` (default `test`/`test`, which is what SDKs pointed at doze-aws use) and any key IAM issued. So a
  request signed with the wrong secret is refused in AWS's words, as it would be on AWS. A key whose secret nothing here knows is
  taken on trust, consistent with every other call under trust, once its scope (`lambda`), its time (5 minutes' skew) and its form
  are checked. New gateway setting `Config::known_keys`, set by `doze-aws` from the configured key.
- Lambda (`web.rs::signature_refusal`): unsigned → `403 {"Message":"Forbidden"}` (memory) whatever the IAM mode, since this is authentication, not authorization; refused → `403` with the gateway's words
  (`The request signature we calculated does not match ...`, AWS's for the JSON services); scoped to another region → `403
  Credential should be scoped to a valid region.` (memory). Then the signer (not the anonymous principal) is asked by IAM for
  `lambda:InvokeFunctionUrl`: enforced under `--iam enforce`, logged under soft.

**Tests.** `aws-gateway/tests/websig.rs` (both modes over a socket: unsigned, right secret, wrong secret, unknown key, wrong service),
`aws-lambda/src/tests/url_auth.rs`, `functions::a_bucket_named_local_is_an_s3_bucket_like_any_other_and_a_zip_layer_is_kept`,
`sam::tests::code_is_an_s3_location_and_a_local_path_is_refused_with_how_to_upload_it`. boto3:
`test_lambda.py::test_function_url_with_iam_auth_refuses_a_bad_signature` (signed with the client's own credentials runs; unsigned and
wrong-secret are 403; UNVERIFIED until recorded).

**Removed test.** `tests/inplace.rs` (code run in place picked up on change): the feature is gone.

## 4. CloudWatch alarm periods

`models/cloudwatch.json`, `PutMetricAlarm` `Period`: "Valid values are 10, 20, 30, and any multiple of 60", and "`Period` multiplied
by `EvaluationPeriods` can't be more than 604,800 seconds. For alarms with a period of less than one hour (3,600 seconds), the total
evaluation period can't be longer than one day (86,400 seconds)." Both are now refused when the alarm is put (`period.rs`), for a
metric alarm's `Period` and for each `MetricStat.Period` of a metric math alarm. The error is `ValidationError`, as the recorded
`PutMetricAlarm` refusals are; the words are **(memory)**: `The parameter Period must be 10, 20, 30 or a multiple of 60 (it is 5).`
and `Period multiplied by EvaluationPeriods must not be more than 86400 seconds (it is 86460).` `eval.rs` no longer says doze accepts
sub-ten-second periods.

The real-time timer tests used one-second periods; they now use ten-second periods on a clock the test moves forward to just before a
window's end (`tests/timer.rs`, `Shifted`), about 11 s each. boto3: `test_cloudwatch.py::test_alarm_periods_aws_refuses` (5 s, 45 s,
a window over a day refused; 10 s accepted; UNVERIFIED until recorded).

## 5. "Go" mentions removed (aws-eventbridge, aws-cloudformation, aws-lambda, aws-cloudwatch)

| Where | What it said | Judged against AWS |
|---|---|---|
| `aws-eventbridge/src/route.rs` | delivery in the call, "as it did in the Go edition" | Wrong for AWS; changed (item 1) |
| `aws-eventbridge/src/deliver_http.rs` | the API destination request made off the actor "as the Go edition made it" | An implementation choice with no AWS-visible effect beyond AWS's own (asynchronous delivery, 5 s client timeout, documented); kept |
| `aws-eventbridge/src/input.rs` | a bad target refuses the whole `PutTargets`, "not a FailedEntries entry, as the Go edition had it" | Recorded 2026-11-15; kept |
| `aws-eventbridge/src/perms.rs` | the same `StatementId` again replaces the statement ("the Go edition refused it") | Recorded 2026-11-15; kept |
| `aws-cloudformation/src/lib.rs`, `res/naming.rs` | `logical_names`: name unnamed resources after their logical id "as the Go edition did" | AWS names them `<stack>-<logical>-<random>`, which is the default; the switch is an opt-in doze-aws convenience, now described as one |
| `aws-cloudformation/src/engine/forward.rs` | `Plan::Go` (a variant meaning "go ahead", not the language) | Renamed `Plan::Start` so no "Go" remains |
| `aws-lambda/src/code.rs` | the `_local_` layer zip "the Go edition's rule" | Not AWS; removed (item 3) |
| `aws-lambda/src/web.rs` (3 places), `tests/behaviour.rs` | the regionless `/_aws/lambda-url/<id>/` "Go edition" path | Not AWS; removed (item 3) |
| `aws-lambda/src/mappings.rs` | an update that keeps the function is not checked against the queue's visibility timeout ("the Go edition refused it") | Recorded 2026-11-15; kept |
| `aws-lambda/src/rt/check.rs` | the host-runtime check "is the Go edition's CheckRuntime" | AWS accepts the function (it cannot know the host); the check is a log warning and the first invoke fails with `Runtime.LaunchError`; kept, described on its own terms |
| `aws-lambda/src/url_event.rs` | which bodies reach the function as base64 ("the Go edition encoded only non-UTF-8") | Recorded 2026-11-15; kept |
| `aws-cloudwatch` | none found | — |

## Gates and numbers

See the report for the gates exit, conformance rows and weight of the final commit.
