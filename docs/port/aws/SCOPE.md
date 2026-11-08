# doze-aws in Rust: local-development scope

Status: **draft for the owner's approval, 2026-11-08.** Merges six relevance reviews into one scope. Nothing here is decided until
the owner says so; section 8 lists the PLAN.md edits to approve and the open decisions. `PLAN.md` is unchanged.

Class vocabulary used throughout: **BUILD**, **ACCEPT-AND-FLAG**, **STAY CUT**, **NOT NEEDED** (section 2). Effort: S up to 2 working
sessions, M 3 to 6, L 7 or more; these are the reviewers' guesses, see section 1.

## 1. Purpose and method

**Purpose.** Decide, for the Rust port, which AWS features a developer running one laptop against doze-aws needs built, which only need
to be accepted and stored so that a template or Terraform run finishes, which stay refused, and which are not needed at all. The
output feeds the phase gates in `PLAN.md` (A0 to A17) and the console's Scope board.

**Method.** Six reviewers each read one domain and judged it against the same question: messaging and events (SQS, SNS, EventBridge,
Kinesis), storage (S3, DynamoDB), compute (Lambda, Step Functions, API Gateway v1/v2), config (SSM, Secrets Manager, KMS),
observability and infrastructure (Logs, CloudWatch, CloudFormation), identity (IAM, STS, policy enforcement). Each row carried an
evidence tag (`iac`, `emulator`, `docs`, `reasoning`) and a confidence. This document dedupes them, resolves contradictions, adds the
cross-cutting themes, and re-checks the claims about the Go code.

**Evidence limits (read the confidence accordingly).**
- **No usage telemetry** for doze, CDK, Terraform, SAM or any emulator. "Common" means "emitted by a default or documented path of a
  mainstream tool", never a measured share. Relevance is judgement.
- **Competitor evidence is thin.** LocalStack ended its community edition on 2026-03-23; it is now one image that needs an account and
  token, and its pages say "Hobby, Base, Ultimate" without marking most features, so free versus paid cannot be told per feature.
  Floci's pages are the project's own claims and were not run. moto pages were read through a summarising fetcher. Terraform registry
  pages did not render; provider source and docs on GitHub were used.
- **Effort sizes are rough guesses** by the reviewers, from Go line counts and plan documents, not sized against the Rust workspace.
  `PLAN.md` itself notes past estimates ran low by a step and re-bases at A5. No total is given here for that reason.
- **AWS behaviour is unrecorded.** `conformance/snapshots/` is empty; anything not quoted from a page is `memory` until the recorder
  keys exist. Items needing a recording are marked in section 8.
- **Nothing was run against AWS or the other emulators.** One check was run on the Go code: the shared event-pattern matcher (row b).

**Go claims, re-checked against `origin/main`.** The default checkout (a2d1e54) is 8 commits behind `origin/main` (f8c31a1). I ran
`git fetch` (refs only) and read files with `git show` and `git grep` on `origin/main`. Those 8 commits touch endpoint and name plumbing
(`cmd/`, `module/`, `internal/awshost`, `internal/config`), one S3 addressing change (`s3/s3.go`), docs (`SUPPORT.md`, `lambda.md`) and site
media. None touches the code behind any claim below, so every verdict holds on both. All cites are `origin/main`.

| Claim | Verdict | Evidence |
|---|---|---|
| (a) Lambda SQS poller deletes the whole batch on a non-error return; ignores `ReportBatchItemFailures`, `FilterCriteria`, the batching window | VERIFIED | `lambda/extras.go:607-611` deletes every message when `err == nil && res.FunctionErr == ""`; the returned payload is never read; records carry six fields (`:578-583`); the batch is gathered by size only (`:547-559`). No `batchItemFailures` in any non-test file; `lambda/validate.go:142-149` only checks field shapes. The mapping record stores six fields (`lambda/store.go:97-104`), so these members are not even stored or returned. |
| (a2) Stream failure semantics | VERIFIED | Kinesis: "a failed invoke does not replay" (`extras.go:771-772`); the DynamoDB poller advances its iterator before the invoke and does not replay (`:616-660`, "at-least-once, no ack"). |
| (b) SNS `FilterPolicyScope: MessageBody` is matched against attributes | VERIFIED, outcome CHANGED vs the docs | `sns/delivery.go:67` calls `matchFilter(sub.FilterPolicy, attrs)` for every subscription; the scope is only stored (`sns/store.go:47-49`) and never read. I ran the shared matcher: a body-style policy against an attribute-less message gives `match=false`, so the message is **dropped**, not "more permissive" as `docs/SUPPORT.md:2453-2455` says. Messaging reviewer was right; the doc is wrong. |
| (c1) Any `Custom::*` fails the whole template | VERIFIED | `classify` (`internal/cfn/resources.go:543-559`); `serviceOf("Custom::X")` returns `X` (`:562`), so the message is "doze-aws does not serve X" (`:556`). `transpile.go:191-195` returns an error unless `AllowUnsupported`, which only the stack-delete path sets (`cloudformation/stacks.go:625`). `AWS::CloudFormation::CustomResource` takes the same path. |
| (c2) Metric-math alarm fails the stack | VERIFIED | `internal/cfn/mapper_cloudwatch.go:24-28` returns an error for `Metrics`; `ThresholdMetricId` likewise (`:29-32`). `AWS::CloudWatch::CompositeAlarm` is in neither table, so "does not serve CloudWatch". |
| (c3) `AWS::Kinesis::Stream` creates nothing | VERIFIED | `mapper.go:89-92` returns nil ("no stack-file section yet"); `docs/cloudformation.md:207`. A non-SQS `EventSourceMapping` is likewise accepted without creating one (`mapper.go:722-728`). |
| (c4) `AWS::Lambda::EventInvokeConfig` rejected | VERIFIED | No mention in `internal/cfn`; not in `nameProperty`; falls to "does not serve Lambda". |
| (c5) SSM-typed parameters and `{{resolve:...}}` not resolved | VERIFIED | No `resolve:` handling anywhere in `internal/cfn` or `cloudformation/`. `coerceParam` (`transpile.go:294-311`) returns the literal default for `AWS::SSM::Parameter::Value<...>`, so a `Ref` yields the parameter **name**, silently. |
| (c6) IAM resources skipped | VERIFIED, with a gap | Role, Policy, ManagedPolicy, InstanceProfile, User, Group, ServiceLinkedRole are ghost-identity skips (`resources.go:31-37`). `AccessKey` and `UserToGroupAddition` are in neither table, so rejected. |
| (c7) Default CDK `Secret` stored as `{}` | VERIFIED | `mapper.go:931-938`: the `SecretStringTemplate` verbatim, else `{}`; `GenerateStringKey` ignored. That CDK always emits `GenerateSecretString` is the reviewer's reading of CDK source, not re-checked. |
| (c8) Other CFN gaps the reviewers named | VERIFIED | Any Transform but SAM refused, so `AWS::LanguageExtensions` fails (`transpile.go:105-108`); SAM events other than SQS, SNS, Schedule/ScheduleV2, EventBridgeRule, HttpApi, Api fail (`mapper.go:778-827`); S3 mapper reads no `EventBridgeConfiguration`, `BucketEncryption`, logging, replication or accelerate. `docs/cloudformation.md:228-229` says `AWS::CloudWatch::*` is skipped; code maps Alarm, Dashboard, MetricFilter and rejects the rest. |
| (d) REST `UpdateMethod/Integration/MethodResponse/Deployment` answer 405 | VERIFIED by reading, not run | The REST handler table (`apigateway/router.go:203-240`) has Put/Get/Delete for methods, integrations and their responses and no Update*; `UpdateDeployment` and `UpdateIntegration` exist for v2 only (`:278`, `:284`). A PATCH on a path that has other verbs reaches chi's `MethodNotAllowed` (`internal/restroute/restroute.go:168`, `router.go:325`). |
| (e) Lambda gets `AWS_ACCESS_KEY_ID=test`, so soft mode cannot show a function's missing permissions | VERIFIED (inputs); consequence follows, not run | `internal/lambdaruntime/env.go:58-60` and `awsident/ident.go:35-36`. `sts/actions.go:119-125` answers `user/test` to everyone; AssumeRole credentials are random and recorded nowhere (`:62-72`). `SUPPORT.md:1111-1115`: with no IAM identities the caller is the account root, admitted. |
| (f1) Pollers wake every 0.5 to 2 s per mapping | VERIFIED | SQS long-polls 2 s per loop (`extras.go:548`), 1 s sleep on error (`:560`); DynamoDB stream 500 ms when empty (`:657`); Kinesis 500 ms when idle (`:776`); one goroutine per enabled mapping. |
| (f2) Step Functions history has no pruning | VERIFIED | `stepfunctions/` expires only task-token tombstones (`store_activities.go:107-117`); `recordFinished` only counts metrics (`metrics.go:43`). |
| (f3) Step Functions 1 Hz tick while a Wait is parked | VERIFIED (reviewer left it untraced) | Ticker runs while `g.runs` is non-empty (`scheduler.go:54-72`); a run leaves `g.runs` only when it finishes (`engine.go:518`). |
| (g) EventBridge cannot target Step Functions | VERIFIED | `eventbridge/actions.go:189-221` handles sqs, lambda, sns, logs, API destination; everything else logs "unsupported target service" (`:220`). Kinesis, event bus and API Gateway targets are equally absent; SQS is sent with no group id. No emitter of `aws.s3`, `aws.states` or `aws.cloudwatch` events exists. |
| Extra: S3 lifecycle widens non-prefix filters (S3-39) | VERIFIED | `lifecycleRule` has only `Prefix` and `Filter.Prefix` (`s3/features.go:216-240`); tag, And and size filters unmarshal to empty, so the sweep (`:268-299`) covers the whole bucket. |
| Extra: `UpdateTable` ignores `StreamSpecification` | VERIFIED | `dynamodb/tables.go:375-`; the field is read only by CreateTable (`:55`, `:214`). |
| Extra: SAM `ScheduleV2` already maps | CHANGED | `mapper.go:778` maps it to a rule. The messaging review cited SAM ScheduleV2 as demand for the Scheduler service; that path is already served. |
| Extra: alarm evaluator "ticks while any alarm exists" | CHANGED | `cloudwatch/evaluate.go:39,217-229` ticks every 10 s unconditionally, started in `New` (`cloudwatch.go:119`). |
| Extra: EventBridge scheduler | CHANGED | Ticks every 1 s while a schedule rule exists and every 5 s when none (`eventbridge/scheduler.go:26,40`), so it is never idle. |

