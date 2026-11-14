# Fix: S3, SNS, EventBridge and Lambda gaps against the Go edition (branch `aws-fix-s3-sns-eb-lambda`, 2026-11-13)

Thirteen behaviour gaps an audit found against the Go edition (`doze-aws/docs/SUPPORT.md`), each checked in the Rust code first and
closed with tests. AWS is the judge: what was not checked against a recording or AWS's documentation is marked `(memory)` in the code.
No deviation is added.

## What changed

| # | Gap | Done | Where | Evidence |
|---|---|---|---|---|
| 1 | S3 CORS was stored only | yes | `aws-s3/src/cors.rs`, `web.rs`; `aws-gateway/src/cors.rs`, `web.rs` | `tests/web.rs` `a_browser_upload_is_preflighted_…`, `cors.rs` unit tests |
| 2 | S3 website hosting | yes | `aws-s3/src/website.rs`, `web.rs` | `tests/web.rs` `a_website_serves_its_index_and_error_documents` |
| 3 | Public-policy test narrower than AWS | yes | `aws-s3/src/public.rs` (used by `validate.rs`) | `public.rs` unit tests (the user guide's three examples and more) |
| 4 | Lifecycle "run now" | was there; HTTP hook added | `S3::sweep_now`; `POST /_aws/s3/lifecycle/<region>` | `tests/lifecycle.rs`, `tests/web.rs` `the_lifecycle_hook_…` |
| 5 | SNS `http` subscriptions | yes; `https` kept pending | `aws-sns/src/webhook.rs`, `confirm.rs`; new crate `aws-webhook` | `aws-sns/tests/http.rs` |
| 6 | SNS `wildcard` operator | yes (SNS documents it) | `aws-sns/src/wildcard.rs`, `filter.rs`, `filter_match.rs` | unit tests |
| 7 | API destination calls held the region | yes | `aws-eventbridge/src/deliver_http.rs`, `retry.rs` | `tests/http.rs` `a_slow_endpoint_does_not_hold_the_region` |
| 8 | Kinesis mapping across a reshard | yes | `aws-lambda/src/poll/kinesis.rs` (`Lineage`) | `tests/reshard.rs` (fails with the old start rule), unit test |
| 9 | `_local_` layer naming a zip | yes | `aws-lambda/src/code.rs` | `tests/functions.rs` `a_layer_named_in_place_by_its_zip_…` |
| 10 | Go's `/_aws/lambda-url/<id>/` | yes | `aws-lambda/src/web.rs` | `tests/behaviour.rs`, `web.rs` unit test |
| 11 | Interpreter checks at create | yes (warning) | `aws-lambda/src/rt/check.rs` | unit test; `tests/functions.rs` `a_function_this_host_cannot_launch_…` |
| 12 | Lambda log lines live | yes | `aws-lambda/src/livelog.rs` | `tests/livelog.rs` |
| 13 | Stale headers | yes | `aws-s3/src/notify.rs`, `config.rs`, `lib.rs`; SNS and EventBridge headers | — |

### 1, 2: S3 for a browser

- **Routing (additive).** `aws_wire::service::Web` gained `owns_preflight` (default `false`): the gateway hands an `OPTIONS` that no
  service owns by address to the service that answers preflights (S3), after every `owns` has said no, so Lambda URLs and API
  Gateway keep their own `OPTIONS`. `aws_wire::rest::RestService` gained `cors` (default: no headers): the gateway asks it for a REST
  request that carries `Origin`, success or error, and adds what it returns (`aws-gateway/src/cors.rs`, three lines in `rest.rs`).
- **Evaluation** (S3 User Guide, "How does Amazon S3 evaluate the CORS configuration"): rules in order; `AllowedOrigin` and
  `AllowedHeader` with one `*`; header names without case; the first rule that admits origin, method and every requested header
  answers. A preflight no rule admits is 403 `AccessForbidden` with `Method` and `ResourceType`; no configuration is 403 "CORS is not
  enabled for this bucket"; no `Origin` is 400. The answer's header set and the refusals' words are (memory).
- **Website, the address chosen.** Go served the index and error documents on the REST path itself (a `GetObject` miss on a website
  bucket answered with the error document). AWS does not: its REST endpoint answers `NoSuchKey` and the website endpoint serves pages.
  So pages are served at AWS's website hosts (`<bucket>.s3-website-<region>.amazonaws.com`, `<bucket>.s3-website.<region>…`, and
  locally `<bucket>.s3-website.localhost:4566`) and at the path form `/_aws/s3-website/<bucket>/<key>` for a client that cannot set
  `Host` (the `/_aws/lambda-url/` convention); the REST API is unchanged (a test holds that). Rules from the user guide: a folder's
  index document; `photos` → 302 to `photos/` when `photos/<suffix>` exists; an object's website redirect location → 301;
  `RedirectAllRequestsTo` → 301; otherwise 404 with the error document, or AWS's HTML error page (memory). GET and HEAD only.
  Routing rules stay cut (SCOPE). A page is served to anyone, as an unsigned request to a trusting gateway is.
- **Region.** A preflight or a page is not signed for a region: the host's region is tried first, then the home region, then every
  region with S3 state on disk.

### 3: public policy

