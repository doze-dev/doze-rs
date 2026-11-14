# fix-iam: IAM and STS behaviour gaps against the Go edition (2026-11-13)

Branch `aws-fix-iam`. Closes six gaps an audit found between the Rust IAM/STS and the Go edition (`doze-aws/docs/SUPPORT.md`, IAM
and STS sections). All changes are in `crates/aws-iam`; nothing outside it changed except this note.

## What changed

1. **`aws:SecureTransport` is always `true`** (`authz.rs`). The local hop is plain HTTP but stands in for AWS's TLS endpoint;
   reporting it made CDK's `enforceSSL` statement (deny when `aws:SecureTransport` is `false`) refuse every request under
   `--iam enforce`. Go did the same. Test: `secure_transport_is_true_so_an_enforce_ssl_policy_does_not_refuse_local_traffic`.

2. **AWS-managed policies have documents made from their names** (`awsmanaged.rs`, `managed.rs`). Go's rules ported:
   `AdministratorAccess`, `ReadOnlyAccess`, `PowerUserAccess`, `IAM*`, `Amazon<Svc>FullAccess` / `ReadOnlyAccess` / `ReadWrite` in
   the `Amazon`, `AWS`, bare and `_` spellings, `AWSLambda{Basic,SQSQueue,DynamoDB,Kinesis}ExecutionRole`. Added beyond Go, because
   CDK names them: `AWSLambdaVPCAccessExecutionRole`, `AWSLambdaRole`, `AmazonAPIGatewayPushToCloudWatchLogs` (CDK's `RestApi`
   makes that role by default), `AmazonAPIGatewayAdministrator`, `AmazonAPIGatewayInvokeFullAccess`. The path (`service-role/`) is
   ignored and the name decides, as in Go. A name the rules do not cover is `NoSuchEntity` on attach and on read. Also:
   - `GetPolicy`, `GetPolicyVersion`, `ListPolicyVersions`, `ListPolicyTags` and `ListEntitiesForPolicy` answer for a known
     AWS-managed policy that the account never attached (AWS's exist regardless); the document is built for that answer and dropped.
   - An AWS-managed policy is kept in the store the first time it is attached **or used as a permissions boundary** (a boundary
     of `PowerUserAccess` was refused before).
   - Its `PolicyId` is stable (`ANPA` + 17 characters from SHA-256 of the name), so the id shown before and after attaching agrees.
   - A record kept by an earlier build with the empty document is given its document when the store is opened (`fill_aws_managed`).
   - Lazy: nothing is built until a policy is named; the tables are a few hundred bytes of static strings.
   - **Approximate** (D9: the real documents are to be recorded from the AWS account). Tests: the four in `awsmanaged.rs` (every
     document passes `aws_iam_policy::check`, the naming rules, unknown names), and in `tests/usage.rs` a role session under
     enforce is allowed and refused by `AWSLambdaSQSQueueExecutionRole`, `ReadOnlyAccess` and `PowerUserAccess`.

3. **`GetAccessKeyLastUsed` and `RoleLastUsed` are real** (`authz.rs::note_use`). When the authorizer resolves a caller to an IAM
   access key it notes the time, the service (the action's namespace, lower-cased) and the region; a role session notes the time and
   region on its role. At most one write per key or role per minute (AWS says its own data may lag by hours). The write is staged and
   sent after the authorizer answers, without waiting on it. Only in soft and enforce: with IAM off the authorizer is never asked.
   Tests: `a_keys_last_use_is_noted_at_most_once_a_minute` (no rows staged within the minute, one after it),
   `a_roles_last_use_is_noted_from_its_sessions`; seen end to end under `--iam enforce` (below).

4. **`DozeAccessLog` and `DozeGeneratePolicy`** (`accesslog.rs`, `lib.rs`). **Exposure, and why it differs from Go:** Go served
   them as extra `Action`s on the IAM Query endpoint. Here the gateway resolves a Query `Action` through the model's tables and
   answers `InvalidAction` for anything else, so that route needs a change to `aws-gateway` and `aws-wire`. Instead IAM uses the
   existing `Web` hook (the one Lambda function URLs use), which needs no change outside `aws-iam`:
   - `GET /_doze/iam/DozeAccessLog` (`?Reset=true` clears) answers JSON: `Mode`, `Entries` (Principal, Action, Resource, Decision
     `allowed`/`implicitDeny`/`explicitDeny`, Reason, Count, FirstSeen, LastSeen), `Evicted`.
   - `GET /_doze/iam/DozeGeneratePolicy?Principal=<arn>&ScopeToResources=true` answers `PolicyDocument` (a JSON object, not a
     string, so `jq .PolicyDocument` gives a file), `StatementCount`, `UnresolvedResources`. A role's ARN finds its sessions.
   - Parameters may also come as a form body. Unsigned, like a function URL; `_` cannot begin a bucket name, so no S3 path-style
     address is taken.

   The log is a map of (principal, action, resource) rows with count and first/last seen, at most 2,000 rows (the least recently
   seen goes first, and `Evicted` counts them). It lives beside the actor (not in it), so it survives the actor's idle sleep, and is
   not allocated until the first question; with IAM off nothing is ever asked. The actor hands the questions it answered back with
   each answer (`Answer::Authorize(verdict, seen)`, `Answer::Call { seen, .. }`). The reason of an allow names the statement and
   policy (`allowed by statement 1 of AWSLambdaSQSQueueExecutionRole (aws-managed)`); of a refusal, the end of AWS's sentence.
   Services answer `*` for an action on no resource and for one they did not resolve, so a scoped policy counts every `*` row in
   `UnresolvedResources` (Go could tell the two apart; Rust services cannot yet).

   **Also:** a request signed with a key IAM cannot have issued (not `AKIA`/`ASIA`) is the account root; it is now answered and
   logged in `Shared` without waking the IAM actor, which makes the long-standing comment in `authz.rs` true ("an account with no
   identities costs nothing"). Test: `the_root_is_answered_without_waking_iam_and_the_log_and_generator_are_served` asserts the
   actor is not live afterwards.

5. **STS `AssumeRoleWithSAML` reads the assertion** (`saml.rs`, `sts.rs`). A small bounded XML walker (100,000 bytes decoded, depth
   32, 10,000 elements, no DOCTYPE, entities and CDATA handled) keeps attributes, which `aws-wire`'s XML reader drops (its `Elem`
   has no attributes, and adding them would change a public struct S3 builds by literal). Answered: `Subject` (NameID), `SubjectType`
   (the NameID format without `urn:oasis:names:tc:SAML:2.0:nameid-format:`), `Issuer` (the assertion's, over the response's),
   `Audience` (the `Recipient` of `SubjectConfirmationData`, else the first `Audience`), `NameQualifier`
   (BASE64(SHA1(issuer + account + "/" + provider name))), `SourceIdentity` (the attribute). The session name is the
   `https://aws.amazon.com/SAML/Attributes/RoleSessionName` attribute. **The signature is not validated.** What is not base64 of a
   SAML response is `InvalidIdentityToken`. The web-identity path is unchanged except that `RoleSessionName` (required by the
   model) is used as given. New dependency for `aws-iam`: `sha1 0.11`, already in the binary through S3 and KMS (no new package).

6. **Stale comments**: `lib.rs` no longer says "soft mode only"; `sts.rs` describes off/soft/enforce.

## Evidence

- `cargo test -p aws-iam`: 52 passed (19 new: 3 access log, 4 managed documents, 3 SAML reader, 8 in `tests/usage.rs`, 1 in
  `tests/sessions.rs`); `aws-iam-policy` 21 passed.
- Weight (`cargo xtask weight doze-aws`, darwin/arm64, release-aws): **4,950,592 B, 233 packages, idle 4,528 to 4,544 KB**, against
  4,917,280 B, 233 packages, 4,464 KB for the base of this branch (`5cc7e6aa`): **+33 KB, no package**, idle within a run's noise
  (+64 to +80 KB, nothing new runs at idle). Limit 7,900,000 B / 240 / 6,144 KB: ok.
- `cargo clippy --workspace --all-targets` clean; `cargo fmt --all`.
- Scoreboard (`cargo xtask conformance ... iam` / `sts` on the release-aws binary): **iam 45 labels, 36 exact, 9 unverified;
  sts 2 of 2 exact** (unchanged).
- End to end against `target/release-aws/doze-aws --iam enforce` with curl (SigV4): a user with `AmazonSQSFullAccess` creates a
  queue and is refused `SNS:CreateTopic` in AWS's words; `GetAccessKeyLastUsed` then shows `sqs`, `eu-west-1` and the time;
  attaching `arn:aws:iam::aws:policy/Nope` is `NoSuchEntity`; `/_doze/iam/DozeGeneratePolicy?Principal=...user/dev` gives
  `["SNS:CreateTopic","sqs:CreateQueue"]` on `*`. Under soft with `test` keys the log shows the root's questions and the scoped
  generator gives one statement per resource.
- No boto3 scenario was added: the access log and generator are doze-only, `RoleLastUsed` lags by hours on AWS, and the SAML path
  needs a provider and a signed assertion AWS would accept. All four would not run against real AWS as written.

## (memory) items, not checked against a recording

- The words for an unknown AWS-managed policy: `Policy <arn> was not found.` (the recorded wording for an absent customer policy on
  `GetPolicy`; AWS may say `... does not exist or is not attachable.` on attach).
- The AWS-managed documents beyond `AdministratorAccess`: the Lambda execution roles' action lists (Kinesis `SubscribeToShard`, the
  VPC role's EC2 actions), `PowerUserAccess`'s second statement, the extra reads in read-only policies (`dynamodb:Query`/`Scan`,
  `sqs:ReceiveMessage`, `logs:FilterLogEvents`/`StartQuery`/`StopQuery`, `kinesis:SubscribeToShard`), `AmazonAPIGatewayPushToCloudWatchLogs`.
- The AWS-managed `CreateDate` (2015-02-06T18:39:46Z).
- `AccessKeyLastUsed.ServiceName` as the lower-case namespace.
- SAML: the `SubjectType` rule, `Audience` as the recipient, the `NameQualifier` formula (all from the STS reference); the
  `InvalidIdentityToken` sentence is doze's; an assertion without a `RoleSessionName` attribute gets `saml-session`.

## Open questions

- Should the Doze* extensions also be reachable as Query `Action`s, as in Go? That needs a hook in `aws-gateway`'s Query path
  (an `extra(action, form)` on `Service`); a small change, but outside this crate.
- Does AWS refuse a SAML assertion with no `RoleSessionName` attribute (and with what)? Recordable once a SAML provider exists in the
  recorder account.
- Services cannot yet tell "no resource" from "resource not resolved" (both `*`), so `UnresolvedResources` over-counts list calls.
