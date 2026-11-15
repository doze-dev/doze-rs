# Verify group a: SQS, SSM, Secrets Manager, KMS, IAM, STS against the 2026-11-15 recordings

Branch `aws-va` from `aws` (6ee5c7ca). AWS is the judge: where a scenario guessed wrong about AWS, the scenario was fixed and only
that scenario re-recorded (account 821712915674, us-east-1, profile `doze-rec-main`); where doze-aws differed from a recording,
doze-aws was fixed.

## Scores (labels / exact / wording / behaviour / unverified)

| module | before | after |
|---|---|---|
| sqs | 181 / 176 / 2 / 3 / 0 | 181 / 180 / 0 / 1 / 0 |
| ssm | 67 / 49 / 12 / 6 / 0 | 67 / 67 / 0 / 0 / 0 |
| secretsmanager | 65 / 33 / 1 / 1 / 30 | 64 / 63 / 0 / 0 / 1 |
| kms | 112 / 93 / 8 / 11 / 0 | 112 / 112 / 0 / 0 / 0 |
| iam | 45 / 44 / 1 / 0 / 0 | 45 / 45 / 0 / 0 / 0 |
| sts | 2 / 2 / 0 / 0 / 0 | 2 / 2 / 0 / 0 / 0 |

Secrets Manager has one label fewer: `gone` (a describe straight after a delete without recovery) asserted a race, and was dropped
(below). The one unverified label is `test_batch_get_secret_value::batch`: the recorder's role is refused
`secretsmanager:BatchGetSecretValue` (action on resource `*`; the call also needs `secretsmanager:GetSecretValue` on each secret,
which the role has). Owner to add it; the scenario then records as it stands.

The one SQS difference left is a **decided deviation** (PLAN.md, "Protocols by service", settled): `test_query_batch_with_a_gap`.
AWS still serves SQS over the Query protocol and answers a gap in the entry numbers with 400 `MissingParameter`; doze-aws serves
SQS over JSON only and answers a Query client 400 `UnsupportedProtocol` (XML, so an old SDK can read it) saying to upgrade.

## SQS: which label AWS now answers differently, and why

`test_move_task_default_destination::listed`. On 2026-11-09 AWS listed a COMPLETED one-message move task with
`ApproximateNumberOfMessagesMoved: 0`; on 2026-11-15 the same scenario got `1`. The port had modelled the 0 as a rule (a finished
task's count held at 0 for five seconds, `COUNTER_LAG_MS`). It was AWS's counter catching up, not behaviour: **Go/port wrong
(fixed)**, a finished task now reports what it moved (`crates/aws-sqs/src/moves.rs`, test `tests::moves`).

The three other old differences, previously written off as unfixable or as AWS sampling, are now exact:
- `not-in-flight`, `change-visibility-with-the-old-handle`: AWS quotes the receipt handle in its refusal. The scenario now names the
  handle it holds (`normalizer.name`), so both sides read `Value <name:receipt-handle> for parameter ReceiptHandle is invalid...`.
  Scenario fix; doze-aws was already AWS-correct.
- `test_redrive_receive_count::in-the-dlq`: AWS handed back one dead-lettered message ten times in one receive with
  `VisibilityTimeout=0` (both recordings). The scenario now asserts it is one message (`distinct: 1`) and what its first copy says.
  Doze-aws returns it once: AWS-correct for what is promised; copying the duplicates would copy an accident.

## SSM

| label | fix | call |
|---|---|---|
| `reserved-prefix`, `reserved-word-starts-a-segment` | doze-aws: a name whose first part begins with `aws` (`/aws/x`, and `/awsome-x/y` too) is `AccessDeniedException` `No access to reserved parameter name: <name without its slash>.` | Go wrong (fixed): it allowed `/awsome/x` and answered a ValidationException |
| `reserved-word` | doze-aws: a name beginning with `ssm` gets the short `Parameter name: can't be prefixed with "aws" or "ssm" (case-insensitive).` | AWS-correct now |
| `path-without-slash`, `by-path-does-not-list-it` | doze-aws: AWS's text has literal backslashes before the quotes around aws and ssm; `GetParametersByPath` under `/aws/reference` is refused with it | Go wrong (fixed): the reference path was listed as empty |
| `tags-of-absent` | doze-aws: `InvalidResourceId` with an empty message | AWS-correct now |
| `tags-of-a-document-type` | doze-aws (`Ssm::invalid`): `Value at 'resourceType' failed to satisfy constraint: Member is not a valid resourceType` | AWS-correct now |
| `bad-filter-key` | doze-aws: the whole path (`parameterFilters.1.member.key`, shared fix below), and SSM prints a `^(...)$` pattern without anchors and group | AWS-correct now |
| `over-4096-standard`, `policies-on-standard` | doze-aws: AWS's sentences, with the documentation link | wording |
| `pattern-mismatch`, `bad-pattern` | doze-aws: `Parameter value, cannot be validated against allowedPattern: <p>` / `The syntax of the allowedPattern parameter is invalid.` | wording |
| `bad-data-type` | doze-aws: any change of an existing parameter's `DataType` is refused `You can’t change a parameter’s data type.` (curly apostrophes, AWS's) | Go wrong (fixed): it judged the value before the change |
| `description-after-overwrite` | scenario: `LastModifiedUser` held opaque as in the other labels (the recorder is an SSO session) | scenario |
| `open`, `bulk`, `sealed`, `absent` | doze-aws: a secret read through `/aws/reference/secretsmanager` carries the secret's ARN, no `DataType`, and `SourceResult` (the secret as JSON, its creation date in Java's medium form); AWS's words for the sealed and absent refusals. Scenario: the ARN's random suffix and the prose date reduced to what two clouds share | Go weak (bettered): `SourceResult` is how a caller learns the secret's version and stages |

