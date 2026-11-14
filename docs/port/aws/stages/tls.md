# Stage: outbound TLS (`tls`)

Branch `aws-tls`, from `70ce8e3d`. doze-aws now calls `https://` wherever AWS would: SNS `https` subscriptions (and their
`SubscriptionConfirmation`), EventBridge API destinations, API Gateway `HTTP`/`HTTP_PROXY` integrations (REST and HTTP APIs), and a
JWT authorizer's key fetch from its issuer. Before this, all four refused or skipped `https://`, and a JWT from an `https://` issuer
had its claims checked and its signature **not verified**.

## What is built

**One client, `crates/aws-webhook`** (the crate SNS and EventBridge already shared; API Gateway had its own copy of a plain-TCP
client, now folded in):

| File | What |
|---|---|
| `lib.rs` | `Request` (method, URL, headers, body, timeout, answer limit, status-only, `skip_verification`), `Answer`, `Failure` (`Unsupported`, `Connect`, `Tls`, `Timeout`, `Protocol`), `Client`, the process client set once by `configure(Trust)` |
| `url.rs` | `http://` / `https://` URLs: host, port (80/443 by default), IPv6 brackets, fragment dropped |
| `answer.rs` | the request bytes (`Host` keeps a non-default port, which the old client dropped), the answer read until its `Content-Length` or chunked end (a server that keeps the connection open is not waited for), a TLS close without `close_notify` taken as the end |
| `tls.rs` | `Trust` (OS store, `--ca-file` roots, insecure), the two connectors (strict; lenient for `insecureSkipVerification`), the refusal wording |
| `test_server.rs` | feature `test-server` (dev-dependencies only): a per-process CA (rcgen), certificates for `localhost`/`127.0.0.1`, self-signed ones, an HTTPS server that records requests |

- **rustls 0.23 with ring, tokio-rustls**: what doze-kafka and doze-names already use. HTTP/1.1, one request per connection
  (`Connection: close`): the old clients kept no connections either, and a pool would hold idle sockets for callers that rarely
  repeat a destination.
- **Trust: the operating system's store, read lazily** (`os_roots.rs`), on the first `https://` call, in `spawn_blocking`. A
  corporate proxy's CA lives there and is trusted.
  - **Linux, Windows:** `rustls-native-certs` (the bundle openssl-probe finds or `SSL_CERT_FILE`; the Windows store).
  - **macOS (tls2):** `/usr/bin/security find-certificate -a -p` over the system roots keychain
    (`/System/Library/Keychains/SystemRootCertificates.keychain`), the system keychain (`/Library/Keychains/System.keychain`, where MDM
    and corporate proxies install CAs) and the user's login keychain (`mkcert -install`, a hand-trusted local CA). Listing needs no
    password and shows no prompt; it takes about 15 ms. The binary no longer links Security.framework or CoreFoundation, which cost
    3.5 MB of idle RSS (below). **Known difference:** keychain trust settings are not read, so every certificate in those keychains
    is a root: one marked "Never Trust" is trusted, and so is one sitting in the login keychain unmarked. `--ca-file` is the precise
    alternative. If no keychain can be read and no `--ca-file` is given, an https call fails with "the OS trust store could not be
    read (…); start doze-aws with --ca-file <PEM> to name the authorities to trust". No bundled fallback (webpki-roots): it would add
    bytes for a case (`/usr/bin/security` missing) that a macOS install does not have.
  - Not `rustls-platform-verifier`: it asks the OS to verify (more faithful on macOS and Windows) but may fetch revocation data inside
    the handshake, on doze-aws's one runtime thread, and links Security.framework on macOS.
  - Checked by hand with the release binary: an HTTP API quick-created with `--target https://example.com` answered 200 with the page
    (161 roots read from the three keychains).
- **`--ca-file <PEM>`**: extra roots (one or more certificates). A self-signed server's own certificate works as its CA. Read and
  checked at start: a missing file or one with no certificate stops doze-aws with the file named.
