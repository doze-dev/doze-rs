# apigw-ops: every planned API Gateway operation (stage note, 2026-11-18)

Branch `aws-apigw-ops`, on `main` at `f864c8dd`. The integrator folds this note into `PLAN.md`, `STATUS.md` and `scoreboard.md`; none of
those, `COMPATIBILITY.md` or `results.tsv` was touched. AWS is the only reference: each behaviour cites the model
(`models/api-gateway.json`, `models/apigatewayv2.json`), the developer guide or the CLI reference; wording not seen recorded is marked
`(memory)` beside it in the code.

**Not recorded.** The `doze-rec-main` SSO session had expired ("Token has expired and refresh failed"), so, as the brief says, nothing
was recorded and no permission was asked for. Nothing was made on AWS; nothing is left there. The seven new scenarios are UNVERIFIED.

## Built (33 operations)

REST (v1, 27): `CreateDocumentationPart`, `GetDocumentationPart`, `GetDocumentationParts`, `UpdateDocumentationPart`,
`DeleteDocumentationPart`, `ImportDocumentationParts`, `CreateDocumentationVersion`, `GetDocumentationVersion`,
`GetDocumentationVersions`, `UpdateDocumentationVersion`, `DeleteDocumentationVersion`, `CreateVpcLink`, `GetVpcLink`, `GetVpcLinks`,
`UpdateVpcLink`, `DeleteVpcLink`, `GenerateClientCertificate`, `GetClientCertificate`, `GetClientCertificates`,
`UpdateClientCertificate`, `DeleteClientCertificate`, `GetModelTemplate`, `GetUsage`, `UpdateUsage`, `ImportApiKeys`,
`TestInvokeAuthorizer`, `TestInvokeMethod`.

HTTP/WebSocket (v2, 6): `CreateVpcLink`, `GetVpcLink`, `GetVpcLinks`, `UpdateVpcLink`, `DeleteVpcLink`, `GetModelTemplate`.

Still refused by name (and `not_planned` in `compatibility-notes.toml`, unchanged): v1 domain-name access associations and SDK
generation; v2 portals, product pages and routing rules. Nothing was moved to `not_planned`.

## 1. Test invocations (`plane/test.rs`)

- The actor finds the method (by `resourceId` and `httpMethod`) or authorizer **in the API as it is now** (a snapshot made on the spot,
  as a deployment makes one), builds the request from `pathWithQueryString`, `headers`/`multiValueHeaders` and `body`, and hands it out
  of the actor (`Out::pending`); `Inner::run` then serves it through the plane's own code: request validation and the integration
  (`rest::test`) for a method, `auth::lambda_v1` or `jwt::cognito` for an authorizer. So a MOCK, a Lambda proxy or custom, an HTTP
  backend or an AWS service integration is really called, with mapping templates run.
- Stage: `test-invoke-stage` with the call's `stageVariables` (`$context.stage` and the Lambda `SourceArn` say so) `(memory)`.
  Authorization and the API key are skipped for a method test `(memory)`. The authorizer test never uses the cache.