Not re-checked (reviewer claims, taken as stated): Lambda foreign-account layer refusal, the Kinesis KMS per-call check, IAM-2 trust
policy, SSM-1 `AllowedPattern`, DDB-12 PITR, every `plan/services` finding id.

## 2. The decision model

| Class | Rule | Wire behaviour | Console (Scope board) |
|---|---|---|---|
| **BUILD** | An application or test needs the behaviour, or a template cannot apply without it and the effect can happen on this machine | Works; refuses what AWS refuses | Drawn as a normal feature |
| **ACCEPT-AND-FLAG** | A template, SDK or Terraform refresh reads or writes it, but nothing local acts on it (retry policy, encryption setting, capacity hint, config for an absent service) | Accepted, stored raw, returned exactly as sent (no drift), validated lazily from the model; flagged "stored, not active locally" | Control may be drawn with the quiet line "Stored, not applied locally" and a **Not active locally** badge |
| **STAY CUT** | The feature's point is an effect that leaves the machine, or it needs a service, transport or infrastructure doze-aws does not have, and a template does not need it to apply | HTTP 501 `NotImplemented` with `X-Doze-Not-Built` and a one-line reason (PLAN quality rule 14 and its 501 bullet) | No screen; where it is a choice inside a form, the option is shown **disabled with its reason**; the operation appears as a **NOT BUILT** wire result |
| **NOT NEEDED LOCALLY** | Neither a template, an app nor a local authorization test uses it: producers of reports, metrics, copies and timers | Not built. Where the call can occur, it is STAY CUT (501); the setting, if any, is ACCEPT-AND-FLAG | Nothing drawn |

How to choose, in order: (1) does a template or Terraform run write or read it? If yes, it must not fail, so it is at least
ACCEPT-AND-FLAG. (2) Does an application's behaviour depend on it, and can that happen on one machine? If yes, BUILD. (3) Otherwise the
machinery is NOT NEEDED. A feature is often split: its **setting** is ACCEPT-AND-FLAG, its **machinery** (the scaler, the copier, the
report producer) is NOT NEEDED. "Silent drop" is never a class: a field is stored and echoed, or the call is refused by name.

DEFER is a schedule, not a class. An item marked DEFER-IF-BUILT sits in one of the four classes and ships after the first cut; its
slot (a shape, an empty layer) is designed now.

Tension to keep in view: the console's scope rule treats "C-tier whose point is an effect that leaves the machine" (email, SMS, mobile
push, Firehose, replication) as S-tier. ACCEPT-AND-FLAG at the API does not contradict that if the console keeps those options disabled;
whether to accept the *calls* is decision D1 in section 8.

## 3. Cross-cutting themes

### 3.1 The IaC apply path

CloudFormation is the integration point: CDK synthesises it, SAM and Serverless transform into it, and Terraform meets the same
refusals through the API. In Go, any unmapped resource type aborts the whole stack (`transpile.go:191-195`), so one incidental VPC or
`Custom::` resource blocks an otherwise servable stack. All three peers default permissive (LocalStack ignores or mocks, Floci stubs with
a warning, moto warns); the cost of that is silent half-deploys, which is what the report-on-the-resource rule below prevents.

**Resource-type policy (recommended; observability review, adopted by the others).**
- **MAP**: create it locally. **SKIP+WARN**: accept, give it an identity so `Ref` and `GetAtt` resolve (id-shaped values; hostname-shaped
  attributes under the reserved `.invalid` top level so a connection fails loudly), report `CREATE_COMPLETE` with a
  `ResourceStatusReason` such as "Not created locally by doze-aws: stored, not active", print it in the transpile report, list it in the
  console. **FAIL**: refuse at CreateStack or CreateChangeSet, before provisioning anything, naming the resource and the reason.
