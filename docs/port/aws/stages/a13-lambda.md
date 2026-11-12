# A13: Lambda and the REST-JSON codec

Branch `aws-a13-lambda`, 2026-11-13, one agent in its own worktree on top of `0b6426c4`. One new crate (`aws-lambda`, about 10,800 lines with its
tests), the REST-JSON codec in `aws-wire` and a gateway branch for it, a few additive hooks in shared crates, and Lambda registered on the gateway
and on the peer bus. This note is for the integrator: what was built, what is refused, the exact scoreboard, what the recording taught, what is
from memory, what is open. `PLAN.md`, `STATUS.md`, `scoreboard.md` and `weight.md` are not edited here.

## Summary

| | |
|---|---|
| Recorded labels exact | **52 of 55** (95%); the Go edition scores 22 of 55 (40%) on the same recording (`baseline.md`). The three that differ are not behaviour doze can have (below) |
| Labels in scenarios written ahead of a recording | 3, UNVERIFIED (layers/tags/settings, response streaming, a function URL over HTTP) |
| Operations | 88 in the model: **70 built, 18 refused by name** (501, `X-Doze-Not-Built`) |
| Tests | 44 in `aws-lambda` (real processes behind the real Runtime API; the real SQS, Kinesis, DynamoDB, Logs, IAM and STS on the bus), 6 new in `aws-wire`, the existing gateway tests |
| Weight (`cargo xtask weight doze-aws`) | 6,102,912 B, 229 packages, 3,872 KB idle (base was 5,371,872 B, 220, 3.6 MB); **no new third-party package** |

## The scoreboard

```
UV_BIN=$(ls -d ~/.proto/tools/uv/*/uv | head -1) cargo xtask conformance --cmd "$PWD/target/release-aws/doze-aws --listen {addr} --data-dir {data} --account 111122223333" lambda
module           labels  exact  wording  behaviour  unverified
lambda               58     52        0          3           3
total: 58 labels, 89% exact, 89% ignoring wording
```

Identical with `--auth verify --access-key test --secret-key test` (every SigV4-signed boto3 request, including the zip bodies, verifies). The other
twelve modules, run once on the same binary: sqs 177 exact of 181, ssm 29, secretsmanager 27, kms 44, iam 36, sts 2, sns 38, dynamodb 53, s3 84,
logs 28, kinesis 27, cloudwatch 28: unchanged.

**The three labels that differ, each read against the recording; none is in `deviations.py`:**

1. `test_versions_and_aliases::invoke-alias-repointed`: AWS invoked version 1 right after `update_alias` moved the alias to 2 (the recording shows
   `ExecutedVersion: "1"` and the old payload). That is AWS's propagation delay; doze moves the alias at once. Matching it would mean answering with
   the old version for some seconds, which is a made-up delay, not behaviour.
