# Correctness: S3 ACLs, SigV2, the anonymous principal, the @connections model, API Gateway logs, IAM Query extensions (branch `aws-correctness`, 2026-11-14)

Six items, one commit each, on `70ce8e3d`. AWS is the judge; what was not seen recorded is marked `(memory)` beside it in the code.
New conformance scenarios are UNVERIFIED until recorded.

## 1. S3 ACLs, the AWS way (`aws-s3/src/acl.rs`, `grants.rs`)

`GetBucketAcl`, `PutBucketAcl`, `GetObjectAcl`, `PutObjectAcl` were refused by name (501), which broke Terraform (`aws_s3_bucket` reads
`GetBucketAcl` on every plan) and the CDK (canned ACLs). The Go edition accepted ACLs and ignored them. Now Object Ownership decides,
as on AWS since April 2023:

| Behaviour | Call |
|---|---|
| A new bucket is `BucketOwnerEnforced` (already returned by `GetBucketOwnershipControls`; now also takes effect). `CreateBucket` takes `x-amz-object-ownership` and stores it | AWS-correct |
| Under `BucketOwnerEnforced`: `Get*Acl` answer the owner with `FULL_CONTROL`; `PutBucketAcl` is refused `AccessControlListNotSupported` (even `private`, as Terraform users see); `PutObjectAcl`, `PutObject`, `CopyObject`, `CreateMultipartUpload` accept only the owner's full control (`bucket-owner-full-control`, `private`, or that grant spelled out) and refuse anything else; `CreateBucket` with an ACL other than `private` is `InvalidBucketAclWithObjectOwnership` | Go weak (bettered: AWS's rules; Go accepted and ignored) |
| Under `BucketOwnerPreferred` / `ObjectWriter`: canned ACLs, `x-amz-grant-*` headers and `AccessControlPolicy` bodies are kept (bucket: with its configurations; object: on its version, record version 3, versions 1 and 2 still read) and returned; a multipart upload keeps its ACL until it completes (`upload_acls`); a copy gets the ACL its own request names | AWS-correct |
| `BlockPublicAcls` refuses a public ACL (403); `IgnorePublicAcls` makes public grants count for nothing | AWS-correct |
| An `AllUsers` grant becomes a resource-policy statement for `Principal: *` (READ on an object → `s3:GetObject`, `s3:GetObjectVersion`; bucket READ → listing; WRITE → put/delete in the bucket; *_ACP → the ACL actions), weighed by IAM for an anonymous caller (item 3). Only computed for an anonymous caller, so a signed request pays nothing | AWS-correct (the action mapping is (memory)) |
| Turning ACLs off (`PutBucketOwnershipControls` `BucketOwnerEnforced`) while the bucket's ACL grants anyone else is `InvalidBucketAclWithObjectOwnership`; object ACLs stay stored and unanswered, and come back if ACLs are enabled again | AWS-correct (memory) |
| **Fixed along the way:** a deleted public access block or deleted ownership controls read as the new-bucket default (all blocked; enforced). Now `DeletePublicAccessBlock` unblocks and `Get…` answers `NoSuchPublicAccessBlockConfiguration`; `DeleteBucketOwnershipControls` enables ACLs (as `ObjectWriter`) and `Get…` answers `OwnershipControlsNotFoundError` | Rust port wrong (fixed) |
| `AccessDenied` from the authorizer is 403 for S3 (it was the authorizer's 400) | Rust port wrong (fixed) |
| CloudFormation `AWS::S3::Bucket.AccessControl` is applied with `PutBucketAcl` after the ownership controls and the public access block (was: stored with a note "doze-aws does not serve ACLs"), so a template that does not enable ACLs fails as on AWS | Go weak (bettered) |

The restXml codec now reads and writes XML attributes (`@xmlAttribute`, the grantee's `xsi:type`, with its `xmlns:xsi` declaration),
which botocore needs to parse a grantee's `Type`. Wording from memory: `AccessControlListNotSupported` "The bucket does not allow ACLs";
`InvalidBucketAclWithObjectOwnership`; `UnresolvableGrantByEmailAddress` (e-mail grantees cannot be resolved locally and are refused);
`MissingSecurityHeader` for a `Put*Acl` with no ACL; `InvalidRequest` for canned + grant headers; `UnexpectedContent` for a body with
headers; the public-ACL `AccessDenied`. A canonical-user grantee other than the owner is accepted as given.

## 2. SigV2 presigned URLs verified (`aws-auth/src/sigv2.rs`, `aws-gateway/src/presign.rs`)

boto3's `generate_presigned_url` for S3 is SigV2; a verifying gateway refused it outright. Now `--auth verify` checks it: the
key (fixed or IAM-issued, through the credential source), the session token (`x-amz-security-token` in the query), `Expires`, and
the base64 HMAC-SHA1 over botocore's string to sign (`VERB\nContent-MD5\nContent-Type\nExpires\n` + sorted `x-amz-*` headers and
query parameters + the bucket-prefixed raw path + the sorted sub-resources). The unit test verifies a URL botocore 1.43 signed.
Errors are S3's (`SignatureDoesNotMatch` with the string to sign, `AccessDenied` "Request has expired", `InvalidAccessKeyId`).
A trusting gateway holds SigV2 URLs to their expiry only, as before. **AWS-correct.**

Under verify the recorded `test_presigned_urls` labels `put` and `what-the-put-wrote` now match (urllib sends a `Content-Type` the URL
did not sign, so AWS and doze both answer 403): **s3 under `--auth verify` is 86 exact (was 84)**, the one remaining difference being
`CompleteMultipartUpload`'s `Location`.

## 3. Anonymous is a real principal for IAM (`aws-iam/src/anonymous.rs`, `aws-s3/src/authz.rs`, `core.rs`)

Before: an unsigned request to a trusting gateway was the account root; a public-read policy was never needed. Now an unsigned
request is `Caller::ANONYMOUS`, and IAM grants it only what a resource policy (or, from item 1, a public ACL) grants to `"Principal":
"*"` (or `{"AWS": "*"}`); no identity policy speaks for it, and IAM's actor is not woken for it.

**The decision, by flag:**

| `--auth` | `--iam` | An unsigned S3 request |
|---|---|---|
| trust | off | served, nothing asked (curl works; as before) |
| trust | **soft (the default)** | judged as anonymous; a refusal is **logged and the request served**: `iam soft mode: would be refused: anonymous, s3:GetObject on arn:aws:s3:::b/k — add a bucket policy granting Principal * (with the bucket's public access block off) or sign the request` |
| trust | enforce | refused unless public: 403 `AccessDenied` "Access Denied" (S3's words for anonymous) |
| verify | soft / enforce | as with trust (was: refused as unauthenticated; AWS serves unsigned requests as anonymous) |
| verify | off | refused as unauthenticated, as before: with nobody to judge it, serving it would undo `--auth verify` |

Soft keeps its meaning (never refuses), so the default keeps curl and browsers working while saying what AWS would do. The access
log records the principal as `anonymous`. `RestrictPublicBuckets` (on for a new bucket) keeps a public policy from letting anonymous
in, as AWS does (only the owner's account and AWS services). **AWS-correct; Go wrong (fixed):** Go served anonymous as the account.

- **S3 website pages** now ask the same question for the page's object (the folder's index document for a folder): under enforce a
  bucket that is not public answers 403, as AWS's website endpoint does. Before, a page was served to anyone. **Rust port weak (fixed).**
- **SNS `ConfirmSubscription` through a `SubscribeURL`** stays allowed: an anonymous `ConfirmSubscription` is not authorized (the token
  is the proof), as on AWS; a signed one is asked like any call. Tested (`a_subscribe_url_confirms_without_asking_who_fetched_it`).
- **Lambda function URLs** (`AuthType: NONE`) are unchanged: still served without asking (`internal: true`). AWS also wants a
  resource policy granting `lambda:InvokeFunctionUrl` to `*`; not done here (open).

Gateway: `Config::anonymous` (set by `doze-aws` to `--iam != off`) lets a verifying gateway serve unsigned S3 requests as anonymous.

## 4. The `@connections` management API from its model (`aws-apigateway/src/management.rs`, `aws-model-apigatewaymanagementapi`)

Fetched `apigatewaymanagementapi` from aws/api-models-aws (commit `46135f84…`, recorded in `models/PROVENANCE.md`), generated with
`cargo xtask codegen`. `Management` is a third API Gateway `Service` (restJson1, signing name `execute-api`) over the same
connections. The gateway routes and verifies it like any REST service; the new `RestService::base_path` lets it route below the stage
address (`/_aws/execute-api/<api>/<stage>` or `<api>.execute-api.<region>.<host>/<stage>`) and hands that base on in the caller's
endpoint, from which the API and stage are read. IAM is asked for `execute-api:ManageConnections` on
`arn:aws:execute-api:<region>:<account>:<api>/<stage>/<METHOD>/@connections/<id>`; a refusal is `ForbiddenException` (memory). An
unsigned call is the anonymous principal (item 3), so it still works in the default modes. Behaviour kept: post, read
(`connectedAt`, `identity`, `lastActiveAt`), close, 410 `GoneException` once gone, 429 when the client's queue is full.
**AWS-correct; the Rust port was weak (signatures unchecked).** Under `--auth verify --iam off` an unsigned management call is now
refused (it was served).

## 5. API Gateway execution and access logs (`aws-apigateway/src/plane/execlog.rs`, `access.rs`)

Access logs were already written; they gain `$context.error.messageString` and `$context.integrationStatus` /
`integration.status` / `integration.integrationStatus`. **New: REST execution logs.** When the method's settings (its own
`<path>/<METHOD>`, else the stage's `*/*`) set `loggingLevel`:

- `INFO`: every request's narrative to `API-Gateway-Execution-Logs_<apiId>/<stage>` (group made on first use, as API Gateway does),
  each line `(<requestId>) …`: Extended Request Id, the usage-plan lines, Starting execution, `HTTP Method: GET, Resource Path: /ping`,
  `Method request path: {id=1}`, `Method request query string: {x=1}`, the endpoint URI and Sending request (for an integration with a
  URI), `Received response. Status: …, Integration latency: … ms`, then `Successfully completed execution` or `Execution failed due to
  configuration error: <message>`, and `Method completed with status: …`.
- `ERROR`: only requests that ended in 4xx/5xx, only the failure and completion lines.
- `dataTraceEnabled`: adds the method request headers and body and the method response body and headers (bodies cut at 10,000
  characters).

Wording, order and the `{k=v, ...}` map form are (memory). **Go weak (bettered):** Go wrote JSON maps and the whole narrative at
`ERROR`. HTTP APIs: access logs only, as AWS. Stream name: `<api>-<stage>` (AWS names its streams by an opaque hash, memory).

## 6. IAM `Doze*` extensions as Query actions (`aws-wire/src/service.rs`, `aws-gateway/src/query.rs`, `aws-iam/src/lib.rs`)

It was a small, clean hook, so it is built: `Service::query_extension(action, form)` (default none), asked by the gateway's Query
path only for an `Action` the model does not have. IAM answers `DozeAccessLog` and `DozeGeneratePolicy` with the same JSON as
`/_doze/iam/<name>`, signed like any IAM call. **Go compatibility (restored).**

## Tests

- `aws-s3`: `tests/acl.rs` (3: enforced, enabled + block + ownership round trip, request checks), `tests/anonymous.rs` (4, with IAM as
  the authorizer: enforce policy / public-access-block / restrict; public-read ACL and `IgnorePublicAcls`; website; soft), `tests/signed.rs`
  `a_sigv2_presigned_url_is_checked_against_the_secret`, unit tests in `grants.rs`, `store.rs` (row versions 1, 2, 3).
- `aws-wire`: XML attributes read (`xmlin`). `aws-auth`: `sigv2` (botocore's string to sign, a botocore-signed URL, expiry, mismatch,
  unknown key). `aws-iam`: `anonymous` (2), `the_doze_actions_are_also_query_actions_as_in_the_go_edition`. `aws-sns`: the SubscribeURL
  test. `aws-apigateway`: `tests/logs.rs` (2, with CloudWatch Logs on the bus), `management` unit test, the WebSocket test now through
  the model's service.
- `conformance/tests/test_s3.py`: `test_acls_are_disabled_on_a_new_bucket`, `test_acls_once_object_ownership_enables_them`,
  `test_anonymous_reads_need_a_public_grant` (UNVERIFIED). `test_refused_by_name` no longer calls the ACL operations (never recorded).
  Note: `anonymous-denied` answers 200 under the runner's default `--iam soft` (by design, see item 3); it matches AWS under enforce.

## Open

- Lambda function URLs with `AuthType: NONE` do not require the `lambda:InvokeFunctionUrl` grant to `*` that AWS requires.
- `AuthenticatedUsers` grants are kept and returned but give no access to anonymous (correct) and are not weighed for signed callers
  (all local callers are the one account).
- The ACL-to-action mapping, the execution-log wording, and the `@connections` refusal wording are from memory; a recording of each
  would settle them (the recorder needs ownership-controls and public-access-block permissions, and a WebSocket API for the last).
- CloudFormation `AccessControl` on update is not re-applied.
