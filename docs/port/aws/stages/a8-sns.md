# A8: SNS

Branch `aws-a8-sns`, crate `crates/aws-sns`. Written 2026-11-12. The integrator folds the PLAN, STATUS, scoreboard and weight lines in
from here; nothing in those four files was edited.

## What was built

`aws-sns` serves SNS over the Query protocol (the codec of A7). Same shape as SSM and Secrets Manager: a sans-IO `Core` behind a
`doze_runtime::Keyed<Core>` (one lazy actor per region, home region `us-east-1` keeps the files in the service directory), state in
`doze_store::Records<T>` in one redb file (`sns.redb`, five tables: topics, subscriptions, applications, endpoints, SMS settings), and
the answer held until the commit is flushed. Nothing is opened until the first request. `authz.rs` describes each request as `Need`s.
Fan-out to SQS goes through `aws_peers::Peers` as `SendMessage`, acting as `sns`.

**37 of 43 operations built.**

| Area | Operations |
|---|---|
| Topics | CreateTopic (idempotent as AWS is), DeleteTopic, ListTopics, GetTopicAttributes, SetTopicAttributes, AddPermission, RemovePermission, TagResource, UntagResource, ListTagsForResource, PutDataProtectionPolicy, GetDataProtectionPolicy |
| Subscriptions | Subscribe, ConfirmSubscription, Unsubscribe, GetSubscriptionAttributes, SetSubscriptionAttributes, ListSubscriptions, ListSubscriptionsByTopic |
| Publishing | Publish (topic, `TargetArn` topic, device endpoint, phone number), PublishBatch |
| Mobile push (D1: stored, nothing sent) | CreatePlatformApplication, DeletePlatformApplication, ListPlatformApplications, Get/SetPlatformApplicationAttributes, CreatePlatformEndpoint, DeleteEndpoint, GetEndpointAttributes, SetEndpointAttributes, ListEndpointsByPlatformApplication |
| SMS (D1: stored, nothing sent) | SetSMSAttributes, GetSMSAttributes, CheckIfPhoneNumberIsOptedOut (always false), ListPhoneNumbersOptedOut (always empty), OptInPhoneNumber |

**Refused by name (501 `NotImplemented`, `X-Doze-Not-Built`, before the request is read), 6:** `CreateSMSSandboxPhoneNumber`,
`DeleteSMSSandboxPhoneNumber`, `GetSMSSandboxAccountStatus`, `ListSMSSandboxPhoneNumbers`, `VerifySMSSandboxPhoneNumber`,
`ListOriginationNumbers`. SCOPE.md puts the SMS sandbox and origination numbers in STAY CUT. Decision D1 (answered 2026-11-09: accept SMS
publish and mobile push as stored, nothing sent) is built, so nothing needed to be refused for it. The deviations list stays empty.

What the operations do:

- **Topics.** Standard and FIFO (`.fifo` name and `FifoTopic=true` go together; either alone is `Invalid parameter: Topic Name`).
  `CreateTopic` again with the same definition returns the topic; different attributes or tags are refused. The default policy and the
  default delivery policy are those recorded. Attributes are checked by name and value; `MaximumMessageSize` (1,024 to 1,048,576, default
  262,144, above 256 KiB needs at most 100 subscriptions of SQS, Firehose or Lambda) is enforced at publish (audit decision G25);
  `TracingConfig`, `KmsMasterKeyId`, `FifoThroughputScope`, `ArchivePolicy`, the delivery-status feedback attributes are stored, not applied.
  `AddPermission`/`RemovePermission` edit the policy. Tags: 50 per topic, `aws:` reserved.
- **Subscriptions.** The nine protocols. `sqs`, `lambda`, `firehose` are confirmed at once when the endpoint is in the topic's account,
  pending otherwise; `http`, `https`, `email`, `email-json` are pending with a token valid two days; `sms` and `application` are
  confirmed at once. `Subscribe` answers `pending confirmation` unless `ReturnSubscriptionArn`. A FIFO queue on a standard topic is refused
  (recorded) and the reverse too. Same topic, protocol and endpoint again is the same subscription. Attributes: `RawMessageDelivery`,
  `FilterPolicy`, `FilterPolicyScope`, `DeliveryPolicy`, `RedrivePolicy`, `SubscriptionRoleArn` (required for Firehose), `ReplayPolicy`
  (FIFO topics). A filter policy is kept compact and gets `FilterPolicyScope=MessageAttributes` when none is given (both recorded).
