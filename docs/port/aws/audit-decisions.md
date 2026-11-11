# Owner decisions, consolidated from the 17 _audit.md files

Rules applied to every recommendation: only what can be true locally; the Go engine is frozen, so engine gaps become Rust behaviour-catalogue entries (not Go fixes); weight is a feature (no new engine state unless it earns its place). "Catalogue" below means: add a behaviour-catalogue entry for the Rust engine and keep the screen matching today's engine until then.

## Decided 2026-11-12 (owner)

**Rule applied to all 60:** AWS is the judge (see `phase0.md`). Every "fix engine in Rust" item whose AWS behaviour is recorded is fixed in Rust and the screen is drawn to match once the engine does; every "catalogue" item gets a behaviour-catalogue entry; every "leave screen as is" stays. Where a recommendation below says otherwise, the owner answers here win.

- **Enable (all accepted):** E1 REST method/integration/response Edit via re-put; E3 CFN "Preserve resources" (and fix SUPPORT, C4); E4 Lambda "Create default role" via iam:CreateRole; E6 S3 Move, Rename, Calculate total size (Download as is dropped); E8 alarm form "Create a new SNS topic"; E9 Logs deletion protection via PutLogGroupDeletionProtection (C3 resolved: it is F).
- **Build in 1.0:** G11 Logs storedBytes (a counter, not a scan); G25 SNS MaximumMessageSize enforced (tracing stays stored-only); G15 a small fixed list of AWS managed policies, built lazily.
- **Not built in 1.0:** G24 CloudWatch keeps 24 h (the 1d/3d/1w range line stays a teaching line); retention is a weight choice.
- **H3 evidence:** keep screens whose operations are F; one live-console pass (H2) settles the wording later.
- **N4 SQS redrive:** build it. Add StartMessageMoveTask, ListMessageMoveTasks and CancelMessageMoveTask scenarios to the SQS recordings (about one session more for SQS; re-plan trigger is 7 sessions, so say so on the scoreboard).
- **Everything else:** the recommendation in the item stands.

## 1. Engine gap: AWS does X, doze-aws ignores or refuses it, screen matches the engine

- **G1 CloudFormation.** Is it acceptable that CreateStack ignores EnableTerminationProtection, RoleARN and StackPolicyBody, CreateChangeSet drops Description, and Describe/ListStacks return no Description?
  Evidence: audit reproduction. Rec: catalogue (Rust); leave screens as is.
- **G2 CloudFormation.** Change-set and event semantics differ: ExecuteChangeSet leaves sibling change sets, parameter-only change is not a Modify (Replacement blank), DeleteStack gives one event and no DELETE_SKIPPED, updates report new resources as UPDATE_*, SNS topic reason is "no change".
  Evidence: reproduction. Rec: catalogue (Rust); leave screens as is.
- **G3 CloudFormation.** DescribeStacks returns NoEcho parameter values in clear, and an output Description only comes back when exported.
  Evidence: reproduction. Rec: fix in Rust (NoEcho masking is cheap and a visible trust issue); catalogue the output case.
- **G4 DynamoDB.** UpdateTable ignores StreamSpecification, Contributor Insights does not round-trip, TableClass is not echoed; the stream-enable and contributor-manage controls and the TableClass card are parked.
  Evidence: plan DDB-1/8/12/13/17. Rec: fix engine in Rust, then restore parked screens (see E5).
- **G5 DynamoDB.** "Table already exists: <name>" differs from AWS's generic ResourceInUseException message.
  Evidence: audit. Rec: leave screen as is (shows what the engine returns); match AWS text in Rust.
- **G6 EventBridge.** Tags at create are F in SUPPORT.md but PutRule and CreateEventBus drop them; retry policy and DLQ are stored, not applied, with no SUPPORT row.
  Evidence: plan EB-7, EB-8. Rec: fix engine in Rust for tags; keep "stored, not applied" for retry/DLQ.
- **G7 Kinesis.** GetResourcePolicy on a stream with no policy answers ResourceNotFoundException; screen shows "None attached" without the error.
  Evidence: kinesis/actions.go, SUPPORT silent. Rec: catalogue what AWS answers; draw the real error in the Rust-era screen.
- **G8 Kinesis.** DeleteStream ignores EnforceConsumerDeletion and never answers ResourceInUseException for streams with consumers.
  Evidence: model vs code. Rec: fix engine in Rust (client code relies on the refusal).
- **G9 KMS.** Key materials table cannot show Key material ID, State or the first material, or a rotations-remaining count, because no KeyMaterialId/State/IncludeKeyMaterial is returned and the 25-rotation limit is not enforced.
  Evidence: KMS-41, KMS-13. Rec: fix engine in Rust (small, state already exists); screen stays until then.
