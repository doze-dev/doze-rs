# ops1: unbuilt SSM, Secrets Manager and KMS operations (stage note, 2026-11-15)

Branch `aws-ops1`, on `aws` at `78ba6190`. The integrator folds this note into `PLAN.md`, `STATUS.md` and `scoreboard.md`; none of
those was touched here. AWS is the only reference: each behaviour cites the model documentation (`models/<service>.json`) or says it
is not recorded. Wording not seen recorded is marked `(memory)` in the code beside it. Every new boto3 scenario is UNVERIFIED until
the integrator records it.

## 0. `aws-yaml` (new crate; `crates/aws-cloudformation/src/yaml` moved)

SSM documents are JSON or YAML, so the bounded YAML reader CloudFormation had (written in-house for size, fuzzed) moved to its own
crate, with a small writer (`emit`: block style, every string double-quoted so nothing re-reads as a number or boolean). CloudFormation
uses it unchanged (`use aws_yaml as yaml`); the `fuzz_yaml` target moved with it and now also checks that what is read writes back as
YAML that reads.

## 1. SSM public parameters (`crates/aws-ssm/src/public.rs`)

- `GetParameter`, `GetParameters` and `GetParametersByPath` (one level or recursive, paged) serve a seeded, read-only table under
  `/aws/service/`: Amazon Linux 2023 and 2 images by kernel and architecture (the names CDK's `latestAmazonLinux2023()` and
  `latestAmazonLinux2()` read), ECS-optimized (the JSON summary and its `image_id`/`image_name`), EKS-optimized AL2023, Bottlerocket,
  Ubuntu 22.04/24.04, Windows Server 2019/2022/2025, and `global-infrastructure` regions (`longName`, `partition`, `domain`) and
  services (`longName`).
- **The image ids are placeholders** shaped like AWS's (`ami-0` and 16 hex digits, stable per region and name). They are not real
  AMIs. They exist so a template parameter of type `AWS::SSM::Parameter::Value<AWS::EC2::Image::Id>` or a CDK app resolves and deploys.
  Version 1 and a fixed date (2026-01-01) are reported. Nothing is stored; the table costs no memory until asked.
- ARN with no account (`arn:aws:ssm:<region>::parameter/aws/service/...`) (memory); `Type` `String`, `DataType` `text`. A `:1`
  selector is the only version. **AWS-correct** as documented; the ARN shape and data type are (memory).
