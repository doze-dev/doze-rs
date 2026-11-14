# verify-d: Step Functions, API Gateway, CloudFormation against the 2026-11-15 recordings

Recordings: account 821712915674, us-east-1, profile `doze-rec-main` (limited SSO role). Scored with the release build
(`cargo xtask conformance`). Columns: labels / exact / wording / behaviour / unverified.

| module | before | after |
|---|---|---|
| stepfunctions | 75 / 60 / 1 / 6 / 8 | 75 / 75 / 0 / 0 / 0 |
| apigateway | 88 / 57 / 3 / 5 / 23 | 92 / 91 / 0 / 0 / 1 |
| cloudformation | 88 / 69 / 0 / 1 / 18 | 88 / 88 / 0 / 0 / 0 |

## Step Functions

Scenario: `test_validation_diagnostics` recorded once the owner's policy allowed `states:ValidateStateMachineDefinition`.
No scenario was wrong.

doze-aws, each **Go wrong (fixed)** unless said:
- A task token answered twice: AWS says `TaskTimedOut` / `Task Timed Out: '<activity ARN>'`, not `TaskDoesNotExist`. The core
  remembers the last 1024 answered tokens in memory (not persisted; a restart forgets them and falls back to `TaskDoesNotExist`).
  A callback token's wording (`'Provided task does not exist anymore'`) is still memory.
- An alias carries no `updateDate` until it is first updated; `UpdateStateMachineAlias` answers the stored update time.
- Weights not adding to 100: `Sum of routing configuration weights must equal 100. Received: [50]`.
- Deleting a version an alias routes to: aliases listed by name, status 400. AWS answered the modelled-409 `ConflictException`
  with 400, so Step Functions now uses flat client errors like KMS (`ResourceNotFound`, modelled 404, follows; unrecorded).
- TestState: a state that leaves its input untouched hands back the caller's text verbatim (AWS kept `{"n": 5}`'s spacing for a
  Choice); a Pass state's inspection carries `afterResultSelector` / `afterResultPath` like a task's; `variables` is left out while
  there are none.
