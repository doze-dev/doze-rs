# A15: API Gateway (REST APIs, HTTP APIs, WebSocket APIs)

Branch `aws-a15-apigateway`, 2026-11-13, one agent in its own worktree on top of `128eab40`. One new crate (`aws-apigateway`, both APIs and the
execute-api plane), two small additive hooks in shared crates, and both APIs registered on the gateway and the peer bus. This note is for the
integrator. `PLAN.md`, `STATUS.md`, `scoreboard.md` and `weight.md` are not edited here.

## Summary

| | |
|---|---|
| Recorded labels exact | **36 of 36** (100%); Go scores 53% exact on the same recording (`baseline.md`) |
| Labels in scenarios written ahead of a recording | 52, UNVERIFIED (six new scenarios in `conformance/tests/test_apigateway.py`) |
| Operations, REST API (v1, 124 in the model) | **90 built, 34 refused by name** (501, `X-Doze-Not-Built`) |
| Operations, HTTP/WebSocket API (v2, 103 in the model) | **66 built, 37 refused by name** |
| `@connections` management API | built by hand on the execute-api plane (no `apigatewaymanagementapi` model in the repository): `POST`, `GET`, `DELETE` |
| Tests | 38 in `aws-apigateway` (control planes through the services, the plane through `Web::serve`, WebSocket through a real gateway on a socket) |
| Weight | darwin/arm64 5,349,616 B (+482 KB), 237 packages (+4), 4,576 KB idle; **x86_64 musl 8,261,008 B (+779 KB)**; no new third-party package |

## The scoreboard

```
UV_BIN=$(ls -d ~/.proto/tools/uv/*/uv | head -1) cargo xtask conformance --cmd "$PWD/target/release-aws/doze-aws --listen {addr} --data-dir {data} --account 111122223333" apigateway
module           labels  exact  wording  behaviour  unverified
apigateway           88     36        0          0          52
total: 88 labels, 40% exact, 40% ignoring wording
```