- `DescribeParameters` does not list them: the model says it "lists the parameters in your Amazon Web Services account".
- Writes under `/aws/` were already refused with AWS's recorded `AccessDeniedException`.
- `/aws/reference/secretsmanager/...` was already built; nothing else lives under `/aws/reference/`.
- Open: `GetParameterHistory` of a public parameter answers `ParameterNotFound` (AWS's answer not known).

## 2. SSM documents (`document.rs`, `documents.rs`, `doc_share.rs`, `doc_shown.rs`, `doc_storage.rs`)

- Built: `CreateDocument`, `GetDocument`, `DescribeDocument`, `UpdateDocument`, `DeleteDocument`, `ListDocuments`,
  `ListDocumentVersions`, `UpdateDocumentDefaultVersion`, `DescribeDocumentPermission`, `ModifyDocumentPermission`,
  `UpdateDocumentMetadata`; tags with `ResourceType=Document`. Nothing runs a document.
- Content checked as the model documents: JSON or YAML per `DocumentFormat` (JSON default, `TEXT` taken as is), at most 64 KB
  (`MaxDocumentSizeExceeded`), an object, a `schemaVersion` the type accepts (Command 1.2/2.0/2.2, Automation and change templates
  0.3, Session 1.0, Policy and Package 2.0: `InvalidDocumentSchemaVersion`), and the part the schema needs (`mainSteps`,
  `runtimeConfig`, `sessionType`: `InvalidDocumentContent`). The model's reserved name prefixes (`aws`, `amazon`, `amzn`) are refused.
  Words (memory).
- `DescribeDocument` reads parameters, description and platforms out of the content (platforms: shell-only Command → Linux, MacOS;
  PowerShell-only → Windows; else all three: (memory)). `Hash` is SHA-256. `CreateDocument` answers `Creating`, `UpdateDocument`
  `Updating`, a describe `Active` (memory: AWS's status flow; Terraform waits for `Active`).
- A new version is not the default until `UpdateDocumentDefaultVersion` (model). Only `$LATEST` can be updated (model);
  identical content is `DuplicateDocumentContent`, a reused `VersionName` `DuplicateDocumentVersionName`. The default version cannot be
  deleted alone (memory). `GetDocument` converts between JSON and YAML when asked (key order of a converted document is sorted).
- `ListDocuments` lists the account's own documents only: AWS-owned documents (`AWS-RunShellScript`) are not served, so
  `Owner=Amazon` is empty. Filters: `Owner`, `Name`, `DocumentType`, `PlatformTypes`, `SearchKeyword`, `TargetType`, `SchemaVersion`,
  `tag:<key>`, and the deprecated `DocumentFilterList`.
- Sharing keeps the account ids (`all` for public), up to 20; a public share is refused while the service setting
  `/ssm/documents/console/public-sharing-permission` is `Disable` (memory). `UpdateDocumentMetadata` works on change templates only
  and sets the version's review status.
- Attachments are refused with a sentence (doze-aws keeps none).
- CloudFormation: `AWS::SSM::Document` moved from "skipped" to mapped (`crates/aws-cloudformation/src/res/ssm_doc.rs`): `Content` as
  an object or text, generated name when none, an update writes a new version and makes it the default (memory: `UpdateMethod:
  NewVersion`; the same is done for `Replace`). Create-only: `Name`, `DocumentType`.

## 3. SSM service settings (`settings.rs`)

- `GetServiceSetting`, `UpdateServiceSetting`, `ResetServiceSetting` for the ten setting ids the model lists, by id or ARN. A setting
  answers its default (`Status: Default`) until updated (`Customized`); values are held to the model's lists; an unknown id is
  `ServiceSettingNotFound`. Defaults are (memory); `default-ec2-instance-management-role` has no default and is not found until set.
- `/ssm/parameter-store/default-parameter-tier` applies: a `PutParameter` that names no tier takes it (what the setting is for in AWS's user guide: memory). The other settings are kept and answered.

## 4. Secrets Manager replicas (`crates/aws-secrets/src/replication.rs`)

- `ReplicateSecretToRegions`, `RemoveRegionsFromReplication`, `StopReplicationToReplica`, and `AddReplicaRegions` /
  `ForceOverwriteReplicaSecret` on `CreateSecret`. A replica is a secret of the same name in the other region (ARN with the region
  changed: model), kept by that region's actor and sealed under a key of that region (`KmsKeyId` or `aws/secretsmanager`: model).
- It follows every change of its primary (value, stages, description, tags) and refuses every change itself
  (`InvalidRequestException`, (memory)). `DeleteSecret` of a primary with replicas is refused (model); `StopReplicationToReplica` in the
  replica's region promotes it and the primary forgets it (model).
- `DescribeSecret` and `ListSecrets` report `PrimaryRegion` and `ReplicationStatus`; the `primary-region` filter works. A secret of
  the same name already in the region makes that replica `Failed` with the model's example message "Secret with this name already
  exists in this region", unless the call forces the overwrite.
- How: the primary's actor answers with the work for other regions; `Secrets::ask` does it outside the actors before the call is
  answered (the replica takes the secret, values open in memory, then the primary is told how it went). No actor waits on another, so
  two regions replicating to each other cannot deadlock. Departure: AWS replicates in the background (`InProgress` first); here a call
  answers once the replicas are in sync, so a describe right after shows `InSync`.
- Replication state is a table of its own (`replication`), so the secret record keeps its on-disk shape. `doze-aws data reset` clears it.

## 5. KMS (`crates/aws-kms/src/agree.rs`, `multi.rs`, `shown.rs`)

- **`DeriveSharedSecret`:** `KeyUsage=KEY_AGREEMENT` on `ECC_NIST_P256/P384/P521` (model: NIST curves; SM2 is China-only), ECDH, the
  raw x-coordinate as `SharedSecret`. `DescribeKey` and `GetPublicKey` list `KeyAgreementAlgorithms: [ECDH]` and no signing algorithms
  for such a key. A peer key on another curve is `ValidationException` (memory); `DryRun` answers `DryRunOperationException` (memory);
  `Recipient` (Nitro attestation) is refused with a sentence. New usage appended to the enum, so keys on disk read the same.
- **Multi-Region keys:** `ReplicateKey` makes a replica in the other region's actor with the same `mrk-` id and material (each region
  seals it under its own sealing key), so either region opens what the other sealed. `UpdatePrimaryRegion` swaps primary and
  replica. `DescribeKey` shows `MultiRegionConfiguration` with the primary and every replica, in every region of the set. Shared, as
  the model says: id, spec, usage, material (a rotation of the primary reaches every replica), rotation (set and done on the primary:
  `UnsupportedOperationException` on a replica, (memory)). Not shared: description, policy, tags, state, aliases. A replica scheduled
  for deletion leaves the set; cancelling brings it back. Same mechanism as Secrets Manager (work across regions done outside the
  actors). The set is a table of its own (`multiregion`).
- Departures: AWS makes a replica in the background (`Creating`) and changes the primary region through an `Updating` state; here
  both are done when the call answers. A primary with replicas cannot be scheduled for deletion (AWS holds it in
  `PendingReplicaDeletion`): the refusal says to delete the replicas first. A multi-Region key with imported material is not
  replicated (refused with a sentence).
- **`GetKeyLastUsage`:** the last successful cryptographic operation (the model's list) per key, with the request id. In memory only:
  `TrackingStartDate` is when the region's KMS last started (or the key's creation, if later), which is how the model describes keys
  used before tracking began. No CloudTrail runs, so `CloudTrailEventId` is never given. An alias is refused (the model: aliases are
  not supported).

## 6. Storage

`Storage` stays correct for every new table: SSM documents with more than one version are listed as `document/<name>` with a note,
`--old-versions` removes versions that are neither default nor latest, and a reset removes documents and changed settings (counted).
Secrets Manager's reset clears replication; a replica is a secret of its region and is listed and emptied as one. KMS counts the
multi-Region sets' bytes and its reset clears them (and the in-memory last use). Tests: `aws-ssm/src/tests/documents.rs`,
`aws-secrets/src/tests/persistence.rs` (replication survives a restart), `aws-kms/src/tests/multi.rs`.

## 7. Scenarios (UNVERIFIED)

`test_ssm.py`: public parameters, document lifecycle, document content checks, service settings. `test_secretsmanager.py`: replica
follows its primary; replica promoted. `test_kms.py`: derive shared secret, multi-Region replica, update primary region, key last
usage. They poll where AWS works in the background. The local harness now accepts `region_name` for a second client.

## 8. Go mentions

`aws-secrets/src/rotate.rs` was the only one in the three crates: the sentence now says what the code does, against AWS.

## Open

- Recording of the new scenarios (owner logged in); every (memory) wording above.
- `AWS::SecretsManager::Secret` `ReplicaRegions` and `AWS::KMS::ReplicaKey` in CloudFormation are not mapped.
- `PendingReplicaDeletion` for a multi-Region primary; the `Creating`/`Updating` states.
- Public parameters' history; AWS-owned SSM documents.
