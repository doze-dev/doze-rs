# Phase 8: security (SASL, ACLs, TLS)

Status: PLANNED (2026-11-12), research done, no code yet. Evidence: `phase8/kafka-semantics.md` (Kafka 4.3.1 source, cited),
`phase8/go-oracle-rust.md` (the recorded oracle, the Go edition, the Rust touchpoints), `phase8/prior-art-weight.md` (prior art,
measured weight). Catalogue to write: `behaviours/security.md`.

Goal: a client can be made to prove who it is (SASL PLAIN, SCRAM-SHA-256, SCRAM-SHA-512), what it may do is decided by ACLs the
way Apache Kafka 4.3.1 decides it, and the channel can be TLS. All of it opt-in: with none of it configured the broker behaves
exactly as at the close of Phase 7 (open, plaintext, one atomic load per check at most), so every non-security scenario and the
23 interop scenarios keep passing untouched.

Done when: the five oracle scenarios that fail today pass (`sasl-plain`, `sasl-scram`, `acl-unknown-codes`,
`idempotent-write-from-topic-write`, and the ACL rows of `version-matrix`): the full replay goes from 43 pass and 15 fail to 47 and
11; the eleven left are the ten `telemetry-*` scenarios and `version-matrix`, which still differs on DescribeCluster and the
telemetry rows (Phase 9); real clients (franz-go, sarama, kafka-go, librdkafka) work over SASL PLAIN and SCRAM, and
over SASL_SSL with the `tls` feature; every authorization row of the matrix has a test written from the Kafka source; the
secret-redaction gate passes; the weight gates hold; two independent audits say done.

## What the research settled

- **Scope in the recordings.** Only five scenarios exercise security, and only `Metadata` (29) and `InitProducerId` (31) denials are
  recorded. Everything else in the authorization matrix (about 45 APIs) is source-derived: each row is written from the Kafka
  source with its file and line, and a recording of it is wanted (below).
- **Kafka's rules that a port gets wrong** (`phase8/kafka-semantics.md`): `allow.everyone.if.no.acl.found` is per resource (any ACL
  matching a topic denies everyone else on it); super users bypass DENY; DENY is exact (DENY Read does not deny Describe) while
  ALLOW implies (Read/Write/Delete/Alter imply Describe, AlterConfigs implies DescribeConfigs); Produce checks only topic Write,
  and only InitProducerId accepts "Write on any topic" in place of cluster IdempotentWrite (`authorizeByResourceType`); the
  SaslHandshake mechanism list is in Java `HashSet` order (PLAIN, SCRAM-SHA-512, SCRAM-SHA-256), not the configured order; a raw
  code 0 in an ACL request closes the connection; AlterConfigs validation runs before authorization; a SASL login that fails is
  answered, then the connection is closed (a SCRAM proof of the wrong length is closed with no answer).
- **The Go edition's flags are the contract of the harness**: `--sasl-mechanisms`, `--sasl-user user:password` (repeatable),
  `--acl-default allow|deny`, `--acl-super-user User:name`, `--tls-cert`, `--tls-key`. The Rust binary accepts the first four
  today and ignores or refuses them (`cli.rs`, `cli/tests.rs`, `tests/process.rs`: to be rewritten).
