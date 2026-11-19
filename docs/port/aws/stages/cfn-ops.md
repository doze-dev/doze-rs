# cfn-ops: the unbuilt CloudFormation operations (stage note, 2026-11-19)

Branch `aws-cfn-ops`, from `origin/main` at `f864c8dd9`. The integrator folds this note into `PLAN.md`, `STATUS.md` and
`scoreboard.md`; none of those was touched here. AWS is the only reference: each behaviour cites the model documentation
(`models/cloudformation.json`) or says it is not recorded. Wording not seen recorded is marked `(memory)` in the code beside it.
**Nothing here was recorded**: the recorder's SSO session (`doze-rec-main`) had expired, so every new boto3 scenario is UNVERIFIED.

Twelve operations were planned. Eleven are built; `EstimateTemplateCost` moved to `not_planned`.

## 1. CancelUpdateStack (`src/cancel.rs`)

- The model: "You can cancel only stacks that are in the UPDATE_IN_PROGRESS state", and the stack "rolls back the update and
  reverts to the previous stack configuration". Any other status is refused with `ValidationError` "CancelUpdateStack cannot be
  called from current stack status" (memory). A stack not there: "Stack with id X does not exist" (the recorded DescribeStacks words).
- How: every update job has a flag (by stack id) while it runs, registered when the job is started, which is before the call that
  asked for the update is answered, so no cancel can come first. Cancelling sets it. The forward pass starts nothing new once it is
  set; what waits (a wait condition, a custom resource's response) stops waiting, and that resource fails with "Resource creation
  cancelled" / "Resource update cancelled" (memory). A call to a service already under way finishes (local calls take milliseconds);
  the rollback puts it back. The stack goes `UPDATE_ROLLBACK_IN_PROGRESS` with reason "User Initiated" (memory) and on to
  `UPDATE_ROLLBACK_COMPLETE`. A cancel rolls back even when the update was made with `DisableRollback` (the model says it rolls back).
- The flag is dropped before the rollback starts, so a rollback is never cancelled. Nested stacks updating under the stack are
  cancelled with it. No idle cost: the flag table is empty unless an update runs.
- Tested with an update that adds a queue and a wait condition that waits an hour: cancel, `UPDATE_ROLLBACK_COMPLETE`, the queue gone,
  and the stack updates normally afterwards.

## 2. SignalResource and wait conditions (`src/signal.rs`, `src/res/wait.rs`)

- Before: a wait condition completed at once with empty `Data`. Now it waits, as the user guide describes:
  - with a `CreationPolicy` (`ResourceSignal: {Count, Timeout}`, defaults 1 and `PT5M`, at most 12 hours) it counts `SignalResource`
    calls (what `cfn-signal --stack --resource` makes);
  - otherwise its `Handle` (`Ref` of a `WaitConditionHandle`), `Count` (default 1) and `Timeout` (seconds, at most 43200) say whose
    signals it counts; signals are `PUT` to the handle's URL as `{"Status", "UniqueId", "Data", "Reason"}` (what `cfn-signal <url>` and
    `curl -T` send). The handle's URL is now `/cloudformation-waitcondition-<region>/...?X-Amz-Signature=<token>` on the local endpoint
    (AWS's is a presigned URL in a bucket of that name), served by the CloudFormation web hook; an unknown or deleted handle answers
    403 "Request has expired" (memory).
- A distinct unique id counts once. `Count` successes complete it, its `Data` attribute is `{"<UniqueId>": "<Data>"}`. The first
  failure fails it; so does the timeout. Words (memory): "Received FAILURE signal with UniqueId X", "Failed to receive N resource
  signal(s) within the specified duration" (creation policy); "WaitCondition received failed message: 'R' for uniqueId: X",
  "WaitCondition timed out. Received N conditions when expecting M" (handle). Each `SignalResource` writes a resource event "Received
  SUCCESS signal with UniqueId X" (memory).
- `SignalResource` refusals: an absent stack (as above), an absent resource ("Resource X does not exist for stack Y", the recorded
  DescribeStackResource words), and a resource that is not waiting for signals ("Resource X is in <status> state and cannot be
  signaled: ...", memory). Signals are held in memory only while something can wait for them (a policy wait's inbox while it waits; a
  handle's until the handle is deleted). A restart forgets them, as it cuts off the operation.
- Only wait conditions wait. EC2 instances and Auto Scaling groups (the other creation-policy types) are skipped types, never made
  locally, so nothing would run `cfn-signal` in them: they do not wait (or the stack would hang for its timeout), and a signal to
  one is refused as not waiting. Same for `UpdatePolicy` signals (`WaitOnResourceSignals` is an Auto Scaling setting). A difference.

## 3. Drift detection (`src/drift/`)

- `DetectStackDrift` answers a detection id at once and runs beside the actor (a task, like a stack operation);
  `DescribeStackDriftDetectionStatus` follows it (`DETECTION_IN_PROGRESS` → `DETECTION_COMPLETE`, or `DETECTION_FAILED` when a
  resource could not be read). `DetectStackResourceDrift` checks one resource at the call. `DescribeStackResourceDrifts` lists the
  latest result of each checked resource, with `StackResourceDriftStatusFilters` and `MaxResults`/`NextToken`.
- Each resource is read back through its own service's read operations, as `cloudformation` acting for the caller:
  `GetQueueAttributes`+`ListQueueTags`, `GetTopicAttributes`+`ListTagsForResource`, `GetParameter`+`DescribeParameters`+tags,
  `DescribeLogGroups`+tags, `HeadBucket`+`GetBucketVersioning`+`GetBucketTagging`, `DescribeTable`, `GetRole`+
  `ListAttachedRolePolicies`, `GetFunctionConfiguration`, `DescribeSecret`, `DescribeKey`+rotation+policy+tags. Types: SQS queue, SNS
  topic, SSM parameter, log group, S3 bucket, DynamoDB table, IAM role, Lambda function, secret, KMS key. Every other type, a skipped
  type (not made locally) and a resource not in a complete state is not checked: it keeps `NOT_CHECKED` and is not in the results (the
  model: "Resources that don't currently support drift detection aren't checked, and so not included"). `DetectStackResourceDrift` on
  one is refused (`ValidationError`, words memory, with the local reason added).
- Compared: only the properties the template set (model). `"30"` and `30` are equal; a JSON document given as an object and kept as
  text by the service is read as JSON (IAM's URL-encoded policy is decoded); tags and lists keyed by `Key`/`AttributeName` compare by
  key; `ManagedPolicyArns` and `Architectures` as sets. A property the reader does not know is not compared. A value with a dynamic
  reference is not compared (what it resolved to is never kept). The stack's own tags and `aws:` tags are not the template's and are
  ignored. Differences: `NOT_EQUAL`, `REMOVE` (a property gone, actual `null`), `ADD` (an element added to a list, a key to a tags
  object), paths `/Prop/Sub/0` (memory). Statuses `IN_SYNC`, `MODIFIED`, `DELETED` (the service says it is not there; no expected or
  actual properties, as the model says), `UNKNOWN` with `DriftStatusReason` when the read failed.
- Stack status: `DRIFTED` if any resource is modified or deleted, else `UNKNOWN` if one could not be read, else `IN_SYNC` (also for a
  stack with nothing checkable: memory). `DriftedStackResourceCount` counts modified and deleted.
- `DescribeStacks`, `ListStacks`, `DescribeStackResource(s)` and `ListStackResources` now show the drift information with
  `LastCheckTimestamp`. A result older than the resource's last change (or a detection older than the stack's last update) shows as
  `NOT_CHECKED` again (memory: AWS resets drift status after an update).
- Kept in two new tables (`drift_detections`, the last ten per stack; `resource_drifts`, the latest per resource), so the stack record
  keeps its on-disk shape. `doze-aws data` counts them with their stack; emptying a deleted stack and a reset remove them.

## 4. DescribeEvents (`src/events.rs`)

- The model's newer operation: events by `OperationId`, `StackName` or `ChangeSetName` (the operation that executed it), with
  `Filters.FailedEvents`. Each `OperationEvent` carries `OperationType` (`CREATE_STACK`, `UPDATE_STACK`, `DELETE_STACK`, `ROLLBACK`,
  `CONTINUE_ROLLBACK`) and `OperationStatus` (`IN_PROGRESS`, `SUCCEEDED`, `FAILED`: an operation that rolled back failed). A new
  table `operations` keeps each operation's type and executed change set, written with its first event (operations from before this
  build have no type shown).
- Not recorded, so the shape choices are guesses written down in the module: `EventType` is `STACK_EVENT` for the stack's own
  events, `PROVISIONING_ERROR` for a failed resource, `PROGRESS_EVENT` otherwise; `StartTime` is when the resource's work in the
  operation began, `EndTime` is set on the event that ends it. No hook or validation events exist locally. Change set creation has no
  operation id here (`CREATE_CHANGESET` is never shown). Asking with none of the three is `ValidationError` (model's sentence, memory
  as the error's words).

## 5. Hooks and account limits (`src/hooks.rs`)

- doze-aws activates no hooks (the extension registry is not planned). `DescribeChangeSetHooks` checks the change set
  (`ChangeSetNotFoundException`) and answers `Hooks: []`, `Status: UNAVAILABLE` (memory); `ListHookResults` answers no results (with
  `TargetType`/`TargetId` for a known change set or stack; an unknown target is `HookResultNotFoundException`; the model's pairing
  rules are checked); `GetHookResult` is always `HookResultNotFoundException`. Messages are doze-aws's own and say why.
- `DescribeAccountLimits`: `StackLimit` 2000, `StackOutputsLimit` 200, `ConcurrentResourcesLimit` 2500 (memory: AWS's defaults from
  the quotas page; the model names the three limits). Not enforced.

## 6. Not planned: EstimateTemplateCost

AWS answers a URL of its Simple Monthly Calculator (the model: "an Amazon Web Services Simple Monthly Calculator URL with a query
string that describes the resources required to run the template"), filled in from AWS's prices. That calculator is retired, and a
local tool has no prices; any URL doze-aws gave would be invented. Listed in `docs/aws/compatibility-notes.toml`.

## 7. Scenarios (UNVERIFIED), tests

`conformance/tests/test_cloudformation.py`: `test_cancel_update_stack_rolls_the_update_back`,
`test_signal_resource_completes_a_wait_condition`, `test_a_failure_signal_fails_the_wait_condition`,
`test_drift_detection_finds_changes_made_out_of_band` (an SQS queue and an SSM parameter changed and deleted out of band, a wait
condition handle that is not checked), `test_describe_events_of_an_operation`, `test_hooks_and_account_limits`. All pass against
doze-aws (cloudformation module: 24 tests, 88 exact, 37 unverified). They poll where AWS works in the background and delete every
stack they make; the recorder permission set already allows every call (prefixed stacks, `Describe*`, the hook reads).

Crate tests: `src/tests/signals.rs` (signals by policy and by handle, failure and timeout, refusals, cancel, DescribeEvents, hooks,
limits), `src/tests/drift.rs` (in sync across eight types, modified/deleted/tag added, filters, single-resource check, reset),
`src/drift/compare.rs` (the comparison rules).

## 8. Open

- Record the six scenarios when the SSO session is renewed: every (memory) above is a guess until then, especially the
  CancelUpdateStack refusal, the drift path format and stack status for a stack with nothing checkable, the hook statuses, and the
  whole `DescribeEvents` shape.
- Readers for more types (Events rules, Step Functions, API Gateway, Kinesis) would widen drift coverage; each is a small reader.
- `UpdatePolicy` signals and creation policies of skipped types are not waited for (see 2).
