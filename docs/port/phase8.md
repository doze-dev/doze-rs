# Phase 8: security (SASL, ACLs, TLS)

Status: **BUILT, audits pending (2026-11-12).** All seven slices are built and gated (ledger below); the replay is 47 pass and 11 fail (the eleven are Phase 9); the catalogue is `behaviours/security.md`. Two independent audits are running; this phase is not marked done until both say so and their fixes are re-audited. Evidence: `phase8/kafka-semantics.md` (Kafka 4.3.1 source, cited),
`phase8/go-oracle-rust.md` (the recorded oracle, the Go edition, the Rust touchpoints), `phase8/prior-art-weight.md` (prior art,
measured weight). Catalogue: `behaviours/security.md` (index at its top, checked by `tools/check-security-catalogue.ts`); debts and left-open items: `debt.md` section I.

**Ledger** (merge commits on `main`, `git log --first-parent`; the gates are those the merge recorded):

| Slice | Commit | What | Gates recorded |
|---|---|---|---|
| plan | 2d4fffb, 2132087 | research, decisions, the owner's choice of RustCrypto | |
| 8a | 39e54f9 | SASL PLAIN, `kafka-auth`, `Session` and the login gating; SEC-1 to SEC-15 | 815 tests, interop 23/23, replay 43/15, packages 107 to 122, CI gate 126 |
| 8b | ea64049 | the authorizer, three ACL APIs, Metadata and InitProducerId checks, `--acl-default`, `--acl-super-user`; SEC-16 to SEC-30 | model test on 6000 random ACL sets; `sasl-plain`, `acl-unknown-codes`, `idempotent-write-from-topic-write` pass |
| 8c | 7d90033 | SCRAM-SHA-256 and -512; SEC-40 to SEC-51 (deviations renumbered to SEC-D12 to D15) | 950 tests, interop 23/23 and 12/12 in SASL mode, replay 47/11 |
| 8e | c8a5391 | TLS behind the `tls` feature; SEC-140 to SEC-154, SEC-D40 to D48; the `weight-tls` CI job | loopback tests, interop `-tls` cells |
| 8d-1 | 9dcfb4f | ACL enforcement on the topic APIs; SEC-60 to SEC-72, SEC-D16 to D20 | 25 of 25 mutations killed |
| 8d-2 | 6ef2b3d | ACL enforcement on the group and transaction APIs; SEC-100 to SEC-123, SEC-D30 to D37 | 1074 tests, interop 23/23, 12/12 SASL, 8/8 TLS, replay 47/11; 40 of 40 mutations killed |
| 8f | 909d147 | credential APIs 50 and 51, the stored table, `--sasl-users-file`, `DOZE_KAFKA_SASL_USERS`, `--sasl-allow-empty-users` (removed by the first audit, SEC-D72); SEC-160 to SEC-179, SEC-D50 to D59 | 1137 tests |
| 8g | branch `p8g` (not merged) | the allowed-hosts guard (`kafka-auth/src/hostguard.rs`, fuzz target, CI step), `--console-allow-host`, `--console-unprotected`, the start-up rule and notes, exit code 2; the catalogue index and its checker; `oracle.md`, `weight.md`, `debt.md`, `PLAN.md`; SEC-180 to SEC-189, SEC-D60 to D63 | fmt, clippy (also with `tls`), `lint-size`, `fuzz-check`, the catalogue checker, 1157 workspace tests |
| audit fixes (authentication half) | branch `p8h` | the first audit's findings: a super user's SCRAM credential (major), the open-ACL note, the failed-login delay, file modes, the 64 KiB stranger cap and pool, the clean close, the crypto vectors, no `--sasl-allow-empty-users`, no plaintext kept for SCRAM alone; SEC-190 to SEC-198, SEC-D70 to D73 | `debt.md` I23 |
| audits | pending | two independent audits of HEAD 909d147 and of the 8g delta; fixes; re-audit | |

