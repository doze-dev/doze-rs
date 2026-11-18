# rec2: the 13 new scenarios recorded on AWS, and doze-aws answering as AWS did (branch `aws-rec2`, 2026-11-15)

On top of the WIP commit that holds the first recordings (`d4aa5144f`). AWS is the judge: every behaviour below was seen recorded
(account 821712915674, `doze-rec-main`, us-east-1 with replicas in us-west-2) unless it is marked `(memory)` in the code.

## Score (`cargo xtask conformance --save`, release build)

| module | labels | exact | wording | behaviour | unverified |
|---|---|---|---|---|---|
| kms | 132 | 130 | 1 | 1 | 0 |
| ssm | 108 | 101 | 1 | 0 | 6 |
| secretsmanager | 78 | 78 | 0 | 0 | 0 |
| cloudwatch | 40 | 40 | 0 | 0 | 0 |
| eventbridge | 90 | 90 | 0 | 0 | 0 |
| lambda | 70 | 70 | 0 | 0 | 0 |

The three recorded labels that are not exact, on purpose (the owner decided 2026-11-18 to keep the helpful wording for the two
empty AWS messages; both are listed as differences in `docs/aws/compatibility-notes.toml`):

- `test_kms::test_derive_shared_secret::other-curve` (wording). AWS answers `ValidationException` with an **empty** message for a
  peer public key on another curve. doze-aws keeps the code and says what is wrong (`The PublicKey is not a DER-encoded
  SubjectPublicKeyInfo of a ECC_NIST_P256 key.`): with no AWS words to put first, an empty message would tell a developer nothing.
- `test_ssm::test_service_settings::unknown-setting` (wording). Same case: AWS answers `ServiceSettingNotFound` with an empty
  message; doze-aws names the setting and says to check its id.
- `test_kms::test_key_last_usage::after-encrypt` (behaviour). Right after an `Encrypt`, AWS's `GetKeyLastUsage` still answers an
  empty `KeyLastUsage`: AWS records usage asynchronously. doze-aws records it at once (operation, time, request id), which is what
  the call is for locally. Not a deviation entry: the difference is a delay on AWS's side, not a different answer.

The six unverified SSM labels are the scenarios the recorder may not run (below): they run against doze-aws only.

## Harness

- `normalize.py`: a KMS multi-Region key id (`mrk-` + 32 hex) is tokenized as `mrk-<mrk:n>`; it was left verbatim, so no other
  target could ever match it.
- `target.py`: the recorder's `doze-rec` mark is hidden from `ReplicateKey`'s `ReplicaTags` too (it was only hidden from
  `ListResourceTags`).
- `snapshot.py`: a scenario skipped on the recording target no longer writes an empty recording.
- Caller: no snapshot holds the SSO user or role name (grep for the user, `AWSReservedSSO`, `doze-recorder`, the account id: none);
  the only caller-shaped text is `User: <caller>` in refusals, which the normalizer already rewrites. No normalizer change needed.

## Scenarios changed

- `test_kms`: `test_update_primary_region` waits until the old primary is `Enabled` again before cleanup (a key still `Updating`
  refuses `ScheduleKeyDeletion`, and the cleanup swallows the error).