2. `test_refusals::absent-mapping`: AWS answered `AccessDeniedException` for `GetEventSourceMapping` of an absent UUID because the **recorder's
   policy** lacks `lambda:GetEventSourceMapping` (the message names the SSO role). doze answers `ResourceNotFoundException` (404, "The resource you
   requested does not exist.", `(memory)`). The recorder policy needs `lambda:GetEventSourceMapping` for this label to say anything about Lambda.
3. `test_a_trigger_whose_queue_hides_for_less_than_the_function_runs::update-after-the-timeout-was-raised`: AWS refused the update with
   `ResourceInUseException` ("Cannot update the event source mapping because it is in use.") because the mapping was still `Creating`. doze's
   mapping is `Enabled` at once (a `Get` shows it; the answer to the create says `Creating`), so the update is accepted.

(`test_a_deleted_functions_mapping::the-mapping` and `bad-runtime` matched after fixes: see "What the recording taught".)

## The REST-JSON codec (shared)

`aws_wire::rest::jsonin` and `jsonout`, modelled on the REST-XML pair and sharing their binding code (`request.rs` and `response.rs` expose
`query`, `header`, `prefixed`, `scalar`):

- **In** (`jsonin::to_json`): path labels, query, headers and header prefixes come from where the model's `@http` binds them; the members with no
  binding are the members of the JSON body, **including ones the model does not know** (the decoder skips them; Lambda's `Command` extension reads
  its from the request); a payload member takes the body whole (a blob as base64 for the decoder, a string as text, a structure as the object).
  A body that is not a JSON object where the model wants one is `Violation::Syntax`, which the service words (`InvalidRequestContentException`).
  Nesting is capped by `serde_json` (128). Fuzz target `fuzz_restjson` (two seeds; compile-checked, not run: no nightly), marked
  `// fuzz:parser fuzz_restjson` for `cargo xtask fuzz-check`. The fuzz crate's `Cargo.toml` had a damaged `[[bin]]` list after the A11 merge
  (`fuzz_xml` and `fuzz_cbor` entries run together); it is repaired.
- **Out** (`jsonout::render`): header-bound members, prefix maps, the response-code member, the payload (raw bytes for a blob, text, JSON, or
  a stream for an event stream), and the other members as a JSON object; 204 and 304 have no body. **Errors** (`jsonout::error`): the code in
  `x-amzn-ErrorType`; body `{"Type":"User","Message":...}` for a service's error (`"Service"` for a 5xx), `{"message":...}` for the ones the front
  end raises (`ValidationException`, `AccessDeniedException`, ...). The recording shows code, message and status only; **the body shape is
  `(memory)`**.
- **Routing** is the existing `rest::route::Router` (the model's `@http` bindings). Test: 22 Lambda routes, including `?find=LayerVersion`
  against `ListLayers`, a percent-encoded ARN in a path, and the trailing slash of `/event-source-mappings/`.
- **The gateway branch** (`aws-gateway/src/rest.rs`): `respond` now serves a service that speaks `RestJson1` as well as `RestXml`; the
  differences are in a handful of places (`is_json`, `family`, the input read, the answer written, `x-amzn-RequestId` instead of S3's headers,
  authentication failures worded as the Coral family). The S3 virtual-host code moved to `s3host.rs` so `rest.rs` stays under the size limit.
  An unrouted request is 404 `UnknownOperationException` (`(memory)`).
- **`Service::web()` and `aws_wire::service::Web`** (new, additive): a service can own addresses that are not signed API calls; the gateway asks
  before it looks for a signature (`aws-gateway/src/web.rs`). Lambda uses it for function URLs and for code downloads. API Gateway can use the
  same for stage URLs.

## Lambda (`crates/aws-lambda`)

**Shape.** Management operations run in a state machine behind a lazy actor per region (`Keyed<Core>`, home `us-east-1`; other regions in
`regions/<region>`; functions, layers, mappings and code signing configurations are kept apart per region). Records are in one redb file
(`lambda.redb`): a function is one record holding its `$LATEST`, its versions, aliases, URLs, policies, async settings, tags; its configuration is
the JSON the caller set, merged on update and rendered with its defaults, so a member the API adds needs no new field. Code is files: `code/<id>/code.zip`
plus the unpacked `root/`, never changed after it is written, shared by `$LATEST` and the versions that have the same code, swept when nothing
refers to it. Running a function does **not** happen in the actor (it takes seconds): `Ask::Resolve` answers what a runner needs and the
runner works beside it.

**Built (70):** `CreateFunction`, `GetFunction`, `GetFunctionConfiguration`, `ListFunctions` (with `FunctionVersion=ALL`), `UpdateFunctionCode`,
`UpdateFunctionConfiguration`, `DeleteFunction` (a version too), `PublishVersion`, `ListVersionsByFunction`; aliases (5, with weighted routing
applied); `Invoke` (RequestResponse, Event, DryRun; `LogType: Tail`; `ClientContext`) and `InvokeWithResponseStream`; layers (publish, get, by ARN,
list, versions, delete, three permission operations); function URLs (5); `AddPermission`, `RemovePermission`, `GetPolicy`, and the whole-document
`PutResourcePolicy`/`GetResourcePolicy`/`DeleteResourcePolicy`; event source mappings (5); reserved concurrency (3); event invoke configs (5);
tags (3); `GetAccountSettings`; and the **accept bundle** (stored as sent, returned as sent, not acted on): provisioned concurrency (4; reported
`READY` with a `StatusReason` that says nothing is pre-warmed), code signing configurations (9), runtime management (2), recursion (2).
**Refused by name (18):** the six capacity-provider operations, `Put`/`GetFunctionScalingConfig` (Managed Instances), the nine durable-execution
operations, and `InvokeAsync` (deprecated).

**The runner** (`rt/`). A function is a host process in its own process group, started with the environment AWS gives it (the
`AWS_LAMBDA_*` set, `_HANDLER`, `LAMBDA_TASK_ROOT`, `LAMBDA_RUNTIME_DIR`, region, `TZ=:UTC`, the function's own variables on top, and
`AWS_ENDPOINT_URL` pointing back at the gateway; the host's own `AWS_*` variables are dropped). Each process gets a Runtime API of its own on a
loopback port (`rt/api.rs`: `next`, `response` (buffered or `Lambda-Runtime-Function-Response-Mode: streaming`), `error`, `init/error`), its stdout
and stderr are read line by line, and it is ended cleanly: on a timeout, when its function's code or configuration is replaced (a busy one finishes
its invocation first), after the idle time (`--lambda-idle-secs`, 600; one timer per warm process, none when there are none), and at shutdown (every
task is tracked in `Tasks`; **the child's group is killed through `kill(1)`** because signalling a group needs `libc`, and a test proves
`Tasks::is_empty()` after shutdown with a function whose grandchild holds the pipe open). Up to 8 processes serve one function version at once;
more invocations wait. Reserved concurrency is enforced (0 throttles everything: 429 `TooManyRequestsException`); the init has its own 10 s budget
that is not charged to the function's timeout. A timeout kills the process and answers `{"errorMessage":"<time> <id> Task timed out after 3.00 seconds"}`
as a 200 with `X-Amz-Function-Error: Unhandled`; a process that exits answers `Runtime.ExitError`; one that cannot start answers
`Runtime.LaunchError` (a 200, as a broken init is on AWS).

**Runtimes.** `provided*`: `bootstrap` from the code (or the code itself, if it is one file). `python*`, `nodejs*`, `ruby*`: the interpreter on `PATH`
(or `DOZE_LAMBDA_PYTHON`, `_NODE`, `_RUBY`, `_JAVA`, `_DOTNET`) runs one of **three embedded Runtime API clients** (`src/shims/bootstrap.py`, `.mjs`,
`.rb`), written under `<data>/shims` when first needed. **These are the Go edition's own shims, copied unchanged** (the author's code; there is
no other way to run those runtimes on a host without `awslambdaric`); they are the only Python, JavaScript and Ruby in the product.
`java*` (`java -cp <root>:<root>/lib/* ...AWSLambda`) and `dotnet*` (`dotnet <assembly>.dll`): the launch lines are built and **not exercised**
(no JDK or .NET on the test machine). Anything else: `Command` (a doze extension, read from the request body, replacing the launch line).
Layers go on `PYTHONPATH`, `NODE_PATH`, `RUBYLIB`, `GEM_PATH`, `PATH`, `LD_LIBRARY_PATH`, `DYLD_LIBRARY_PATH` in order (tested for Python);
`LAMBDA_LAYERS_DIRS` names them. A layer ARN of another account, or one that is not kept here, is accepted and the function runs without it with a
warning in the log. **Memory:** `MemorySize` is the environment variable and the number in the report; **no limit is enforced** (a host process
cannot be held to one portably). **Code in place:** `Code: {S3Bucket: "_local_", S3Key: "/abs/path"}` runs the directory (or a single
binary); a change to the tree (looked at again at most once a second) starts fresh processes, and a published version is a copy.

**Logs.** After each invocation `START`, the lines the function printed, `END` and `REPORT` (Duration, Billed Duration, Memory Size, Init Duration on
a cold start; **no `Max Memory Used`**, which nothing measures) are written to `/aws/lambda/<name>` (or the function's `LoggingConfig.LogGroup`) in a
stream named `YYYY/MM/DD/[<version>]<32 hex>` over the peer bus (group and stream made on first write, again if deleted under a warm process);
`LogFormat: JSON` writes the `platform.start`/`platform.report` records instead. Written at the end of an invocation, not line by line.
`LogType: Tail` returns the last 4 KB. Lines are echoed to doze-aws's own log unless `--lambda-quiet`. The `AWS/Lambda` CloudWatch metrics are **not
produced** (below).

**Run as the function's role** (`role.rs`). At create/update the role must exist and its trust policy must allow `lambda.amazonaws.com`
(`The role defined for the function cannot be assumed by Lambda.`); a role IAM has never heard of is **refused only when IAM enforces**, because a
local account is often made without the roles a template names (open decision). The process holds an STS session for the role, made with
`AssumeRole` over the bus (Lambda acts as itself, so the trust policy is not asked), kept until ten minutes before it ends and replaced for the
next process (a process with under five minutes left is not given another invocation). IAM resolves that session like any other: a test creates a
role allowed `sqs:SendMessage` only, and asks the authorizer as the function's own key: `SendMessage` passes and `ReceiveMessage` is refused in
AWS's words. **No change to `aws-iam` was needed**: STS `AssumeRole` over the bus is the API, and the existing session token is what IAM resolves. `iam:PassRole`
(with `iam:PassedToService=lambda.amazonaws.com`) is a need on `CreateFunction` and on `UpdateFunctionConfiguration` that names a role. A mapping's
execution role is asked for the source's read permissions when the mapping is made or changed (SQS: `The provided execution role does not have
permissions to call ReceiveMessage on SQS`; refused under `enforce`, logged under `soft`).

**Authorization** (`authz.rs`, `resolve.rs`). Actions are `lambda:<Operation>` (`lambda:InvokeFunction` for `Invoke`, `lambda:InvokeFunctionUrl` for a URL),
resources are function, layer, mapping and code-signing ARNs, the function's own policy for the ARN form the caller named is `Need.policy`.
**A service that invokes a function** (S3, SNS, EventBridge, CloudWatch alarms) calls `peers.call("<service>", caller, "lambda", "Invoke", body)`;
it is let in only if the function's policy has a statement for `<service>.amazonaws.com`, with `aws:SourceArn` from an optional `SourceArn` member
that the sender puts in the body beside the model's members (**this is a convention the other services' authors need to know**; without it a
statement with a `SourceArn` condition does not match). Refused as `AccessDeniedException` under `enforce`, logged under `soft`.

**Asynchronous invocation and destinations** (`async_invoke.rs`). `InvocationType: Event` answers 202 and runs in a tracked task; a failure is
retried `MaximumRetryAttempts` times (default 2) after 1 s and 2 s (AWS waits one minute and two: SCOPE's "async retry delay realism" is NOT NEEDED),
bounded by `MaximumEventAgeInSeconds`; the result goes to `OnSuccess`/`OnFailure` destinations (SQS, SNS, another function, an event bus) as AWS's
destination document (`version`, `requestContext.condition` `Success`/`RetriesExhausted`/`EventAgeExceeded`, `requestPayload`, `responseContext`,
`responsePayload`), and a failed one also to the function's `DeadLetterConfig` (SQS, SNS). The EventBridge destination calls `PutEvents` on the
`events` service, which is not on this branch (untested; it logs if absent).

**Event source mappings** (`poll/`). SQS, Kinesis and DynamoDB streams. A poller is one tracked task per enabled mapping; with none enabled there is
no task, no timer, and nothing but a marker file (`mappings.armed`) so a restart resumes them (`Lambda::resume()`, called by `run.rs` after the
gateway is bound). **Notify-driven:** SQS says a message arrived (`Sqs::message_arrived()`, new and additive), Kinesis says records were appended
(`Kinesis::appended()`), DynamoDB says a stream grew (`Dynamodb::stream_arrived()`); the wait is armed before the read it guards, so a wake-up
between the two is not lost. The one place a timer is used: **while an SQS queue has messages in flight or delayed** (they can come back by the
clock alone) the poller looks again each second; an empty queue is waited on without one.
- *Common:* `BatchSize`, `MaximumBatchingWindowInSeconds` (a partial batch waits out the window), `FilterCriteria` (the EventBridge pattern language:
  exact values, `prefix`, `suffix`, `equals-ignore-case`, `wildcard`, `anything-but`, `numeric`, `exists`, `$or`; validated when the mapping is made;
  what a filter drops is deleted from an SQS queue and skipped on a stream), `FunctionResponseTypes: ReportBatchItemFailures`, `Enabled`, `State`
  (`Creating`/`Updating`/`Deleting` in the answers to create/update/delete, `Enabled`/`Disabled` otherwise), `LastProcessingResult`
  (`OK`, `PROBLEM: ...`; absent until the function has run).
- *SQS:* the queue must exist and hide messages at least as long as the function runs (recorded wording); success deletes the batch, or all but
  the items the function reports as failed (`batchItemFailures`; an answer that is not a valid report fails the whole batch, as AWS's documentation
  says); a failure deletes nothing (the visibility timeout and the queue's redrive policy do the rest).
- *Kinesis / DynamoDB streams:* position saved per shard (`TRIM_HORIZON`, `LATEST`, `AT_TIMESTAMP`; `LATEST` is fixed **when the mapping is made**,
  not when the poller task first runs); a failed batch is retried with a growing pause up to `MaximumRetryAttempts`, split in two when
  `BisectBatchOnFunctionError`, dropped when older than `MaximumRecordAgeInSeconds`, and described to the `OnFailure` destination when given up on
  (`KinesisBatchInfo` / `DDBStreamBatchInfo`); a function that reports item failures is retried from the first failed record.
- *Refused:* any other source (Kafka, MQ, DocumentDB, self-managed) answers 501 `NotImplemented` naming the ARN, never a validation error.
- *Not built:* tumbling windows, `ParallelizationFactor` above 1 (shards are read in turn by one task), `ScalingConfig.MaximumConcurrency`, enhanced
  fan-out; one invocation is in flight per mapping.

**Response streaming** (`stream.rs`, `eventstream.rs`). `InvokeWithResponseStream` answers an event stream (`PayloadChunk` events as the function
writes, `InvokeComplete` last, carrying the error and the log tail), with the AWS framing (prelude, headers, CRCs). A runtime that posts with
`Lambda-Runtime-Function-Response-Mode: streaming` is forwarded chunk by chunk; the embedded Node client does not stream (a function that does not
answers with one chunk). **Verified with botocore's own event-stream parser** (the `test_response_streaming` scenario passes over real HTTP).

**Function URLs** (`urls.rs`, `web.rs`). `CreateFunctionUrlConfig` returns `http://<id>.lambda-url.<region>.localhost:<port>/`; a request with
that `Host` (any host with a `.lambda-url.` label, so the `on.aws` form works for a client that can set `Host`) **or** to
`/_aws/lambda-url/<region>/<id>/<path>` becomes the payload-format-2.0 event; the answer is decoded by AWS's rule. `AuthType: AWS_IAM` refuses an
unsigned request under `--iam enforce` (403 `Forbidden`) and lets a signed one through **without checking the signature**. CORS config is applied
(origin allow-list on the answer and on `OPTIONS`). `InvokeMode: RESPONSE_STREAM` is accepted and the answer is buffered.

## Parity against Go (`docs/SUPPORT.md`, Lambda)

Every operation Go marks F or C is built. Go-served behaviour **not** built here, with the reason:

| Go | Here | Why |
|---|---|---|
| `go1.x` and `provided` (Amazon Linux 1) runtimes accepted | refused at create/update ("no longer supported") | AWS refuses runtimes it has retired; the recorded list (31 runtimes) is the whole accepted set |
| Container images refused by name | stored, listed, read back; an invoke says why it cannot run | decision D3 |
| Java package without the runtime interface client refused at create; .NET class-library handler refused | not checked | the launch line is built; an unusable package fails when invoked |
| A missing interpreter is a warning at `CreateFunction` | said at the first invoke (`Runtime.LaunchError`) | not built |
| Per-service `AWS_ENDPOINT_URL_*` variables | only `AWS_ENDPOINT_URL` | every current SDK honours it; an old one needs its own |
| `[lambda].idle-timeout`, `[lambda.runtimes]` in a TOML file | `--lambda-idle-secs`, `--lambda-quiet`, `DOZE_LAMBDA_*` environment variables | there is no config file in the Rust binary |
| Function URL at `<endpoint>/_aws/lambda-url/<id>/` | `/_aws/lambda-url/<region>/<id>/` | the id alone does not say which region's actor to ask |
| CloudWatch `AWS/Lambda` metric datapoints (`lambda/metrics.go`) | not produced | SCOPE says KEEP-LAZY (a ring materialised on read); `aws-cloudwatch` has no producer hook, and a `PutMetricData` per invocation would wake and fill its database for nobody |
| CloudFormation `AWS::Lambda::*` | A16 | not this stage |

Beyond Go: the REST-JSON codec; real SQS semantics (batch item failures, filters, window); Kinesis and DynamoDB streams with retry, bisect, age and
destinations (Go advanced the iterator whatever the result); notify-driven pollers (Go woke every 0.5 to 2 s per mapping); run-as-role with IAM
resolving the function's own session; `iam:PassRole`; weighted aliases applied; `MaximumEventAgeInSeconds` honoured; response streaming; versions
share code; reserved concurrency; resource policies by qualifier; code downloads (`Code.Location`); retired runtimes refused; layers of foreign
accounts accepted.

## What the recording taught

- `create` answers carry one `RevisionId` and every later read another (the function becomes active under a new revision); doze does the same
  (`create`, `update-configuration` answer with a revision that the next read replaces). `publish-unchanged` answers the version it already has,
  **with its own description** (the comparison ignores `Description`), 201.
- `Runtime` outside the model's list is a `ValidationException` that lists **31 values in AWS's order, and then `or be a valid ARN`**; the model has
  more runtimes than AWS accepts, so the recorded list is kept as the accepted set (`config::RUNTIMES`) and the model's extras are refused as retired.
- A payload that is not JSON: `InvalidRequestContentException` with Jackson's sentence, `column: 2` for `{not json`. Reproduced for a key that is
  not quoted; **any other parse failure is worded from `serde_json`** `(memory)`.
- `get-absent` is `Function not found: <arn>` with no qualifier; `invoke-absent` appends `:$LATEST`; `absent-qualifier` ends with `:9`;
  `create-twice` is `Function already exist: <name>` (sic); `alias-twice` names the alias ARN; `change-a-published-version` is
  `This API only supports unqualified ARN or $LATEST.`; an unzippable package is `Could not unzip uploaded file...`; a timeout of 86400 is the
  model's `less than or equal to 5400`; **901 is accepted** (the model's limit is 5400, not 900).
- `GetFunction` has no `State` or `RuntimeVersionConfig` worth matching; `LoggingConfig` always shows `LogFormat: Text` and the default group;
  `EphemeralStorage: 512`, `SnapStart: {ApplyOn: None, OptimizationStatus: Off}`, `TracingConfig: PassThrough`, `Architectures: [x86_64]`.
- `AddPermission`'s statement: a service principal is `{"Service": ...}`, `SourceArn` is an `ArnLike` on `AWS:SourceArn`; removing the last statement
  makes `GetPolicy` and `RemovePermission` say "No policy is associated with the given resource." / "The resource you requested does not exist."
- A mapping to a queue that does not exist: `Error occurred while ReceiveMessage. SQS Error Code: AWS.SimpleQueueService.NonExistentQueue. SQS Error
  Message: The specified queue does not exist.`; one whose queue hides messages for less than the function runs:
  `Queue visibility timeout: 2 seconds is less than Function timeout: 10 seconds`; a create answers 202 with `FunctionResponseTypes: []`,
  `MaximumBatchingWindowInSeconds: 0`, `BatchSize: 10`, no `LastProcessingResult` until the function has run (a mapping whose function was deleted shows
  none, so doze does not note a result when it could not find the function).

## From memory (`(memory)`: not checked against a recording)

The REST-JSON error body shape and `UnknownOperationException` for an unrouted request; every `ResourceNotFoundException`/`ResourceConflictException`
message not listed above (alias not found, URL config exists/absent, provisioned concurrency absent, event invoke config absent, layer version absent,
statement absent); the wording for a retired runtime; the `AssumeRole` session name (`awslambda_<function>_<ms>`); the reserved-variable refusal wording;
the 900 s account-concurrency arithmetic (1000 total, 100 unreserved); `X-Amzn-Trace-Id` content; the Kinesis/DynamoDB filter document (patterns
match the record with `data` decoded, both at the top and under `kinesis`); `LastProcessingResult` values; `Invoke` with no payload runs the function
with `{}`; the account settings numbers; the destination documents for stream sources; the Kinesis and DynamoDB execution-role refusal wording; a
function URL's response when the function fails (502 `Internal Server Error`).

## Shared code touched (outside `aws-lambda`)

| File | Change |
|---|---|
| `aws-wire/src/rest/jsonin.rs`, `jsonout.rs` (new), `rest.rs`, `rest/request.rs`, `rest/response.rs` | the codec; four helpers made `pub(super)` |
| `aws-wire/src/service.rs` | `Web`, `WebRequest`, `WebReply`, `Service::web()` (default `None`) |
| `aws-wire/Cargo.toml`, `fuzz/*` | `aws-model-lambda` as a dev-dependency; `fuzz_restjson`; the fuzz `Cargo.toml` repaired |
| `aws-gateway/src/rest.rs`, `s3host.rs` (new), `web.rs` (new), `pipeline.rs`, `lib.rs` | the restJson1 branch; the unsigned-address hook; `Shared::webs()` |
| `aws-peers/src/lib.rs` | `Peers::call_rest` (a call whose answer keeps its stream; Lambda reads a zip from the local S3 with it, as the caller, after asking `s3:GetObject`) |
| `aws-sqs/src/lib.rs` | `Sqs::message_arrived()` |
| `doze-aws/src/run.rs`, `cli.rs`, `Cargo.toml` | Lambda registered and connected to SQS, Kinesis and DynamoDB; `--lambda-idle-secs`, `--lambda-quiet`; `DOZE_LAMBDA_*` |
| `conformance/tests/test_lambda.py` | three scenarios appended (UNVERIFIED until recorded) |
| `xtask/weight-limits.txt` | the doze-aws line: 7,700,000 B and 233 packages, reason in a comment |
| `Cargo.lock` | `aws-lambda` only |

`aws-iam` was **not** touched. S3's `call` still answers peers 501 (it serves HTTP only); the bus reaches S3 through `RestService`.

## Weight

`cargo xtask weight doze-aws`: **6,102,912 B, 229 packages, 3,872 KB idle** on darwin/arm64 under `release-aws` (+731 KB, +9 packages, +0.27 MB idle
over the 5,371,872 B / 220 / 3.6 MB of the base). **No third-party package is added.** `Cargo.lock` gains `aws-lambda`; `aws-model-lambda` is linked for the
first time. The tool counts distinct lines of `cargo tree`, so a crate reached by a new edge is counted again as a `(*)` line: the other seven are such
repeat lines of `hyper`, `http-body-util`, `tokio-util`, `flate2`, `crc32fast`, `sha2` and `tokio` (tokio gains its `process` and `fs` features, which
add no package). The bytes are the Lambda model's tables (88 operations, about 840 constraints), the codec, the Runtime API server, the zip reader, the
three shims (about 19 KB of text) and the service. **Idle:** an unused Lambda is a handle; nothing runs, no timer, no file until the first request
(or until a restart finds `mappings.armed`). **The Linux build is about 1.45 times the macOS one (A11's measurement), so this is about 8.9 MB there
against the owner's 10 MB ceiling**: the limit line is 7,700,000 B and 233 packages (macOS size times 1.2 plus 5%); with EventBridge, Object Lock
and the rest still to merge the ceiling is the constraint to watch, and Lambda is the largest single service added since S3.

## Tests

- `aws-wire`: `jsonin` (labels/query/headers/body/payload; a body that is not an object), `jsonout` (an Invoke answer, a structure, an error), the
  22-route table. `aws-lambda` (44): `arn`, `zipread` (modes, bad and escaping archives), `config`, `perms`, `urls`, `rt::spec`, `rt::logs`,
  `eventstream`, `web`, `role`, `poll::filter`, and the integration tests in `src/tests/`: **functions** (lifecycle, versions, aliases, permissions,
  layers/URLs/tags/concurrency, refusals by name), **invoking** (a `sh` bootstrap with `curl`: answer, handled error, timeout, environment; Python
  through the embedded client; no task left after shutdown; `Command`; a layer on the Python path), **sources** (SQS delete-on-success, batch item
  failures, filters, the visibility rule, the missing queue and Kafka refusals; a Kinesis stream and a DynamoDB stream; the log shape), **behaviour**
  (run as the role under `enforce`; a role Lambda cannot assume; async retries and destinations; reserved concurrency 0; a container image;
  response streaming; a function URL; code in place; a service invoking through the function's policy; the execution-role check of a mapping).
  A test that needs a runtime the host lacks (`curl`, `python3`) prints `skipped` and passes.
- Not tested: Java, .NET, Node and Ruby functions (the host has no usable Node here and Ruby 2.6 only; the shims are Go's, which tested them),
  stream-source retry/bisect/age and failure destinations (written, not driven), the Kinesis `AT_TIMESTAMP` start, the EventBridge destination,
  `--iam enforce` on a function URL end to end, a restart with an enabled mapping (the marker is written and read; not run through a restart).

## Open decisions for the owner

1. **A role IAM has never heard of.** Accepted unless IAM enforces (the function then runs as the local test identity). AWS always refuses.
   Strict everywhere would break `--role arn:aws:iam::000000000000:role/any` flows and templates written before A16 maps `AWS::IAM::*`.
2. **The `SourceArn` convention** for services that invoke a function through the bus (above): the EventBridge, SNS and S3 authors must put it in the
   body for an `aws:SourceArn` condition to match. A field on `Caller` would be cleaner and was not added because every service builds `Caller` by literal.
3. **The AWS/Lambda metrics.** Needs a hook on the CloudWatch side (a bounded in-memory ring read lazily) before Lambda can push to it.
4. **Weight.** +731 KB for Lambda. If the Linux total matters more than the Go-parity breadth, the candidates are the embedded Node and Ruby clients
   (compile them out unless a feature is on) and the Lambda model's unused operation tables (the 18 refused operations still carry their shapes).
5. **The recorder policy** needs `lambda:GetEventSourceMapping` (and, to record the three new scenarios, layers, tagging, URL and streaming actions) on
   `doze-rec-*`.
6. **One poller task per mapping, one invocation at a time.** Enough for a laptop; `ParallelizationFactor` and SQS `MaximumConcurrency` would need a
   semaphore per mapping.