- Choose SKIP+WARN when the resource has no behaviour the application can observe and its attributes are opaque ids, ARNs or names. Choose
  FAIL when application code must connect to it, or a skipped copy would change behaviour without the developer noticing.
- A switch `[cloudformation] unsupported = "default" | "skip" | "fail"` with per-type overrides; `TranspileOptions.AllowUnsupported`
  already exists internally but is used only to re-derive a stack for deletion.
- Fix regardless: `classify` calls an unknown type "does not serve <Service>" even for served services (`Lambda::EventInvokeConfig`,
  `CloudWatch::CompositeAlarm`) and names the suffix of a `Custom::` type as the service. Distinguish "unknown to this version" from
  "service not served". Do not port the `docs/cloudformation.md` claim that `CloudWatch::*` is skipped.

| Policy | Families |
|---|---|
| MAP | The core mapped set (SQS, SNS, S3, DynamoDB, Lambda, Events, KMS, Secrets, SSM, Step Functions, Logs, API Gateway v1/v2, nested Stack); plus the types that are served but not created today: Kinesis Stream, Kinesis/DynamoDB event source mappings, `Lambda::EventInvokeConfig`, `Events::EventBus/Archive/EventBusPolicy`, SAM `S3`/`DynamoDB`/`Kinesis`/`CloudWatchLogs` events, state-machine `DefinitionUri`/`DefinitionS3Location` (fetch from local S3), the `AWS::LanguageExtensions` transform, IAM Role/Policy/ManagedPolicy/User/Group/AccessKey/UserToGroupAddition (the Go plan's decision 2, as cited by the identity and observability reviews) |
| MAP, stored (ACCEPT-AND-FLAG) | Queue/Topic inline policies, Secrets `ResourcePolicy`/`RotationSchedule`/`SecretTargetAttachment`, API Gateway `Model`/`RequestValidator`/`GatewayResponse`, CloudWatch math/composite/anomaly alarms, Logs resource/account/query-definition types, SSM Document, OIDC/SAML providers |
| SKIP+WARN | EC2 networking (VPC, Subnet, IGW, route tables, NAT, EIP, SecurityGroup, NACL, endpoints); RDS subnet and parameter groups; Application Auto Scaling; CloudFront, Route 53, ACM, WAFv2; governance (X-Ray, Config, CloudTrail, GuardDuty, Backup and similar); SSM Association/MaintenanceWindow/PatchBaseline; API Gateway DomainName/BasePathMapping/VpcLink/ClientCertificate/DocumentationPart; CDK `Custom::` helpers that only touch EC2 or IAM; IAM InstanceProfile/ServiceLinkedRole |
| FAIL (default; downgradeable) | Databases and caches (RDS instances/clusters, ElastiCache, MemoryDB, DocumentDB, Neptune, OpenSearch, MSK, Redshift, EFS); EC2 compute; ECS, ELBv2, EKS, Batch, AppRunner; Cognito; AppSync, Glue, Athena, Firehose, SageMaker, Bedrock, Cloud Map; WebSocket APIs; Macro, Hook, Module, SAR application |

**Custom resources.** Native in-process handlers first for the CDK built-ins that are brittle as real Lambdas
(`Custom::S3AutoDeleteObjects`, bucket notifications, `LogRetention`: S each; `CDKBucketDeployment` and `AwsCustomResource`: M each), then a
generic Lambda-backed runner (M) with a plain-http presigned response URL. Why native first: doze runs functions as host processes and
CDK's Node handlers import `@aws-sdk/*` expecting the Lambda runtime (reviewer's reasoning, unverified). Total L (obs, sto).
**Dynamic references and SSM-typed parameters** resolve from local SSM and Secrets Manager at create or update only; an unresolvable
reference is an error, never an empty string. **IAM resources** map into the IAM store; this is the gate for the identity work (3.2).

### 3.2 IAM and identity depth

State in Go (verified, 1(e)): soft mode is the default and silent when no IAM identities exist; Lambda runs as `test`; CloudFormation
skips `AWS::IAM::*`; AssumeRole mints credentials that nothing recognises. So the evaluator is exercised only by hand-made users and the
most useful message it could give ("your function role lacks `sqs:SendMessage`") is unreachable. Peers: moto and Floci already honour
sessions; LocalStack charges for enforcement and policy streams; doze's soft mode plus `DozeGeneratePolicy` is free.

**Build order:** (1) sessions real (trust policy lenient in soft mode so `cdk deploy`'s bootstrap-role assumptions work; `GetCallerIdentity`
never denied; stateless signed session tokens so there is no session store and no restart loss; revocation unsupported and said so);
(2) `AWS::IAM::*` from CloudFormation into the store; (3) Lambda, Step Functions and EventBridge targets run as their roles, with
`iam:PassRole` checked at the receiving service.

**Evaluator on day one** (costly to retrofit): typed policy AST with policy variables and `BinaryEquals` in the first grammar; a typed
`Principal` (Root, User, RoleSession, FederatedUser, Service with `aws:SourceArn`/`SourceAccount`, Anonymous); context keys in three states
(supplied, absent, known-to-AWS-but-not-supplied-locally, so enforce says "result unreliable" instead of a confident wrong answer); a
decision trace (verdict, deciding layer, matched statements, unsupplied keys) feeding the access log, AccessDenied text, simulator and
policy generator; lazy per-service resolution tables; purity (no I/O, `now` passed in); a never-evaluated list (`GetCallerIdentity`,
`GetSessionToken`); soft as a first-class result.

| Layer | Fed in v1 | Note |
|---|---|---|
| Identity (user, groups, role, managed) | yes | AWS-managed documents: record exact ones for the served services and the Lambda execution roles from the AWS account; flag the rest "approximate" |
| Resource policy | yes (seven services today) | EventBridge bus, DynamoDB, API Gateway REST, Logs, layers: store and flag, evaluation later |
| Permission boundary | yes | Stored at create and returned by Get (Go drops it at create, so Terraform diffs forever) |
| Service gate (KMS key policy gating identity policies) | yes | |
| Session policy | empty slot | STS validates and carries the document; evaluating later is one intersect |
| SCP/RCP | empty slot, no feed | No Organizations API; "paste an SCP from config" is the cheap form if ever asked |

Later, slot only: web-identity trust conditions on token claims, session tags and `SourceIdentity` as evaluated data, `aws:ResourceTag` and
`aws:RequestTag` beyond S3, SQS, SNS, DynamoDB, Lambda and Secrets, cross-account (foreign account ids are accepted as strings and flagged).
`aws:MultiFactorAuthPresent` is never set (faithful for long-term keys).

### 3.3 Idle weight

Headline numbers (PLAN spike C): Go idles at 132 to 583 wake-ups per 30 s; the Rust target is 0. Every Go timer below is verified to exist on
`origin/main`; the Rust design column is the reviews' recommendation.