## Secrets Manager

| label | fix | call |
|---|---|---|
| (scenario failures) | scenario: BatchGetSecretValue split into its own scenario (permission, above); `gone` dropped and the empty secret given a fresh name: AWS deletes without recovery asynchronously, so a describe straight after still answered | scenario |
| `by-name` | scenario: waits until all three new secrets are listed (AWS listed two of three straight after creating them) | scenario |
| `bad-filter` | doze-aws: whole path (`filters.1.member.key`) and the filter keys in AWS's order | AWS-correct now |
| `now-current`, `tags-now` | doze-aws: staging labels keep the order they were attached (`["PENDING", "AWSCURRENT"]`); stored as the same sequence on disk | Go wrong (fixed): it sorted them |
| `same-token-other-value` | doze-aws: `You can't use ClientRequestToken <t> because that value is already in use for a version of secret <arn>.`; scenario names the secret ARN it holds | wording |
| `neither-version` | doze-aws: `InvalidRequestException` `To update staging label X, you must specify RemoveFromVersionId and/or MoveToVersionId.` | Go wrong (fixed): wrong code |
| `drop-current`, `password-nothing-left`, `put-bad-policy` | doze-aws: AWS's sentences | wording |

## KMS

| label | fix | call |
|---|---|---|
| `create-signer`, `public-key` | doze-aws: RSA signing algorithms listed PKCS #1 v1.5 first, then PSS | AWS-correct now |
| `encrypt-without-algorithm` | doze-aws: no algorithm is `SYMMETRIC_DEFAULT`: `InvalidKeyUsageException` `Algorithm SYMMETRIC_DEFAULT is incompatible with key spec RSA_2048.` | Go wrong (fixed): ValidationException |
| `decrypt-without-key` | doze-aws: an asymmetric algorithm with no `KeyId` is `ValidationException` `KeyId must not be null` | Go wrong (fixed): it tried the ciphertext as symmetric |
| `too-long` | doze-aws: the OAEP limit is computed and named (`... cannot encrypt data larger than 190 bytes.`) | Go weak (bettered): the developer is told the limit |
| `digest-wrong-length` | doze-aws: `Digest is invalid length for algorithm <alg>.` | wording |
| `same-name-same-grant`, `grants`, `no-grants` | doze-aws: a grant is the same grant only when the name and everything else match; the same name asking for something else is a new grant. Scenario: grants listed unordered (AWS's order changed between recordings), grant ids opaque in `no-grants` | Go wrong (fixed): it deduplicated by name alone |
| `revoke-absent` | doze-aws: `Grant ID <id> not found`; scenario names the grant id it holds | wording |
| `material-changed`, `rotations` | scenario: AWS rotates on demand in the background (nothing listed and the material unchanged straight after); it now waits for the end state. Doze-aws: a rotation lists `KeyMaterialState` (`CURRENT` or `NON_CURRENT`) | AWS-correct; doze-aws rotates at once, which is the state AWS reaches |
| `re-encrypt` | scenario: `SourceKeyMaterialId`/`DestinationKeyMaterialId` held opaque (hashes of random material) | scenario |
| `policy-bad-name` | doze-aws: `ValidationException` `Invalid policy name` | Go wrong (fixed): NotFoundException |
| `put-bad-policy`, `rotate-a-signing-key` | doze-aws: AWS's messages are empty | AWS-correct now |
| `reserved-prefix`, `reserved-tag` | doze-aws: AWS's sentences | wording |
| `bad-operation` | doze-aws: a bad element of a list of strings is shown as the list (`Value '[Nonsense]' at 'operations' ... Member must satisfy constraint: [Member must satisfy enum value set: [...]]`, shared fix below) with the grant operations in AWS's order | AWS-correct now |

## IAM

`delete-user-with-a-key`: AWS asks about group membership before access keys, in the words it uses for a group (`must remove users
from group first.`). Go wrong (fixed), `crates/aws-iam/src/entity.rs`.

## Shared: where a failed model check sits (`crates/aws-wire/src/located.rs`)

The decoder reports the member that broke its constraint, not the way to it; AWS names the whole way. On the refusal path only, the
gateway (`pipeline.rs`) now re-reads the body leniently, walks it to the member holding the bad value, and rewrites a Coral-worded
message: a member inside a list of structures is `filters.1.member.key`, and a bad element of a list of scalars is shown as the list
in Java's form. A top-level member is unchanged; a service's own wording (not the Coral form) is left alone. `enum_order` lets a
service list an enum in the order AWS printed it where the reverse of the model's is not it (KMS grant operations, Secrets Manager
filter keys). Only the JSON pipeline uses it; Query and REST-JSON refusals are as they were.

## What AWS taught us that the code had wrong

- A finished SQS move task's count is not held at 0.
- Staging labels are a list in attach order, not a sorted set.
- KMS grant idempotency needs every parameter equal, not just the name.
- KMS on-demand rotation is asynchronous on AWS (the listing appears before `DescribeKey` shows the new material).
- SSM reserves every first segment that begins with `aws` (AccessDenied), not just `aws` itself, and refuses listing
  `/aws/reference`.
- A secret read through Parameter Store is the secret (its ARN, `SourceResult`), not a parameter.
- IAM checks a user's groups before its keys.

## Resources

Every re-recorded scenario cleaned up after itself (its `cleanup` fixture ran; KMS keys are left in PendingDeletion for the 7-day
minimum, as every KMS scenario does). Nothing is known to be left behind.
