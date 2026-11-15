# Stage fix2: AWS endpoints answer as AWS does; identity is checked where AWS checks it

Branch `aws-fix2`, from `78ba6190`. Judged against AWS only: the STS and IAM models (`models/sts.json`, `models/iam.json`), the
recorded snapshots, and, where marked, memory.

## 1. doze-only operations leave AWS's endpoints

| Before | Now | Judged against AWS |
|---|---|---|
| `Action=DozeAccessLog` / `DozeGeneratePolicy` answered on IAM's Query endpoint (`Service::query_extension`) | Refused like any action outside the model: 400 `InvalidAction`, `Could not find operation <name> for version 2010-05-08`. Served only at `GET /_doze/iam/DozeAccessLog` (`?Reset=true`), `GET /_doze/iam/DozeGeneratePolicy?Principal=&ScopeToResources=` (a `POST` takes the same parameters as a JSON object) | AWS's Query front end refuses an unknown action with `InvalidAction` (code from memory; the sentence is (memory)). Scenario `test_iam.py::test_an_action_outside_the_model_is_invalid` sends three raw signed actions (status and error type only): UNVERIFIED, recordable |
| `X-Amz-Target: AmazonSQS.DozePeek` on SQS's JSON endpoint (`Service::extension`, `PEEK_OP`) | 404 `UnknownOperationException`, as any target outside the model. The look moves to `GET /_doze/sqs/<region>/<queue>/peek[?max=N]` (`crates/aws-sqs/src/peek.rs`) | AWS answers only modelled targets. The hook is doze's |
| `aws_wire::service::{Extension, Service::extension, Service::query_extension}`, gateway `target::extension`, `query::extension_reply` | Removed: nothing else used them | |

**The SQS look** (`/_doze/sqs/<region>/<queue>/peek`): read only (no receive count, no visibility change, no new handle; a message
received before carries its latest handle). Answer: `ReceiveMessage`'s JSON, `{"Messages": [...]}` (absent when empty), every
visible message (not in flight, not delayed, not expired), every FIFO group's, oldest first, with every system attribute except
`SenderId` (a look has no sender) and every message attribute. `max` 1 to 100, default 10; out of range is 400
`InvalidParameterValue`; an unknown queue is 400 `QueueDoesNotExist` with AWS's sentence; a region with nothing on disk is not woken.
Errors are `{"Code", "Message"}` like the Scheduler hook. Nothing in `web/` called `DozePeek`; the console will read this path.

## 2. One guard for every `/_doze/` address

`aws_wire::local::guard` (with `reply`, `header`, `KEY_HEADER`) is the one function: no page from another origin, a `POST` only as
`application/json`, and under `--auth verify` the configured secret key in `X-Doze-Key`. `/_doze/data` (`data_web.rs`) now calls it
(its own copy removed; its sentence for another origin now reads "may not use doze's /_doze/ addresses"). A service's web side says
it is local with `Web::local()`; the gateway then applies the guard before `serve`/`upgrade`, with `Config::local_key` (set by
`doze-aws`'s `wire.rs` from `--auth verify` + `--secret-key`, one line beside the data hook). IAM and SQS say `local() = true`.
**Not changed:** the Scheduler hook (`POST /_doze/scheduler/...`) is not marked local: its scenario posts an empty non-JSON body,
and marking it would refuse that with 415. Open: decide whether it should be guarded too (then the scenario sends `{}` as JSON).

## 3. Signatures AWS checks

### AssumeRoleWithWebIdentity (`crates/aws-iam/src/oidc.rs`)
In AWS's order, as documented (STS model: "Tokens must be signed using either RSA keys (RS256, RS384, or RS512) or ECDSA keys (ES256,
ES384, or ES512)"; IAM model, `CreateOpenIDConnectProvider`):
1. The issuer (`iss`) must have an OpenID Connect provider in the account (matched without `https://` and a trailing `/`), else
   `InvalidIdentityToken` "No OpenIDConnect provider found in your account for <iss>" (memory). Checked by the actor **before**
   anything is fetched, so a token cannot make doze-aws call an address nobody registered.