| Timer or poller | Go today | Rust design | What the lazy form gives up |
|---|---|---|---|
| Lambda mapping pollers (SQS, DynamoDB stream, Kinesis) | `extras.go:548,560,657,776`: wake every 0.5 to 2 s per enabled mapping, forever | A notifier per queue and stream (an `aws-peers` message); the poller awaits it; one `Expiring` entry for the next visibility or delay expiry; checkpoint only on batch success | Nothing observable in one process; needs a notify hook in SQS, DynamoDB streams, Kinesis. Largest idle cost in the Go build |
| Step Functions driver | `scheduler.go:54-72`: 1 Hz while any execution is loaded, including a Wait of hours | Sleep to the earliest `WakeAt` or deadline, or a nudge | Nothing at AWS's 1 s Wait resolution |
| CloudWatch alarm evaluator | `evaluate.go:39,217`: 10 s, unconditional | **Deadline-driven**: keep each alarm's next possible state-change instant, one timer to the earliest, none when no alarm is pending | Nothing observable (see 7) |
| CloudWatch sample sweeper, Logs retention | `cloudwatch.go:129`, `logs.go:174`: 1 min | Trim on write and read when the oldest-event watermark passes retention | Physical deletion waits until something touches it |
| S3 lifecycle janitor | `s3.go:141`: 1 min even with no rule | Arm on the first enabled rule; sleep to the next UTC midnight; a "run now" hook for tests; **fix filter semantics (tag, And, size) before enforcing** | Minute-level expiry in tests, unless the hook or a clock override is used |
| DynamoDB TTL janitor | `dynamodb.go:116`: 1 min even with no TTL table | Arm when any table has TTL (also at boot); reads keep filtering lazily; sweep every few minutes while armed | The stream REMOVE record and disk reclaim arrive at the next sweep (AWS itself waits up to days) |
| EventBridge rule scheduler | `scheduler.go:26,40`: 1 s with rules, 5 s with none | `Expiring` keyed by the next fire from `aws-cron`; no rules, no timer | Nothing |
| SQS, Kinesis, KMS, Secrets, SSM janitors | 1 min each (`sqs.go:122`, `kinesis.go:142`, `kms.go:126`, `secretsmanager.go:127`, `ssm.go:119`) | Visibility, delay, retention, scheduled deletion and parameter expiration as `Expiring` entries; parameter expiry also checked lazily at read | Physical removal timing; expiry events |
| SQS message-move task | `move_task.go:225`: 100 ms tick while running | Defer the feature (section 6) | |
| Lambda warm-process reaper | `pool.go:147`: one timer per pool | Already lazy; keep the shape | |

Rules the reviews converge on: (1) sleep to the next deadline, never tick; (2) notify, never poll; (3) arm on configuration, so an unused
feature costs nothing; (4) compute on read where nobody can observe the difference (ConsumedCapacity arithmetic, restore expiry, usage-plan
refill by timestamp); (5) bound by count on the write path instead of sweeping (Step Functions history: newest N executions per machine, a
cap on events per execution, an Express ring); (6) generate big tables and key material on first use (validation tables, RSA-4096).
Counter-evidence kept: the Go build measured that folding nine janitors into one saved about 25 KB and five wake-ups a minute, so merging
is not the lever; arming and deadlines are. Eager regex compilation was 46% of the idle heap, which is why tables are lazy (PLAN rule 12).

### 3.4 The generic accept-and-store path

Why it is a foundation item: dropped fields cause permanent Terraform diffs and refused calls fail the whole apply. Verified examples in
Go: the Lambda mapping stores six fields (`store.go:97-104`) and `UpdateTable` ignores the stream specification; reviewers add PITR
answering DISABLED after being enabled (DDB-12), KMS `MultiRegion: true` reading back false (KMS-16, P0), permission boundary dropped at
create (IAM-40), API Gateway `cacheClusterEnabled` read back false (AGW-15).

Design: per service, one table of `(kind, id) -> raw document` plus an `active: false` marker; validate lazily from the model; return
exactly what was sent on Get, Describe and List; no typed model per family and no timer. The flag must travel where the user looks: a
response header beside `X-Doze-Not-Built` (name to choose), the access log, the CloudFormation `ResourceStatusReason` and transpile
report, a `StateReason` where one exists (alarms), and the console badge. One line per stored setting is printed at `doze-aws apply`.
It must not fake an effect: no pretend delivery, no READY that implies capability.

First families to wire (all ACCEPT-AND-FLAG): Lambda (full mapping fields, foreign and AWS-hosted layer ARNs such as Powertools,
provisioned concurrency, code signing, SnapStart/RuntimeManagement/RecursiveLoop/scaling blocks, weighted alias routing, VpcConfig and
similar settings dropped on update today); API Gateway (stage cache, canary, tracing, WAF, usage-plan throttle and quota, resource policy,
custom-domain records); S3 (`BucketEncryption`, logging, replication, accelerate, requester pays, MFA delete, lifecycle transitions,
website routing rules, and one id-keyed raw store for the 16 analytics, inventory, metrics and intelligent-tiering operations);
DynamoDB (PITR with a true round trip, table class, warm and on-demand throughput, SSE, resource policy, replicas, contributor insights);
SQS/SNS/Kinesis (SSE and KMS flags, topic attributes, subscription redrive and delivery policy); EventBridge (bus policy, target DLQ,
retry and parameter blocks, bus LogConfig, archive retention); SSM, Secrets, KMS (parameter policies and service settings, rotation
rules and replicas, key policy, grants, `MultiRegion` with an `mrk-` id, rotation flag and period, key specs generated lazily);
Logs and CloudWatch (resource, account, data-protection and index policies, query definitions, math/composite/anomaly alarms,
non-deliverable alarm actions); IAM/STS (OIDC and SAML provider records, password policy, login profiles, session policy and tags carried,
foreign account ARNs); Step Functions (tracing and encryption blocks, alias weights).

## 4. The top 20, consolidated and ordered

Source keys: msg messaging, sto storage, cmp compute, cfg config, obs observability, idn identity. Items several reviews raised are merged
(Lambda mapping semantics appeared in msg, cmp, sto, obs; accept-and-store in all six).