What is not done and who decides is `debt.md` I1 to I25: chiefly the recording of `authz-matrix` (D9, needs Docker, which has a client but no running daemon on the owner's machine), and the owner's calls on mTLS, re-authentication, SASL v0, the failed-login delay and zeroing.

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
| D1 | Crypto for SCRAM and PLAIN | **RustCrypto crates, chosen by the owner (2026-11-12): `hmac`, `sha2` and `subtle`, with PBKDF2 (about 15 lines over HMAC) and base64 (about 30) written in-tree**, pinned by RFC 6070/7677/5802 and NIST vectors. `ring` only behind the `tls` feature | Pure Rust, no crypto primitives of our own, no C compiler. **The cost is packages: 107 to 122 (the spike measured +14 with `pbkdf2` and `base64` as crates, +12 without them; the broker is +15 lines of `cargo tree`, 12 distinct packages; 123 by the darwin count since slice 8e), over the 115 CI gate.** Slice 8a measured it, raised the gate in `ci.yml` to 126 and wrote the reason in `weight.md`. Size is about +50 KB. A hand-written or `ring` backend stays possible later: the boundary is `kafka-auth::crypto` and the swap is one file |
| D2 | TLS | **Cargo feature `tls`, off by default** (`rustls` 0.23 + `tokio-rustls`, provider `ring`, PEM through `rustls-pki-types`). PEM files only, any error aborts startup, never a fallback to plaintext. No cert generation, no client certificates | +0.87 MB cannot be always on. CI gains a second weight job for `--features tls`. If the owner wants TLS in the default binary the CI limits must rise (about 5.8 MB Linux, 121 packages) |
| D3 | Users and credentials | **Slices 8a-8c: flags only, as the Go edition** (`--sasl-user`, derived at start, nothing stored). **Slice 8f: Kafka's `AlterUserScramCredentials` / `DescribeUserScramCredentials`** (APIs 50, 51) with a credential table in `kafka-meta`, so `kafka-configs.sh` and admin clients can manage users at runtime | The flags alone cannot add a user to a running broker; the API alone locks you out (every comparable system has a bootstrap user). 50/51 are not recorded: source-derived tests only. Dropping 8f costs nothing else |
| D4 | Authorization when SASL is off | **As the Go edition**: the ACL APIs always work and are stored; ACLs are enforced for the principal `User:ANONYMOUS`; `--acl-default allow` is the default, so an empty ACL set changes nothing | Kafka without an authorizer answers `SECURITY_DISABLED` (54) and enforces nothing; that is a deliberate difference (deviation `SEC-D7` in `behaviours/security.md`, written in slice 8b; D1 to D6 are 8a's). It is what lets `--acl-default deny` be used without SASL |
| D5 | A request in the middle of a login | **As Kafka where recorded**: an ApiVersions after the handshake and a second SaslHandshake are answered with `ILLEGAL_SASL_STATE` (34) in the request's own shape, then the connection closes; anything else closes unanswered, **including a second ApiVersions before the handshake (recorded `versions-twice`) and every other API (SEC-D1: Kafka answers those in their own shape; the Go edition closes, as built)** | Needs the stranger's header only, not its body, for all but those two APIs. Unrecorded cases follow the Kafka source |
| D6 | SCRAM channel binding | **As Kafka**: the header `n,,` logs in; `y,,`, `p=...` and any other header are refused at client-first; the client-final channel binding is never compared with the header (`c=eSws` after `n,,` logs in) | Kafka parity is what the oracle records; the Go edition refuses the mismatch (known difference; SEC-47 and the SCRAM rules) |
| D7 | `--kafka-version 1` and the ACL APIs | **Keep the pinned deviation** (not offered): Kafka 1.x admin tools speak only v0, which the vendored protocol crate cannot decode | Decoding v0 ourselves is a small later item if anyone asks |
| D8 | Allowed-hosts guard | **A pure function with a fuzz target and its flags in 8g; mounted by the console in Phase 9.** It guards the HTTP listeners (DNS rebinding), not the Kafka port. **Built:** `kafka-auth/src/hostguard.rs` (not `doze-kafka`), `--console-allow-host`, `--console-unprotected`, and the start-up rule whose inputs exist (SEC-180 to SEC-189, SEC-D60 to D63) | There is nothing to mount it on until Phase 9 |
| D9 | Handler denials with no recording | **Write each from the Kafka source (file:line in the test), then record `authz-matrix` against `apache/kafka:4.3.1` if Docker is available and diff. Built: every row is from the source; the recording is not made (Docker has no daemon here); the proposal is in `oracle.md` and `debt.md` I1** | The one real risk of the phase is a source reading that differs from the JVM; a recording is the cure. Needs the Go harness's recorder and Docker |
| D10 | Pre-auth limits | A stranger's frame is capped at **64 KiB** (built, SEC-4, SEC-194; Kafka's `sasl.server.max.receive.size` is 512 KiB and this broker is stricter on purpose, SEC-D71; the first audit measured 325 MB for 1000 strangers at 300 KiB each, which the 512 KiB left to a gigabyte at the connection limit); 30 s from accept to the first complete login (the Go edition's rule; Kafka has none) | Unrecorded; a dev broker should not sit open to a silent socket |

## Design

**Crates.** One new leaf crate, `kafka-auth` (dependencies: `hmac`, `sha2`, `subtle`; `forbid(unsafe_code)`; sans-IO; the fuzz targets live here):
`crypto` (the one wrapper over `hmac`, `sha2` and `subtle`; PBKDF2; base64), `plain`, `scram` (parse, keys, the two-message state machine), `principal`
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
| `kafka-auth` | `crypto.rs`, `crypto/{pbkdf2,base64}.rs`, `plain.rs`, `scram.rs`, `scram/{parse,keys}.rs`, `principal.rs`, `authz.rs`, `authz/{acl,filter,rules}.rs`, `credentials.rs` |
| `kafka-meta` | `acl.rs`, `store/acls.rs`, `credential.rs` + `store/credentials.rs` (8f); `store.rs` gains the tables |
| `kafka-broker` | `caller.rs`, `authorizer.rs`, `handlers/{create_acls,describe_acls,delete_acls}.rs`, `handlers/{describe,alter}_user_scram_credentials.rs` (8f); **split `broker.rs`** (`handle` into `broker/dispatch.rs`); one check in each served handler (list in 8d) |
| `kafka-server` | `session.rs`, `session/steps.rs`, `tls.rs` (feature), `connection.rs` (split `exchange` out), `config.rs`, `frames.rs` (pre-auth budget), `accept.rs` (handshake in the spawned task) |
| `kafka-wire` | `api_versions.rs` (the illegal-state answer) |
| `doze-kafka` | `cli.rs` (the flags; rewrite `cli/tests.rs` and `tests/process.rs`), `cli/console.rs` (8g: the console flags and the start-up rule; the guard itself is `kafka-auth/src/hostguard.rs`) |
| tests | `kafka-testkit` gets the raw-protocol client with SASL helpers the plan promised; `interop/go` gains SASL and ACL cells; fuzz targets `plain`, `scram_first`, `scram_final`, `acl_filter` |
| docs | `behaviours/security.md`, `oracle.md`, `weight.md`, `debt.md`, `PLAN.md` log (the facts view for the console's Access page moved to Phase 9: `debt.md` I20) |

## Slices

| | Slice | Gate |
|---|---|---|
| 8a | `kafka-auth`: crypto wrapper and in-tree PBKDF2/base64 with vectors, PLAIN, `Principal`; `Session` and the gating in the connection; `--sasl-mechanisms` and `--sasl-user` live; ApiVersions lists the two SASL APIs only when SASL is on | crypto vectors; the package gate raised with its reason; `Session` table tests; the handshake and PLAIN steps of `sasl-plain` pass |
| 8b | ACLs: `kafka-auth::authz`, the `kafka-meta` table, `Authorizer`, `Caller`, the three ACL handlers, the `Metadata` and `InitProducerId` checks, `--acl-default`, `--acl-super-user` | oracle `sasl-plain` (whole), `acl-unknown-codes`, `idempotent-write-from-topic-write`, `version-matrix`; a model test of the authorizer against Kafka's algorithm written from the source |
| 8c | SCRAM-SHA-256 and -512: parsers, keys, the state machine, wired into the handshake and authenticate | oracle `sasl-scram`; franz-go, sarama, kafka-go (where it supports it) and librdkafka log in over SCRAM |
| 8d | The authorization matrix on every remaining served handler (about 45 APIs, the Phase 6 transaction APIs included), one test per row from the source; secret-redaction tests; the fuzz targets | a test per matrix row; every non-security scenario and `interop/` unchanged (the allow-all path is exact); interop with an ACL'd user (allowed and denied) |
| 8e | TLS behind `tls`: acceptor, handshake in the spawned task, peer address taken before the handshake; client interop over SASL_SSL; a CI weight job for the feature | loopback TLS tests; franz-go, sarama, librdkafka over SASL_SSL; weight of the `tls` build recorded |
| 8f | `AlterUserScramCredentials` / `DescribeUserScramCredentials` with the credential table (D3); a users file or environment source for bootstrap passwords so they stay out of `ps` | tests from the source; `kafka-configs.sh`-shape requests through the raw client; a restart keeps the credentials |
| 8g | The allowed-hosts guard (pure, fuzzed, flags); the catalogue; `oracle.md`, `weight.md`, `debt.md`; the Access facts view (moved to Phase 9); independent audit, fixes, re-audit | the done-when list above. **Built; the audit is pending** |

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
- **Crypto**: RFC/NIST vectors for the in-tree PBKDF2 and base64 and for the whole SCRAM proof; a differential test of PBKDF2 against the `pbkdf2` crate (dev-dependency, not shipped).
- **Parsers** (PLAIN token, SCRAM client-first and client-final, ACL filter) are marked for `cargo xtask fuzz-check` and have targets.
- **Secret redaction**: a planted password must not appear in logs at any level, in `Debug` output or in an error text; the Go
  edition's suite (`cmd/doze-kafka/secrets*_test.go`) is the model.
- **Mutation**: the method of Phase 7 (every mutation checked as applied; the audit's list rerun) applies to `kafka-auth`.

## Risks

- **Source-derived rows.** About 40 of the 45 matrix rows have no recording. The mitigation is D9; the honest status of a row without
  a recording is "from the source", and the catalogue says so per row.
- **Weight of the crypto choice** (D1): +14 packages is the largest weight step since Phase 3. It is a written, owner-approved rise of
  the package gate, measured in 8a on the real build (CI counts with `sort -u`, so the figure can differ from a distinct count).
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
Slice 8a is a little smaller with RustCrypto than it would have been with hand-written primitives. Rough split: 8a 1, 8b 1 to 1.5, 8c 0.5 to 1, 8d 1.5 to 2, 8e 0.5 to 1, 8f 0.5, 8g with audits 1.5 to 2.

## Not in this phase

OAUTHBEARER, GSSAPI, delegation tokens, SASLprep (bootstrap passwords are restricted to ASCII, as the Go edition), client
certificates and mTLS principals, `ssl.principal.mapping.rules`, re-authentication (KIP-368), quotas, user ACL resource types
beyond storing them, certificate generation, and mounting the allowed-hosts guard (Phase 9).