- The answer: `status`, `body`, `headers`, `multiValueHeaders`, `latency` (ms), and `log`: `Execution log for request <id>` then
  `<date> : <line>` per step, `<date>` as `Wed Feb 24 00:51:47 UTC 2016` (CLI reference examples). The first line (`Starting
  execution for request:`), `HTTP Method: GET, Resource Path: /pets/1` and the last (`Method completed with status: 200`) are the
  examples'; the lines between are the execution log's (`execlog.rs`, already `(memory)`), with the data trace on and the usage-plan lines
  left out. Authorizer: `clientStatus` 0 when allowed (the model), else the status the client would get; `principalId`, `policy` (the
  returned document as JSON text), `authorization` (the headers given, as lists, as the CLI example shows), `claims` (Cognito's).
  `Starting authorizer: <id> for request: <id>` and `Successfully completed authorizer execution` are the example's; `Incoming
  identity: {...}`, `Unauthorized request: <id>` and the failure lines are `(memory)`.

## 2. Usage metered and enforced (`usage.rs`)

- The stage path did not count before. Now the plane's key check (`Ask::Key`) counts each request a key lets through on the plan that
  puts the key on the stage, per UTC day, in a new `usage` table (`<plan>/<key>` → `{days, grants}`), written with the next group
  commit; days older than 400 are dropped when counted. No timer.
- Enforced: the plan's quota (`DAY`, `WEEK` from Monday plus offset, `MONTH` from the 1st plus offset: the period starts are
  `(memory)`) answers 429 `Limit Exceeded` (`QUOTA_EXCEEDED` gateway response), the throttle a token bucket (`burstLimit` deep,
  `rateLimit` a second, in memory, made on first use) 429 `Too Many Requests` (`THROTTLED`). Both words `(memory)`.
- `GetUsage`: `values` (the model's jsonName) `{keyId: [[used, remaining], ...]}` per day of the range (model), keys paged; a bad date
  or an end before the start is `BadRequestException`; ranges over 366 days are refused (ours). No quota: remaining 0 `(memory)`.
  `UpdateUsage`: `replace /remaining` sets what is left of the current period (a grant kept per period).
- Deleting a plan or key, or taking a key out of a plan, forgets its counts.
- Not enforced: per-method throttles in a plan's `apiStages`, the account throttle.

## 3. Client certificates (`certs.rs`)

`GenerateClientCertificate` makes a real X.509 v3 certificate, DER written by hand (no new crate): a new RSA 2048 key
(the `rsa` crate KMS already links, same features), SHA-256 with RSA, `CN=ApiGateway`, a random 16-byte serial, valid 365 days
("The client certificate generated by API Gateway is valid for 365 days", developer guide). `openssl verify` accepts it. Key size,
algorithm and subject are `(memory)`. The key is made off the actor (`spawn_blocking` in `Inner::run`). Only the PEM is shown; the
private key is kept (`_key`). Tags on `/clientcertificates/<id>` work. **Not done:** presenting the certificate to HTTPS backends
(needs client auth in `aws-webhook`); listed as a difference.

## 4. Documentation (`v1/docs.rs`)

Parts in the API document (`documentation`), locations checked by the model's rules per type (what fields each takes, `name`
required for AUTHORIZER, MODEL, the parameters, REQUEST_BODY, RESPONSE_HEADER; defaults `/`, `*`, `*`), one part per location
(`ConflictException`), `properties` a JSON object. Filters `type`, `name`, `path`, `locationStatus` (DOCUMENTED = non-empty properties,
our reading). Versions snapshot every part; `stageName` sets the stage's `documentationVersion`. `GetExport` with
`extensions=documentation` writes `x-amazon-apigateway-documentation` (the stage's version) and puts `API` parts' description/title/version
on `info` and `METHOD` parts' summary/description/tags on the operations (the model: only OpenAPI fields are published). Import reads
that extension, `merge` or `overwrite`, warnings or `failonwarnings`. Words `(memory)`.

## 5. VPC links (`links.rs`)

Both APIs, kept apart (`v1/<id>`, `v2/<id>` in a `links` table). Create answers `PENDING` (v2 `vpcLinkVersion: V2`), every later read
`AVAILABLE`, with status messages `(memory)`; no timer. v1 takes one `arn:aws:elasticloadbalancing:` target `(memory)`; v2 needs subnets.
A REST `PutIntegration` with `connectionType: VPC_LINK` must name an existing link in `connectionId` (unless a stage variable), and the
plane calls the URI directly. A v2 HTTP integration whose URI is an ARN (listener, Cloud Map) answers 500 with the reason in the log.
Not recorded on purpose: a real link needs an NLB, which bills hourly.

## 6. Model templates (`template.rs`) and `ImportApiKeys` (`v1/import_keys.rs`)

- Generated from the JSON schema: `#set($inputRoot = $input.path('$'))` and the empty model's `{ }` are AWS's (CLI example); strings
  quoted, numbers/booleans bare, objects in place, arrays as `#foreach ... #if($foreach.hasNext),#end`, `$ref`s to other models followed
  once per path: `(memory)` for the layout. The output runs on the plane's VTL (tested).
- CSV as the developer guide's "API key file format": `Name` and `Key` required, case ignored, any order, `Description`, `Enabled`
  (default true), `UsagePlanIds` quoted lists, unknown columns ignored, same value overwrites; bad rows are warnings, `failonwarnings`
  refuses all. Warning texts ours.

## 7. Storage, tests, scenarios

- Three new tables (`certs`, `links`, `usage`) in `apigateway.redb`, counted, reset and flushed with the rest.
- 9 new crate tests (`tests/ops.rs`) plus unit tests (periods, buckets, CSV, DER, locations, templates, log time); 65 tests pass.
- 7 new boto3 scenarios in `test_apigateway.py` (documentation, documentation import, client certificates, model templates, usage
  metered and granted, CSV import, test invocations with a Python Lambda authorizer). Against doze-aws all 18 apigateway scenarios
  pass: 92 recorded labels exact, 0 different, 41 unverified.

## Open

- Recording the seven scenarios when the SSO session is renewed (`apigateway:*` on the run's APIs, keys and plans,
  `apigateway:POST` on `/clientcertificates`, Lambda for the authorizer). `test_usage_is_metered_and_granted` waits up to 10 minutes for
  AWS's usage report.
- Every `(memory)` above, chiefly the middle of the test-invoke log, the period starts, the 429 words, the template layout.
- mTLS with the stage's client certificate; CloudFormation `AWS::ApiGateway::DocumentationPart`/`DocumentationVersion`/
  `ClientCertificate`/`VpcLink`/`UsagePlan` usage mapping is A16's crate and untouched.
