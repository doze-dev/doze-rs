# rec3: the planned-operations scenarios recorded on AWS, and doze-aws answering as AWS did (branch `aws-rec3`, 2026-11-19)

Every scenario written ahead of a recording by the planned-operations PRs (#80 to #91) was run against AWS (account in
`DOZE_REC_ACCOUNT`, profile `doze-rec-main`, us-east-1), in passes: record, read the failures, fix the scenario or doze-aws,
record again. Then doze-aws was held to every recording.

## Score (`cargo xtask conformance --save`, all 25 modules)

1,882 labels, **1,846 exact (98%)**; 98% ignoring wording. Every recorded label of API Gateway, CloudFormation, CloudWatch,
EventBridge, IAM, Kinesis, Lambda, CloudWatch Logs, Secrets Manager, SNS, Step Functions and STS matches. What does not:

| Module | Not exact | Why |
|---|---|---|
| dynamodb | 3 | a stream's shard count (AWS 4, doze 1), the restorable window's clock, an export's file count: deviations |
| kms | 2 | `other-curve` (owner's wording, 2026-11-18), `after-encrypt` (AWS records usage later): rec2 |
| lambda_durable | 1 | AWS reads the last invocation's end apart from the execution's: deviation |
| logs_insights | 4 | AWS's internal tokens and clock in a record and a described query, Live Tail and export timing: deviations |
| s3 | 4 | decided earlier (path-style location, trusted signatures, `--iam soft`) |
| sqs | 1 | decided earlier |
| ssm | 1 | `unknown-setting` (owner's wording, 2026-11-18) |

Unverified by design: 12 SSM labels (scenarios the recorder may not run: change templates, see below; public parameter writes;
setting changes), 8 `wire`, 1 Scheduler.

## What AWS taught us (each fixed in doze-aws, or listed in `docs/aws/compatibility-notes.toml`)

About 140 labels answered differently at first. The larger ones:

- **aws-wire**: a member bound with `httpQueryParams` (API Gateway `GetExport`'s `parameters`, also `GetSdk`, `ImportRestApi`,
  `PutRestApi`) was dropped by both REST decoders; it is now the map of query pairs no `httpQuery` member claims.
- **API Gateway**: a documentation version a stage publishes cannot be deleted; documentation, certificate, usage-date and key
  import words; model templates written as AWS writes them; a Lambda authorizer's test-invoke shape; exported documentation in
  location-type order.
- **CloudFormation**: `SignalResource` to a finished wait condition or an absent resource, and drift detection naming an absent
  resource, are accepted; a FAILURE signal has no event of its own; the second "Resource creation Initiated" event;
  `DescribeEvents`' operation and progress events; hook and stack-missing words.
- **CloudWatch**: metric-math refusals ("Invalid syntax", cycle paths), search labels, empty `Dimensions`, log alarm defaults and
  words, composite-rule errors.
- **DynamoDB**: `KeyConditions` with a `FilterExpression` is accepted (only a legacy parameter beside its own expression is
  refused); export prefixes of 3+ characters; full exports show no `ExportType`; imports name the bucket owner; Kinesis
  streaming shows no precision unless set, and a second enable is a ValidationException; compact resource policies.
- **IAM**: virtual MFA seeds are 40 bytes; the same signing certificate again answers the one held; MFA, certificate, SSH key
  and service-credential words and codes; last-accessed details list DynamoDB's and S3's tracked actions.
- **Kinesis**: every `SubscribeToShard` takes over (AWS refused none, even at once; the model says one within 5 seconds is).
- **Lambda durable**: a name already started answers that execution whatever the payload; a second stop answers the first
  stop's time; AWS's words; `Truncated` data; event names; an X-Ray trace header.
- **Logs**: grok takes no type conversions; `parse` fields may not be named again by a bare `fields`; moveKeys moves the
  source *into* a target the event must already have; `GetTransformer` without a transformer answers an empty config;
  `StartTimeOffset` is required; Insights refusals carry AWS's back-end suffix and positions in its rewritten query.
- **SSM**: change templates need `emergencyChange` and approval steps with `timeoutSeconds`; a parameter holds one resource
  policy; policy ids and hashes in AWS's shape.

## Scenarios changed

Waits for AWS's delays (a new key, a new table's backups, a resource policy, Insights indexing, a deleted transformer, an
import that fails); refusals AWS does not give turned into recorded outcomes; ids a refusal quotes masked with `snapshot.known`;
a fixed rule text where a run-numbered name moved a character position; the MFA listings keep only the scenario's own device;
log alarms make their own query role. `harness/normalize.py` never keeps an assumed-role caller (the recorder's SSO session
named a person in one recording; the snapshot was rewritten). No snapshot holds the SSO user, role or account id.

## Not recordable

- `test_sts::test_web_identity_needs_a_token_its_provider_signed`: AWS would have to reach an identity provider on this machine.
- `test_ssm::test_change_template_review_history`: AWS closed Change Manager to new customers (`NoLongerSupportedException`);
  doze-aws keeps change templates (noted).

## Recorder

The permission set gained `iam:CreateServiceLinkedRole` for `kinesisreplication.dynamodb.amazonaws.com` and write access to
`/aws-dynamodb/*` log groups (a DynamoDB import logs as its caller); pasted by the owner 2026-11-19. `teardown.ts` now sweeps
user sub-credentials and MFA devices, server certificates, log alarms, DynamoDB backups, API Gateway client certificates, Logs
destinations and scheduled queries. The account was swept clean after every module.
