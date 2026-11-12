# A9b: S3 Object Lock and GetBucketPolicyStatus (branch `aws-s3-lock`, 2026-11-13)

Closes the gap the integrator's parity check found against the Go edition (tier F there, refused by name here after A9): Object Lock
retention and legal hold with real enforcement, the bucket's default retention taking effect, and `GetBucketPolicyStatus`. Five
operations move from "refused by name" to built (85 of the model's 112 now; the rest stay refused by name, unchanged).

## What was built (`crates/aws-s3`)

**`lock.rs` (new).** `GetObjectRetention` / `PutObjectRetention` (GOVERNANCE and COMPLIANCE), `GetObjectLegalHold` /
`PutObjectLegalHold`, `GetBucketPolicyStatus`, and the rules around them:

- A version's lock is `Lock { retention: Option<{mode, until}>, legal_hold: Option<bool> }` (`model.rs`). `legal_hold` is `None` until
  one was ever set, which is what lets a read of it answer as AWS does for an object that never had one.
- A delete that names a version (`DeleteObject`, `DeleteObjects`) is refused with `AccessDenied` while a legal hold is on, or while
  retention has not run out: COMPLIANCE always, GOVERNANCE unless the request sends `x-amz-bypass-governance-retention: true`. A hold
  is never bypassed. A delete that names no version in a versioned bucket only adds a delete marker and is not stopped. `DeleteObjects`
  reports each refused key in the output's `Errors` and goes on with the rest. Lifecycle expiration goes through the same check (a
  locked noncurrent version is skipped and tried again at the next sweep). An overwrite that would replace a locked `null` version
  (versioning off or suspended) is refused too; unreachable on a bucket with Object Lock, which cannot be suspended, but the check is
  there.
- `PutObjectRetention`: making retention stricter is always allowed; shortening, removing, or turning COMPLIANCE into GOVERNANCE is
  refused for COMPLIANCE and, for GOVERNANCE, refused without the bypass. Retention that has run out can be set to anything. Mode and
  date must come together; the date must be in the future.
- `PutObject`, `CopyObject` and `CreateMultipartUpload` take `x-amz-object-lock-mode` / `-retain-until-date` / `-legal-hold`. A copy gets
  the lock its own request names, never the source's. An upload remembers its lock in a new table (`upload_locks`, in memory like the
  uploads) and the object gets it at `CompleteMultipartUpload`. Naming any of it on a bucket without Object Lock is `InvalidRequest`.
- The bucket's default retention (`Rule.DefaultRetention`: Mode with Days or Years, calendar years) is applied to a new version that
  names no retention of its own.
- `PutObjectLockConfiguration` is now checked (`ObjectLockEnabled` must be `Enabled`; a default retention needs a Mode and exactly one
  positive Days or Years) and takes effect: it turns Object Lock on for an existing versioned bucket (a bucket made with
  `ObjectLockEnabledForBucket` has it from the start, and now reads back `ObjectLockEnabled: Enabled` before any configuration is put;
  before this it answered "not found"). Once on it stays on, and versioning cannot be suspended (already so).
- `GetObject` / `HeadObject` return `x-amz-object-lock-mode`, `-retain-until-date` and `-legal-hold`.
- IAM (`authz.rs`): a request that asks to bypass also needs `s3:BypassGovernanceRetention` on each object. Not exercised by any test
  (see "Not done").
- `GetBucketPolicyStatus`: `IsPublic` from the stored policy by the rule the stage already used for `BlockPublicPolicy` (an `Allow`
  statement with `Principal` `*` or `{"AWS": "*"}` and no `Condition`; the shared `statement_is_public` in `validate.rs`), and
  `NoSuchBucketPolicy` when there is no policy. Subject to the public access block in the way that rule already is: a public policy can
  only be stored while the bucket's `BlockPublicPolicy` is off.

**Refused by name, not faked:** Object Lock *event holds* (the newer `x-amz-object-lock-event-hold*` headers, `EventHold` in a
retention, `DefaultEventHold` in a configuration) answer 501 `NotImplemented`. The Go edition has no such thing.

**Storage format.** Version rows are now record version 2 (the lock is the last field). Version 1 rows (written by A9 before this)
are still read, as having no lock: `store::decode_version`, with a unit test. The new table needs no migration. Nothing else changed
shape.