| # | Recommendation | Why (evidence) | Class | Effort | PLAN phase |
|---|---|---|---|---|---|
| 1 | CloudFormation resource-type policy: MAP / SKIP+WARN / FAIL, a config switch, an honest "unknown vs not served" message, skipped types reported on the resource | Any unmapped type aborts the stack today (`transpile.go:191-195`); one VPC or `Custom::` blocks a servable stack (obs; sto, cfg, cmp, idn all depend on it) | BUILD, with ACCEPT-AND-FLAG for skipped types | M | A16; the flag record in A4 |
| 2 | Lambda mappings: store every field; honour `ReportBatchItemFailures`, `FilterCriteria`, batching window, `StartingPosition`, retries and OnFailure; notify-driven pollers; mappings from templates | Go deletes the whole SQS batch (`extras.go:607-611`), advances stream iterators regardless (`:771-772`), stores six fields; largest idle cost (msg, cmp, sto, obs) | BUILD | M to L (batch failures S, filters S, window S, stream controls M) | A13, with notify hooks in A5, A10, A11 |
| 3 | Generic accept-and-store path with the "Not active locally" flag, wired family by family (3.4) | Dropped fields are perpetual Terraform diffs; a refusal fails the whole apply (all six) | ACCEPT-AND-FLAG | M for the path, S per family | A4 (record and flag), used A5 to A16 |
| 4 | Real role sessions: AssumeRole credentials resolve to the role, trust policy lenient in soft mode, `GetCallerIdentity` never denied | Go mints credentials nothing recognises and answers `user/test` to all (`sts/actions.go:65-125`); moto and Floci do it (idn) | BUILD | M + S + S | A7 (Principal in A3 and A4) |
| 5 | Zero-idle rules on every timer: deadline-driven alarms, notify-driven pollers, arm-on-configuration janitors, lazy retention (3.3) | Go has the timers in 3.3, three unconditional; PLAN target is 0 wake-ups (obs, sto, cmp, cfg) | BUILD (design rule) | S per janitor, M for alarms | A1 (`Expiring`), A4, each service phase |
| 6 | Map `AWS::IAM::*` from CloudFormation into the IAM store (including AccessKey, UserToGroupAddition, boundary) | Roles and grants never reach the evaluator (`resources.go:31-37`); gate for 7 (idn, obs) | BUILD | M | A16, needs A7 |
| 7 | Run Lambda, Step Functions and EventBridge targets as their execution roles; check `iam:PassRole` at the receiving service | Functions run as `test` (`env.go:58-60`), so soft mode cannot say a role lacks a permission; LocalStack charges for the equivalent (idn) | BUILD | M, then S per service | A13, A14, A12, A15 |
| 8 | Evaluator with its full shape on day one, four layers fed, two empty slots (3.2) | Retrofitting variables or a decision trace is the expensive way (idn) | BUILD | Within A4's own sessions | A4 |
| 9 | CloudFormation custom resources: native CDK built-ins first, then BucketDeployment and AwsCustomResource, then a generic Lambda-backed runner | Any `Custom::*` aborts with a misleading message (`resources.go:556,562`); CDK emits them for `autoDeleteObjects`, `addEventNotification`, `BucketDeployment` (obs, sto) | BUILD | L in total (S, S, S, M, M, M) | A16, needs A9 and A13 |
| 10 | CloudFormation dynamic references and SSM-typed parameters, resolved from local SSM and Secrets | A `Ref` yields the parameter name, not the value (`transpile.go:294-311`); every default CDK stack carries `BootstrapVersion` (cfg, obs) | BUILD | S to M | A16, needs A6 |
| 11 | EventBridge targets that exist locally (Step Functions, Kinesis, EventBus, API Gateway), `SqsParameters`, complete input transformer, DLQ and retry round trip | Dispatch handles five target kinds (`actions.go:189-221`); others are accepted then dropped (msg; cmp flags the same gap) | BUILD, ACCEPT-AND-FLAG for no-service targets | S each, M together | A12 (Step Functions dispatch lands with A14) |
| 12 | S3 events complete: EventBridge configuration, per-key events from DeleteObjects, delete-marker events, TestEvent; SAM S3 event | Stored and never delivered; mapper drops the setting; DeleteObjects raises nothing (sto) | BUILD | S + S | A9, A12 |
| 13 | DynamoDB streams through every door (UpdateTable, template, stream mappings) and the DynamoDB-to-Kinesis destination | `UpdateTable` ignores the spec (`tables.go:375`); the not-built reason is stale now that Kinesis is local (sto) | BUILD | S + M | A10, A11, A16 |
| 14 | REST `UpdateMethod`, `UpdateIntegration`, `UpdateMethodResponse`, `UpdateIntegrationResponse`, `UpdateDeployment`, `FlushStageCache` | Create works and the next edit meets a 405; Terraform change-in-place calls them (cmp) | BUILD | S | A15 |
| 15 | OpenAPI import (`ImportRestApi`, `PutRestApi`, `ImportApi`, `ReimportApi`; CFN `Body`, `DefinitionBody`), in-house subset parser | Terraform `body`, SAM `DefinitionBody`, CDK `SpecRestApi` deploy through it; not-built files it under "publishing" (cmp) | BUILD | M subset, L full | A15; reclassify in not-built |
| 16 | SNS `FilterPolicyScope: MessageBody` | Go ignores the scope and drops body-style messages (`delivery.go:67`; run check); shares the EventBridge pattern engine (msg) | BUILD | S | A8 |
| 17 | Secrets apply path: generate the password for `GenerateSecretString`, return rotation rules, accept `RotationSchedule`, `ResourcePolicy`, `SecretTargetAttachment` | Go stores the template or `{}` (`mapper.go:931-938`) (cfg) | BUILD | S | A6, A16 |
| 18 | Parameter Store that refuses and pages as AWS does: `AllowedPattern`, 4 KB / 8 KB tiers, 10-per-page paging, write rules | `AllowedPattern` silently dropped (SSM-1, P0); callers that forget to page pass locally (cfg) | BUILD | S each | A6 |
| 19 | Step Functions local-testing set: Wait scaling, mock config in Step Functions Local's format, `TestState` completeness | AWS marks Step Functions Local unsupported and points at `TestState`; LocalStack and Floci ship mocks; a `Wait 3600` is untestable (cmp) | BUILD | S + M + S to M | A14 |
| 20 | CloudWatch kept small and always-apply: accept-and-flag math, composite and anomaly alarms and non-deliverable actions; `SetAlarmState` to SNS and Lambda; PutMetricData and GetMetricData; Lambda producer and EMF ingestion, lazily | Math alarm aborts the stack today (`mapper_cloudwatch.go:24-28`); composite rejected (obs) | BUILD (subset), ACCEPT-AND-FLAG | M to L subset, S flag | A11, A16 |

**Next in line, not in the 20.** KMS key policy, `MultiRegion` with `mrk-` ids and stored grants (S; Terraform otherwise plans a key
replacement every run); Lambda accept bundle (S); `kms:Decrypt` check on CMK-backed values when IAM is on (M); lite JWT and Cognito
authorizers (S); API Gateway gateway responses (S) and request validators with models (M); HTTP Task (S to M) and wider `aws-sdk:` and
`apigateway:invoke` integrations (S); native service events on the default bus through one publish hook (S3 object events, Step Functions
status, CloudWatch alarm state, SSM Parameter Store Change, Secret Label Updated); `_local_` code reload on change (S); opt-in fault
injection for Kinesis and DynamoDB (S); SAM `S3`, `DynamoDB`, `Kinesis` events (S each); `ListSecrets` filters and paging (S); Logs
read API with space-delimited filter patterns (M); S3 default object-lock retention, ACL refusal under BucketOwnerEnforced and
archive-class read refusal (S each); refuse DynamoDB legacy parameters instead of ignoring them (DDB-32, P0, S); `ConsumedCapacity` (S).
Correctness items already in the Go findings (951) travel through the behaviour catalogue, not this list.

## 5. STAY CUT and NOT NEEDED, per domain

"Setting" means the configuration is still ACCEPT-AND-FLAG (3.4). Items marked **revisit** are the reviewers' first candidates after the
first cut.