`public.rs` follows the user guide's rule: assumed public, not public only when every allowing statement grants to fixed values (no
`*`, `?` or `${…}`) of a principal or of one of the documented keys (`aws:SourceIp` no broader than /8 or /32 for IPv6, `SourceArn`,
`SourceVpc`, `SourceVpce`, `SourceOwner`, `SourceAccount`, `aws:userid` outside `AROLEID:*`, `s3:DataAccessPointArn` with a fixed
account, `s3:DataAccessPointAccount`, and the `aws:Principal*` keys). Taken from Go's engine and marked (memory): a negated or
`…IfExists` operator does not narrow, and an unconditional `Deny` to everyone that covers the grant cancels it. `GetBucketPolicyStatus`
and the `BlockPublicPolicy` refusal both use it.

### 5, 6: SNS

- `http`: Subscribe sends the `SubscriptionConfirmation` (headers `x-amz-sns-message-type`, `-message-id`, `-topic-arn`; the documented
  body with `Token` and `SubscribeURL`); fetching the `SubscribeURL` confirms, unsigned. The URL is AWS's query on a doze path
  (`<endpoint>/_aws/sns/confirm?Action=ConfirmSubscription&TopicArn=…&Token=…`), because an unsigned request to `/` is S3's; clients
  fetch the URL as given. Then every notification is POSTed (the SQS envelope, or the raw message with `x-amz-sns-rawdelivery`).
- Off the actor: one task per POST, retried on no answer, 429 or 5xx (memory) under the subscription's `DeliveryPolicy` or the topic's
  effective one (default three retries 20 s apart; the backoff phase is linear, AWS's other functions are approximated (memory)), then
  dropped with a log line.
- `https`: stored, kept pending, and a warning says doze-aws has no TLS client (the owner's open decision). **(Superseded 2026-11-14: outbound https is built; see `stages/tls.md`.)**
- `wildcard` and `anything-but: {wildcard}` are in the SNS Developer Guide ("String value matching"), so they are accepted; the matcher
  is EventBridge's, copied (40 lines) rather than depending on EventBridge.
- The shared client: EventBridge's `http.rs` moved, unchanged, to a new first-party crate `aws-webhook`, which both use.

### 7: EventBridge

The actor builds the request and hands it to a task; `PutEvents` answers at once. The task applies the target's retry policy with the
actor's own rule (`retry::next`, factored out) and sends what runs out to the dead-letter queue (`retry::dead_letter`, now a free
function). Each attempt is recorded in `deliver_http::Calls` (last 100, and a count under way, capped at 10,000).

### 8: Kinesis

`Lineage` keeps which shards are finished and which were followed. A child is read only once every listed parent is read to its end
(a parent trimmed out of the list does not hold it); a child of a parent the mapping followed starts at `TRIM_HORIZON`, one of a parent
already closed when first read starts at the mapping's own position (so `LATEST` on an old reshard does not replay the child). A saved
position counts as followed, so a restart keeps the rule. AWS Lambda Developer Guide: parents before children.

### 11: interpreter checks

The Go code (`lambdaruntime.CheckRuntime`, called from `warnRuntime`) logs a warning at create and update and lets the first invoke
fail; Go's SUPPORT.md says "refuses", but its code did not, and AWS accepts such a function (the host's interpreters are not a fact AWS
knows). So: the call succeeds, the log names what is missing (interpreter, the Java runtime interface client in the package, a .NET
self-hosting assembly), and the first invocation fails with `Runtime.LaunchError`. No `StateReason` (Go set none).

### 12: live logs

`livelog.rs`: while an invocation runs, what the function printed is written to its stream every 200 ms (`START` first); at the end
only the rest (last lines, `END`, `REPORT`) is written. The `Invoke` log tail still has the whole log.

## Numbers

- Scoreboard (release-aws, exact): s3 84, sns 38, eventbridge 55, lambda 52: unchanged.
- Weight (darwin/arm64, release-aws): 4,967,376 B, 235 packages, idle 4,464 KB; the base it was branched from measured 4,917,280 B,
  233 packages, 4,416 KB. +49 KB; +1 package, first party (`aws-webhook`; `cargo tree` lists it twice, hence +2). No third-party
  package is new (`tokio-util`, `bytes`, `tokio` were already in the graph; `aws-sns` and `aws-eventbridge` now name them).

## Files outside the four crates

- `crates/aws-wire/src/service.rs` (`Web::owns_preflight`, default), `crates/aws-wire/src/rest.rs` (`RestService::cors`, default).
- `crates/aws-gateway/src/web.rs` (the preflight fallback), `rest.rs` (three lines), `cors.rs` (new), `lib.rs` (`mod cors`).
- `crates/aws-webhook` (new crate; the old `aws-eventbridge/src/http.rs`).

To stay under the 450-line file limit, `Inner::stage` moved from `aws-lambda/src/lib.rs` to `code.rs`, and the code-in-place test
from `tests/behaviour.rs` to `tests/inplace.rs`, unchanged.

## Open

- `https` endpoints for SNS and EventBridge wait for the owner's TLS decision. **(Superseded 2026-11-14: outbound https is built; see `stages/tls.md`.)**
- The website is served to any caller; AWS wants a public-read policy (and `BlockPublicPolicy` off) for its website endpoint.
- An SNS `ConfirmSubscription` through the `SubscribeURL` is asked as an anonymous caller of the topic's account; under `--iam enforce`
  that is the account's root, as every unsigned request to a trusting gateway is.