- Validation diagnostics (all recorded): `There should only be one of the following fields: [Next, End]`, `This object should have
  only one of the following fields: [Next, End]`, the Wait state's `[SecondsPath, Seconds, TimestampPath, Timestamp]`, `Field 'X' is
  not supported` placed at the state, `The field 'Type' is required`, `Value is not a Reference Path: '[' must be followed by ' or
  '['`, `States.ALL must appear alone and at end of list` at `Retry[0]/ErrorEquals[0]`, and a new rule:
  `Parameters field is required for resource ARN: arn:aws:states:::lambda:invoke` at `/Resource`. doze applies the Parameters rule
  to every `arn:aws:states:::` integration except `aws-sdk:` ones (only `lambda:invoke` is recorded; the breadth is a judgement).

## API Gateway

Scenarios (AWS taught us; fixed and re-recorded):
- `test_rest_api_update_operations`: `add` on `/variables/<name>` is refused by AWS (`Invalid add operation with path`); variables
  are set with `replace`. The refusal is now a label of its own. X-Ray tracing (`/tracingEnabled`) made AWS try to create the
  service-linked role `AWSServiceRoleForAPIGateway`, which the recorder may not: it moved to its own scenario,
  `test_rest_api_stage_tracing`, unrecorded until the owner allows `iam:CreateServiceLinkedRole` for
  `ops.apigateway.amazonaws.com` (resource `arn:aws:iam::821712915674:role/aws-service-role/ops.apigateway.amazonaws.com/AWSServiceRoleForAPIGateway`)
  or creates the role once.
- `test_http_api_routes_stages_and_deployments`: an HTTP API's `HTTP_PROXY` integration needs `PayloadFormatVersion` (and only
  `1.0`); both refusals are labels now.
- Refusals that name a minted id (`API Stage not found: <api id>:nope`, `... Routes with Ids: [<route id>]`) and the integration's
  `cacheNamespace` (its resource id) could never compare: `snapshot.known(value, label)` (new in the harness) reads such an id as
  `<name:label>` on both sides.

doze-aws:
- HTTP APIs: `PayloadFormatVersion is a required parameter for integration` for proxy integrations without one, and
  `PayloadFormatVersion 2.0 is not supported for integration of type HTTP_PROXY`. **Go wrong (fixed)**: Go defaulted 1.0/2.0.
  Integrations with an `IntegrationSubtype` still default to 1.0 (unrecorded).
- WebSocket integrations: `PayloadFormatVersion: 1.0` and `WHEN_NO_MATCH` for every type, and
  `IntegrationResponseSelectionExpression: ${integration.response.statuscode}` for non-proxy ones (probed on AWS for MOCK, HTTP,
  HTTP_PROXY, AWS_PROXY).
- Reads and updates show `tags: {}` (GetRestApi, UpdateRestApi, GetApi, UpdateApi, GetApiKey); creates do not.
- A usage plan's quota gets `offset: 0`; an absent plan stage is `NotFoundException` 404; `PutGatewayResponse` without templates
  keeps the default template; method settings have no `loggingLevel` until set; v2 `CreateStage` has no `AutoDeploy` unless given;
  `UpdateRoute` answers 201 (the model says 200).
- Wording: `Model name must be alphanumeric: <name>`, `Unable to determine OAS/Swagger version`, `Invalid routeSelectionExpression`,
  `Invalid patch path <path>` (Go listed the paths it would take; AWS does not), `Active stages pointing to this deployment must be
  moved or deleted`, `Cannot delete Integration because it is referenced by the following Routes with Ids: [...]`.
- Tried and reverted: v2 timestamps to the second. AWS's two recordings disagree on whether a stage and its deployment share a
  time (same-second luck); doze keeps millisecond times, which matches the newer recording.

## CloudFormation

**One cause for the 11 failures and the error:** DNS resolution failed on the recorder's machine part-way through the run
(`nodename nor servname provided` for `cloudformation.us-east-1.amazonaws.com`, `sqs.`, `ssm.`, the S3 bucket host): the first
seven scenarios passed, every later one died on its first call, and the teardown of
`test_a_failed_create_can_be_left_or_deleted` could not delete `doze-rec-88117c4f-7-kept` (the stack the integrator removed). Not a
permission, a naming rule or a timeout. All 11 re-recorded and passed as written (22 min; nested stacks, SAM, a custom resource
and IAM all work under the recorder policy).

Scenarios:
- `test_refusals` `bad-stack-name` used `has_underscore`, which lacks the `doze-rec-` prefix, so the recorder policy refused
  (AccessDenied) before CloudFormation looked at it. Now `names("has_underscore")`; re-recorded: CloudFormation's own
  `ValidationError`, whose pattern is `[a-zA-Z][-a-zA-Z0-9]*` (no `|arn:...` alternative on create).
- `test_a_custom_resource_backed_by_an_inline_function` now deletes `/aws/lambda/<fn>`, which the function writes and the stack
  never owns.

doze-aws:
- **Go wrong (fixed)**: deleting a stack whose export another stack imports. AWS starts the delete and cancels it: the stack
  returns to the status it had (`CREATE_COMPLETE` here) with the event reason `Delete canceled. Cannot delete export X as it is in
  use by Y.` (probed on AWS with two small stacks to rule out a race in the scenario; cleaned up). Go left it `DELETE_FAILED`.
- **Go weak (bettered)**: change sets. A resource that reads an attribute of a resource modified in place is itself a `Modify`
  with a `Dynamic` detail (`ResourceAttribute`, `CausingEntity: Q.Arn`); a Ref to such a resource stays static (its physical id
  cannot change). A property changed by a parameter is told twice, `ParameterReference`/`Static` and
  `DirectModification`/`Dynamic`, as AWS tells it.
- `GetTemplateSummary` leaves out `DeclaredTransforms` when there are none.
- The CreateStack name refusal names the create pattern only.

## Left open

- `test_rest_api_stage_tracing` (1 unverified label): needs `iam:CreateServiceLinkedRole` with `iam:AWSServiceName =
  ops.apigateway.amazonaws.com` on `arn:aws:iam::821712915674:role/aws-service-role/ops.apigateway.amazonaws.com/AWSServiceRoleForAPIGateway`,
  or the owner creating that role once (`aws iam create-service-linked-role --aws-service-name ops.apigateway.amazonaws.com`).
- Memory wording that remains: a spent callback token's message, `TaskDoesNotExist` for an unknown token, the two-or-more
  Wait fields and Seconds/SecondsPath pair wording, the Parameters rule beyond `lambda:invoke`, `PayloadFormatVersion` for
  integrations with a subtype, `Invalid patch path` for operations other than UpdateMethod.
- No decided deviations: every recorded difference in these three modules is closed.