| Domain | Decision | Items and the one-line reason |
|---|---|---|
| Messaging | STAY CUT | SNS message signing (a validator wanting a certificate cannot be satisfied; keep the envelope fields); SNS SMS sandbox and origination numbers (not in LocalStack or moto); EventBridge Pipes (LocalStack Ultimate only; a pipe is a trigger joined to a target; **revisit** after Scheduler), schemas registry, global endpoints, partner sources, VPC Lattice connectivity; asynchronous or cancellable replays (racy tests); Kinesis Analytics, Managed Flink, Video, channels; `SubscribeToShard` (needs HTTP/2, which PLAN refuses; KCL with enhanced fan-out cannot run locally; **revisit**, size L) |
| Messaging | NOT NEEDED | SQS FIFO throughput, in-flight and TPS quota enforcement (settings only); SNS retry schedule and delivery-status logging beyond accepting the role attributes; EventBridge 24-hour retry (flag the difference) |
| Storage | STAY CUT | S3 Select and Object Lambda (closed to new customers), Torrent, S3 Express and directory buckets (make `ListDirectoryBuckets` refuse), `UpdateObjectEncryption`, metadata tables and annotations, access points, S3 Control, Batch Operations, Storage Lens, Access Grants, S3 Tables and Vectors; DynamoDB DAX, on-demand backups, PITR time-travel restore, exports, multi-attribute GSI keys and vector search (refuse, never truncate) |
| Storage | NOT NEEDED | Replication copy, access-log delivery, inventory, analytics and metrics report production, Intelligent-Tiering movement, website redirect-all and routing rules, request and presigned-URL signature verification by default (opt-in later), global-table replication, throttling and capacity metering (opt-in fault injector), transient table statuses (opt-in later), Contributor Insights rules, auto-scaling behaviour |
| Compute | STAY CUT | Kafka, MQ, DocumentDB mappings (answer 501, not a validation error); durable functions; capacity providers and Managed Instances; Extensions API; response streaming (**revisit**, first Lambda addition); container executor; VTL and non-proxy integrations (stop forwarding HTTP non-proxy silently); WebSocket APIs (**revisit**, first API Gateway addition; needs a connection registry and idle timers); VPC links, private integrations, client certificates; SDK and export generation, documentation parts, portals, TestInvoke*; usage-plan and stage-cache enforcement (store only) |
| Compute | NOT NEEDED | Async retry delay realism (`async-retry-delays = "aws"`), `--lambda-strict` and `--sfn-strict` windows, DELETING states; REST execution-log narrative (keep the access-log line); Lambda account counters beyond nominal constants |
| Config | STAY CUT | SSM Run Command, Automation, Session Manager, State Manager associations, Inventory, Compliance, Patch Manager, Maintenance Windows, OpsCenter, Resource Data Sync (refuse by name before parsing; their CFN types SKIP+WARN); RAM parameter sharing; KMS custom key stores, Nitro `Recipient` (refuse, never return plaintext), `DryRun`, lockout safety check, SM2; hosted and managed rotation, partner secrets |
| Config | NOT NEEDED | Secret and KMS rotation schedulers and asynchronous rotation (rotation stays synchronous; `RotateSecret`/`RotateKeyOnDemand` stay); parameter policy notifications; AMI and ECS public parameter sets (keep a tiny `/aws/service/*` seed); quotas (100 versions, 20 labels, 500,000 secrets); real replicas |
| Observability | STAY CUT | Logs Insights (**revisit** a five-command subset), Live Tail, export and import tasks, deliveries, destinations, anomaly detectors, transformers; Contributor Insights rules, metric streams, mute rules, `GetMetricWidgetImage`; CloudFormation drift detection, StackSets, extension registry, resource scanning, `CancelUpdateStack`, import, macros |
| Observability | NOT NEEDED | Dashboard rendering and the widget editor (keep list and raw JSON), anomaly modelling, Application Signals, Synthetics, RUM, cross-account metrics, resolution-tiered retention, the fixed 10 s tick |
| Identity | STAY CUT | SCP and RCP evaluation and the Organizations API (empty slot only); cross-account chains; credential report; access advisor; MFA fleet and MFA keys; token-signature and thumbprint validation; `AssumeRoot` (501); server and signing certificates, SSH keys, service-specific credentials; Access Analyzer, Identity Center, Roles Anywhere |
| Identity | NOT NEEDED | Eventual-consistency and `EntityTemporarilyUnmodifiable` timing (no setting); principal unique-id rewriting; asynchronous service-linked-role deletion (return SUCCEEDED at once); IAM decisions as CloudWatch metrics; simulator statement positions; session-policy evaluation, web-identity condition evaluation and `aws:ResourceTag` breadth (deferred behind slots) |

## 6. Built in Go, rarely needed locally, and costly

Recommendation vocabulary: DEFER (ship the Rust first cut without it, keep a slot), DROP (do not port), KEEP-LAZY (keep, built lazily).
"Already settled" means PLAN.md decided it.

| Item | Domain | Recommendation | Why |
|---|---|---|---|
| SQS Query/XML wire | msg | Already settled | PLAN refuses Query on SQS and CloudWatch; the review confirms demand is low (current SDKs speak JSON); LocalStack and Floci serve both |
| SQS message-move tasks with 100 ms pacing | msg | DEFER | Manual operations action; a synchronous move is the racy shape the Go plan removed |
| SNS delivery-status logging to CloudWatch Logs | msg | DEFER; accept the attributes | Log records nobody reads in a local loop |
| EventBridge OAuth token fetch and body-parameter merging | msg | DEFER; keep CRUD, Basic and API key | LocalStack ships a switch to skip OAuth; about 30% of the Go EventBridge package |
| EventBridge archives and replays | msg | DEFER as a pair; accept `CreateArchive` | Low-use; moto and LocalStack have it |
| Kinesis resharding, lineage, closed-shard reclamation | msg, cmp | DEFER, but keep the shard record open for `ParentShardId`; refuse Split, Merge, UpdateShardCount by name first | Rarely tested; hardest to defer because lineage is baked into the store |
| Kinesis KMS check on every call; stream resource-policy evaluation | msg | DEFER; make the KMS rule uniform with SQS and SNS | Catches a deploy-time problem; policy evaluation is cheap only if it falls out of the shared guard |
| S3 lifecycle with full rule semantics | sto | DEFER enforcement; port store and validation; KEEP-LAZY when built | Day-granular rules; the only P0 data-deleting bug (S3-39); armed on first rule |
| `GetBucketPolicyStatus` analysis | sto | DEFER | No tool depends on it; keep `BlockPublicPolicy` refusal at put |
| PartiQL batch and transaction forms | sto | DEFER; keep single-statement PartiQL for the console | `ExecuteTransaction` is non-atomic in Go (DDB-33, P0); application code rarely uses PartiQL |
| Step Functions Distributed Map and Map Runs, `RedriveExecution`, Activities, weighted alias routing | cmp | DEFER | Inline Map covers local logic; LocalStack and Floci omit Redrive; keep alias CRUD |
| Express executions stored durably plus logging-off vended logs | cmp | Pick one: a bounded Express ring or the log group | One run is recorded twice |
| Per-invocation metric datapoints from Lambda, API Gateway, Step Functions | cmp, obs | KEEP-LAZY for Lambda; DEFER for the other two | See 7 |
| REST execution-log narrative; Java and .NET runtime special-casing | cmp | DEFER; route Java and .NET through `Command` first | LocalStack says execution logs have no effect |
| Model-derived rejection tables for every operation (Lambda 842 constraints, Step Functions 229, REST 118, SSM 1,638 cases of which 1,538 sit on refused fleet operations, IAM 1,268 cases) | cmp, cfg, idn | KEEP-LAZY for hot operations; DROP the rest; do not port IAM or SSM fleet case tables | Checked newtypes already refuse these in Rust; eager tables were the dominant idle cost |
| Planned-only Go work: `--iam-strict`, `async-retry-delays`, strict switches, async rotation, DELETING windows | cmp, cfg, idn | DROP | Plans themselves say "nothing here is worth a default" |
| KMS and Secrets resource-policy evaluation tied to IAM modes | cfg | DEFER to the IAM stage; store first | Observable only when IAM is not `off` |
| `RotateSecret` driving a real Lambda through four steps | cfg | KEEP, sequence after Lambda | Couples Secrets Manager to Lambda |
| Fixed 10 s alarm tick; Logs retention sweeper | obs | DROP the tick; KEEP-LAZY retention | Idle wake-ups |
| CloudWatch console dashboards and widget editor (22 screens in the board set, 20 of 21 differ from the built page); CloudFormation create-stack and update-stack wizards; IAM create wizards (26 boards) | obs, idn | DEFER; keep list, raw JSON, read views, access log, generate, simulate | Largest console blocks; developers deploy with CLI and IaC. The console review owns the decision |
| Per-operation prose reasons for refused operations and the two-way reconciliation test | obs | Generate from the model with a structured reason; prose only for the dozen that teach | Hand-written bookkeeping |
| Nested-stack merge; per-type update convergence for the long tail of mapped kinds | obs | DEFER nested; in-place updates for common types, replace-with-warning for the tail | Largest CFN cost centre |
| Regex filter patterns (`%...%`) | obs | DEFER; build space-delimited first | Pulls a regex engine into the binary |
| `GetAccountSummary` full key set, `GetAccountAuthorizationDetails` paging fidelity, SAML audience hash, simulator positions | idn | Trim to what stacks and tools read | Read by audit tools |
| Large crypto shapes (RSA-3072/4096, P-521, data-key pairs) | cfg | KEEP-LAZY: accept at CreateKey, generate on first use | Key-generation latency, one more curve crate |

