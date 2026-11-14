# verify-b: sns, s3, logs, cloudwatch, kinesis held to the 2026-11-15 recording

Branch `aws-vb` from `aws` (6ee5c7ca). AWS is the judge: a scenario that guessed wrong about AWS was fixed and re-recorded alone;
where doze-aws answered differently from a recording, doze-aws was fixed.

## Scores (labels / exact / wording / behaviour / unverified)

| module | before | after |
|---|---|---|
| sns | 104 / 62 / 7 / 6 / 29 | 106 / 106 / 0 / 0 / 0 |
| s3 | 171 / 154 / 3 / 12 / 2 | 172 / 168 / 0 / 4 / 0 (the 4 are written deviations, below) |
| logs | 51 / 50 / 0 / 1 / 0 | 51 / 51 / 0 / 0 / 0 |
| cloudwatch | 36 / 28 / 0 / 0 / 8 | 36 / 36 / 0 / 0 / 0 |
| kinesis | 58 / 43 / 6 / 2 / 7 | 58 / 58 / 0 / 0 / 0 |

## Re-recorded (single scenarios, each cleans up after itself)

| scenario | why it failed | scenario change |
|---|---|---|
| sns `test_topic_permissions_attributes_and_tags` | AWS refuses a delivery policy naming only `numRetries` | the partial policy is an `outcome`; a full one is then set |
| sns `test_confirmation_is_awaited_by_https_and_not_by_a_queue` | AWS refuses to unsubscribe a pending subscription; topic counts lag | `unsubscribe-pending` is an `outcome`; `counts` waits for `SubscriptionsPending` 2 and drops `SubscriptionsDeleted` (AWS counted 2 deleted though none was: weather from example.com) |
| sns `test_batch_reports_each_message` | a bad attribute type refuses the whole batch | `mixed` is an `outcome`; an `all-fine` batch is added |
| sns `test_refusals` | `bad-topic-name` recorded the recorder policy's refusal, not SNS's | the bad name is under the run prefix, so it reaches SNS's own check |
| s3 `test_bucket_configuration` | a lifecycle just put reads as absent for a moment | `lifecycle` waits (`eventually`) |
| s3 `test_anonymous_reads_need_a_public_grant` | a public policy takes seconds to reach readers (not account Block Public Access: after the wait AWS served the read) | waits up to 20 s, then holds the outcome |
| s3 `test_multipart_parts_are_replaced_listed_and_aborted` | (no failure) | `bad-order-real-tags` added: with wrong ETags AWS says InvalidPart before it looks at order, so order needed real tags |
| s3 `test_acls_once_object_ownership_enables_them` | (no failure) | re-recorded after the normalizer fix below |
| kinesis `test_consumers_policy_and_tags_by_arn` | RegisterStreamConsumer said "not found" right after ACTIVE; GetResourcePolicy without a policy is not a refusal | register waits (`eventually`); `no-policy` is an `outcome` |
| kinesis `test_account_settings`, cloudwatch `test_composite_alarms_dashboards_and_history` | blocked by the recorder policy | recorded once the owner's paste landed |

## Harness