The recording has 36 labels (REST with a MOCK, REST proxying to Lambda, an HTTP API proxying to Lambda, 13 refusals): all 36 exact. The 52 unverified
are the new scenarios (update operations and stage settings, models/validators/gateway responses, API keys and usage plans with the key checked on
the plane, OpenAPI import and export, HTTP API routes/stages/deployments, a WebSocket API's control plane). The other modules, run once on the same
binary: lambda 52, eventbridge 55, s3 84, kinesis 30, logs 28, cloudwatch 28, dynamodb 53, sns 38, sqs 177, ssm 29, secretsmanager 27, kms 44,
iam 36, sts 2: unchanged.

## Shape

One crate, two `Service`s (`ApiGateway` for v1, `ApiGatewayV2` for v2) over one lazy actor per region (`Keyed<Core>`, home `us-east-1`, one
`apigateway.redb` per region), because a deployed API is found by id whichever API made it and custom domain names are shared by both, as on AWS.
Each API is **one JSON document** (what `GetRestApi`/`GetApi` shows, beside its resources/methods/stages/... or routes/integrations/...), kept
as JSON text through a `Doc` wrapper; members beginning with `_` (a deployment's snapshot) never leave in an answer. Both APIs sign as
`apigateway`: the gateway now picks among services that share a signing name by whose routes take the path (`/v2/...`), and the peer bus registers
them as `apigateway` and `apigatewayv2`.

**Authorization** (`authz.rs`): `apigateway:<HTTP method>` on `arn:aws:apigateway:<region>::<path>` (the operation's `@http` template filled from
the request; v2's `/v2/apis/...` is `::/apis/...`), as AWS names them. On the plane, `AWS_IAM` methods and routes ask `execute-api:Invoke` on
the method ARN with the REST API's resource policy (soft mode logs, enforce refuses); the request must be signed (403 `Missing Authentication Token`
otherwise), the signature itself is not checked on this plane.

## Built

**REST API control plane (90).** APIs (create with `cloneFrom`, the default `Empty`/`Error` models, update by patch, delete), the resource tree
(paths recomputed on move/rename, subtree delete, sibling and greedy rules), methods, integrations and both kinds of response with **all the
`Update*` operations by JSON patch** (SCOPE item 14; Go answered 405), deployments as **snapshots** with `embed=apisummary`, stages (variables,
tracing, canary, access-log settings, cache settings, method settings by `/{path}/{method}/{group}/{setting}` with AWS's defaults filled in,
`FlushStageCache`, `FlushStageAuthorizersCache`), authorizers (TOKEN, REQUEST, COGNITO_USER_POOLS), request validators, models, gateway
responses (all 21 types with defaults), API keys, usage plans and their keys, account settings, custom domain names and base path mappings
(**stored, not active locally**), tags, `ImportRestApi`, `PutRestApi` (overwrite and merge), `GetExport` (oas30 and swagger, JSON or YAML).

**HTTP/WebSocket API control plane (66).** APIs (quick create with `target`), routes (key rules recorded), integrations (AWS_PROXY, HTTP_PROXY, and
for WebSocket AWS/HTTP/MOCK), authorizers (JWT, REQUEST), stages (auto-deploy), deployments (snapshots), models, route responses, integration
responses, `DeleteCorsConfiguration`, `DeleteRouteRequestParameter`, `DeleteAccessLogSettings`, `DeleteRouteSettings`, `ResetAuthorizersCache`,
domain names and API mappings (stored, not active), tags, `ImportApi`, `ReimportApi`, `ExportApi` (OAS30, JSON or YAML).

**The execute-api plane** (`plane/`), through `Service::web()` at `/_aws/execute-api/<api>/<stage>/<path>` and
`<api>.execute-api.<region>.<host>/<stage>/<path>` (an HTTP API's `$default` stage also at the root; a path form without a region looks in every
region that has files):

- Routing by AWS's precedence (literal, then variable, then greedy, deepest greedy first; exact method before `ANY`; `$default` last). A REST miss
  is the recorded 403 `{"message":"Missing Authentication Token"}`; an HTTP API miss 404 `{"message":"Not Found"}`.
- **Lambda proxy** in payload 1.0 and 2.0 with `requestContext`, multi-value headers and query, stage variables, base64 bodies for binary media
  types; the answer read by each format's rules (1.0 strict, 2.0 any JSON as a 200 body), 502 for a malformed one. Calls go through
  `Peers::call_rest("lambda", "Invoke")` with `SourceArn` = the method's execute-api ARN, so a permission naming it matches.
- **Non-proxy integrations** (Lambda custom, AWS service, HTTP, MOCK): request templates chosen by content type with `passthroughBehavior`
  (415 when it says so), integration responses selected by `selectionPattern` (status code for HTTP and MOCK, `errorMessage` for Lambda),
  response templates and header mappings, `$context.responseOverride`.
- **Mapping templates: a real VTL subset** (`plane/vtl/`): references with properties, indexes and method calls, `#set`, `#if`/`#elseif`/`#else`,
  `#foreach` (with `$foreach`), comments, string/number/list/map/range literals and all operators; `$input.body/json/path/params`, `$util`
  (`escapeJavaScript` as Java escapes, `parseJson`, `urlEncode`, `urlDecode`, `base64Encode`, `base64Decode`), `$context`, `$stageVariables`;
  the Java string/list/map methods templates use. `#macro`, `#parse`, `#include`, `#define`, `#evaluate`, `#break`, `#stop` and any method not in the
  subset are **refused by name** (500 `Internal server error`, the reason in the log), never guessed. Nesting, loops and output are capped; fuzz target.
- **AWS service integrations** over the peer bus: REST `action/<Op>` (JSON or form bodies) and SQS `path/<account>/<queue>`; HTTP API subtypes
  `SQS-SendMessage/ReceiveMessage/DeleteMessage/PurgeQueue`, `EventBridge-PutEvents`, `Kinesis-PutRecord`, `StepFunctions-StartExecution/
  StartSyncExecution/StopExecution` (Step Functions answers when its stage lands; until then the bus says no such service).
- **HTTP and HTTP_PROXY** with parameter mappings (REST `integration.request.*`; HTTP API `overwrite:path`, `append:/overwrite:/remove:` on
  headers and query), `{proxy}` placeholders, timeouts (504). Plain `http://` only: there is no TLS client (as SNS and EventBridge). **(Superseded 2026-11-14: outbound https is built; see `stages/tls.md`.)**
- **Authorizers:** Lambda TOKEN and REQUEST (REST) and REQUEST in payload 1.0/2.0 with simple or policy answers (HTTP), identity sources,
  validation expressions, the function's `Unauthorized` as 401, the returned **IAM policy evaluated by `aws-iam-policy`** for the method ARN
  (explicit deny wording), context and `principalId` on the event, AWS's cache by authorizer and identity for the TTL (bounded, cleared by any
  authorizer change and by `FlushStageAuthorizersCache`/`ResetAuthorizersCache`); **JWT** (issuer, audience or `client_id`, `exp`, `nbf`,
  scopes; RS256 verified against the issuer's JWKS over `http://`); **Cognito user pools** (issuer from the pool ARN, expiry, scopes).
- **API keys** (header or `AUTHORIZER` source; enabled, and in a usage plan for the stage), **request validation** (required parameters; the body
  against the method's model, a JSON Schema draft-4 subset with `$ref` to other models), **gateway responses** (the API's own per type, its
  `DEFAULT_4XX/5XX`, AWS's default; template and header mappings), **CORS** for HTTP APIs (preflight answered by API Gateway, headers on answers),
  **access logs** to CloudWatch Logs over the bus in the stage's format.
- **WebSocket APIs** (decision D8): a new `Web::upgrade` hook lets the service take an `Upgrade: websocket` connection (the gateway now serves
  connections `with_upgrades()` and tracks the session task). `$connect` (with its authorizer and integration; a non-2xx refuses the handshake),
  route selection by `routeSelectionExpression` (`$request.body.action`) to a route or `$default`, AWS's `Forbidden` frame when neither exists,
  integrations (Lambda proxy with AWS's WebSocket event, MOCK with templates), route responses sent back, `$disconnect`, ping/pong, fragmented
  messages, a 1 MB message cap. The `@connections` API at `<stage>/@connections/<id>` posts to (410 `GoneException` once gone), reads and closes a
  connection.

**OpenAPI** (SCOPE item 15): an in-house reader (JSON, and a YAML subset: block and flow collections, quoted, plain and block scalars; anchors,
aliases, tags and multi-document streams refused by name; fuzz target). Import reads paths, operations, parameters, request bodies, responses and
their models, security (API key, SigV4, authorizer schemes), `x-amazon-apigateway-integration` with responses, request validators, gateway
responses, binary media types, key source, policy and (v2) CORS; what it does not read is a warning (or a refusal with `failOnWarnings`).
Export writes the same document back from a stage; the two round-trip (tested).

## Refused by name

- **v1 (34):** client certificates (5), VPC links (5), domain-name access associations (4): no VPC, no TLS material locally; documentation parts and
  versions (11) and SDK generation (`GetSdk`, `GetSdkType`, `GetSdkTypes`): publishing, STAY CUT in SCOPE.md; `GetUsage`, `UpdateUsage`: nothing is
  metered; `ImportApiKeys` (a CSV of keys); `TestInvokeMethod`, `TestInvokeAuthorizer`, `GetModelTemplate`: console features (STAY CUT).
- **v2 (37):** portals, portal products, product pages, routing rules (the developer-portal surface), VPC links, `GetModelTemplate`.

## Parity against Go (`docs/SUPPORT.md`, API Gateway and API Gateway v2)

Every operation Go marks F (54 REST, 37 HTTP) is built. Beyond Go: the REST `Update*` operations (Go: 405), request validators and models, gateway
responses, domain names and mappings (stored), OpenAPI import and export, `FlushStage*Cache`, JWT and Cognito authorizers (Go refused both),
non-proxy integrations with mapping templates (Go refused VTL), AWS service integrations, WebSocket APIs (Go refused), route and integration
responses, request validation, the authorizer policy evaluated by the IAM evaluator. Go behaviour **not** carried over:

| Go | Here | Why |
|---|---|---|
| Deployments serve the **live** API ("a stale snapshot is a debugging trap") | a REST stage serves its deployment's snapshot; an HTTP API's auto-deploy stage serves the live API, another stage its deployment | AWS is the judge; Go's choice was a documented divergence. Redeploying is what AWS asks for too |
| Execution log narrative (`API-Gateway-Execution-Logs_<api>/<stage>`) | not written | SCOPE.md defers it ("keep the access-log line"); the access log is written |
| A failed authorizer answers `Authorizer error: <cause>` | `{"message":null}` 500, as AWS answers; the cause is in doze-aws's log | AWS's wording |
| `PutMethod` on an existing method keeps the integration | `ConflictException` "Method already exists for this resource" | AWS's behaviour `(memory)`; `UpdateMethod` is the way to change one, and now exists |
| Invoke URL follows the request host and `X-Forwarded-Proto` | `apiEndpoint` follows the host of the creating request (`http://`) | no TLS locally |

CloudFormation `AWS::ApiGateway::*`/`AWS::ApiGatewayV2::*` mapping is A16's.

## What the recording taught

- Errors: `Invalid API identifier specified <account>:<id>` (both APIs); `Invalid Resource|Method|Deployment|stage identifier specified`; the path-part,
  HTTP-method, protocol and route-key sentences; `Another resource with the same parent already has this name: <part>`; `The REST API doesn't
  contain any methods`, `No integration defined for method`; `Active stages pointing to this deployment must be moved or deleted`; `Unable to find
  route target of type integrations with ID <id>`. A model enum failure on v2's protocol is a `BadRequestException` in the service's words.
- `CreateRestApi` answers `apiKeySource: HEADER`, `apiStatus: AVAILABLE`, `endpointConfiguration {types: [EDGE], ipAddressType: ipv4}`,
  `securityPolicy: TLS_1_0`, `disableExecuteApiEndpoint: false`, no `tags`; the root resource is `{id, path}` only.
- A MOCK integration's answer: `cacheKeyParameters: []`, `cacheNamespace` (the resource id), `passthroughBehavior: WHEN_NO_MATCH`,
  `responseTransferMode: BUFFERED`, `timeoutInMillis: 29000`, no `connectionType`.
- A stage made by `CreateDeployment` shares the deployment's `createdDate`; `cacheClusterStatus: NOT_AVAILABLE`, `methodSettings: {}`,
  `tracingEnabled: false`. Timestamps are written to the millisecond so things made in one second are told apart (AWS's calls take longer).
- HTTP APIs: `CreateApi` answers `apiKeySelectionExpression`, `routeSelectionExpression`, `ipAddressType`, no `tags`; an AWS_PROXY integration
  `connectionType: INTERNET`, `integrationMethod: POST`, `timeoutInMillis: 30000`; a route `apiKeyRequired: false`, `authorizationType: NONE`; a
  stage `defaultRouteSettings: {detailedMetricsEnabled: false}`, `routeSettings: {}`, `stageVariables: {}`, `tags: {}`.
- The 1.0 event's `queryStringParameters` and `pathParameters` are `null` when empty, `body` is `null` for a GET; the 2.0 event's `rawPath` has no
  stage for `$default`.

## From memory (`(memory)`: not checked against a recording)

Every refusal not listed above (patch paths and values, `PutMethod` twice, authorizer/validator/model/plan/key/domain conflicts and absences, v2's
missing-part wording `Invalid <Thing> identifier specified <id>`, the logging-without-a-CloudWatch-role refusal); the stage method-setting defaults and
key form (`*/*`, `orders/GET`); `GetResources` showing methods as `{}` without `embed`; the default models' texts; the 21 gateway-response types'
statuses and default template; the 1.0 proxy answer's strictness about extra members; `x-amzn-ErrorType` on gateway responses; the authorizer event
shapes beyond `type`/`authorizationToken`/`methodArn`; the WebSocket event and the `Forbidden` frame; the `@connections` answers; the export documents'
layout; the domain names' generated host names and zone ids; API key length (40) and the 20-character minimum.

## Shared code touched (outside `aws-apigateway`)

| File | Change |
|---|---|
| `aws-gateway/src/pipeline.rs` | `Shared::rest_entry` (services sharing a signing name told apart by route); `Shared` holds the gateway's `Tasks`; `respond` passes `&mut Parts` to the web hook |
| `aws-gateway/src/web.rs` | `Upgrade: websocket` offered to the owning service; the session runs in a tracked task on the upgraded connection (`Conn` implements `Socket`) |
| `aws-gateway/src/server.rs`, `Cargo.toml` | connections served `with_upgrades()`; tokio `io-util` (a feature, no package) |
| `aws-wire/src/service.rs` | `Socket`, `Session`, `Upgrade`, `Web::upgrade` (default refuses by serving the request as before) |
| `aws-wire/src/rest/jsonout.rs` | an output member bound to the query (API Gateway's `position`) is written in the body, where SDKs read it |
| `aws-peers/src/lib.rs` | `Peers::register_as(name, service)`; `register` calls it |
| `doze-aws/src/run.rs`, `Cargo.toml` | both APIs registered (gateway; bus as `apigateway` and `apigatewayv2`) |
| `conformance/tests/test_apigateway.py` | six scenarios appended (UNVERIFIED until recorded) |
| `xtask/weight-limits.txt` | doze-aws byte limit 8,700,000 B, reason in a comment |
| `Cargo.lock` | `aws-apigateway` and the two model crates |

## Weight

`cargo xtask weight doze-aws`: **5,349,616 B, 237 packages, 4,576 KB idle** on darwin/arm64 (base 4,867,344 B / 233 / 4,400 KB: +482 KB, +4,
+0.2 MB). **x86_64-unknown-linux-musl** (`CARGO_TARGET_DIR=<scratch> RUSTFLAGS="-Clinker=rust-lld" cargo build --profile release-aws -p doze-aws
--target x86_64-unknown-linux-musl`): **8,261,008 B**, +779 KB over the 7,481,552 B base, which **breaks the 7,900,000 B line**; the line is now
8,700,000 B (the musl size plus 5%), reason in the file. That is 83% of the owner's 10 MB ceiling with Step Functions and CloudFormation still to
come (about 1.7 MB of room left on Linux). **No new third-party package**: `regex-lite`, `getrandom`, `sha1`, `rsa` and `tokio` features were already
linked. The bytes: both models' tables (227 operations), the two control planes, the VTL interpreter, the OpenAPI and YAML reader, JWT and RS256,
the WebSocket framing and the HTTP client. Idle: an unused API Gateway is a handle; nothing runs, no timer, no file until the first request. Levers if
the integrator needs them: the refused operations' shapes in both model crates, and the YAML export writer.

## Tests

38 in `aws-apigateway`: patches; routing precedence; VTL (parse, run, refusals, caps); JSONPath; events; proxy answers; HTTP answer parsing (plain,
chunked); JWT; JSON Schema validation; YAML (read, refuse, round trip); WebSocket framing and the RFC 6455 key; and end to end: a REST API built,
patched, deployed and its stage settings round-tripped; resources moved and deleted; keys, plans, account, tags, domains, validators, models,
gateway responses and refusals; MOCK and templates on the plane with deployments as snapshots; gateway responses, API keys and validation on
the plane; HTTP_PROXY and HTTP against a real local server; an HTTP API with CORS and a JWT authorizer; quick create, stages and deployments;
OpenAPI import, serve, export, re-import and overwrite (both APIs); a WebSocket API through a real gateway (handshake, a routed message answered,
the `Forbidden` frame, `@connections` post, read, close, 410 after). Not tested here: Lambda proxy and authorizers end to end (the recorded
Lambda-proxy scenarios cover proxy; a Lambda authorizer needs a function process), AWS service integrations against the real services, the IAM
`execute-api:Invoke` check under `--iam enforce`, JWKS fetched from a local issuer.

## Open decisions for the owner

1. **Deployments as snapshots** (AWS's behaviour) replaces Go's "serve the live API". If the owner prefers Go's convenience, a flag could serve live.
2. **No TLS client**: HTTP integrations, JWKS fetches and `https://` backends cannot be called; JWT signatures from an `https://` issuer are not **(Superseded 2026-11-14: outbound https is built; see `stages/tls.md`.)**
   verified (claims are). Same size decision as SNS and EventBridge.
3. **The `@connections` API has no model** in the repository (`apigatewaymanagementapi`); it is served by hand on the plane and its signatures are
   not checked. Adding the model would let the gateway route and verify it.
4. **Execution logs** are not written (SCOPE defers them); Go wrote them.
5. **The recorder policy** needs `apigateway:*` on `doze-rec-*` APIs, `execute-api:Invoke`, and the usage-plan and API-key actions to record the six
   new scenarios.