- `test_ssm::test_public_parameters_are_read_only`: records the namespace refusals (below) and a `DescribeParameters` inside a real
  namespace; the version and date of `.../regions/us-east-1/longName` are dropped (they are its own, not the image's).
  The write refusal moved to `test_public_parameters_cannot_be_written`, skipped on AWS: the recorder's own policy refused
  `PutParameter` first, so AWS's answer was never reached.
- `test_ssm::test_service_settings`: `GetServiceSetting` only (recorded). Update and reset moved to
  `test_service_settings_update_and_reset`, skipped on AWS: changing a setting changes the real account.
- `test_ssm::test_document_content_is_checked`: the reserved-name refusal (`AWS-...`) was recorded as the recorder's own
  `AccessDenied`; it moved to `test_document_names_aws_reserves`, skipped on AWS, and the label was removed from the recording.
- `test_ssm::test_document_lifecycle`: shares with `444455556666`, not `111122223333` (doze-aws's own account in the suite, which
  normalized to `<account>` on one side only).
- `test_secretsmanager::test_replica_follows_its_primary`: `replicate-again` is an outcome, not an error (AWS accepts it, below);
  the secret's ARN is named (`known`) because a refusal quotes it with its random suffix.

## What AWS taught us, and what doze-aws now does

KMS (`aws-kms`):
- `ReplicateKey` answers the replica as `Creating`, `Enabled: false`, without `CurrentKeyMaterialId`; it is `Enabled` moments later.
  doze-aws makes the replica at once, so only that answer says `Creating` (`shown::creating`). `ReplicaTags` lists the tags given.
- Refusals in AWS's words: `... is not a multi-region key.`, `... is not a multi-region primary key.`, `You can create only one
  replica of each primary key in each AWS Region.` (AlreadyExists), `... is a multi-region replica key which is not valid for
  RotateKeyOnDemand.` (same form for Enable/DisableKeyRotation, memory).
- `UpdatePrimaryRegion` to a region without a replica is `ValidationException` `<key id> does not have a replica key in the
  specified Region.` (it was `UnsupportedOperationException`).
- `GetPublicKey` of a key that does not sign leaves `SigningAlgorithms` out (it was an empty list).
- A primary scheduled for deletion while it has replicas goes to `PendingReplicaDeletion` on AWS; doze-aws still refuses that
  (see `multi.rs`, departure from `ops1`). The scenarios' cleanup deletes the replica first, so it does not matter to them.

SSM (`aws-ssm`):
- A name under `/aws/service/` outside every namespace AWS publishes in is refused by `GetParameter`, `GetParameters` (the whole
  call, not one `InvalidParameters` entry), `GetParametersByPath` and `DescribeParameters`: `AccessDeniedException`, `No access to
  "/aws/" namespace: aws/service/<segment> is not a valid namespace` (the namespace is the first three segments; `/aws/service/`
  itself names `aws/service/`). A name AWS does not publish inside a real namespace is an ordinary miss. New `namespace.rs`; its
  list of real namespaces beyond the ones doze-aws serves is (memory).
- `DescribeParameters` with a `Name` `BeginsWith` inside a real namespace lists AWS's public parameters (10 a page, `String`,
  `Standard`, `text`, no policies, an ARN without account). doze-aws said they were never described.
- The `global-infrastructure` public parameters have no `DataType`.
- `GetServiceSetting` of a default setting has a `LastModifiedDate`.
- Documents: versions are listed newest first; `DescribeDocument` alone carries empty `Category` / `CategoryEnum`; neither
  `ListDocuments` nor `UpdateDocumentDefaultVersion` answers a `VersionName` / `DefaultVersionName`; a document without
  parameters has no `Parameters`; messages: `Document with same name <name> already exists`, `Document to be updated with name
  <name> has the same metadata and content as document version provided in request`, `Default version of the document can't be
  deleted.`, `Document with name <name> does not exist.`, `JSON not well-formed. at Line: 1, Column: 2`, `Missing "mainSteps" in
  the document.`, and a schema version no type takes is `InvalidDocumentContent` `Document schema version is invalid. Value: 9.9
  at Line: 1, Column: 24` (the position of the token after the value; a version valid for another type keeps
  `InvalidDocumentSchemaVersion`, memory).

Secrets Manager (`aws-secrets`):
- `ReplicateSecretToRegions` to a region that already holds the replica is accepted: the replica is made again (`InProgress`, then
  `InSync`). doze-aws refused it.
- Replica refusals are `InvalidParameterException`, not `InvalidRequestException`: `Operation not permitted on a replica secret.
  Call must be made in primary secret's region.`, `Operation not permitted on a primary/standalone secret.`, and `You can't delete
  secret <arn> that still has replica regions [us-west-2].`
- A replica status names its key even when it is the default (`alias/aws/secretsmanager`) and says `Replication succeeded` when in
  sync; `RemoveRegionsFromReplication` lists what is left even when nothing is; a replica has its own `CreatedDate` and names its
  key in `DescribeSecret`.

CloudWatch (`aws-cloudwatch`): `Period must be 10, 20, 30 or a multiple of 60`; `Metrics cannot be checked across more than a day
(EvaluationPeriods * Period must be <= 86400) for alarms using period < 3600` (the week form, memory).

Lambda (`aws-lambda`): an `AWS_IAM` function URL refuses a wrong signature with the body `{"message": ...}` (lower case), where an
unsigned request gets `{"Message": "Forbidden"}`.

## Left behind on AWS

Nothing live. Every KMS key these runs made is scheduled for deletion (7 days) in both regions. The four multi-Region primaries
wait in `PendingReplicaDeletion` for their replicas (all `PendingDeletion`): `mrk-111bc2b4b5434e44b56a36c40289642c`,
`mrk-6264a9e41fc34809a986fd235e7d3daf` (primary in us-east-1), `mrk-1222dd80834049258e6f3e5e1be4f366`,
`mrk-46ed9f99a200492c95671ffd82406792` (primary moved to us-west-2 by `test_update_primary_region`). The only enabled customer key,
`mrk-4b75e6d2357a44c1b6cd554c5252c1fa` (2026-10-17), is IAM Identity Center's (tag `CreatedBy: IAM Identity Center`), not ours.
No secret is left in either region, the replicas included.

## Open

- doze-aws does not keep a multi-Region primary in `PendingReplicaDeletion`.
- The namespace list beyond the served ones, the YAML schema-version error's position, and the other operations' wording forms are
  (memory) until a scenario records them.