- **The wire crate was built for this**: `Trust::{Anonymous, Authenticated}`, the header/body split ("a stranger's body is never
  decoded"), `Features.sasl`. SaslHandshake (v1), SaslAuthenticate (v0-2), the three ACL APIs (v1-3) and the two SCRAM credential
  APIs decode already. The connection loop hard-codes `Trust::Authenticated`; `Broker::handle` answers the five APIs with `Close`.
- **Weight** (measured on a scratch server, darwin/arm64): TLS on rustls with `ring` is +872,608 B and +12 CI packages, so it cannot
  be always on (the Linux CI gate is 5.15 MB and 115 packages; the binary is 4.90 MB there). SCRAM and PLAIN on `ring` cost +34 KB
  and +3 packages; on RustCrypto they cost +14 packages and break the gate. No crypto or TLS crate is in the lock today, and the
  default build is pure Rust (`ruzstd`, `miniz_oxide`, no `cc`).

## Decisions taken (each can be changed by the owner before its slice starts)

| # | Question | Taken | Why, and what changes if it is overturned |
|---|---|---|---|
| D1 | Crypto for SCRAM and PLAIN | **Hand-written SHA-256, SHA-512, HMAC, PBKDF2 and a constant-time compare, in a leaf crate `kafka-auth` with no dependencies.** `ring` only behind the `tls` feature | Keeps the default build pure Rust and the package count at 107; the cost is about 400 lines we own. It is pinned by NIST and RFC 4231/6070/7677/5802 vectors, a differential test against `ring` and RustCrypto as dev-dependencies, the recorded SCRAM scenario (real Kafka's own client math), and real clients. If overturned: `ring` in the default build is +34 KB, +3 packages and a C compiler; the module boundary (`kafka-auth::crypto`) makes the swap a one-file change |
| D2 | TLS | **Cargo feature `tls`, off by default** (`rustls` 0.23 + `tokio-rustls`, provider `ring`, PEM through `rustls-pki-types`). PEM files only, any error aborts startup, never a fallback to plaintext. No cert generation, no client certificates | +0.87 MB cannot be always on. CI gains a second weight job for `--features tls`. If the owner wants TLS in the default binary the CI limits must rise (about 5.8 MB Linux, 121 packages) |
| D3 | Users and credentials | **Slices 8a-8c: flags only, as the Go edition** (`--sasl-user`, derived at start, nothing stored). **Slice 8f: Kafka's `AlterUserScramCredentials` / `DescribeUserScramCredentials`** (APIs 50, 51) with a credential table in `kafka-meta`, so `kafka-configs.sh` and admin clients can manage users at runtime | The flags alone cannot add a user to a running broker; the API alone locks you out (every comparable system has a bootstrap user). 50/51 are not recorded: source-derived tests only. Dropping 8f costs nothing else |
| D4 | Authorization when SASL is off | **As the Go edition**: the ACL APIs always work and are stored; ACLs are enforced for the principal `User:ANONYMOUS`; `--acl-default allow` is the default, so an empty ACL set changes nothing | Kafka without an authorizer answers `SECURITY_DISABLED` (54) and enforces nothing; that is a deliberate difference (ADM-style entry `SEC-D1`). It is what lets `--acl-default deny` be used without SASL |
| D5 | A request in the middle of a login | **As Kafka where recorded**: the second ApiVersions, a handshake out of order and a SaslHandshake/ApiVersions mid-token are answered with `ILLEGAL_SASL_STATE` (34) in the request's own shape; anything else closes | Needs the stranger's header only, not its body, for all but those two APIs. Unrecorded cases follow the Kafka source |
| D6 | SCRAM channel binding | **As Kafka**: `n,,` and `c=eSws` log in; `p=` binding is refused | Kafka parity is what the oracle records; the Go edition refuses it (known difference) |
| D7 | `--kafka-version 1` and the ACL APIs | **Keep the pinned deviation** (not offered): Kafka 1.x admin tools speak only v0, which the vendored protocol crate cannot decode | Decoding v0 ourselves is a small later item if anyone asks |
| D8 | Allowed-hosts guard | **A pure function with a fuzz target and its flags in 8g; mounted by the console in Phase 9.** It guards the HTTP listeners (DNS rebinding), not the Kafka port | There is nothing to mount it on until Phase 9 |
| D9 | Handler denials with no recording | **Write each from the Kafka source (file:line in the test), then record `authz-matrix` against `apache/kafka:4.3.1` if Docker is available and diff** | The one real risk of the phase is a source reading that differs from the JVM; a recording is the cure. Needs the Go harness's recorder and Docker |
| D10 | Pre-auth limits | A stranger's frame is capped at Kafka's `sasl.server.max.receive.size` (512 KiB; the wire crate has 1 MiB today); 30 s from accept to the first complete login (the Go edition's rule; Kafka has none) | Unrecorded; a dev broker should not sit open to a silent socket |

## Design

**Crates.** One new leaf crate, `kafka-auth` (no dependencies; `forbid(unsafe_code)`; sans-IO; the fuzz targets live here):
`crypto` (SHA-2, HMAC, PBKDF2, constant-time compare), `plain`, `scram` (parse, keys, the two-message state machine), `principal`
(`Principal`, `Mechanism`), and `authz` (`Acl` with the raw wire codes, `AclFilter`, `AclSet`, the decision procedure, the
`authorizeByResourceType` variant). The ACL types keep the integers a client sent (a client gets back exactly what it stored), as
the Go edition does. `kafka-meta` stores `AclRecord` (version byte plus postcard) in a new table; `kafka-broker` holds the
handlers and the `Authorizer` (an `ArcSwap` snapshot, rebuilt after every Create/Delete under the write lock; with default allow
and no ACLs a check is one atomic load); `kafka-server` holds `Session` and the gating; the optional `tls` module lives in
`kafka-server` behind the feature. `doze-kafka` has the flags.

**Session.** `enum Session { Unauthenticated, Handshaken(Mechanism), Authenticated(Principal) }` in `kafka-server/src/session.rs`;
`Session::admit(key) -> Verdict` encodes the recorded order (`phase8/go-oracle-rust.md` A2.3): before the handshake one
ApiVersions then SaslHandshake, anything else closes; after the handshake only SaslAuthenticate; once authenticated everything is
served except SaslHandshake/SaslAuthenticate, which answer 34 and keep the connection. The decoder starts `Trust::Anonymous` when
SASL is on and is switched at login. A failed login is answered and flushed before the close. Nothing the client sent is ever
logged or put in an error text.

**Authorization threading.** Handlers receive `Caller { principal, host }` (not one more argument: `too_many_arguments` is a lint
error), built per request from the connection. Each handler checks as the matrix says, **before** existence checks. A denial is
the API's own code (29 topic, 30 group, 31 cluster, 53 transactional id), per item where Kafka answers per item.

**Files** (new unless marked; all target under 200 lines, none over 450):

| Area | Files |
|---|---|
| `kafka-auth` | `crypto/{sha256,sha512,hmac,pbkdf2,ct}.rs`, `plain.rs`, `scram.rs`, `scram/{parse,keys}.rs`, `principal.rs`, `authz.rs`, `authz/{acl,filter,rules}.rs`, `credentials.rs` |
| `kafka-meta` | `acl.rs`, `store/acls.rs`, `credential.rs` + `store/credentials.rs` (8f); `store.rs` gains the tables |
| `kafka-broker` | `caller.rs`, `authorizer.rs`, `handlers/{create_acls,describe_acls,delete_acls}.rs`, `handlers/{describe,alter}_user_scram_credentials.rs` (8f); **split `broker.rs`** (`handle` into `broker/dispatch.rs`); one check in each served handler (list in 8d) |
| `kafka-server` | `session.rs`, `session/steps.rs`, `tls.rs` (feature), `connection.rs` (split `exchange` out), `config.rs`, `frames.rs` (pre-auth budget), `accept.rs` (handshake in the spawned task) |
| `kafka-wire` | `api_versions.rs` (the illegal-state answer) |
| `doze-kafka` | `cli.rs` (the flags; rewrite `cli/tests.rs` and `tests/process.rs`), `hostguard.rs` (8g) |
| tests | `kafka-testkit` gets the raw-protocol client with SASL helpers the plan promised; `interop/go` gains SASL and ACL cells; fuzz targets `plain`, `scram_first`, `scram_final`, `acl_filter` |
| docs | `behaviours/security.md`, `oracle.md`, `weight.md`, `debt.md`, `PLAN.md` log, a facts view for the console's Access page |

## Slices

| | Slice | Gate |
|---|---|---|
| 8a | `kafka-auth`: crypto with vectors, PLAIN, `Principal`; `Session` and the gating in the connection; `--sasl-mechanisms` and `--sasl-user` live; ApiVersions lists the two SASL APIs only when SASL is on | crypto vectors; `Session` table tests; the handshake and PLAIN steps of `sasl-plain` pass |
| 8b | ACLs: `kafka-auth::authz`, the `kafka-meta` table, `Authorizer`, `Caller`, the three ACL handlers, the `Metadata` and `InitProducerId` checks, `--acl-default`, `--acl-super-user` | oracle `sasl-plain` (whole), `acl-unknown-codes`, `idempotent-write-from-topic-write`, `version-matrix`; a model test of the authorizer against Kafka's algorithm written from the source |
| 8c | SCRAM-SHA-256 and -512: parsers, keys, the state machine, wired into the handshake and authenticate | oracle `sasl-scram`; franz-go, sarama, kafka-go (where it supports it) and librdkafka log in over SCRAM |
| 8d | The authorization matrix on every remaining served handler (about 45 APIs, the Phase 6 transaction APIs included), one test per row from the source; secret-redaction tests; the fuzz targets | a test per matrix row; every non-security scenario and `interop/` unchanged (the allow-all path is exact); interop with an ACL'd user (allowed and denied) |
| 8e | TLS behind `tls`: acceptor, handshake in the spawned task, peer address taken before the handshake; client interop over SASL_SSL; a CI weight job for the feature | loopback TLS tests; franz-go, sarama, librdkafka over SASL_SSL; weight of the `tls` build recorded |
| 8f | `AlterUserScramCredentials` / `DescribeUserScramCredentials` with the credential table (D3); a users file or environment source for bootstrap passwords so they stay out of `ps` | tests from the source; `kafka-configs.sh`-shape requests through the raw client; a restart keeps the credentials |
| 8g | The allowed-hosts guard (pure, fuzzed, flags); the catalogue; `oracle.md`, `weight.md`, `debt.md`; the Access facts view; independent audit, fixes, re-audit | the done-when list above |

Order is 8a, 8b, 8c, 8d, 8e, 8f, 8g. 8e and 8f do not depend on each other. If `authz-matrix` can be recorded (D9), it is done after
8d, before the audit, and its diff is the first thing the audit reads.

## Testing

- **Oracle first.** The five scenarios are the acceptance of 8a to 8c. The Go harness speaks SASL/PLAIN itself (admin, alice, bob) and
  passes the flags above.
- **Real clients** are the check that caught most bugs in Phases 4 to 7 and the recordings do not cover them for security: the
  `interop/` suite gains cells for SASL PLAIN, SCRAM-SHA-512 and an ACL'd user, and (8e) SASL_SSL, for the four client libraries.
- **A model for the authorizer**: a few dozen lines transcribing `StandardAuthorizerData.authorize` from the source, run against
  the real one on random ACL sets and requests (proptest). The DENY-wins, per-resource default and implication rules are where a
  hand-written authorizer goes wrong.
- **Crypto**: NIST/RFC vectors, and a differential test against `ring` and RustCrypto (dev-dependencies, not shipped).
- **Parsers** (PLAIN token, SCRAM client-first and client-final, ACL filter) are marked for `cargo xtask fuzz-check` and have targets.
- **Secret redaction**: a planted password must not appear in logs at any level, in `Debug` output or in an error text; the Go
  edition's suite (`cmd/doze-kafka/secrets*_test.go`) is the model.
- **Mutation**: the method of Phase 7 (every mutation checked as applied; the audit's list rerun) applies to `kafka-auth`.

## Risks

- **Source-derived rows.** About 40 of the 45 matrix rows have no recording. The mitigation is D9; the honest status of a row without
  a recording is "from the source", and the catalogue says so per row.
- **Hand-written crypto** (D1). Mitigated as above, and it is behind one module boundary. A reviewer may prefer a library; that is
  the owner's call and cheap to reverse.
- **Unknowns in `phase8/kafka-semantics.md` section 7** (the long cluster-authorization message text, the order of DescribeAcls and
  DeleteAcls results, the 10,000-ACL limit, re-authentication, mTLS principals, IPv6 ACL hosts). None is recorded; tests must not pin
  the first two (clients sort); re-authentication and mTLS are out of scope until asked.
- **ACL checks on the hot path.** The Produce and Fetch paths gain a check per topic; the allow-all fast path must stay one atomic
  load, and the weight and throughput gates (`weight.md`, the produce benchmark) are rerun at 8d.
- **`broker.rs` and several handlers are near the 300-line soft cap** (`create_topics.rs` 282, `produce.rs` 288, `fetch/round.rs` 270,
  `broker.rs` 275): the checks go in with a split, not on top.

## Sizing

The plan's table says 3 to 4 sessions. The honest figure is **7 to 10**, because the plan did not count: the 45-API authorization
matrix (8d alone is 1.5 to 2 sessions: many handlers, one source-derived test per row, interop with ACLs), the crypto
and its vectors, a security cell in the interop suite, and the audit loop (Phase 7's two audits took about as long as the build).
Rough split: 8a 1, 8b 1 to 1.5, 8c 0.5 to 1, 8d 1.5 to 2, 8e 0.5 to 1, 8f 0.5, 8g with audits 1.5 to 2.

## Not in this phase

OAUTHBEARER, GSSAPI, delegation tokens, SASLprep (bootstrap passwords are restricted to ASCII, as the Go edition), client
certificates and mTLS principals, `ssl.principal.mapping.rules`, re-authentication (KIP-368), quotas, user ACL resource types
beyond storing them, certificate generation, and mounting the allowed-hosts guard (Phase 9).