## Tests

- `tests/lock.rs` (3) and `tests/lock_more.rs` (4), over a real socket with a manual clock (`tests/support/lab.rs`): the retention /
  bypass / shorten / extend rules, COMPLIANCE not bypassed and expiring with the clock, `DeleteObjects` partial refusal, legal hold on
  and off, default retention and explicit headers (copy does not inherit), refusals on an unlocked bucket, header validation,
  multipart, lifecycle leaving locked noncurrent versions alone and taking one once the hold is off, policy status (private, public,
  conditional, none).
- Unit: `lock::tests` (what a lock forbids; calendar years), `store::tests` (version 1 rows read), `tests.rs` (the five operations
  served; the refusal list no longer holds `GetBucketPolicyStatus`).
- `conformance/tests/test_s3.py`: four new scenarios (33 labels), all UNVERIFIED until recorded: `test_object_lock_configuration_and_the_default_retention`,
  `test_object_lock_retention_blocks_deletes_and_governance_can_be_bypassed`, `test_object_lock_legal_hold`, `test_bucket_policy_status`.
  They use only GOVERNANCE retention of a day or more and switch holds off in cleanup, so a recording leaves nothing undeletable. No
  Object Lock behaviour was in the existing recording.

## Scoreboard

```
UV_BIN=$(ls -d ~/.proto/tools/uv/*/uv | head -1) cargo xtask conformance --cmd "$PWD/target/release-aws/doze-aws --listen {addr} --data-dir {data} --account 111122223333" s3
module           labels  exact  wording  behaviour  unverified
s3                  148     84        0          3          61
```

Before: 115 / 84 / 0 / 3 / 28. No exact label lost; the 33 new labels are all UNVERIFIED (nothing recorded). The whole-suite run after the
change leaves every other module at its figure (SQS 177, KMS 44, SNS 38, IAM 36, DynamoDB 53, ...).

## Wording and behaviour from memory (`(memory)` in the code)

- `AccessDenied` / "Access Denied because object protected by object lock." for a refused delete or retention change (`lock.rs::locked`).
- `InvalidRequest` / "Bucket is missing Object Lock Configuration" (retention, hold or lock headers on a bucket without Object Lock).
- `404 NoSuchObjectLockConfiguration` / "The specified object does not have a ObjectLock configuration" for reading retention, or a legal
  hold, an object never had (the Go edition answers a never-set hold as `OFF`; AWS is remembered to give the error; a hold set `OFF`
  reads `OFF`).
- `InvalidArgument` / "The retain until date must be in the future!"; "x-amz-object-lock-retain-until-date and x-amz-object-lock-mode
  must both be supplied".
- `InvalidBucketState` / "Versioning must be 'Enabled' on the bucket to apply a Object Lock configuration" for turning Object Lock on for
  a bucket whose versioning is not enabled.
- `x-amz-object-lock-retain-until-date` written on a read as ISO 8601 with milliseconds (`2023-11-20T00:00:00.000Z`), as a string, because
  the response writer would render a date-time header from a number as an HTTP date.
- The default retention of a multipart upload is applied when the upload completes, not when it starts.
- Whether `GOVERNANCE` -> `COMPLIANCE` with an unchanged date is allowed (it is here), and the public-policy rule's narrowness against
  AWS's (AWS also lets a condition that does not name the caller leave a policy public, and weighs `Deny`; this is the Go edition's rule).

## Weight

`cargo xtask weight doze-aws` (macOS arm64, `release-aws`): **5,404,960 B, 220 packages, idle 3,632 KB, verdict ok** against 6,800,000 B /
224 / 6,144 KB. Before the change (the integrator's figure): 5,371,872 B, 220, 3,600 KB. Delta **+33,088 B, +0 packages, +32 KB idle**
(the upload-lock table adds a small `Records` in memory per region, empty until an upload names a lock). No third-party package added.

## Not done

- No test, automated or by hand, of the extra `s3:BypassGovernanceRetention` need under `--iam enforce`.
- Objects made before this change have no lock; a bucket that already had a stored `object-lock` configuration (it could be put and
  returned before) gets its default retention for new objects from now on.
