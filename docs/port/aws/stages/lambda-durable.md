# lambda-durable: Lambda durable functions (stage note, 2026-11-19)

Branch `aws-lambda-durable`, on `origin/main` at `f864c8dd9`. The integrator folds this note into `PLAN.md`, `STATUS.md` and
`scoreboard.md`; none of those was touched here. AWS is the only reference. Nothing here is recorded (the recorder SSO session had
expired): each behaviour cites the model (`models/lambda.json`), AWS's developer guide, or AWS's own local stand-in for the
service, the checkpoint server in `@aws/durable-execution-sdk-js-testing` 1.3.2 (`dist/checkpoint-server`), whose validators say
they were measured against the service. Wording from none of those is marked `(memory)` in the code.

## 0. What a durable function is, as built

A function created with a `DurableConfig` runs as a durable execution: its SDK (`@aws/durable-execution-sdk-js`,
`aws-durable-execution-sdk-python`) records each step, wait, callback, child context and chained invoke with
`CheckpointDurableExecution`; when nothing is left but waiting it answers `{"Status": "PENDING"}`, and Lambda invokes it again when a
wait ends, a step retry is due, a callback is answered or a chained invoke returns. The invocation payload is the SDK's
`DurableExecutionInvocationInput`: `DurableExecutionArn`, `CheckpointToken`, `InitialExecutionState.Operations` (times as ISO-8601
strings, as the SDK's `normalizeOperation` says the event carries them) and `UpdatedOperationIds`.

**AWS's JavaScript SDK runs unchanged** (`crates/aws-lambda/src/tests/durable_js.rs`, run here with SDK 3.0.0 on Node 22.23):
`step`, `map`, `runInChildContext`, `wait`, a step that fails once and is retried, `invoke` of another function, and
`waitForCallback` answered with `SendDurableExecutionCallbackSuccess`. The test skips unless `DOZE_DURABLE_SDK_JS` names a
`node_modules` holding the SDK (and Node 22+ is `node` or `DOZE_LAMBDA_NODE`): the SDK is not vendored. A stdlib-only Python
function that speaks the protocol by hand (`tests/durable.rs`) covers the same path in every CI run.

## 1. Built (`crates/aws-lambda/src/durable/`)

- **Configuration** (`config.rs`): `DurableConfig` is settable on `CreateFunction`, shown with `RetentionPeriodInDays` defaulting
  to 14 (guide). No `ExecutionTimeout` is `InvalidParameterValueException` (guide; words memory). Turning durability on with
  `UpdateFunctionConfiguration` is refused (guide: "You cannot enable it on existing functions"; words memory).
- **Invoke** (`run.rs`): a durable function must be invoked qualified (guide, "Qualified ARNs requirement"; words memory); the
  execution is pinned to the version resolved at start (an alias resolves then; `$LATEST` resumes on the latest code, as the guide
  warns). `Event` answers 202 with `DurableExecutionArn` at once; `RequestResponse` waits for the execution to end, at most
  min(900 s, function timeout, execution timeout) (guide), and answers its result as the payload, or its error as a function error
  document (memory), or a timeout document if it has not ended (memory). `DurableExecutionName` is idempotent: same name and payload
  is the same execution (running or closed), another payload is 409 `DurableExecutionAlreadyStartedException` (guide's table; unique
  per function as the exception's model documentation says, where the guide says per account and region). Durable functions skip
  destinations (guide: they "don't support Lambda destinations").
- **The SDK's two operations** (`ops.rs`, `check.rs`, `apply.rs`): `CheckpointDurableExecution` takes the current token only (a
  stale one is `InvalidParameterValueException` "Invalid checkpoint token", the checkpoint server's words), checks every update
  before applying any (one `EXECUTION` update, last; a parent must be a `CONTEXT`; no operation twice in one request except a
  step or context started and closed; no change of type, subtype, name or parent; payload limits 256 KB / 1 MB / 6 MB; the actions
  each type allows; the status transitions), applies them, and answers a new token and every operation changed since the SDK last
  looked (the ones it just wrote included, which is how the SDK learns a callback's id). After a terminal `EXECUTION` update no
  token is given (the SDK logs that case). `GetDurableExecutionState` pages the operations, without the children of a context that
  completed without `ReplayChildren` (checkpoint server: "The service omits every descendant of a completed CONTEXT ...").
- **Callbacks**: `SendDurableExecutionCallbackSuccess` (the blob result, as a string), `...Failure` (the `ErrorObject`), and
  `...Heartbeat` (moves the heartbeat deadline). A closed callback is `CallbackTimeoutException` with the model's documentation
  sentence; an id never issued is `ResourceNotFoundException` (memory). Timeouts and heartbeat timeouts time the callback out with
  the checkpoint server's words ("Callback timed out", "Callback timed out on heartbeat").
- **Reading and stopping**: `GetDurableExecution` (with `IncludeExecutionData`), `GetDurableExecutionHistory` (paged, `ReverseOrder`,
  `IncludeExecutionData` removing `Input`/`Result`/`Error` from details: which members AWS removes is memory),
  `ListDurableExecutionsByFunction` (filters by qualifier, name, statuses, start time; newest first unless `ReverseOrder`, as the
  model says), `StopDurableExecution` (an execution already closed is `InvalidParameterValueException`, memory).
- **History events** per the model's `Event`: `ExecutionStarted/Succeeded/Failed/TimedOut/Stopped`, `Step*`, `Context*`, `Wait*`,
  `Callback*`, `ChainedInvoke*`, `InvocationCompleted` (start, end, request id, error). `RetryDetails` as the checkpoint server writes
  them.
- **Re-invocation and timers** (`timers.rs`, `life.rs`): one invocation at a time per execution. A wait's end, a step's next attempt
  (`PENDING` → `READY`), a callback's deadlines, the execution timeout and a closed execution's retention are one
  `doze_runtime::Deadlines` task per region, asleep until the soonest and gone when nothing is due (no idle timers). What changed
  while an invocation ran is seen by its next checkpoint, or makes another invocation follow it. `PENDING` with nothing to wait for
  fails the execution with the words of AWS's local test runner ("Cannot return PENDING status with no pending operations."). An
  invocation that fails without the SDK's answer (crash, timeout, no `Status`) is tried three times, one and two seconds apart, then
  the execution fails with its error (memory: AWS retries; its policy is not recorded).
- **Chained invokes**: a non-durable target is invoked once and its payload (or its error document as an `ErrorObject`) completes the
  operation; a durable target runs as an execution of its own, waited for to its end.
- **Wait scaling** (not AWS): `DOZE_LAMBDA_DURABLE_WAIT_SCALE` multiplies every wait and step retry delay, as
  `DOZE_SFN_WAIT_SCALE` does for Step Functions. The history still says the duration asked for; `ScheduledEndTimestamp` is the
  real, scaled end.
- **IAM**: the actions' resource is the execution ARN (the callback id names it) (memory). `AWSLambdaBasicDurableExecutionRolePolicy`
  is now a managed policy (`crates/aws-iam/src/awsmanaged.rs`): logs plus the two SDK actions (memory).

## 2. Storage

One record per execution in `lambda.redb`, table `durable`, key `<function>/<id>` (operations and history inside, as JSON text,
postcard outside). A closed execution is removed `RetentionPeriodInDays` after it ended. `doze-aws data` counts a function's
executions in its bytes with a note ("in N durable executions (M running)"); `--before` on a function removes its closed executions
that ended before then; `reset` removes them all. Deleting a function deletes its executions (memory). A restart starts again what was
running (from its last checkpoint) and remakes chained invokes that were under way (at least once). Weight: one more `Records`
table loaded with the region's actor, one empty `Deadlines` map in `Inner`; nothing runs while no execution waits. Not measured
(`cargo xtask weight` reads `./target`).

## 3. Scenarios (UNVERIFIED, 21 labels)

`conformance/tests/test_lambda_durable.py`: a wait and a callback (configuration, unqualified invoke refused, `Event` with a name,
idempotent name, other payload refused, running, heartbeat, success, success again refused, history, history newest first without
data, list, state with a made-up token); a stopped execution; durability chosen at creation. Passes against doze-aws (3 tests).
The function speaks the protocol itself: with the runtime's boto3 on AWS, with the standard library under doze-aws (which sets
`AWS_ENDPOINT_URL`). Not recorded: the SSO session had expired. To record: python3.13 functions, a wait of 2 s and a callback in each of two executions;
everything is deleted by the scenario's cleanup.

## 4. Open

- Everything marked (memory) above waits for a recording: the unqualified-invoke, timeout and not-found words; what a synchronous
  invoke answers for a failed or unfinished execution; the retry policy for failed invocations.
- `InvokeWithResponseStream` of a durable function runs it as a plain function (AWS's answer not known).
- `ChainedInvokeStartedDetails` does not carry the child's `DurableExecutionArn` or `ExecutedVersion`.
- The invocation payload carries every operation (no `NextMarker` paging of a huge state).
- `StopDurableExecution` leaves the execution's open operations as they were (the model: "Any in-progress operations are terminated").
- The Python SDK (`aws-durable-execution-sdk-python`) is not exercised.