- **Filter policies** (`filter.rs`, `filter_match.rs`; the parser has a fuzz target `fuzz_filter_policy`): attribute scope and
  `FilterPolicyScope: MessageBody`; exact strings, numbers, booleans and `null`; `prefix`, `suffix`, `equals-ignore-case`,
  `anything-but` (value, list, prefix, suffix), `numeric` (`=`, `>`, `>=`, `<`, `<=`, ranges), `exists`, `cidr` (IPv4 and IPv6), `$or`;
  `String.Array` attributes and arrays in a body; nested objects in a body. The body scope reads the body as JSON (SCOPE.md row 16: the Go
  edition ignored the scope and dropped body-style messages; here the scope is read and a test says so). Limits: 150 combinations, five
  attribute names on the attribute scope, 200 filtered subscriptions to a topic, 10,000 to an account.
- **Publish.** Message, subject, `MessageStructure=json` (the member for the protocol, else `default`), up to 10 attributes of
  `String`, `String.Array`, `Number`, `Binary` (and custom suffixes), size against the topic's limit. FIFO: group required (recorded),
  deduplication id or content-based hash, five-minute window (persisted), `SequenceNumber`; a duplicate is accepted and not delivered
  and answers with the original ids. `PublishBatch`: the three recorded batch refusals, bad ids, total size, and per-entry `Failed`.
- **Delivery.** SQS only: the envelope (fields and order recorded) or, for a raw subscription, the message with its attributes as SQS
  attributes; FIFO group and deduplication ids pass through. **The queue's policy is asked** (`aws:SourceArn`, `aws:SourceAccount`,
  principal `sns.amazonaws.com`) with the same `--iam` mode as everything else: off asks nothing, soft logs a delivery AWS would refuse and
  delivers, enforce drops it. A queue with no policy takes nothing under enforce, as on AWS.
- **Authorization.** Actions are spelled `SNS:Publish` (the recorded refusal does), `PublishBatch` is `SNS:Publish`, the topic's ARN is the
  resource and its policy (default or set) comes with it, with `AWS:SourceOwner` set to the caller's account so the default policy works.
  A refusal is 403 `AuthorizationError`. Checked end to end against the binary under `--iam enforce` with boto3 (below).

**Stored and not delivered, plainly:** `http`/`https`/`email`/`email-json` subscriptions (kept, pending forever: no confirmation message
is sent, so no one learns the token), `lambda` (waits for A13), `firehose`, `sms`, `application`. Every such skip is logged at debug.
`RedrivePolicy` is stored; an undeliverable message is dropped with a warning, not sent to the dead-letter queue.

## The scoreboard

`UV_BIN=~/.proto/tools/uv/0.13.7/uv cargo xtask conformance --cmd "$PWD/target/debug/doze-aws --listen {addr} --data-dir {data} --account 111122223333" sns`

```
module           labels  exact  wording  behaviour  unverified
sns                 104     38        0          1          65
total: 104 labels, 36% exact, 36% ignoring wording
```

- **Recorded: 38 of 39 labels exact (97%).** Go on the same snapshots: 64% exact / 74% ignoring wording (39 labels).
- **The one that differs, `test_refusals::bad-topic-name`:** AWS answered `AuthorizationError` (403) because the recorder's policy allows
  only names under its prefix and the scenario creates `not a topic name!`. doze-aws answers `Invalid parameter: Topic Name`. It cannot
  be matched; it needs a scenario that stays inside the prefix and a re-recording (owner item, like the four SSM labels).
  The wording of the denial itself (`SNS:CreateTopic`, upper-case, `because no identity-based policy allows the SNS:CreateTopic action`) is
  now reproduced under `--iam enforce` and is a recorded fact.