- **`--insecure-outbound-tls`**: nothing is verified (the handshake's signatures still are). Off by default; a warning at start says
  what it covers and points to `--ca-file` instead.
- **A refused certificate names its fix.** Unknown issuer (the localhost self-signed case): "the certificate 127.0.0.1:8443 presented
  is not signed by an authority this machine trusts (self-signed, or from a private CA). If it is your server, start doze-aws with
  --ca-file <the CA certificate, PEM> to trust the authority that signed it, or with --insecure-outbound-tls to skip verification of
  outbound https calls (local development only)". Expired, not yet valid, and wrong-name certificates each have their own sentence.
- **No idle cost in time**: nothing runs on a timer; the TLS configuration is built on first use; the JWKS cache reads its age when a
  token arrives.

**Per service:**

| Behaviour | Call |
|---|---|
| SNS `https` subscription is sent its `SubscriptionConfirmation` and, once confirmed, every notification, as `http` is | **Go wrong (fixed)**: Go and the first port stored it pending forever |
| SNS: an attempt waits 15 s (memory: the SNS guides give no figure); 2xx is delivered, 429/5xx/no answer retried under the delivery policy, other statuses not | AWS-correct (the retry classes are documented; the 15 s is memory, was 5 s) |
| SNS: a refused certificate is logged at warn with its fix on each attempt, and retried as a failed connection | Go weak (bettered: the cause and the flag are said; AWS's delivery logs say only that delivery failed) |
| EventBridge API destination to `https://` is called | **Go wrong (fixed)**: was stored and skipped |
| API destination timeout 5 s: "must have a maximum client execution timeout of 5 seconds" (EventBridge User Guide) | AWS-correct |
| API destination: a refused certificate is `ConnectionFailed`, retryable under the target's retry policy, warned with its fix | AWS-correct in class (memory); bettered in the log |
| API Gateway `HTTP`/`HTTP_PROXY` to `https://` | **Go wrong (fixed)**: was a 500 with "no TLS client" in the log |
| API Gateway REST `tlsConfig.insecureSkipVerification`: an unknown issuer is accepted, the name is still checked | AWS-correct, except the expiry date, which AWS also checks and this does not (open, below) |
| API Gateway REST `HTTP_PROXY` with a refused certificate: 500 `{"message":"Internal server error"}`, `API_CONFIGURATION_ERROR`, logged "Execution failed due to configuration error: <the cause and the flag>" | AWS-correct (memory: AWS logs "General SSLEngine problem"); was the 504 `INTEGRATION_FAILURE` path. HTTP (non-proxy) integrations already took this path |
| Integration timeouts: REST 29 s, HTTP APIs 30 s, or the integration's `timeoutInMillis` | AWS-correct (unchanged) |
| JWT authorizer: keys fetched from `<issuer>/.well-known/openid-configuration` → `jwks_uri` over `https://` (or `http://` for a local IdP) | **Go wrong (fixed)**: an `https://` issuer's signatures were not verified, so a forged token passed |
| JWKS kept per issuer for two hours, refetched on an unknown `kid` (at most every 30 s, memory), still used while fresh if a refetch fails; 1,500 ms and 150,000 bytes per fetch | AWS-correct ("caches the public key for up to two hours on a best-effort basis"; the JWKS quotas) |
| JWT algorithms: RS256, RS384, RS512 verified; anything else refused | AWS-correct: "Currently, only RSA-based algorithms are supported". **The brief asked for ES256 too; AWS refuses ES256 tokens, so they are refused** ("signing method ES256 is invalid") |
| JWT refusal: 401 `{"message":"Unauthorized"}` with `www-authenticate: Bearer scope="" error="invalid_token" error_description="…"` (the token has expired / has an invalid issuer / does not have a valid audience / signing method X is invalid / the token's signature is invalid / the key id k is not in the issuer's JWKS / unable to fetch the issuer's public keys); a missing scope 403 `Forbidden` with `error="insufficient_scope"`; no token at all 401 with no header | header and descriptions (memory); status and body AWS-correct |
| JWT whose issuer cannot be reached (down, or its certificate refused): refused 401; the log says why, with the TLS flag when that is the cause | AWS-correct (refused); bettered in the log |
| Order of checks, as the JWT authorizer page lists them: algorithm and signature (the configured issuer's keys), then `iss`, `exp`/`nbf`, audience, scopes | AWS-correct in order; which description wins when several fail is (memory) |
| REST `COGNITO_USER_POOLS`: claims checked, signature not | unchanged: Cognito is not served here, so no local token can carry a real pool's signature |

## Tests

- `aws-webhook` (12): a CA-signed server called by IP and by name (headers, `Host` with port, body); a self-signed server refused
  with both flags named; `--ca-file` holding the server's own certificate; `--insecure-outbound-tls`; `skip_verification` accepting an
  unknown issuer and still checking the name; plain `http`, status-only reads, a silent server timing out, `ftp://` refused; the OS
  store read and not trusting a fresh self-signed certificate; `--ca-file` errors; URL and answer parsing.
- `aws-sns/tests/http.rs`: an `https` endpoint gets the confirmation over TLS, confirms through the `SubscribeURL`, receives the
  notification; a self-signed endpoint receives nothing.
- `aws-eventbridge/src/tests/http.rs`: an `https` API destination receives the request (path parameter, credentials, connection
  query); a self-signed one records `ConnectionFailed`, retrying, and receives nothing.
- `aws-apigateway/src/tests/tls.rs`: an HTTP API with a JWT authorizer whose issuer serves its discovery document and JWKS over
  https (an RSA test key, `fixtures/jwt-rsa-test-key.pem`), proxying to an https backend: a good token reaches the backend; a tampered
  signature, an unknown `kid`, an ES256 header and an expired token are each 401 with their `www-authenticate`; the keys were fetched
  exactly once. An issuer with a self-signed certificate: 401 "unable to fetch". A REST `HTTP_PROXY` to a self-signed backend: 500,
  then 200 with `tlsConfig.insecureSkipVerification`. The existing HTTP API test (`tests/v2.rs`) now signs its tokens with the test
  issuer, since unsigned tokens are refused.
- Fuzz: the answer parser moved to `aws-webhook` (`fuzz_http_answer`), the URL splitter is new (`fuzz_webhook_url`).

## Weight

`xtask/weight-limits.txt`, 2026-11-14 (tls): macOS 8,077,296 B (+633,072), 274 packages (+16); Linux musl 12,733,856 B (+968,816). The
limit moves to 13,400,000 B / 280 packages. Idle: Linux unchanged (nothing is read at start); **macOS idle RSS 5,280 → 9,104 KB**,
because the binary now links Security.framework and CoreFoundation; the process's own memory (phys_footprint) is +576 KB, the rest
are shared framework pages. **Superseded by tls2 (integrator's call):** the keychains are read through `/usr/bin/security` instead, and on the merged
`aws` line the macOS figures went from 8,360,592 B / 276 packages / 9,968 KB idle (over the 9,413 KB limit) to 8,342,432 B / 270 packages
/ 6,432 KB idle; `otool -L` shows only libSystem and libiconv again. Linux code is unchanged by tls2, so its size is too.

Cross-building for musl now compiles ring's C. clang 21's `<stddef.h>` sends a `*-linux-musl` triple to a musl sysroot that is not
there, so the `CC_x86_64_unknown_linux_musl` wrapper used for the measurement rewrites the triple to `x86_64-unknown-linux-gnu`, adds
`-ffreestanding`, and an include directory with a stub `<assert.h>` (`assert` as nothing; ring builds with `NDEBUG`) and `<string.h>`
(the `mem*` prototypes, which musl provides at link time). CI builds natively and needs none of this.

## Open

- `insecureSkipVerification` does not check the expiry date (AWS does). rustls exposes the name check alone; an expiry check
  would mean parsing the certificate.
- SNS's 15-second attempt timeout and the JWT `www-authenticate` descriptions are from memory; recording against AWS settles them.
- No boto3 conformance scenario: the harness starts doze-aws without `--ca-file` and has no TLS endpoint to point at; every
  behaviour above is covered by the Rust tests instead.
- Connections are not reused. If a caller ever posts to one destination often enough for it to matter (an SNS fan-out to one
  webhook), a small per-host pool with an idle deadline is the next step.