2. The signature, outside the actor (network): keys from `<iss>/.well-known/openid-configuration` → `jwks_uri`, cached per issuer two
   hours, refetched for an unknown `kid` at most every 30 s (memory). RS256/384/512, ES256/384/512. Keys that cannot be fetched:
   `InvalidIdentityToken` "Couldn't retrieve verification key from your identity provider,  please reference
   AssumeRoleWithWebIdentity documentation for requirements" (memory, as commonly reported). Other algorithms, an unknown `kid`, a
   signature that does not verify: `InvalidIdentityToken` with doze's sentence (memory).
3. `aud` (string or array) must be one of the provider's client ids: `InvalidIdentityToken` "Incorrect token audience" (memory).
4. `exp` must not have passed: `ExpiredTokenException` "Token expired: current date/time <now> must be before the expiration
   date/time<exp>" (memory, including AWS's missing space).

Fetch timeout 5 s (memory: STS publishes none), answer cap 150,000 B (API Gateway's JWKS quota, reused).

**Thumbprints:** AWS verifies the JWKS endpoint's TLS certificate against its trusted CAs and uses the thumbprints "only then" when
the certificate is not signed by one (IAM model). Here: the machine's trust store and `--ca-file`; the thumbprint fallback is **not
built** (open), so a private-CA IdP needs its CA in `--ca-file`. Thumbprints are stored and returned as before.

**Local IdP:** AWS requires `https://` provider URLs and cannot reach this machine. doze also fetches over `http://` when the issuer
is on this machine (`localhost`, `127.0.0.0/8`, `[::1]`): register the provider with that exact URL. Any other `http://` issuer is
refused. Nothing is accepted unsigned.

**Shared code:** the token reader, key cache and verification moved from `aws-apigateway/src/plane/{jwt,jwks}.rs` into
`aws-webhook` (`jwt.rs`, `jwks.rs`), used by both; API Gateway keeps RSA only and its 1,500 ms / 150,000 B quotas. ES* adds no crate
(p256/p384/p521 were linked by KMS). `fuzz_jwt` added to aws-webhook's fuzz targets (the marker moved with the reader).

**Found on the way, fixed:** both operations are `smithy.api#auth: []` in the STS model, and boto3 sends them **unsigned**; the gateway
refused every unsigned non-S3 request (`MissingAuthenticationToken`), so `AssumeRoleWithWebIdentity`/`AssumeRoleWithSAML` had never
worked through an SDK. `Service::unsigned(op)` (STS names the two) and `query::unsigned` in the gateway route an unsigned form POST
whose `Action` is such an operation; anything else unsigned is refused as before (same Coral body, so recorded labels are unchanged).

### AssumeRoleWithSAML (`crates/aws-iam/src/xmldsig/`, `samltrust.rs`)
- `PrincipalArn` must be a SAML provider of the account; its metadata must give a signing certificate
  (`IDPSSODescriptor/KeyDescriptor`, `use` signing or absent).
- The response must carry an XML signature (Response, Assertion, or both; each present one must verify): exclusive c14n (with
  `InclusiveNamespaces`), enveloped-signature, SHA-1/256/512 digests, RSA-SHA1/256/512 (PKCS #1 v1.5). ECDSA/DSA, inclusive c14n
  and `WithComments` are refused by name, as are encrypted assertions.
- Wrapping defences: the signature must be a direct child of the element its single `Reference` names (`URI="#<ID>"`); one
  Assertion only; duplicate `ID`/`Id`/`id` refused; DOCTYPE refused. Only the signed assertion's canonical bytes are read.
- The assertion's `Issuer` must be the metadata's `entityID`.
- Refusals: `InvalidIdentityTokenException` with doze's sentences naming what failed (memory: none recorded).
- Certificate validity dates are not checked (memory: AWS documents the metadata certificate as the trust anchor).
- Proven against an independent implementation: fixtures signed by xml-crypto 6.4.0 (`testdata/gen.ts`, run by bun); the canonical
  assertion matches xml-crypto's byte for byte. 31 tests (13 c14n vectors incl. W3C exc-c14n §2.2; 18 end to end and refusals).
- Weight: one `rsa` line in aws-iam (already in the binary); no new crate.
- **Local IdP:** register its metadata (with its signing certificate) with `CreateSAMLProvider`, exactly as on AWS.

Not built (open): the assertion's `NotOnOrAfter`/`SessionNotOnOrAfter` (ExpiredToken), the `Role` attribute naming the
RoleArn/PrincipalArn pair, the `Recipient`; `CreateSAMLProvider` does not validate the metadata yet (AWS does).

## 4. "Go" mentions in aws-iam, aws-gateway, aws-sqs, aws-wire

| Where | Behaviour | Judgement |
|---|---|---|
| `aws-iam/src/authz.rs` `aws:SecureTransport` always true | Kept | AWS sets it when the request came over TLS; the code under test reaches AWS's endpoints over https (SDKs and CLI by default), so on AWS it is true; the plain-HTTP hop is the local stand-in, not the code's choice. Reporting it would make deny-unless-TLS policies refuse what AWS never refuses |
| `aws-wire/src/auth_error.rs` codes "follow the Go edition" | Unchanged codes; now "follow AWS's public documentation and memory" (still unverified, on the recording queue) | |
| `aws-iam` `query_extension` doc ("as the Go edition served them") | Removed with the behaviour (section 1) | AWS has no such actions |
| `aws-iam/src/saml.rs` "the signature is not checked ... as the Go edition did" | Replaced: signatures verified (section 3) | |
| `aws-wire/src/service.rs` `query_extension` doc | Removed with the hook | |
| `aws-iam/src/providers.rs` "nothing is fetched ... STAY CUT" | Rewritten: providers are how STS trusts tokens and assertions | |

No other "Go" mention remains in the four crates.

## Tests and scenarios
- Unit: `aws_wire::local` (guard), `aws-sqs` `tests/peek.rs` (core and web side), `aws-iam` `tests/webidentity.rs` (https IdP from
  `aws_webhook::test_server`, ES256: good token, forged claim, unknown kid, `alg: none`, wrong audience, expired), `sessions.rs`
  (provider required; SAML signed/unsigned/tampered), `usage.rs` (JSON POST on `/_doze/iam`), xmldsig (31), aws-webhook jwt (ES256).
- Gateway: `an_operation_outside_the_model_is_unknown` (`AmazonSQS.DozePeek` is 404).
- boto3: `test_iam.py::test_an_action_outside_the_model_is_invalid` (3 labels, UNVERIFIED, recordable);
  `test_sts.py::test_web_identity_needs_a_token_its_provider_signed` (doze target only, skipped on AWS: AWS cannot reach a local IdP;
  serves its own discovery document and JWKS on 127.0.0.1 and signs RS256 in pure Python; asserts, no labels).

## Gates, conformance, weight
- `cargo xtask gates`: all passed, exit 0.
- Conformance (release-aws binary): iam 48 labels, 45 exact, 3 unverified (the new raw scenario); sts 2/2; sqs 180/181 (the decided
  SQS-over-Query deviation, unchanged); apigateway 91/92 (unchanged). The doze-only web identity scenario passes.
- Weight (`cargo xtask weight doze-aws`, macOS): 8,726,672 B, 274 packages, 6,624 KB idle; inside the CI line (13,900,000 B,
  280 packages, 10,526 KB). No crate was added to the binary (rsa, p256/p384/p521, sha1/sha2 were linked already); the increase over
  the base was not measured separately.

## Open
- Thumbprint fallback for an OIDC provider whose certificate no trusted CA signed.
- SAML: `NotOnOrAfter`, the `Role` attribute pair, `Recipient`; `CreateSAMLProvider` metadata validation.
- Trust-policy evaluation for `AssumeRoleWithWebIdentity`/`AssumeRoleWithSAML` (Federated principal, `<provider>:aud`/`:sub`
  conditions) under `--iam enforce`.
- Whether the Scheduler hook should be marked local (guarded).
- All error sentences in section 3 are (memory) until recorded; the web identity scenario cannot be recorded (local IdP).