- **65 labels are UNVERIFIED** (scenarios written ahead of the recording, 10 tests added to `test_sns.py`; they pass against
  doze-aws through boto3, which proves the Query transport for every operation they touch, and compare with nothing).
  They cover tags, permissions, attributes, confirmation, subscription attributes, filter operators on attributes and bodies, publish
  checks, JSON structure, batches and FIFO deduplication. The recorder's policy allows `sns:*` on its prefix, so they can be recorded as
  they are.
- **Four more tests in `conformance/tests/test_sns_local.py`** hold mobile push and SMS through boto3 against doze-aws only. They are
  skipped against AWS (a push credential must be real, a text costs money) and are in no score.

## What the recording taught

- A filter policy comes back **compact**: `{"kind": ["order"]}` was `{"kind":["order"]}`. The first run was 36 of 39 for this and for
  the principal below.
- `SubscriptionPrincipal` is the **caller's own ARN**; the recorded one is the recorder's SSO session, which no one else can have.
  **Harness change, owner to approve:** `SubscriptionPrincipal` joined `ALWAYS_OPAQUE` in `conformance/harness/normalize.py` (the KMS
  key-material ids are there on the same grounds) and the two recorded values were rewritten offline to `<SubscriptionPrincipal:1>`
  (checked: exactly two occurrences, one per test). doze-aws answers `arn:aws:iam::<account>:root`.