- `AWS_HOST` no longer treats `http://acs.amazonaws.com` (S3's ACL group namespace) as an endpoint; the three S3 grantee URIs it
  had rewritten to `<endpoint>/groups/...` are recorded as AWS sent them.
- `<caller>`: a refusal's "User: <arn> is not authorized" names whoever ran the scenarios (the recorder's SSO session on AWS, the
  local keys' root on doze-aws). Both sides now write `User: <caller> `. Only that phrase is rewritten: the same ARN as a policy
  principal is data. `test_s3.json` and `test_sns.json` were rewritten to match; the other modules' snapshots
  (cloudformation, eventbridge, lambda, ssm) still hold the literal session ARN and gain from the same one-line rewrite or a
  re-record (integrator).

## What AWS taught us, and the call on each

### logs
- `%a% %b% %c%` is `InvalidParameterException: Invalid character(s) in term ' '`: the whitespace after a `%...%` is a term of its
  own, refused before any count of expressions. Go wrong (fixed). Two expressions in an unstructured pattern are therefore only
  possible back to back (`%a%%b%`); JSON and space-delimited patterns hold two as before.

### kinesis
- Reshard refusals are all recorded now (they were memory): a shard merged with itself is "not an adjacent pair" (same words as a
  gap, no full stop), a missing shard in MergeShards is `Could not find shard ...`, a closed shard "has already been merged or
  split, and thus is not eligible for merging or splitting", SplitShard's range words say "less than", not "less than or equal
  to". Go wrong (fixed).
- UpdateShardCount past double is `InvalidArgumentException: UpdateShardCount cannot scale up over double your current open shard
  count. Current open shard count: 2 Target shard count: 5`, not LimitExceeded. Go wrong (fixed). The halving words follow the
  same form (memory).
- The two parents of a merge end at different `EndingSequenceNumber`s; doze-aws closed both at the same instant's number. Go wrong
  (fixed: the number carries the shard id, as a child's first number already did).
- DeleteStream with consumers: AWS's words, typographic apostrophe included. Go wrong (fixed).
- GetResourcePolicy on a stream with none answers `{"Policy": "{}"}`. The Go edition's G7 (a not-found refusal) was wrong (fixed).

### cloudwatch
- `Dashboard <name> does not exist` (no "named", no full stop). Go wrong (fixed).

### sns
- Set* attribute refusals carry no `Attributes Reason:` (that is Create/Subscribe's form): `Invalid parameter: FilterPolicy: ...`,
  `Invalid parameter: MaximumMessageSize: 100 is not an integer between 1024 and 1048576 bytes`, `Invalid parameter: DeliveryPolicy:
  ...`. Go wrong (fixed, `req::as_set`).
- Filter policy refusals found while reading carry Jackson's location over the compacted policy: `\n at [Source: (String)"...";
  line: 1, column: N]`, the column being where Jackson's reader stands after the offending token (past a number, past only the
  opening quote of a string, past a bracket). Go weak (bettered: `filter_at.rs` reproduces it; the 500-character snippet cut is
  memory). A policy that is not an object is `Filter is not an object`.
- Clearing a filter policy removes its scope. Go wrong (fixed).
- ContentBasedDeduplication set on a standard topic is `Invalid parameter: AttributeName`. Go wrong (fixed).
- A delivery policy's retry policy must name `minDelayTarget` (recorded) and `maxDelayTarget` (memory); a kept `DeliveryPolicy`
  gains `"disableSubscriptionOverrides": false`. Go wrong (fixed).
- An HTTP(S) subscription reports `EffectiveDeliveryPolicy` in AWS's own shape and key order. Go wrong (fixed; a subscription's
  own delivery policy laid over it is memory).
- RemovePermission of a label that is not there succeeds. Go wrong (fixed).
- Unsubscribe of a pending subscription is refused (`Cannot unsubscribe a subscription that is pending confirmation`). Go wrong
  (fixed).
- Message attribute refusals are `ParameterValueInvalid` without `Invalid parameter:`; eleven attributes on a publish are taken
  (the ten-attribute limit was SQS's, and still applies where SNS hands a message to a queue). Go wrong (fixed).
- PublishBatch: an attribute AWS will not take refuses the whole batch; an Id with a bad character is named in the refusal. Go
  wrong (fixed).
- `aws:` tags: `Caller is an end user and not allowed to mutate system tags`; MessageDeduplicationId on a standard topic gains
  `MessageDeduplicationId Reason:`. Go wrong (fixed).

### s3
- PutBucketAcl and PutObjectAcl ask Block Public Access before Object Ownership: a public ACL on a new bucket is AccessDenied, 403,
  "User: ... is not authorized to perform: s3:PutBucketAcl on resource: ... because public ACLs are prevented by the
  BlockPublicAcls setting in S3 Block Public Access." PutObject asks ownership first. Go wrong (fixed). The caller is named as the
  account root (the local keys' principal); an IAM user's own ARN is not passed down to S3 (memory).
- CompleteMultipartUpload looks for each part (and its ETag) before it looks at the order; the order refusal says "Parts must be
  ordered by part number." Go wrong (fixed).
- Content-MD5 mismatch: `did not match what we received.` Go wrong (fixed).
- Lifecycle: no action is `InvalidRequest: At least one action needs to be specified in a rule`; `Days: 0` is `InvalidArgument:
  'Days' for Expiration action must be a positive integer`; a bad enum inside an XML body (`Status: Maybe`) is MalformedXML (a
  header's stays InvalidArgument). Go wrong (fixed).
- RestoreObject on an object outside an archive class: `InvalidObjectState`, 403, `Restore is not allowed for the object's current
  storage class`. Now answered; restoring from GLACIER / DEEP_ARCHIVE is still refused by name (501, `X-Doze-Not-Built`).
  Go weak (bettered).
- Lock headers on a write to an unlocked bucket: `Bucket is missing ObjectLockConfiguration`; retention calls keep `Bucket is
  missing Object Lock Configuration`. Both recorded.

## Decided deviations (`conformance/deviations.py`), for the owner to see

1. `test_multipart_upload::complete`: `Location` is doze-aws's own URL (`<endpoint>/<bucket>/<key>`); AWS's
   `https://<bucket>.s3.amazonaws.com/<key>` cannot exist locally.
2. `test_presigned_urls::put` and `::what-the-put-wrote`: the default gateway trusts signatures (`--auth trust`), so a SigV2
   presigned PUT whose signature does not cover urllib's Content-Type is served. Under `--auth verify` it is refused as AWS
   refuses it (checked: the labels match AWS and the listing reports "no longer differs").
3. `test_anonymous_reads_need_a_public_grant::anonymous-denied`: `--iam soft` (the default) logs the refusal and serves; under
   `--iam enforce` it matches AWS (checked the same way).

xtask counts these as behaviour 4 for s3 because it reads only the "answered differently" line; pytest passes them as listed.

## Open
- Nothing left behind in the account by the re-recordings (every scenario ran its cleanup and passed).
- No new recorder permissions are needed from this group.