- **G10 KMS.** AWS managed keys report KeyManager=CUSTOMER and are created on first use, so that page lists by alias prefix only.
  Evidence: audit. Rec: fix engine in Rust (return KeyManager=AWS for alias/aws/*).
- **G11 CloudWatch Logs.** storedBytes is always 0, so the column and live totals are not drawn.
  Evidence: plan ledger. Rec: leave screen as is; sum event sizes in Rust only if cheap (counter, not scan).
- **G12 CloudWatch Logs.** Screens assume fixes the engine lacks: metricFilterCount always 0 (LOG-4), logGroupClass and kmsKeyId dropped (LOG-8), defaultValue emitted at the wrong time (LOG-32), `aws/` prefix accepted (LOG-9).
  Evidence: plan/services/logs.md. Rec: fix engine in Rust; until then keep screens as drawn.
- **G13 IAM.** Names are case sensitive locally; AWS treats them case-insensitively.
  Evidence: store.go keys on exact name. Rec: fix engine in Rust.
- **G14 IAM.** The two-access-key limit is not enforced.
  Evidence: store.go CreateAccessKey. Rec: fix engine in Rust (one counter check).
- **G15 IAM.** ListPolicies returns only customer managed policies, so the AWS managed picker rows have no list call behind them.
  Evidence: audit. Rec: leave screen as is (says so); a small fixed list in Rust is the one option, lazily built.
- **G16 Secrets Manager.** ListSecrets ignores Filters, so search and Name filter return everything; screens draw them because the two seeded secrets give the same rows.
  Evidence: plan SM-7. Rec: fix engine in Rust; do not draw search until then.
- **G17 Secrets Manager.** Tag rules (50-tag cap, reserved `aws:` prefix, key uniqueness) are accepted by the engine; screens state them as AWS rules.
  Evidence: plan SM-15. Rec: fix engine in Rust; keep lines as AWS rules.
- **G18 Secrets Manager.** RotateImmediately=false is not honoured (always rotates), and rotation is synchronous where AWS is asynchronous.
  Evidence: SM-3, SUPPORT. Rec: fix engine in Rust; synchronous stays documented.
- **G19 SSM.** Versions are never trimmed (SSM-12), label rules and the 1,011-character limit are unchecked (SSM-30), LastModifiedUser is absent (SSM-10 proposes account root ARN).
  Evidence: plan. Rec: fix engine in Rust; restore the "Last modified user" column when it lands.
- **G20 SSM.** Console search defaults to `contains`; engine filters Name on Equals and BeginsWith only.
  Evidence: model allows Contains. Rec: fix engine in Rust (add Contains).
- **G21 S3.** Lifecycle janitor reads only Filter.Prefix and day counts, so tag, size and And filters make a rule act on the whole prefix.
  Evidence: source. Rec: fix engine in Rust (data-loss risk); draw only prefix rules meanwhile.
- **G22 S3.** ObjectRemoved fires for single DeleteObject only, not DeleteObjects (the console Delete button) or delete markers; no s3:TestEvent is sent.
  Evidence: source. Rec: fix engine in Rust.
- **G23 Lambda.** SQS event-source-mapping settings and alias weights are not built (disabled on screen).
  Evidence: plan LAM-3, SUPPORT. Rec: leave disabled; build in Rust only if the behaviour is implementable locally.
- **G24 CloudWatch.** Time ranges 1d, 3d, 1w are offered but only 24 hours are kept.
  Evidence: audit. Rec: leave screen as is (teaching line); retention is a weight choice.
- **G25 SNS.** MaximumMessageSize and FifoThroughputScope are not enforced; TracingConfig is stored-only.
  Evidence: `_plan.md` scope decision. Rec: catalogue; enforce size in Rust, keep tracing stored.

## 2. A screen or control is drawn disabled or omitted and could be enabled

- **E1 API Gateway.** Edit on a REST method, integration or response is read-only because UpdateMethod/UpdateIntegration are unrouted; a re-put (PutMethod etc.) is F and could back an Edit form.
  Evidence: audit decision 1. Rec: enable control via re-put.
- **E2 API Gateway.** Edge-optimized and Private are drawn disabled, yet the engine accepts and stores any endpoint type (defaults to EDGE).
  Evidence: restapis.go. Rec: leave screen as is (nothing local to edge or private); optionally reject in Rust.
- **E3 CloudFormation.** "Preserve resources" on update is disabled, but UpdateStack honours DisableRollback (ends UPDATE_FAILED).
  Evidence: rollback.go; SUPPORT lists only default. Rec: enable control and fix SUPPORT.
- **E4 Lambda.** "Create default role" is disabled ("create the role in IAM") though doze-aws has IAM and could do it via iam:CreateRole.
  Evidence: dg/getting-started. Rec: enable control.
- **E5 DynamoDB.** Parked stream-enable and contributor-manage controls.
  Evidence: depends on G4. Rec: enable after G4 lands in Rust.
- **E6 S3.** "Download as", "Calculate total size", "Move" and "Rename object" are drawn dim and unlinked; all are doable locally (copy plus delete).
  Evidence: audit. Rec: enable control for Move/Rename/Calculate size (cheap); drop Download as.
- **E7 Kinesis.** The AWS managed key `aws/kinesis` is offered by the docs as the default encryption choice; KMS already creates alias/aws/* on first use.
  Evidence: audit. Rec: enable control (after G10).
- **E8 CloudWatch.** "Create a new SNS topic" in the alarm form is an unselected option with no screen behind it.
  Evidence: no fetched page describes it. Rec: enable control (CreateTopic exists) or drop the option.
- **E9 CloudWatch Logs.** Deletion protection is disabled on the create form; if promoted it needs Actions, Edit deletion protection and a refusal dialog screen.
  Evidence: plan LOG-8 vs SUPPORT S (see C3). Rec: enable control with PutLogGroupDeletionProtection in Rust.
- **E10 SSM.** Only data type `text` is offered.
  Evidence: SUPPORT silent. Rec: leave screen as is until a support line exists.

## 3. Contradictions between Go docs and Go code

- **C1 Step Functions.** SUPPORT.md contradicts itself twice: Express executions "not listable" vs "durable ... call DescribeExecution"; GetActivityTask 60 s (table, code) vs "no long-poll ceiling".
  Evidence: SUPPORT table vs Differences list. Rec: follow the table and code; fix the Differences list in the Rust docs.
- **C2 CloudWatch.** Alarm history after DeleteAlarms: model says AWS retains it, SUPPORT.md says it goes with the alarm.
  Evidence: model vs SUPPORT. Rec: leave screen as is (makes no claim); catalogue the model's behaviour.
- **C3 CloudWatch Logs.** Deletion protection is S in SUPPORT.md and not-built.md, F in plan LOG-8.
  Evidence: docs vs plan. Rec: decide with E9; fix whichever doc loses.
- **C4 CloudFormation.** SUPPORT.md lists only the default DisableRollback; code honours it (see E3).
  Evidence: rollback.go. Rec: update doc.
- **C5 Umbrella.** SUPPORT.md "Differences from AWS" omits behaviours the audits found: IAM case sensitivity and key limit, Kinesis DeleteStream, S3 lifecycle filters and events, EB retry/DLQ, Lambda throttle/error bodies.
  Evidence: G13, G14, G8, G21, G22, G6. Rec: one catalogue pass, not 17 edits.
- **C6 SNS.** The developer guide says tracing works on standard and FIFO; the model says standard only; X-Ray is a service doze-aws lacks, so tracing is "stored, not applied" (SFN does the same).
  Evidence: DG vs model. Rec: leave screen as is (stored-only on standard; FIFO form shows neither).
- **C7 API Gateway.** "No AWS WAF locally" has no line in SUPPORT.md or not-built.md; HTTP API nav labels "CORS" and "Stages" and the HTTP wizard's default route method are undocumented.
  Evidence: audit. Rec: add one line to docs; leave screens as is.
- **C8 Lambda.** Three `source:` notes (runtime.go, invoke.go, bootstrap.py) back local behaviour SUPPORT.md does not state (no Max Memory Used, throttle message, error body keys).
  Evidence: audit. Rec: leave screen as is; state them in the Rust catalogue.
- **C9 Docs vs docs.** S3: empty-bucket.html says type the bucket name, GetStartedWithS3.html says type "permanently delete".
  Evidence: two AWS pages. Rec: leave screen as is (follows dedicated page).

## 4. AWS console behaviours not drawn on purpose

- **N1 CloudFormation.** Create stack menu (standard vs import) and the Execute change set dialog (failure options, rollback config) are not drawn.
  Evidence: UG pages. Rec: leave screen as is (import declined).
- **N2 EventBridge.** Create archive is two steps on AWS (one page here); the scheduled-rule flow reuses event-pattern steps; the Advanced/Enhanced builder page and bus-detail Archives/Tags tabs are absent.
  Evidence: screen budget 24/24. Rec: leave screen as is; split Create archive only if budget allows.
- **N3 Secrets Manager.** The "Rotate secret" and "Cancel secret deletion" confirm dialogs are not drawn; the buttons act directly.
  Evidence: UG steps; budget 15 of 16. Rec: leave screen as is.
- **N4 SQS.** The "Dead-letter queue redrive tasks" tab is not drawn; redrive limits (36 h, 100 tasks, new IDs) removed as unestablished; Subscribe to SNS sits on the SNS tab not the Actions menu; the received-message view merges two doc vintages.
  Evidence: screenshot, blog. Rec: drop the tab unless redrive is built; keep the rest.
- **N5 STS.** Switch Role needs a signed-in console session, which locally does not exist; the "role in effect" state is cosmetic and nothing runs as the role.
  Evidence: not-built.md (no console sign-in). Rec: keep as a credentials-minting form with "Stored, not applied" lines.
- **N6 Lambda.** Layers entry is a link row (kit draws no left navigation); SNS/S3 triggers are not listed on the Triggers view; failed test and throttle show API result words.
  Evidence: audit. Rec: leave screen as is.
- **N7 CloudWatch.** Dashboards are C-tier yet `dashboard-view` draws charts from GetMetricData/DescribeAlarms; may the console render a stored body?
  Evidence: audit decision 1, `_plan.md`. Rec: leave screen as is (console draws, engine stores).
- **N8 IAM.** Policy editor live validation (Next disabled, error callout) mirrors parseIdentityPolicy in the console, not an IAM operation; role link on a Summary is documented for cross-account roles only.
  Evidence: audit. Rec: leave screen as is for validation (it is local); show the link only when trust names an account root.
- **N9 DynamoDB.** PartiQL editor is a button on Tables, not left navigation; Delete table/index/item confirmations are generic safeguards beyond the docs.
  Evidence: kit has no per-service nav. Rec: leave screen as is.
- **N10 Kinesis, KMS, Step Functions, SQS.** Dialog versus page for Edit flows (Kinesis); Tags save drawn as one Save (KMS); Delete state machine returns to list (SFN).
  Evidence: docs silent. Rec: leave screen as is.
- **N11 S3.** Delete bucket warning keeps AWS's "another AWS user" text although there is one account locally.
  Evidence: verbatim AWS. Rec: leave screen as is.

## 5. Documentation hygiene (one line each)

- **H1 Stale `_plan.md`.** CloudFormation `_plan.md` is stale (Exports tab, VisibilityTimeout, DELETE_SKIPPED, role picker); CW, EB and SNS point at items "already in `_plan.md`". Rec: regenerate plans from the audits, do not hand-edit.
- **H2 Console wording unverifiable from docs.** Column, tab and heading names in IAM, S3, CloudWatch, Secrets, SFN, SNS, Logs, KMS, CFN ("Created time" vs "Creation time"), SQS use API names or neutral wording. Rec: leave screens as is; one live-console pass would settle all of them.
- **H3 Evidence from archived, blog or third-party sources.** SSM Tags tab (archived source), SFN Definition tab and Manage tags (archived GitHub), Logs Lambda filter labels and delete wording, SQS DLQ rows (AWS blog), DDB "Indexes" tab (archived 2024), CW Dashboard Delete (solutions guide), CW GetMetricData in ops. Rec: keep where the operations are F; drop SSM Tags tab only if you want the strict standard.
- **H4 Unreadable doc pages.** Three S3 pages and several CloudWatch API reference pages returned blank or were not opened. Rec: re-fetch, no decision.
- **H5 Kit and data notes.** kit/paper.css `.lnk` loses to `.root a{color:inherit}`; CFN timestamps and wire-dock rows are illustrative; Kinesis error text at row 34 comes from engine source. Rec: fix the kit when it is next open; otherwise ignore.

## Counts per theme

| Theme | Items |
|---|---|
| 1 Engine gap | 25 (G1 to G25) |
| 2 Disabled or omitted, could enable | 10 (E1 to E10) |
| 3 Go docs vs code contradictions | 9 (C1 to C9) |
| 4 Not drawn on purpose | 11 (N1 to N11) |
| 5 Documentation hygiene | 5 (H1 to H5) |
| Total | 60 |

## Top 10 for the owner's attention

1. **G21 / G22 (S3).** Lifecycle rules with tag/size filters act on the whole prefix, and ObjectRemoved misses console deletes: silent data loss and broken notifications.
2. **G3 (CloudFormation).** NoEcho parameter values returned in clear.
3. **G13 / G14 (IAM).** Case-sensitive names and no two-access-key limit differ from AWS and from SUPPORT.md.
4. **G8 (Kinesis).** DeleteStream never refuses a stream with consumers.
5. **E1 (API Gateway).** Whether REST method and integration screens stay read-only or get an Edit via re-put.
6. **E4 (Lambda).** Whether "Create default role" works through iam:CreateRole, since both services exist.
7. **G4 + E5 (DynamoDB).** Fix UpdateTable streams and Contributor Insights in Rust and restore the parked controls.
8. **C1 (Step Functions).** SUPPORT.md contradicts itself on Express listing and GetActivityTask polling.
9. **C3 + E9 (Logs).** Deletion protection is S in docs, F in plan: pick one, because it changes screens.
10. **G16 (Secrets Manager).** List search is drawn although the engine ignores Filters; keep or drop it until fixed.