- The AWS wording that the recording fixed (all now tests): `Topic does not exist` (404 `NotFound`), `An ARN must have at least 6
  elements, not 1`, `Amazon SNS does not support this protocol string`, `Empty message`, `The MessageGroupId parameter is required for FIFO
  topics`, `FIFO SQS Queues can not be subscribed to standard SNS topics` (with its doubled `Invalid parameter:`), the create-again
  messages for attributes and tags, `Invalid parameter: Topic Name` for a `.fifo` mismatch, the three batch refusals, deleting a topic
  twice answers 200, `Invalid parameter: AttributeName` for an unknown topic attribute, and the Jackson sentence for a filter policy that
  is not JSON (reproduced for the one mistake the scenario makes, a key without quotes; other JSON mistakes carry serde's words).
- The default policy has `AWS:SourceOwner` as its condition; for it to mean anything the evaluator is given `AWS:SourceOwner` = the
  caller's account. Under `--iam enforce` an IAM user with no policy can therefore publish to a topic with the default policy and cannot tag
  it, create a topic or list topics. **That is the reading of the default policy; no recording shows it (memory).**

## (memory) items: wording or behaviour not checked against a recording

All of these are marked `(memory)` in the code where they sit, or are listed here as documentation-only:

- Every refusal not listed above: attribute validation, subject, message-attribute rules (`invalid message attribute type`, `non-empty
  message attribute value`, `Could not cast ... to number`, more than 10), `Message too long`, `MessageStructure` errors, FIFO deduplication id
  required / not valid on a standard topic, token refusal (`Invalid parameter: Token`), `Subscription does not exist`, `Resource does not
  exist`, the permission refusals (`Statement already exists`, `Statement does not exist`, `Policy statement action out of service scope!`,
  `Account ID`), tag refusals, `TopicLimitExceeded`, `FilterPolicyLimitExceeded`, the platform and SMS refusals.
- Whether unsubscribing an unknown subscription is `NotFound` (built) or 200.
- Whether a duplicate FIFO publish answers with the original `MessageId` and `SequenceNumber` (built) or new ones; the `SequenceNumber`
  shape (20 digits, `1` then a counter times 1,000).
- The matching rules where the documentation is silent: numbers match only numeric conditions and `"100"` does not match `100`; booleans and
  `null` match JSON values only; an array of objects in a body matches if any object does; `anything-but` over an array matches if any
  element qualifies; a body that is not a JSON object matches nothing.
- Confirmation: the 64-hex token, two-day life (documented), a queue of another account pending, `ConfirmationWasAuthenticated`.
- `GetTopicAttributes` of a FIFO topic: `FifoTopic` and `ContentBasedDeduplication` (default `false`); the merge of a topic's
  `DeliveryPolicy` over the default; documents other than `FilterPolicy` kept as given rather than compacted.
- Mobile push: the platform list, the attribute names, `PlatformCredential` required, the credential never returned, the endpoint-token
  refusal sentence, `EndpointDisabled` on a publish, `PlatformApplication does not exist`.
- The five-name limit of a filter policy on attributes (documented) and the 200 and 10,000 filter-policy subscription limits.
- The envelope's `Signature` is a digest of the text AWS would sign, not a signature; `SigningCertURL` names a certificate the endpoint does
  not serve; `UnsubscribeURL` is the local endpoint's (`/?Action=Unsubscribe&SubscriptionArn=...`), which the gateway does not serve as a
  GET. SCOPE.md cuts message signing; the fields are kept.

## Open decisions (for the owner)

1. **HTTP/S delivery and confirmation.** Today an `http`/`https` subscription is stored pending forever. A webhook on localhost is the commonest
   local SNS target after SQS. Building it needs an HTTP client (a hand-written HTTP/1.1 POST for `http://` is small; `https://` needs TLS,
   which is a size decision) and sends the `SubscriptionConfirmation` so a token is learned. Nothing was decided; not built.
2. **Dead-letter delivery for `RedrivePolicy`.** Stored only. Sending an undeliverable message to the dead-letter queue is small (the SQS
   call exists) but its attributes (`ERROR_CODE`, `ERROR_MESSAGE`) are unrecorded.
3. **The optional recorded outbox of D1** (SMS and push messages kept so a test can read them) is not built; there is no API to read it with.
4. **The harness change** (`SubscriptionPrincipal` opaque, snapshot rewritten) above.
5. **Re-record `bad-topic-name`** with a name inside the recorder's prefix, then record the 65 new labels (`CONFORMANCE_TARGET=aws
   CONFORMANCE_RECORD=1`). Read each diff; where AWS contradicts the build, AWS wins.
6. Lambda and Firehose delivery follow A13 and a Firehose decision.

## Tests

79 in the crate: the filter parser and matcher (operators, scopes, limits, the Jackson sentence), topic and subscription rules, publishing
(checks, FIFO windows with a manual clock, batches), delivery to a real SQS beside it on the bus (envelope, raw, attributes, JSON
structure, FIFO group, duplicate not delivered, absent queue, queue policy under off, soft and enforce), push and SMS, authorization
(what each request asks, the refusal's shape), and persistence: a restart after every call of a 22-call scenario compares everything the
service holds before and after, and a count of rows per call (a read writes none; the same subscription again writes none). Clippy is
clean at `-D warnings`. Checked by hand against the binary with boto3: two regions kept apart (same topic name in both; a topic of one
region is `NotFound` in the other; fan-out inside a region), state surviving an idle reap (`--idle-secs 1`), and `--iam enforce` with an IAM
user (above).

## Weight (ready to paste)

2026-11-12, with SNS (A8): `doze-aws` is 5,550,528 B (release, darwin/arm64) and 201 packages in `cargo tree`, 3,440 KB idle. That is +436 KB
over the 5,114,720 B of regions kept apart. The new packages are `aws-sns` and `aws-model-sns` only (`sha2`, `tracing` and
`aws-iam-policy` were already linked; the tree without the two counts 199, one more than the 198 the log gave for the base). Idle memory is
unchanged: the SNS actor, its tables and its validation patterns are made on the first request in a region. Limits unchanged (7,000,000 B,
203 packages); two packages of room.

## Outside the crate

`Cargo.lock` (the new package), `crates/doze-aws/Cargo.toml` and `src/run.rs` (registered on the gateway and the bus),
`xtask/weight-limits.txt` (a comment only), `conformance/harness/normalize.py` (one name), `conformance/snapshots/test_sns.json` (the two
`SubscriptionPrincipal` values), `conformance/tests/test_sns.py` (10 tests added; the 8 recorded ones untouched) and
`conformance/tests/test_sns_local.py` (new).