## 7. Metrics, alarms and dashboards: how much belong locally

The owner's doubt is half right. **Right** for dashboards, the evaluator as a headline feature, and S3, DynamoDB and Kinesis built-in
metrics: a one-user laptop has hours of fixture data, nobody views a local dashboard (all three peers store dashboards and none renders
them), and CDK tests alarm definitions at synth time. **Wrong** for apply: a CDK stack contains alarms, composite alarms, math alarms and
dashboards, and it must apply. The valuable part is that the API accepts what IaC sends, round-trips it, and lets a test drive a state
change (`SetAlarmState` to SNS or Lambda, the one thing all three peers converge on). Developers asserting on emitted metrics mostly
capture Powertools' EMF lines from stdout and need no emulator.

| Tier | Items | Class |
|---|---|---|
| A, build | PutMetricData, ListMetrics, GetMetricStatistics, GetMetricData (single MetricStat), tags (bounded series, sample cap); JSON 1.0 and CBOR wires; EMF ingestion from stdout and PutLogEvents; `AWS/Lambda` producer; alarms as objects (single metric) with history and enable/disable; **`SetAlarmState` firing SNS and Lambda with AWS's alarm payload**; dashboards Put, Get, List, Delete (body verbatim); alarm evaluation, deadline-driven | BUILD |
| B, store and flag | Alarms with `Metrics` or `ThresholdMetricId` (state INSUFFICIENT_DATA, `StateReason` "not evaluated locally: metric math"); composite alarms until evaluation is built; anomaly detectors, insight rules, metric streams, mute rules in the generic store; alarm actions that are not SNS or Lambda (ARN stored, flagged "not delivered locally"); dashboard widget bodies | ACCEPT-AND-FLAG |
| C, drop | Dashboard rendering; anomaly modelling; Contributor Insights computation; metric stream delivery; widget images; Application Signals, Synthetics, RUM; cross-account and cross-region; resolution tiers; a metric-math evaluator for now (revisit as one S to M evaluator serving both alarms and GetMetricData) | NOT NEEDED |

Where the reviews disagreed, and the resolution:

| Question | Positions | Resolution |
|---|---|---|
| Built-in metrics for SQS | msg: BUILD the three Approximate* gauges plus Sent, Received, Deleted; obs: BUILD for SQS only; both cite the AWS and CDK DLQ-alarm example. Recorded owner decision (SUPPORT.md): SQS, SNS, DynamoDB and Kinesis publish nothing because an alarm "would fire on a fixture" | Open decision D4. If approved: SQS only, materialised lazily and only when an alarm, dashboard or metric read names the namespace; the fixture is the test the developer wants. SNS, EventBridge, DynamoDB, S3, Kinesis publish nothing (S3 and DynamoDB reviewers agree) |
| Lambda, API Gateway, Step Functions datapoints | cmp (N1): defer all, lazy; obs: `AWS/Lambda` in Tier A so Errors and Duration alarms have data | Lambda producer built lazily (an in-memory ring materialised on read, so the canonical `Errors / Invocations` alarm works); API Gateway and Step Functions producers deferred |
| Alarm evaluation design | obs compared four designs | Deadline-driven (below) |

Alarm evaluation designs (observability review): a fixed tick costs a wake-up every 10 s forever (Go today). Evaluate-on-read-and-write has
zero wake-ups but an unattended CI test waiting for a notification on silence would hang. Evaluate only while the console is open fails the
headless case, which is the main use. **Deadline-driven** keeps each alarm's next possible state-change instant, arms one timer to the
earliest, and has no timer when nothing is pending; it costs complexity (M), not behaviour. The same lazy rule serves the Logs retention
sweeper and metric trimming.

CloudWatch Logs: streaming stdout alone covers watching, not asserting. Build a bounded store with the read API (CreateLogGroup/Stream,
PutLogEvents, GetLogEvents, FilterLogEvents with term, phrase, JSON and space-delimited patterns, Describe*, lazy retention, tags;
in-process writers) because tests that invoke a function and assert on what it logged need it; add metric filters and subscription filters;
accept and flag the policy documents, query definitions and group attributes through one generic table; drop Insights (v1), Live Tail,
exports and deliveries. If weight forces a cut, make storage memory-only with persistence on clean shutdown; do not cut the read API.

## 8. Implications for PLAN.md

### Suggested edits (the owner approves; nothing applied)

| # | Phase | Edit | Source |
|---|---|---|---|
| E1 | A4 | Gate changes from "identity, boundary, session, SCP and resource layers" to "identity, boundary and resource layers, plus empty session-policy and SCP slots". Add to the case table: policy variables, `BinaryEquals`, three-state context keys, decision trace, AWS-shaped AccessDenied text | idn |
| E2 | A4 | Add the stored-only record and flag to `aws-core` (kind, id, raw document, `active: false`) and a response-header sibling of `X-Doze-Not-Built`; A3 carries it on the wire. This is the generic accept-and-store service every later phase uses | all |
| E3 | A1, A4 | Make `Expiring` the only timer mechanism; add a weight-gate case "30 s wake-ups with one alarm, one mapping, one lifecycle rule and one TTL table configured" next to the idle case, so arm-on-configuration is tested, not asserted | obs, sto, cmp |
| E4 | A5, A10, A11 | Queue, DynamoDB stream and Kinesis services publish an `aws-peers` notification on append; A13 pollers await it | cmp, sto |
| E5 | A6 | Parameter Store limits, paging, `AllowedPattern`; refuse the SSM fleet half by name before parsing (no validation tables for it); lazy key generation; stored grants; `mrk-` ids; `/aws/reference/secretsmanager/*` read-through | cfg |
| E6 | A7 | Add: stateless signed session tokens, `GetCallerIdentity` exemption, boundary at create and returned, access-key rules in one case-folded store, OIDC/SAML/password-policy records, `aws:PrincipalTag` from IAM tags. Remove: credential report, access advisor, `AssumeRoot`, strict-time setting, async simulations | idn |
| E7 | A8 | Add `FilterPolicyScope: MessageBody` to the gate; record decision D1 for SMS and push | msg |
| E8 | A9 | Add EventBridge notification config, per-key DeleteObjects events, delete-marker events; lifecycle janitor armed lazily with correct tag, And, size filters before enforcement; ACL refusal and archive-class read refusal | sto |
| E9 | A10, A11 | `UpdateTable` stream spec; PITR true round trip; legacy-parameter refusal; DynamoDB-to-Kinesis destination; opt-in fault injectors; Kinesis shard record keeps `ParentShardId`, Split/Merge refused by name | sto, msg |
| E10 | A11 | CloudWatch scope = Tier A, B, C in section 7; Logs store with read API; one generic stored-document family; lazy retention; dashboard rendering console deferred | obs |
| E11 | A12 | Add local targets (Step Functions, Kinesis, EventBus, API Gateway), `SqsParameters`, complete input transformer, DLQ and retry round trip; one default-bus publish hook for S3, Step Functions, CloudWatch, SSM, Secrets events; decision D2 on Scheduler | msg |
| E12 | A13 | Mapping semantics (item 2), run-as-role (item 7), accept bundle (foreign layers, provisioned concurrency, code signing, config blocks), `_local_` reload on change | cmp |
| E13 | A14 | Wait scaling, mock config, `TestState` completeness, HTTP Task, wider `aws-sdk:`; driver sleeps to deadline; history bounded by count; Express ring | cmp |
| E14 | A15 | REST Update* operations, OpenAPI import subset, gateway responses, lite JWT authorizers, stage settings round trip | cmp |
| E15 | A16 | Resource-type policy and switch, custom resources, dynamic references, IAM mapping, LanguageExtensions, `DefinitionUri`, Kinesis stream creation, SAM `S3`/`DynamoDB`/`Kinesis`/`CloudWatchLogs` events; do not port the doc/code mismatches | obs, cfg, sto, idn |
| E16 | A17 | Regenerate `not-built.md` for the Rust edition from the four classes; reclassify OpenAPI import as deployment, not publishing; correct the SNS filter-scope statement | cmp, msg |
| E17 | Open decisions | Console Scope board implements the four classes (disabled option with reason, Not active locally badge, NOT BUILT result) | all |

Items needing an AWS recording before they are fixed (recorder keys pending): exact AWS-managed policy documents, IAM policy-size
quotas, `AllowedPattern` regex dialect and error code, AccessDenied message texts, `PassRole` error text, whether KCL defaults to enhanced
fan-out.

### Open decisions that need the owner

**Resolved 2026-11-09:** the owner took the recommended lean on D1 to D7 and D9, and moved D8 into 1.0 (Lambda response streaming and API Gateway WebSocket are both part of the first release). E1 to E17 are applied to `PLAN.md`. The table below keeps the original wording for the record.

| # | Decision | Context |
|---|---|---|
| D1 | Accept SMS publish to a phone number and mobile-push platform applications and endpoints (stored, nothing sent, optional recorded outbox), or keep them declined | Reverses a `not-built.md` row and brushes the console scope rule on effects that leave the machine. Email, SMS and application subscriptions are already stored in Go; only the Publish and platform calls are refused. A refusal fails a whole apply that merely registers a device |
| D2 | Build EventBridge Scheduler as a fifth service in the group | Scope rule lists it as not served. SAM `ScheduleV2` already maps to rules in Go; the gap is one-time `at()`, time zones, flexible windows and `AWS::Scheduler::*` from CDK and Terraform. M, reusing the cron parser and target dispatch |
| D3 | Does accept-and-flag override "refuse at create so a deploy cannot succeed and then fail" for: container-image Lambda functions, Step Functions task integrations doze does not serve, foreign layer ARNs | Compute review recommends accepting; each reverses a Go rule. Invoke on an image function would return a clear error; Step Functions would fail the state catchably unless mocked |
| D4 | SQS built-in metrics (the three Approximate* gauges and Sent, Received, Deleted), lazily | Reverses the recorded "fires on a fixture" decision for SQS only (section 7) |
| D5 | CloudFormation default for unmapped types: permissive SKIP+WARN for opaque infrastructure and FAIL for behaviour-bearing types, with the switch | Matches the three peers' permissive default; the report-on-resource rule is what keeps it honest |
| D6 | Custom resources: native-first with a generic Lambda-backed runner behind it | Whether host-process Lambdas can run CDK's Node handlers is unverified |
| D7 | Application Auto Scaling as a tiny stub service (register targets and policies, never scale) | CDK `Table` autoscaling and Terraform modules need it (M, a new service surface); the reviewer's confidence is low. The alternative is SKIP+WARN in CloudFormation only |
| D8 | First post-1.0 additions: Lambda response streaming and API Gateway WebSocket | Both reviewers named them; neither has a peer that ships it free |
| D9 | Record AWS-managed policy documents from the AWS account once recorder keys exist | Otherwise the approximate set is flagged and enforce may false-deny |

## 9. Appendix: the six reviews

The reviews stay in the session scratchpad and are not part of this repository. Each has its own source keys, full feature tables
(with evidence tags and confidence) and a top five; this document supersedes their ordering.

- `/private/tmp/claude-501/-Users-srini-Code-doze-dev/82293829-caa2-4b6e-aae2-03330680b388/scratchpad/aws-console-direction/src/_review/messaging.md`
- `.../src/_review/storage.md`
- `.../src/_review/compute.md`
- `.../src/_review/config.md`
- `.../src/_review/observe.md`
- `.../src/_review/identity.md`

Also read: `docs/port/aws/PLAN.md` (phases and decisions), `doze-aws/docs/SUPPORT.md` and `docs/not-built.md` (tiers, declined entries), and
the console scope rule in `.../scratchpad/aws-console-direction/kit/README.md`. The scratch check for row (b) lives in
`.../scratchpad/verify-ep/` (a copy of the `internal/eventpattern` package from `origin/main` and a ten-line program).
