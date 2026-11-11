> Research record, 2026-11-12, kept as evidence for [`../phase8.md`](../phase8.md). Part A quotes the recordings in the Go repo (`~/Code/doze-dev/doze-kafka/conformance`); Part B describes the shipped Go edition (facts, not a template); Part C was written against `main` at 9492d39 and its file-level breakdown is superseded where `phase8.md` says so.

# Phase 8 reconnaissance: the recorded oracle, the Go edition, and the Rust touchpoints

Written 2026-11-12 for Phase 8 (TLS, SASL PLAIN and SCRAM, ACLs, allowed-hosts guard) of the Rust port of doze-kafka.
Nothing in any repo was edited. Sources read: Go repo `~/Code/doze-dev/doze-kafka` (branch `schemas-stage5`, 54 commits ahead
of `origin/main` 4081fa6, none of which touch recordings, `internal/broker/auth*`, `acls*`), the oracle worktree
(`.../7b22fd62-.../scratchpad/kafka-oracle`, branch `rust-oracle` `0dfb284`), and `~/Code/doze-dev/doze-rs` (`main` `9492d39`).
Recordings are `apache/kafka:4.3.1`. Step lists below are generated from the recording JSON by a script (bun), not retyped.

---

# Part A. The recorded oracle

## A0. What counts as "Phase 8" in the 81 recordings

I scanned every recording for authorization/authentication error codes (29 TOPIC_AUTHORIZATION_FAILED, 30 GROUP_AUTHORIZATION_FAILED,
31 CLUSTER_AUTHORIZATION_FAILED, 33 UNSUPPORTED_SASL_MECHANISM, 34 ILLEGAL_SASL_STATE, 53 TRANSACTIONAL_ID_AUTHORIZATION_FAILED,
54 SECURITY_DISABLED, 58 SASL_AUTHENTICATION_FAILED, and the neighbours 32/55-57/65-69), for the words SASL, ACL, Principal, `User:`, scram,
and for scenarios that set `Users`/`Admin`/`Mechanisms`/`KafkaEnv` in the Go source (`grep Users: conformance/scenarios*.go`).

| Recording | Auth involvement | Phase |
|---|---|---|
| `sasl-plain` | SASL/PLAIN listener, 3 users, StandardAuthorizer, deny-by-default | **8** |
| `sasl-scram` | PLAIN + SCRAM-SHA-512 + SCRAM-SHA-256, same users, same authorizer | **8** |
| `acl-unknown-codes` | the three ACL APIs with coded fields 0 / 99 / -1 / ANY, as super user `admin` over PLAIN | **8** |
| `idempotent-write-from-topic-write` | InitProducerId under an authorizer: 0 for Write-on-topic, 31 for nothing | **8** |
| `version-matrix` | 9 rows: DescribeACLs/CreateACLs/DeleteACLs v1..v3 on a plain, no-authorizer broker | **8** (the other 168 rows already pass) |
| `telemetry-acl` | authorizer on; user with no ACL is refused a topic (29) yet may GetTelemetrySubscriptions / PushTelemetry (0) | **9** (needs Phase 8 auth plus telemetry APIs) |
| `telemetry-instance-binding`, `telemetry-instance-binding-spaced` | SASL/PLAIN login for users `alice`, `bob` (and `admin`), **no authorizer**; they test whose instance id it is | **9** (needs only PLAIN) |
| `versions-not-served` | contains ACL/PLAIN keywords only as the three ACL APIs at v0 (closed) | already passes (V2) |
| `produce-validation`, step `transactional-bit-no-producer-id` | **53** TRANSACTIONAL_ID_AUTHORIZATION_FAILED on a broker with *no* authorizer (transactional batch, null transactional id in the request) | already passes (Phase 3/6) |
| `describe-groups-order`, `mixed-protocol-*`, `group-requests-*`, `offsets-retention*` | codes 68/69 only (NON_EMPTY_GROUP, GROUP_ID_NOT_FOUND); the "Authoriz" keyword hit is the `AuthorizedOperations` field | not auth |
| `produce-validation` (five steps), `full-disk*` | codes 32 (INVALID_TIMESTAMP) and 56 (KAFKA_STORAGE_ERROR) | not auth |

Everything else (`api-versions`, `apiversions-*`, `createtopics-*`, ...) has `AuthorizedOperations:-2147483648` fields and nothing else.
No other recording has a principal, an ACL, a secured listener or TLS. **TLS is not recorded at all** (`claims/auth.md`: "Not recorded:
TLS (a client certificate as the principal), SASL over TLS, SaslHandshake v0, OAUTHBEARER, GSSAPI, delegation tokens, re-authentication").
DescribeUserScramCredentials / AlterUserScramCredentials appear **only in the recorder's setup code** (not recorded as steps).

## A1. How the harness drives authentication, and what the Rust binary needs

**The Go harness speaks SASL itself**, on raw TCP, with franz-go's `kmsg` types, not with a client library
(`conformance/wire.go:189-210`, `conformance/harness.go:158-205`, `conformance/targets.go:230-242`):

- `dial` opens a TCP connection; the client id is `doze-conformance`; request header v1 (v2 for flexible versions).
- `Env.Open(name, user, pass)` = connect, then `saslPlain`: **SaslHandshake v1** (`PLAIN`) then **SaslAuthenticate v2** with
  `"\x00" + user + "\x00" + pass`. No ApiVersions is sent first on these connections. The same login is repeated when the
  harness reconnects after a recorded "closed by the broker" (`Env.Reconnect`).
- Scenario scripts that test the login (`sasl-plain`, `sasl-scram`) use `fresh(e, name)` = an **unauthenticated** connection of their
  own and send the handshake/tokens themselves, at the versions written in the step lists below (SaslHandshake always v1; SaslAuthenticate
  v2 unless `/v0` `/v1` is shown).
- SCRAM is computed by the script (`scenarios_sasl.go:218-300`): PBKDF2 / HMAC / SHA-256 or SHA-512; it **verifies the server signature
  itself and fails the run if `v=` is wrong**. So the Rust SCRAM must produce a correct ServerSignature and `i=4096`.
- A closed connection is detected when the read fails (EOF/reset); it is recorded as `{"(connection)":"closed by the broker"}` and the harness
  reconnects (re-logging in if that connection had credentials). Hence after any `CLOSED`, a following `WHO→CLOSED` on a `fresh` connection
  means nothing more than "an unauthenticated Metadata is closed".
- Replay against a foreign binary compares responses after normalisation. `Known` entries (Go list) are an allow-list of steps/paths where a
  difference is tolerated: see A2.4. A foreign binary's *stale* Known entries are not reported, so the Rust edition may either match Kafka
  or be as strict as Go on those steps.

**Credentials of the recorded users** (`scenarios_sasl.go:41`, `harness.go:58-68`): `saslUsers = {admin: admin-secret, alice: alice-secret,
bob: bob-secret}`; `Admin: "admin"`.

**What the recorded Apache Kafka ran with** (`targets.go:47-86`):

| Setting | Value |
|---|---|
| Listener | PLAINTEXT name with protocol `SASL_PLAINTEXT` (no TLS anywhere); controller listener plain |
| Authorizer | `org.apache.kafka.metadata.authorizer.StandardAuthorizer` (KRaft); no `allow.everyone.if.no.acl.found`, so **deny by default** |
| Super users | `User:admin;User:ANONYMOUS` (ANONYMOUS only so the controller listener works) |
| PLAIN | JAAS `PlainLoginModule` with `username="admin" password="admin-secret" user_admin=... user_alice=... user_bob=...` (sorted) |
| Mechanisms | `sasl-plain`: `PLAIN` only. `sasl-scram` and the others using SCRAM: `PLAIN,SCRAM-SHA-512,SCRAM-SHA-256` (this order is what a SaslHandshake answer lists) |
| SCRAM users | written through `AlterUserScramCredentials` by `admin` over PLAIN before the script starts: every user gets SCRAM-SHA-256 and -512 credentials, password = the PLAIN password, **iterations 4096**, **salt = `"salt-of-" + user`** (`ScramIterations`, `ScramSalt`) |
| Re-authentication | off (`connections.max.reauth.ms` unset); SessionLifetimeMillis is 0 in every answer |
| Inter-broker | PLAIN, as `admin` |

**Flags the harness gives the Rust binary** (`process_target.go:62-86` on `rust-oracle`; `docs/port/oracle.md`). Always: `--listen --data-dir --meta-db
--group-initial-rebalance-delay --consumer-session-timeout --offsets-retention --acl-default <allow|deny>`. The SASL scenarios add (nothing
else; any other differing config field fails loudly):

| Scenario | Flags added to the common set |
|---|---|
| `sasl-plain` | `--sasl-mechanisms PLAIN --sasl-user admin:admin-secret --sasl-user alice:alice-secret --sasl-user bob:bob-secret --acl-default deny --acl-super-user User:admin` |
| `sasl-scram` | same but `--sasl-mechanisms PLAIN,SCRAM-SHA-512,SCRAM-SHA-256` (the order is preserved in the flag) |
| `acl-unknown-codes`, `idempotent-write-from-topic-write` | same as `sasl-plain` (PLAIN only) |
| `telemetry-acl` (Phase 9) | PLAIN, users from `telemetryUsers` (identical three), `--acl-default deny --acl-super-user User:admin` |
| `telemetry-instance-binding*` (Phase 9) | `--sasl-mechanisms PLAIN` and the three `--sasl-user`s; `--acl-default allow` (default), no super user |

Flag details the Rust CLI must honour: `--sasl-user name:password` repeated (the harness sorts by name); `--acl-super-user` repeated, one principal
per flag (the CLI today declares it with `value_delimiter = ','`, fine); `--acl-default` is `allow` or `deny` and is **always** passed, including `allow`
for every non-security scenario. Today all of these parse but the SASL/TLS ones are refused (A: `cli.rs:138-146`).
Because SCRAM credentials are derived from the plain `--sasl-user` password, the broker needs no credential API for the oracle: it must derive
SCRAM verifiers for the enabled SCRAM mechanisms from the same `user:password`, with **4096 iterations** (recorded as the literal `i=4096`)
and any salt (recorded as `<salt>`).

## A2. `sasl-plain` (and its twin `sasl-scram`)

**Purpose** (`conformance/claims/auth.md` A1-A18): what a real Kafka answers to every SASL/PLAIN and SASL/SCRAM exchange, right or wrong, in order or
out of it, and *who the authorizer then takes the connection to be*. After every exchange the script sends a Metadata v9 for topics
`for-alice` and `for-bob` ("WHO"). The fixture gives `alice` Describe on `for-alice` and `bob` Describe on `for-bob`; the answer is therefore the
identity probe: the topic the principal may describe comes back with error 0 and its partition; the other has error 29 with
`Partitions:null`, `IsInternal:false`, `TopicID` zero, name kept; the Brokers/ClusterID are present; `AuthorizedOperations:-2147483648` at both levels.

**Fixture steps** (connection `admin`, logged in with PLAIN at connect; not part of the login tests):

| Step | Request | Recorded response |
|---|---|---|
| `fixture-topics` | CreateTopics v7, topics `for-alice`, `for-bob` (1 partition, RF 1, `Configs:null`, `TimeoutMillis 15000`) | both `ErrorCode 0`, `ErrorMessage null`, `ConfigErrorCode 0`, `Configs []`, TopicID `<topic-id:N>` |
| `fixture-acls` | CreateACLs v3: `[{TOPIC, for-alice, LITERAL, User:alice, Host "*" (recorded <host>), DESCRIBE, ALLOW}, {.. for-bob, User:bob ..}]` | `Results:[{ErrorCode 0, ErrorMessage ""}, {0, ""}]`, `ThrottleMillis 0` (**empty string, not null**) |
| (await, unrecorded) | DescribeACLs v3 filter `TOPIC, ANY pattern, ANY op, ANY perm`, repeated until `len(Resources)==2` | grouping by resource: one Resource per (type, name, pattern) |

**Legend for the step lists.** Format `connection: step → response ; ...` in the order sent. `HS(M)` = SaslHandshake v1 naming M; `AUTH("tok")` =
SaslAuthenticate v2 (`/v0`, `/v1` when older) with the token (`␀` is a NUL byte); `APIV` = ApiVersions v0 (empty body); `WHO` = the Metadata probe above,
shown as `alice=<code>,bob=<code>` or `nobrokers` (Brokers null and ClusterID null, ControllerID -1) or `CLOSED`. A number after `→` is ErrorCode;
`CLOSED` = no answer, the broker closed the connection. A response that says `mechs=null` has `SupportedMechanisms:null`; `mechs=list` has the list of
the mechanisms the listener offers (`["PLAIN"]` in sasl-plain; `["PLAIN","SCRAM-SHA-512","SCRAM-SHA-256"]` in sasl-scram); a 0 handshake answer has the same list.
Message abbreviations (all quoted exactly, with `ErrorMessage` set on SaslAuthenticate answers; success answers have `ErrorMessage ""`, empty `SASLAuthBytes`
unless shown, `SessionLifetimeMillis 0`, at v0, v1 and v2):

| Abbrev | ErrorMessage text |
|---|---|
| `PLAIN_BAD` | `Authentication failed: Invalid username or password` |
| `NO_USER` | `Authentication failed: username not specified` |
| `NO_PASS` | `Authentication failed: password not specified` |
| `AUTHZID` | `Authentication failed: Client requested an authorization id that is different from username` |
| `TOKENS_n` | `Invalid SASL/PLAIN response: expected 3 tokens, got n` |
| `SCRAM_BAD` | `Authentication failed during authentication due to invalid credentials with SASL mechanism SCRAM-SHA-256` (or `-512` as per the handshake) |
| `AFTER_OK` | `SaslAuthenticate request received after successful authentication` |

SCRAM notation: `NC.` = the client nonce the script chose (`nonce-of-<connection>.`; in `replay-elsewhere` the final message carries the *other*
connection's `nonce-of-replay.<SN>`); `SN` = the server's half (recorded `<server-nonce:N>`); `<proof:pw>` = a correct proof for password `pw`;
`<zero-proof>` = 32 (SHA-256) or 64 (SHA-512) zero bytes base64 (placeholder sent when no server-first arrived). `c=biws` is base64 of `n,,`.

### A2.1 `sasl-plain` step list (41 connections after the fixture)

- `good`: HS(PLAIN)→0 ; AUTH("␀alice␀alice-secret")→0 ; WHO→alice=0,bob=29
- `good-bob`: HS(PLAIN)→0 ; AUTH("␀bob␀bob-secret")→0 ; WHO→alice=29,bob=0
- `authzid-same`: HS(PLAIN)→0 ; AUTH("alice␀alice␀alice-secret")→0 ; WHO→alice=0,bob=29
- `authzid-other-user`: HS(PLAIN)→0 ; AUTH("bob␀alice␀alice-secret")→58 AUTHZID ; WHO→CLOSED
- `authzid-admin`: HS(PLAIN)→0 ; AUTH("admin␀alice␀alice-secret")→58 AUTHZID ; WHO→CLOSED
- `authzid-not-a-user`: HS(PLAIN)→0 ; AUTH("mallory␀alice␀alice-secret")→58 AUTHZID ; WHO→CLOSED
- `authzid-other-user-wrong-password`: HS(PLAIN)→0 ; AUTH("bob␀alice␀nope")→58 PLAIN_BAD ; WHO→CLOSED
- `authzid-other-user-empty-password`: HS(PLAIN)→0 ; AUTH("bob␀alice␀")→58 NO_PASS ; WHO→CLOSED
- `wrong-password`: HS(PLAIN)→0 ; AUTH("␀alice␀nope")→58 PLAIN_BAD ; WHO→CLOSED
- `other-users-password`: HS(PLAIN)→0 ; AUTH("␀alice␀bob-secret")→58 PLAIN_BAD ; WHO→CLOSED
- `password-prefix`: HS(PLAIN)→0 ; AUTH("␀alice␀alice-secre")→58 PLAIN_BAD ; WHO→CLOSED
- `password-longer`: HS(PLAIN)→0 ; AUTH("␀alice␀alice-secret1")→58 PLAIN_BAD ; WHO→CLOSED
- `unknown-user`: HS(PLAIN)→0 ; AUTH("␀mallory␀alice-secret")→58 PLAIN_BAD ; WHO→CLOSED
- `user-in-capitals`: HS(PLAIN)→0 ; AUTH("␀ALICE␀alice-secret")→58 PLAIN_BAD ; WHO→CLOSED
- `empty-password`: HS(PLAIN)→0 ; AUTH("␀alice␀")→58 NO_PASS ; WHO→CLOSED
- `empty-user`: HS(PLAIN)→0 ; AUTH("␀␀alice-secret")→58 NO_USER ; WHO→CLOSED
- `empty-user-and-password`: HS(PLAIN)→0 ; AUTH("␀␀")→58 NO_USER ; WHO→CLOSED
- `no-nuls`: HS(PLAIN)→0 ; AUTH("alice alice-secret")→58 TOKENS_1 ; WHO→CLOSED
- `two-parts`: HS(PLAIN)→0 ; AUTH("alice␀alice-secret")→58 TOKENS_2 ; WHO→CLOSED
- `four-parts`: HS(PLAIN)→0 ; AUTH("␀alice␀alice-secret␀extra")→58 TOKENS_4 ; WHO→CLOSED
- `trailing-nul`: HS(PLAIN)→0 ; AUTH("␀alice␀alice-secret␀")→58 TOKENS_4 ; WHO→CLOSED
- `empty-token`: HS(PLAIN)→0 ; AUTH("")→58 TOKENS_1 ; WHO→CLOSED
- `not-utf8`: HS(PLAIN)→0 ; AUTH("base64:AGFsaWNlAP/+/Q==")→58 PLAIN_BAD ; WHO→CLOSED
- `token-of-64k`: HS(PLAIN)→0 ; AUTH("␀alice␀x…(64 KiB of x)")→58 PLAIN_BAD ; WHO→CLOSED
- `unknown-mechanism`: HS(BOGUS)→33 mechs=list ; AUTH("␀alice␀alice-secret")→CLOSED ; WHO→CLOSED
- `lowercase-mechanism`: HS(plain)→33 mechs=list ; AUTH("␀alice␀alice-secret")→CLOSED ; WHO→CLOSED
- `empty-mechanism`: HS()→33 mechs=list ; AUTH("␀alice␀alice-secret")→CLOSED ; WHO→CLOSED
- `scram-not-enabled`: HS(SCRAM-SHA-256)→33 mechs=list ; AUTH("␀alice␀alice-secret")→CLOSED ; WHO→CLOSED
- `refused-then-right`: HS(BOGUS)→33 mechs=list ; HS(PLAIN)→CLOSED ; AUTH("␀alice␀alice-secret")→CLOSED ; WHO→CLOSED
- `no-handshake`: AUTH("␀alice␀alice-secret")→CLOSED ; WHO→CLOSED
- `no-login`: WHO→CLOSED
- `request-before-token`: HS(PLAIN)→0 ; WHO→alice=34,bob=34 nobrokers ; WHO→CLOSED ; AUTH("␀alice␀alice-secret")→CLOSED ; WHO→CLOSED
- `versions-first`: APIV→0 75apis ; HS(PLAIN)→0 ; AUTH("␀alice␀alice-secret")→0 ; WHO→alice=0,bob=29 ; APIV→0 75apis ; WHO→alice=0,bob=29
- `versions-before-token`: HS(PLAIN)→0 ; APIV→34 noapis ; AUTH("␀alice␀alice-secret")→CLOSED ; WHO→CLOSED
- `versions-twice`: APIV→0 75apis ; APIV→CLOSED ; HS(PLAIN)→0 ; AUTH("␀alice␀alice-secret")→0 ; WHO→alice=0,bob=29
- `handshake-twice`: HS(PLAIN)→0 ; HS(PLAIN)→34 mechs=null ; AUTH("␀alice␀alice-secret")→CLOSED ; WHO→CLOSED
- `twice`: HS(PLAIN)→0 ; AUTH("␀alice␀alice-secret")→0 ; WHO→alice=0,bob=29 ; AUTH("␀alice␀alice-secret")→34 AFTER_OK ; WHO→alice=0,bob=29 ; HS(PLAIN)→34 mechs=null ; AUTH("␀bob␀bob-secret")→34 AFTER_OK ; WHO→alice=0,bob=29 ; AUTH("␀alice␀nope")→34 AFTER_OK ; WHO→alice=0,bob=29 ; HS(BOGUS)→34 mechs=null ; WHO→alice=0,bob=29
- `authenticate-v0`: HS(PLAIN)→0 ; AUTH/v0("␀alice␀alice-secret")→0 ; WHO→alice=0,bob=29 ; AUTH/v0("␀alice␀alice-secret")→34 AFTER_OK
- `authenticate-v0-wrong`: HS(PLAIN)→0 ; AUTH/v0("␀alice␀nope")→58 PLAIN_BAD ; WHO→CLOSED
- `authenticate-v1`: HS(PLAIN)→0 ; AUTH/v1("␀alice␀alice-secret")→0 ; WHO→alice=0,bob=29 ; AUTH/v1("␀alice␀alice-secret")→34 AFTER_OK
- `authenticate-v1-wrong`: HS(PLAIN)→0 ; AUTH/v1("␀alice␀nope")→58 PLAIN_BAD ; WHO→CLOSED

Counts (from the scan): 13 steps with code 29 (all `WHO` probes), 23 with 58, 5 with 33, 11 with 34.

Note `not-utf8` is the token bytes `00 'alice' 00 ff fe fd` (recorded as `base64:AGFsaWNlAP/+/Q==`); `token-of-64k` is `␀alice␀` plus 65,536 `x`.

### A2.2 `sasl-scram` step list (same fixture; 69 connections)

Every `-256` and `-512` connection is the same handshake/answers with the other mechanism name in `HS(...)`. Where no suffix is shown the connection is on SCRAM-SHA-256.

- `good-256`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,r=NC.SN,p=<proof:alice-secret>")→0 bytes="v=<server-signature>" ; WHO→alice=0,bob=29
- `wrong-password-256`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,r=NC.SN,p=<proof:nope>")→58 SCRAM_BAD ; WHO→CLOSED
- `other-users-password-256`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,r=NC.SN,p=<proof:bob-secret>")→58 SCRAM_BAD ; WHO→CLOSED
- `unknown-user-256`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=mallory,r=NC.")→58 SCRAM_BAD ; AUTH("c=biws,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `authzid-same-256`: HS(SCRAM-SHA-256)→0 ; AUTH("n,a=alice,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=bixhPWFsaWNlLA==,r=NC.SN,p=<proof:alice-secret>")→0 bytes="v=<server-signature>" ; WHO→alice=0,bob=29
- `authzid-other-user-256`: HS(SCRAM-SHA-256)→0 ; AUTH("n,a=bob,n=alice,r=NC.")→58 AUTHZID ; AUTH("c=bixhPWJvYiw=,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `authzid-admin-256`: HS(SCRAM-SHA-256)→0 ; AUTH("n,a=admin,n=alice,r=NC.")→58 AUTHZID ; AUTH("c=bixhPWFkbWluLA==,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `authzid-other-user-unknown-user-256`: HS(SCRAM-SHA-256)→0 ; AUTH("n,a=bob,n=mallory,r=NC.")→58 SCRAM_BAD ; WHO→CLOSED
- `good-512`: HS(SCRAM-SHA-512)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,r=NC.SN,p=<proof:alice-secret>")→0 bytes="v=<server-signature>" ; WHO→alice=0,bob=29
- `wrong-password-512`: HS(SCRAM-SHA-512)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,r=NC.SN,p=<proof:nope>")→58 SCRAM_BAD ; WHO→CLOSED
- `other-users-password-512`: HS(SCRAM-SHA-512)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,r=NC.SN,p=<proof:bob-secret>")→58 SCRAM_BAD ; WHO→CLOSED
- `unknown-user-512`: HS(SCRAM-SHA-512)→0 ; AUTH("n,,n=mallory,r=NC.")→58 SCRAM_BAD ; AUTH("c=biws,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `authzid-same-512`: HS(SCRAM-SHA-512)→0 ; AUTH("n,a=alice,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=bixhPWFsaWNlLA==,r=NC.SN,p=<proof:alice-secret>")→0 bytes="v=<server-signature>" ; WHO→alice=0,bob=29
- `authzid-other-user-512`: HS(SCRAM-SHA-512)→0 ; AUTH("n,a=bob,n=alice,r=NC.")→58 AUTHZID ; AUTH("c=bixhPWJvYiw=,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `authzid-admin-512`: HS(SCRAM-SHA-512)→0 ; AUTH("n,a=admin,n=alice,r=NC.")→58 AUTHZID ; AUTH("c=bixhPWFkbWluLA==,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `authzid-other-user-unknown-user-512`: HS(SCRAM-SHA-512)→0 ; AUTH("n,a=bob,n=mallory,r=NC.")→58 SCRAM_BAD ; WHO→CLOSED
- `first-empty`: HS(SCRAM-SHA-256)→0 ; AUTH("")→58 SCRAM_BAD ; AUTH("c=,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `first-garbage`: HS(SCRAM-SHA-256)→0 ; AUTH("this is not a scram message")→58 SCRAM_BAD ; AUTH("c=,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `first-no-gs2-header`: HS(SCRAM-SHA-256)→0 ; AUTH("n=alice,r=NC.")→58 SCRAM_BAD ; AUTH("c=,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `first-no-nonce`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice")→58 SCRAM_BAD ; AUTH("c=biws,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `first-empty-nonce`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=")→58 SCRAM_BAD ; AUTH("c=biws,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `first-no-user`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,r=NC.")→58 SCRAM_BAD ; AUTH("c=biws,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `first-empty-user`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=,r=NC.")→58 SCRAM_BAD ; AUTH("c=biws,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `first-nonce-before-user`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,r=NC.,n=alice")→58 SCRAM_BAD ; AUTH("c=biws,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `first-channel-binding-required`: HS(SCRAM-SHA-256)→0 ; AUTH("p=tls-unique,,n=alice,r=NC.")→58 SCRAM_BAD ; AUTH("c=cD10bHMtdW5pcXVlLCw=,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `first-channel-binding-y`: HS(SCRAM-SHA-256)→0 ; AUTH("y,,n=alice,r=NC.")→58 SCRAM_BAD ; AUTH("c=eSws,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `first-unknown-gs2-flag`: HS(SCRAM-SHA-256)→0 ; AUTH("x,,n=alice,r=NC.")→58 SCRAM_BAD ; AUTH("c=eCws,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `first-mandatory-extension`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,m=ext,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,r=NC.SN,p=<proof:alice-secret>")→58 SCRAM_BAD ; WHO→CLOSED
- `first-optional-extension`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.,x=ext")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,r=NC.SN,p=<proof:alice-secret>")→0 bytes="v=<server-signature>" ; WHO→alice=0,bob=29
- `first-user-bad-escape`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=al=ice,r=NC.")→58 SCRAM_BAD ; AUTH("c=biws,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `first-user-escaped`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=al=2Cice,r=NC.")→58 SCRAM_BAD ; AUTH("c=biws,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `first-user-escaped-equals-sign-as-alice`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alic=65,r=NC.")→58 SCRAM_BAD ; AUTH("c=biws,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `first-nonce-with-comma`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.,,")→58 SCRAM_BAD ; AUTH("c=biws,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `first-not-utf8`: HS(SCRAM-SHA-256)→0 ; AUTH("base64:biwsbj1hbGljZSxyPW5vbmNlLW9mLWZpcnN0LW5vdC11dGY4Lv8=")→58 SCRAM_BAD ; AUTH("c=biws,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `first-plain-token`: HS(SCRAM-SHA-256)→0 ; AUTH("␀alice␀alice-secret")→58 SCRAM_BAD ; AUTH("c=,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `final-empty`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("")→58 SCRAM_BAD ; WHO→CLOSED
- `final-garbage`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("this is not a scram message")→58 SCRAM_BAD ; WHO→CLOSED
- `final-no-proof`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,r=NC.SN")→58 SCRAM_BAD ; WHO→CLOSED
- `final-proof-not-base64`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,r=NC.SN,p=!!!not-base64!!!")→58 SCRAM_BAD ; WHO→CLOSED
- `final-proof-too-short`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,r=NC.SN,p=AAAA")→CLOSED ; WHO→CLOSED
- `final-proof-empty`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,r=NC.SN,p=")→CLOSED ; WHO→CLOSED
- `final-no-nonce`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,p=<proof:alice-secret>")→58 SCRAM_BAD ; WHO→CLOSED
- `final-no-channel-binding`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("r=NC.SN,p=<proof:alice-secret>")→58 SCRAM_BAD ; WHO→CLOSED
- `final-wrong-channel-binding`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=eSws,r=NC.SN,p=<proof:alice-secret>")→0 bytes="v=<server-signature>" ; WHO→alice=0,bob=29
- `final-channel-binding-not-base64`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=!!!,r=NC.SN,p=<proof:alice-secret>")→58 SCRAM_BAD ; WHO→CLOSED
- `final-wrong-nonce`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,r=NC.not-the-servers-half,p=<proof:alice-secret>")→58 SCRAM_BAD ; WHO→CLOSED
- `final-client-nonce-only`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,r=NC.,p=<proof:alice-secret>")→58 SCRAM_BAD ; WHO→CLOSED
- `final-nonce-with-suffix`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,r=NC.SNx,p=<proof:alice-secret>")→58 SCRAM_BAD ; WHO→CLOSED
- `final-extension-before-proof`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,r=NC.SN,x=ext,p=<proof:alice-secret>")→58 SCRAM_BAD ; WHO→CLOSED
- `final-trailing-attribute`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,r=NC.SN,p=<proof:alice-secret>,x=ext")→58 SCRAM_BAD ; WHO→CLOSED
- `replay`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,r=NC.SN,p=<proof:alice-secret>")→0 bytes="v=<server-signature>" ; WHO→alice=0,bob=29 ; AUTH("c=biws,r=NC.SN,p=<proof:alice-secret>")→34 AFTER_OK ; WHO→alice=0,bob=29
- `replay-elsewhere`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,r=NC.SN,p=<proof:alice-secret>")→58 SCRAM_BAD ; WHO→CLOSED
- `reauth`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,r=NC.SN,p=<proof:alice-secret>")→0 bytes="v=<server-signature>" ; WHO→alice=0,bob=29 ; AUTH("n,,n=bob,r=NC.")→34 AFTER_OK ; AUTH("c=biws,r=NC.,p=<zero-proof>")→34 AFTER_OK ; WHO→alice=0,bob=29 ; HS(SCRAM-SHA-256)→34 mechs=null ; AUTH("n,,n=bob,r=NC.")→34 AFTER_OK ; WHO→alice=0,bob=29 ; HS(PLAIN)→34 mechs=null ; AUTH("␀bob␀bob-secret")→34 AFTER_OK ; WHO→alice=0,bob=29
- `switch-to-plain`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; HS(PLAIN)→34 mechs=null ; AUTH("␀bob␀bob-secret")→CLOSED ; WHO→CLOSED
- `switch-to-512`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; HS(SCRAM-SHA-512)→34 mechs=null ; AUTH("c=biws,r=NC.SN,p=<proof:alice-secret>")→CLOSED ; WHO→CLOSED
- `handshake-256-then-512`: HS(SCRAM-SHA-256)→0 ; HS(SCRAM-SHA-512)→34 mechs=null ; AUTH("n,,n=alice,r=NC.")→CLOSED ; AUTH("c=biws,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `proof-of-512-under-256`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,r=NC.SN,p=<proof:alice-secret>")→CLOSED ; WHO→CLOSED
- `plain-token-as-final`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("␀alice␀alice-secret")→58 SCRAM_BAD ; WHO→CLOSED
- `request-before-final`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; WHO→alice=34,bob=34 nobrokers ; WHO→CLOSED ; AUTH("c=biws,r=NC.SN,p=<proof:alice-secret>")→CLOSED ; WHO→CLOSED
- `request-before-first`: HS(SCRAM-SHA-256)→0 ; WHO→alice=34,bob=34 nobrokers ; AUTH("n,,n=alice,r=NC.")→CLOSED ; AUTH("c=biws,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `versions-before-final`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; APIV→34 noapis ; AUTH("c=biws,r=NC.SN,p=<proof:alice-secret>")→CLOSED ; WHO→CLOSED
- `final-without-first`: HS(SCRAM-SHA-256)→0 ; AUTH("c=biws,r=NC.,p=<zero-proof>")→58 SCRAM_BAD ; WHO→CLOSED
- `first-twice`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("n,,n=alice,r=NC.")→58 SCRAM_BAD ; AUTH("c=biws,r=NC.,p=<zero-proof>")→CLOSED ; WHO→CLOSED
- `scram-no-handshake`: AUTH("n,,n=alice,r=NC0")→CLOSED ; WHO→CLOSED
- `third-message`: HS(SCRAM-SHA-256)→0 ; AUTH("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH("c=biws,r=NC.SN,p=<proof:alice-secret>")→0 bytes="v=<server-signature>" ; AUTH(null)→34 AFTER_OK ; WHO→alice=0,bob=29
- `scram-authenticate-v0`: HS(SCRAM-SHA-256)→0 ; AUTH/v0("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH/v0("c=biws,r=NC.SN,p=<proof:alice-secret>")→0 bytes="v=<server-signature>" ; WHO→alice=0,bob=29
- `scram-authenticate-v0-wrong`: HS(SCRAM-SHA-256)→0 ; AUTH/v0("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH/v0("c=biws,r=NC.SN,p=<proof:nope>")→58 SCRAM_BAD ; WHO→CLOSED
- `scram-authenticate-v1`: HS(SCRAM-SHA-256)→0 ; AUTH/v1("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH/v1("c=biws,r=NC.SN,p=<proof:alice-secret>")→0 bytes="v=<server-signature>" ; WHO→alice=0,bob=29
- `scram-authenticate-v1-wrong`: HS(SCRAM-SHA-256)→0 ; AUTH/v1("n,,n=alice,r=NC.")→0 bytes="r=NC.SN,s=<salt>,i=4096" ; AUTH/v1("c=biws,r=NC.SN,p=<proof:nope>")→58 SCRAM_BAD ; WHO→CLOSED

### A2.3 Behaviour derived from the two scenarios (the checks that must hold; claim ids from `claims/auth.md`)

| Rule | Evidence step(s) |
|---|---|
| PLAIN token = `authzid NUL authcid NUL password`; exactly 3 NUL-separated parts or `TOKENS_n` | `no-nuls` (1), `two-parts` (2), `four-parts` / `trailing-nul` (4), `empty-token` (1) |
| Check order: parts count, empty user (`NO_USER`; both empty gives `NO_USER`), empty password (`NO_PASS`), password (`PLAIN_BAD`, same text for unknown user, wrong password, other user's, prefix, longer, capitalised user, non-UTF-8), **then** authzid (`AUTHZID` only if the password was right) | `authzid-other-user-wrong-password` = `PLAIN_BAD`, `-empty-password` = `NO_PASS` |
| Authzid empty or equal to the user: success as the user | `authzid-same` → `alice=0,bob=29` |
| A failed login (58) is answered, **then the connection is closed** (next request gets nothing) | every `...-who` after 58 is `CLOSED` |
| Success answer: ErrorCode 0, `ErrorMessage ""` (not null), `SASLAuthBytes ""`, `SessionLifetimeMillis 0`, at v0/v1/v2 | `good`, `authenticate-v0`, `authenticate-v1` |
| A mechanism not offered (`BOGUS`, `plain` lower case, `""`, `SCRAM-SHA-256` on a PLAIN-only listener): HS answer 33 + the list, **then closed** (the following HS(PLAIN) and AUTH get no answer) | `unknown-mechanism`, `lowercase-mechanism`, `empty-mechanism`, `scram-not-enabled`, `refused-then-right` |
| Before the SaslHandshake: one ApiVersions is answered (full list), nothing else is: `AUTH` without HS, any other request, a second ApiVersions are closed unanswered | `no-handshake`, `no-login`, `versions-first`, `versions-twice` |
| ApiVersions after login is answered normally | `versions-first`: `APIV→0 75apis` after the login |
| Between HS and the end of the login: a second HS gets 34 + `mechs=null` **and the connection is closed**; ApiVersions gets 34 with `ApiKeys null` (error shape only) **and closes**; Metadata gets the error in its own shape (each topic 34, no brokers) **and closes** | `handshake-twice`, `versions-before-token`, `request-before-token` / `request-before-final` / `request-before-first` |
| After a successful login: `AUTH` (same token, other user's valid token, wrong password, empty/null token) → 34 `AFTER_OK`; `HS` (same, other, unknown mechanism) → 34 `mechs=null`; connection stays open and the principal never changes | `twice`, `authenticate-v0/v1` `-again`, `reauth`, `third-message` |
| SCRAM: `HS` → 0 with `["PLAIN","SCRAM-SHA-512","SCRAM-SHA-256"]`; client-first `n,,n=user,r=cn` → 0 with `r=cn+sn,s=<salt>,i=4096`; client-final `c=biws,r=cn+sn,p=<proof>` → 0 with `v=<server-signature>` | `good-256`, `good-512`, v0/v1 variants |
| SCRAM wrong proof (wrong or another user's password), unknown user: 58 `SCRAM_BAD`. **Unknown user is rejected at client-first** (the answer to client-first is 58), a wrong password at client-final; connection closed after | `unknown-user-*`, `wrong-password-*`, `other-users-password-*` |
| SCRAM authzid `a=`: own name OK (`c=bixhPWFsaWNlLA==` = base64 `n,a=alice,`); another's or admin's → 58 `AUTHZID` **at client-first** (only after the user lookup succeeded; unknown user with authzid → `SCRAM_BAD`) | `authzid-*-256/512` |
| Client-first that is not valid → 58 `SCRAM_BAD` and closed: empty, garbage, no GS2 header, no `r`, empty `r`, no user, empty user (`n=`), `r` before `n`, `p=tls-unique,,`, `y,,`, `x,,`, bad escape `n=al=ice`, escape `n=al=2Cice`, `n=alic=65`, nonce with `,,` after, non-UTF-8, a PLAIN token. Only the `n` flag is taken | `first-*` (19 connections; 17 refused, 2 accepted: see next row) |
| Client-first `m=ext,n=alice,...` (reserved/mandatory extension) is **accepted** (server-first sent) then the proof fails (58) because Kafka does not sign it; `x=ext` after the nonce is accepted **and signed** (login succeeds) | `first-mandatory-extension`, `first-optional-extension` |
| Client-final not valid → 58 and closed: empty, garbage, no proof, proof not base64, no nonce, no `c=`, `c=!!!` not base64, wrong nonce, client nonce only, nonce + `x`, `,x=ext` before `p=`, `,x=ext` after `p=`; a PLAIN token as final; a final with no first; a second client-first | `final-*`, `plain-token-as-final`, `final-without-first`, `first-twice` |
| Client-final whose proof is base64 of the **wrong length** (`AAAA`, empty, the 512 proof under 256) → **no answer, connection closed** | `final-proof-too-short`, `final-proof-empty`, `proof-of-512-under-256` |
| `c=eSws` (base64 `y,,`) after an `n,,` first, with a valid proof, **logs the connection in** (Kafka does not compare the binding) | `final-wrong-channel-binding` → `0`, `WHO alice=0,bob=29` (Go refuses: Known) |
| Replay of a client-final: same connection → 34 `AFTER_OK`; on another connection that sent its own client-first → 58 `SCRAM_BAD` and closed | `replay`, `replay-elsewhere` |
| Mechanism switch in the middle (`switch-to-plain`, `switch-to-512`, `handshake-256-then-512`): second HS → 34 `mechs=null`, **closed** | those three |
| Client nonce is echoed first in `r=`; server nonce must be a non-empty suffix; server-first has exactly `r=,s=,i=` in that order | normalizer expects it (`normalize.go:467-510`) |

### A2.4 The Go `Known` differences for these two scenarios (steps where the Rust edition may differ without failing)

`sasl-plain`: `request-before-token-who` (Go closes instead of the Metadata-shaped 34 answer), `versions-first-versions`,
`versions-first-versions-after-login`, `versions-twice-first` (path `ApiKeys`: the advertised API list is the Rust table's, not Kafka's 75).
`sasl-scram`: `request-before-final-who`, `request-before-first-who` (same close-instead-of-answer), `final-wrong-channel-binding-client-final` and
`-who` (Go refuses the mismatched binding; Kafka logs in). Note `versions-before-token-versions` and `versions-before-final-versions` (APIV→34 noapis) and
`request-before-token-who-again` are *not* Known: Go answers these as Kafka does, so Rust must too (or the Known list must grow, which is a
change to the Go branch of the oracle).

## A3. `acl-unknown-codes` (77 steps)

**Purpose** (`claims/api.md` V5): what a super user gets from DescribeACLs / DeleteACLs / CreateACLs (all at **v3**) when any coded field (resource type,
pattern type, operation, permission) is 0 (UNKNOWN), a number that is no code at all (99 or -1), or (create only) ANY / MATCH. Each step is followed by
`<step>-then`: a **Metadata v9 with no topics** on the same (re-opened) `admin` connection, to prove the broker is still there; it always answers 0 with
1 broker. Configuration: as `sasl-plain` (PLAIN, deny, super user admin); the script connects as `admin` (a super user). One first step
`create-topic` (CreateTopics v7, topic `acl`, 1 partition) answers as in the fixture.

Codes are written in the script (`scenarios_refusals.go:138-196`, `aclUnknownCodes`) as (resource, pattern, operation, permission). **The recording prints 99 and -1 as
`UNKNOWN`** (kmsg's enum string), so the JSON cannot tell 0 from 99; the numbers below come from the Go source.
Wire values: ResourceType ANY 1, TOPIC 2, GROUP 3, CLUSTER 4, TRANSACTIONAL_ID 5, DELEGATION_TOKEN 6, USER 7; Pattern ANY 1, MATCH 2, LITERAL 3, PREFIXED 4;
Operation ANY 1, ALL 2, READ 3, WRITE 4, CREATE 5, DELETE 6, ALTER 7, DESCRIBE 8, CLUSTER_ACTION 9, DESCRIBE_CONFIGS 10, ALTER_CONFIGS 11,
IDEMPOTENT_WRITE 12, CREATE_TOKENS 13, DESCRIBE_TOKENS 14; Permission ANY 1, DENY 2, ALLOW 3. There are no ACLs in the store when the filter steps run.

### A3.1 Filter steps (DescribeACLs v3 request: ResourceType, Pattern, Operation, Permission, name/principal/host null; DeleteACLs v3 request: one filter, same)

| Tag | (res, pat, op, perm) | DescribeACLs response | DeleteACLs response |
|---|---|---|---|
| `any` | 1,1,1,1 | `ErrorCode 0, ErrorMessage "", Resources null (empty), Throttle 0` | `Results:[{0, ErrorMessage null, MatchingACLs null}]` |
| `topic-literal` | 2,3,1,1 | same as `any` | same as `any` |
| `resource-0` | 0,3,1,1 | **CLOSED** | **CLOSED** |
| `pattern-0` | 2,0,1,1 | **CLOSED** | **CLOSED** |
| `operation-0` | 2,3,0,1 | **CLOSED** | **CLOSED** |
| `permission-0` | 2,3,1,0 | **CLOSED** | **CLOSED** |
| `all-0` | 0,0,0,0 | **CLOSED** | **CLOSED** |
| `resource-99` | 99,3,1,1 | `0, "", Resources null` | `Results:[{ErrorCode 42, ErrorMessage "Unknown patternFilter.", MatchingACLs null}]` |
| `pattern-99` | 2,99,1,1 | `0, "", null` | `42 "Unknown patternFilter."` |
| `operation-99` | 2,3,99,1 | `0, "", null` | `42 "Unknown entryFilter."` |
| `permission-99` | 2,3,1,99 | `0, "", null` | `42 "Unknown entryFilter."` |
| `resource-negative` | -1,3,1,1 | `0, "", null` | `42 "Unknown patternFilter."` |

(DescribeACLs response `ErrorMessage` is the **empty string**, not null, in all non-closed rows.) `-then` Metadata: 0 in every row, and in the CLOSED rows the
harness has reconnected and logged in again as admin.

### A3.2 Create steps (CreateACLs v3, one creation: `TOPIC? res`, name `acl`, principal `User:alice`, host `*`)

Tuple is (resource, pattern, operation, permission); the unchanged fields are the TOPIC / LITERAL / READ / ALLOW of `good` (2,3,3,3).

| Tag | Tuple | Response |
|---|---|---|
| `good` | 2,3,3,3 | `Results:[{0, ErrorMessage ""}]` (the ACL is stored: alice READ topic `acl`) |
| `resource-0`, `pattern-0`, `operation-0`, `permission-0` | 0,3,3,3 / 2,0,3,3 / 2,3,0,3 / 2,3,3,0 | **CLOSED** (no ACL created) |
| `resource-any` | 1,3,3,3 | `{ErrorCode -1 (UNKNOWN_SERVER_ERROR), ErrorMessage null}` |
| `pattern-any`, `pattern-match` | 2,1,3,3 / 2,2,3,3 | `{-1, null}` |
| `operation-any` | 2,3,1,3 | `{-1, null}` |
| `permission-any` | 2,3,3,1 | `{-1, null}` |
| `resource-99` | 99,3,3,3 | `{42 INVALID_REQUEST, "Invalid resourceType UNKNOWN"}` |
| `pattern-99` | 2,99,3,3 | `{42, "Invalid patternType UNKNOWN"}` |
| `operation-99` | 2,3,99,3 | `{42, "Invalid operation UNKNOWN"}` |
| `permission-99` | 2,3,3,99 | `{42, "Invalid permissionType UNKNOWN"}` |

All responses carry `ThrottleMillis 0`. Not recorded: a request with several creations/filters of which only one is unknown (Go closes the whole
request if any is 0 before authorizing); an unknown code of 8+ for resource type or 5+ for pattern (Go treats resource 1..7, pattern 1..4, operation 1..14,
permission 1..3 as codes).

## A4. `idempotent-write-from-topic-write` (4 steps)

**Purpose** (`claims/api.md` V8; KIP-679): with an authorizer, a principal with Write on a topic (and nothing on the cluster) may InitProducerId with no
transactional id; a principal with no ACL may not. Config as `sasl-plain` (deny, super user admin). Connections `admin`, `alice`, `bob`, all logged in with PLAIN
at open (before any ACL exists).

| Step | Conn | Request | Recorded response |
|---|---|---|---|
| `create` | admin | CreateTopics v7, topic `iw`, 1 partition | topic created, error 0 |
| `allow-alice-write-iw` | admin | CreateACLs v3: `{TOPIC, "iw", LITERAL, User:alice, "*", WRITE, ALLOW}` | `Results:[{0, ""}]` |
| (await, unrecorded) | alice | InitProducerId v4 repeated until ErrorCode 0 (Kafka applies the ACL asynchronously) | |
| `alice-init-producer-id` | alice | InitProducerId v4 `{TransactionalID null, TransactionTimeoutMillis 60000, ProducerID -1, ProducerEpoch -1}` | `{ErrorCode 0, ProducerEpoch 0, ProducerID <producer:1>, Throttle 0}` |
| `bob-init-producer-id` | bob | same request | `{ErrorCode 31 CLUSTER_AUTHORIZATION_FAILED, ProducerEpoch -1, ProducerID -1, Throttle 0}` |

Kafka's rule (`KafkaApis.handleInitProducerIdRequest`, not in the recording): null transactional id needs `IdempotentWrite` on the cluster **or** Write on any topic
(`authorizeByResourceType`); with a transactional id it needs Write on that transactional id (53 on refusal). Only the first path is recorded.
Debt note (`docs/port/debt.md:95`): ACLs on the transaction APIs (TRANSACTIONAL_ID_AUTHORIZATION_FAILED, GROUP_AUTHORIZATION_FAILED on TxnOffsetCommit,
TOPIC_AUTHORIZATION_FAILED on AddPartitionsToTxn) were carried to Phase 8 with **no recording** to judge them by.

## A5. `version-matrix`, ACL rows (9 of 177 steps)

Plain broker, defaults (no SASL, no authorizer configured by the harness, `--acl-default allow`). Each request is the API's default
request at the version (`MatrixRequest`): all fields zero/default except the few listed in the Go source. The ACL rows:

| Step | Request | Recorded response |
|---|---|---|
| `DescribeACLs v1`, `v2`, `v3` | `{ResourceType 0 UNKNOWN, ResourceName null, PatternType 3 LITERAL, Principal null, Host null, Operation 0 UNKNOWN, PermissionType 0 UNKNOWN}` | **CLOSED** (UNKNOWN code; same rule as A3) |
| `CreateACLs v1`, `v2`, `v3` | `{Creations: []}` | `{Results: [] (null in JSON), ThrottleMillis 0}` |
| `DeleteACLs v1`, `v2`, `v3` | `{Filters: []}` | `{Results: [] , ThrottleMillis 0}` |

Surprise: the three DescribeACLs rows **already pass today** only because the Rust broker answers `Outcome::Close` to every unserved API; they will fail the day a
DescribeAcls handler lands unless it keeps the "UNKNOWN code closes the connection" rule. CreateACLs/DeleteACLs with an empty list are answered with an empty
list (`ThrottleMillis 0`). Note this means that on a
broker with **no authorizer** an empty request is answered normally. What a real Kafka without an authorizer answers to a *non-empty* CreateAcls
(SECURITY_DISABLED 54 per creation) and DescribeAcls is **not recorded**.

## A6. `telemetry-acl` (Phase 9, listed because it fixes a Phase 8 rule)

User `alice` (no ACLs, authorizer `deny`): `metadata-refused` = Metadata v9 for `acl-topic` → topic error **29**; then `subscribe-without-acls` =
GetTelemetrySubscriptions v0 with zero client instance id → `ErrorCode 0` (new `ClientInstanceID`, `PushIntervalMillis 300000` in Kafka, `AcceptedCompressionTypes [4,3,1,2]`,
`DeltaTemporality true`, `TelemetryMaxBytes 1048576`) and `push-without-acls` = PushTelemetry v0 → `ErrorCode 0`. Rule: the two telemetry APIs need **no ACL**.
The two `telemetry-instance-binding*` scenarios need only PLAIN login for three users (no authorizer).

## A7. What the recordings do not cover (Phase 8 will need decisions or new recordings)

TLS in any form; SASL over TLS; SaslHandshake v0 and unframed tokens; OAUTHBEARER/GSSAPI; SASL_SSL principal from a client certificate; re-authentication;
`allow.everyone.if.no.acl.found` (the `acl-default allow` flag); wildcard/PREFIXED matching and DENY precedence (Describe/Create/Delete results with real
entries beyond `fixture-acls`, the `acl` topic entry, and the two-resource Await); any authorization **denial** other than Metadata-topic 29 and
InitProducerId 31; DescribeACLs / DeleteACLs with entries (their response shape `Resources[{ResourceType,ResourceName,PatternType,ACLs[...]}]`,
`MatchingACLs`); ACL checks for any handler (Produce, Fetch, groups, txns, configs, CreateTopics...); a non-super-user's CreateAcls/DescribeAcls
(CLUSTER_AUTHORIZATION_FAILED); SECURITY_DISABLED; an unauthenticated connection's ApiVersions at an unsupported version (v5) before the handshake;
SaslHandshake/SaslAuthenticate on a listener with SASL off; PLAIN with SCRAM-only listener; passwords with non-ASCII (SASLprep); SCRAM with iteration
counts other than 4096; a DescribeUserScramCredentials / AlterUserScramCredentials answer.

---

# Part B. The Go edition (shipped doze-kafka): facts, not a template

Source: `~/Code/doze-dev/doze-kafka`, `docs/auth-and-tls.md`, `docs/authorization.md`, `docs/limitations.md`, `internal/{broker,protocol,config,hostguard}`.

## B1. Users and credentials

| Item | Go behaviour | Where |
|---|---|---|
| Creating users | flags `--sasl-user user:password` (repeatable) or the config file `[credentials]` table (`alice = "s3cret"`); needs `--sasl-mechanisms` (and the reverse: mechanisms without users is an error). No API creates users. | `cmd/doze-kafka/cli.go:276-279,394-395`; `internal/config/config.go:467-487`; `doze-kafka.example.toml:99-113` |
| Storage | **None.** Read from configuration at every start. PLAIN keeps the plaintext password in memory (constant-time compare); SCRAM keeps only a derived verifier (salt 16 random bytes made at start, StoredKey, ServerKey, **4096 iterations**), derived once at start, per mechanism, per user. Nothing on disk; metadata DB is `0600`. | `internal/broker/auth.go:23-62`, `scram.go:19-72` |
| Mechanisms | `PLAIN`, `SCRAM-SHA-256`, `SCRAM-SHA-512`, listed in the configured order in SaslHandshake. SaslHandshake served at v1 only; SaslAuthenticate v0-v2; both advertised only when SASL is on. No GSSAPI, OAUTHBEARER, delegation tokens, SASLprep. | `internal/protocol/versions.go:112,127`, `limitations.md:285-299` |
| Not served | DescribeUserScramCredentials (50), AlterUserScramCredentials (51), delegation-token APIs are not in the Go API table (only recognised as credential-carrying for log/inspector redaction). | `internal/protocol/authapis.go` |
| Principal | `User:<name>`; `User:ANONYMOUS` when SASL is off. One login per connection, for life of the connection. The authzid must be empty or the user. | `internal/protocol/conn.go:113-124`, `broker/auth.go` |
| Pre-auth gating | `authRequired && !authenticated`: only ApiVersions (once), then SaslHandshake, then SaslAuthenticate; frames capped at 1 MiB; pre-auth budget **30 s from accept** (not reset per request); after auth idle limit 10 min. A first SCRAM message over 4096 bytes is refused. | `conn.go:28` (30 s), `:245-330` (preAuth gate), `:330-400` (read loop), `frame.go:14-22`, `limitations.md:112` |
| Logging | passwords/tokens never logged (`saslRefusal.why` never contains client input); a redaction suite (`cmd/doze-kafka/secrets*_test.go`); inspector shows `"AuthBytes": "(hidden)"`, `0 B in` | `docs/auth-and-tls.md:"Where a credential is never shown"` |
| Console | when SASL is on, the console logs in as `__console` with a random per-start password; it is always a super user | `docs/authorization.md:"Settings"` |

## B2. ACLs and the authorizer

- **Store**: bbolt bucket `acls` in the metadata DB: key = the seven fields joined by NUL, value JSON `{resource_type,name,pattern_type,principal,host,operation,permission}` (wire integers kept so a client gets back exactly what it sent); duplicates ignored. `internal/storage/meta/acls.go`. (A stale doc comment there still says "NOT enforced".) In memory: `atomic.Pointer[[]meta.ACL]`, replaced after every Create/Delete under a mutex (`broker/acls.go:165` refreshAuthorizer, handlers at `:184`, `:239`, `:300`); with no ACLs and default allow a check is one atomic load.
- **Resource types enforced**: TOPIC (2), GROUP (3), CLUSTER (4, one name `kafka-cluster`), TRANSACTIONAL_ID (5, `canTxnID`, `broker/txnapis.go:23`). Not enforced: DELEGATION_TOKEN, USER (storable but meaningless). Patterns: LITERAL (`*` = every resource of the type), PREFIXED; MATCH/ANY are filter values only. Principals: `User:` names and `User:*`; hosts matched as written (`*` or exact string, no CIDR); the connection's host is its remote IP.
- **Rules** (in order): super user (`--acl-super-user`, exact principal match) never refused -> any matching DENY refuses (a DENY is exact: DENY Read does not deny Describe; DENY All denies all) -> any matching ALLOW permits (ALL grants everything; Read/Write/Delete/Alter imply Describe; AlterConfigs implies DescribeConfigs) -> if no ACL at all is about the resource, `--acl-default` decides (`allow`, the default; or `deny`, Kafka's) -> else refused. `broker/authorizer.go:110` (`allowed`).
- **Flags**: `--acl-default allow|deny` (default `allow`), `--acl-super-user User:admin` (repeatable / comma list; `Type:name` form required). File: `acl-default`, `acl-super-user`. DescribeConfigs on the broker reports `allow.everyone.if.no.acl.found` and `super.users`.
- **CRUD**: CreateAcls/DeleteAcls need Alter on the cluster, DescribeAcls Describe on the cluster (`CLUSTER_AUTHORIZATION_FAILED`, 31, on every result, after the UNKNOWN-code check); Go's `validateCreation` adds its own messages for an empty name/host, a principal without `Type:`, NUL bytes and a cluster resource not named `kafka-cluster` (not in any recording). A filter that is invalid (code outside the enum) matches nothing in Describe and is `Unknown patternFilter.`/`Unknown entryFilter.` in Delete; a code of 0 anywhere closes the connection (`errUnknownACLCode`). DescribeAcls results are grouped by (type, name, pattern) and sorted by type, name, pattern.
- **Which APIs check what** (from `docs/authorization.md` and the `b.can*` call sites, about 60 in `internal/broker`): see C6 where this is mapped to the Rust handler files. A refusal is the API's own code (29 / 30 / 31 / 53). `ApiVersions`, `SaslHandshake`, `SaslAuthenticate`, GetTelemetrySubscriptions and PushTelemetry need nothing.
- **Known differences documented by Go** (`docs/limitations.md`): principals are `User:` only; hosts matched as written; no quotas; no `SASLprep`; `SaslHandshake` v0 closes; a request in the middle of a login closes unanswered (Kafka answers in the request's own shape: Known steps); SCRAM channel-binding mismatch refused (Kafka accepts: Known steps); a first SCRAM message over 4096 bytes refused; the schema registry authenticates but does not authorize. Go enforces ACLs even with no SASL (anonymous principal); real Kafka with no authorizer answers SECURITY_DISABLED to CreateAcls/DescribeAcls (Go stores and serves them).

## B3. TLS

- Flags `--tls-cert FILE --tls-key FILE` (both or neither), file `[tls] cert=, key=`. `tls.LoadX509KeyPair`, **server certificate only, no client authentication, `MinVersion: TLS 1.2`**, wrapped around the Kafka listener with `tls.NewListener` (and the schema-registry listener, with the same certificate, unless `--schema-registry-tls=false`; the console is always plain HTTP). A bad cert/key is reported at start-up before anything else. TLS and SASL are independent (PLAINTEXT, SSL, SASL_PLAINTEXT, SASL_SSL all work). `internal/config/config.go:610-624`, `internal/protocol/server.go:66,87,145`.
- Local dev certificates: not generated by the binary; the docs give a two-step `openssl ecparam ... -genkey` + `openssl req -x509 -addext subjectAltName=...` (LibreSSL-safe); the compat matrix makes an RSA-2048 self-signed cert (`CN=localhost`, SAN `localhost`, `host.docker.internal`, `127.0.0.1`, 2 days) and runs every client **plain and over SASL/SCRAM-SHA-512 + TLS** (`compat/run.sh:74-132`; 12 clients x 2 cells). The "second identity" e2e (`TestSecondIdentity`, Taskfile `:145,150`) boots SASL/SCRAM-SHA-512 + TLS with a console prefix and runs the Playwright suite against it.
- No mTLS, no client-certificate principal, no custom cipher suites, no SNI routing.

## B4. The allowed-hosts guard

`internal/hostguard/hostguard.go` (87 lines, fuzzed): **a DNS-rebinding guard for the two HTTP listeners (console and schema registry), not for the Kafka port.** `Allowed(hostport, extra)` is true for an IP literal, `localhost` / `*.localhost`, the `.doze` zone, or a name in `extra` (`--console-allow-host`, repeatable; `*` disables the check); a port is only trailing digits; brackets only around an IPv6 literal. A refused request is `403`. The console also refuses a state-changing request from another origin. Related start-up rules: with SASL or TLS on, a console/registry address beyond loopback is refused (exit 2) unless `--console-unprotected` (`config.go:497-510,560-600`); `Warnings()` lists what is left open. Rust PLAN (line 19-20): "an explicit allowed-hosts guard (DNS-rebinding defence) with no `.doze` special case"; PLAN puts the layer in `kafka-console` (a one-line stub today).

## B5. Module, Larder and website

- The kafka **module** (`module/config.go`) decodes only `auto_create_topics`, `default_partitions`, `retention`, `retention_bytes` and `topic` blocks: **no `sasl`, `tls` or `acl` attributes exist in the doze `kafka` block**. The Larder example (`doze-examples/larder/doze.hcl:59-68`) uses `module kafka "kafka" { version=4  port=9092  auto_create_topics=false  topic ... }` and every service connects with `KAFKA_BROKERS = module.kafka.address`, plaintext, no credentials. So **nothing in the doze module or the example needs security from the Rust broker**; Phase 8 does not change them.
- Website: `doze-website/src/content/docs/docs/kafka/{auth-and-tls,authorization}.md` are copies of the Go docs and `src/pages/kafka.astro:131-140` advertises "SASL · PLAIN · SCRAM-256 · SCRAM-512", "Under SASL `doze-kafka env` prints KAFKA_SASL_MECHANISM, never a password". Those claims are about the Go edition.
- "Open · no SASL yet": not found as a string in any repo; it corresponds to the Rust console's Access section, whose design is "one honest page, **Open**, that says what is on and off and that principals and ACLs arrive with security work. No previews of ACLs or denied requests are drawn" (`docs/port/console/DIRECTION.md:32-33`, `REALITY.md:28,112`). When Phase 8 lands that page must report the facts in C8 instead.
- Go console Access/Connect pages: `console/access.go` (574 lines) lists stored ACLs and the rule that applies where there is none; `console/aclcheck.go` (233) is a "could this principal do this from this host" checker using the authorizer's order; `/connect` fills in SASL mechanism and TLS in client snippets (`GROUNDING.md:50-51`).

## B6. Tests that define the Go edition's behaviour (not oracles)

`internal/broker/{auth_test,authz_test,authz_apis_test,acls_test,authorizer_internal_test,fuzz_sasl_test,fuzz_acl_test,handshake_test,mutation_auth_internal_test,mutation_acls_test,scram_internal_test,oracle_findings_test}.go`, `internal/storage/meta/acls_test.go`, `internal/hostguard/*_test.go`, `cmd/doze-kafka/{secrets_test,secrets_surfaces_test,identity_test,registry_tls_test,console_tls_test}.go`, `compat/` (12 clients x plain + scram-tls). The Rust `interop/` (sarama, franz-go, kafka-go, librdkafka; 23 of 23) runs with **no** SASL or TLS, so client-level security compatibility has no Rust gate yet.

---

# Part C. Rust touchpoints (`~/Code/doze-dev/doze-rs`, `main` 9492d39)

## C0. Facts found

| Fact | Detail |
|---|---|
| PLAN design | `PLAN.md:70`: `enum Session { Unauthenticated, Handshaken(Mechanism), Authenticated(Principal) }`; SCRAM a pure state machine; ACLs an immutable `ArcSwap` snapshot read lock-free. Crate map `PLAN.md:125-142`: `kafka-broker` holds "ACL, SCRAM" pure machines; `kafka-server` holds "listener, TLS, connection task, Session, shutdown". Features gate weight: `tls` (`PLAN.md:147`). Phase 8 row `PLAN.md:214`: gate = auth/authorization conformance + secret-redaction tests, 3-4 sessions. |
| Wire types | `kafka-protocol` 0.18 (vendored, `vendor/kafka-protocol`, Kafka 4.1 specs). Present and decodable: SaslHandshake **v0-1** (table offers v1 only), SaslAuthenticate **v0-2**, DescribeAcls / CreateAcls / DeleteAcls **v1-3** (KIP-896 removed v0), DescribeUserScramCredentials v0, AlterUserScramCredentials v0. ACL fields are raw `i8` (`resource_type_filter`, `pattern_type_filter`, `operation`, `permission_type`; `AclCreation.resource_type`...), so a code 0/99/-1 decodes and the handler decides; error codes `ClusterAuthorizationFailed 31`, `UnsupportedSaslMechanism 33`, `IllegalSaslState 34`, `TransactionalIdAuthorizationFailed 53`, `SecurityDisabled 54`, `SaslAuthenticationFailed 58` exist in `error.rs`. |
| Why the ACL APIs are not offered at `--kafka-version 1` | `kafka-wire/src/api.rs` rows are `v(0, 0)` for Kafka 1 (the real Kafka 1.1 range); `Profile::range` (`profile.rs:155-162`) clamps to `key.valid_versions()` which for these three is 1..=3, so the range is empty and the API vanishes from the Kafka 1 profile. Pinned by `profile/tests.rs:55-80` ("the APIs dropped because the crate cannot decode them are known"; `behaviours/wire.md` W10). Kafka 1.x admin tooling speaks only v0, so under `--kafka-version 1` ACL management is unavailable. Fix options: decode v0 ourselves (v0 differs from v1 by missing `resource_pattern_type` / `pattern_type_filter`, LITERAL implied) or accept the pinned deviation. |
| Today's behaviour | `Broker::handle` returns `Outcome::Close` for `SaslHandshake`, `SaslAuthenticate`, `DescribeAcls`, `CreateAcls`, `DeleteAcls` (`kafka-broker/src/broker.rs:238-246`). `SERVED` (`broker.rs:22-55`) has none of the five, so `Features.served` excludes them from ApiVersions and `Incoming::decode` rejects them with `NotAdvertised`. The `match` has no wildcard: a handler must be added or listed. |
| CLI | `doze-kafka/src/cli.rs:104-124`: `--acl-default` (`Option<String>`, line 109), `--acl-super-user` (`Vec`, comma delimiter, 111) accepted and ignored; `--sasl-mechanisms`, `--sasl-user`, `--tls-cert`, `--tls-key` -> `bail!` (`:138-146`). Tests `cli/tests.rs:64-73` and `tests/process.rs:254-271` pin the refusal and must be rewritten. `ignored()` (`:228`) only reports `--console`. |
| Connection | `kafka-server/src/connection.rs`: `serve<S: AsyncRead+AsyncWrite+Unpin>(stream, ctx, peer: Option<IpAddr>, stop)`; `FrameDecoder::new(Trust::Authenticated)` (**hard-coded**); `exchange` = `header::split` -> `Incoming::decode` (decodes the body immediately) -> `ApiVersions` branch or `ctx.broker.handle(listener, head, request)`. `Outcome::{Respond, NoResponse, Close}` (`outcome.rs`). The wire crate was built for Phase 8: `Trust::{Anonymous, Authenticated}` and `FrameDecoder::set_authenticated()` (`frame.rs:22-66`, 1 MiB cap for anonymous), the header/body split with the comment "a stranger ... must never have a body decoded for an API it may not use" (`incoming.rs:1-8`), `Features.sasl` and `Condition::Sasl` (`profile.rs:20-92`) that advertise SaslHandshake / SaslAuthenticate only when on. `server.rs:34-39` builds `Features { served, ..Default }` (so `sasl: false` always). |
| Per-request identity | None. `Broker::handle(&self, listener: &Listener, head: &RequestHead, request: Request)`; `Listener` is cloned per connection (`seen_from(peer)` fills `client_host = "/ip"`), so it is the natural carrier of a per-connection identity, or a new `Caller` can replace it (every handler that takes `Listener` or `Client{id,host}` is in `broker.rs:189-195`). |
| Metadata store | `kafka-meta` over redb 4.5.0 through the `Fs` seam; tables are `const TableDefinition` in `store.rs:23-36` and created for new databases in `make()` (`:112-135`); records are a **version byte + postcard** (`record.rs`, `txn.rs`, `life.rs`); existing databases without a newer table are tolerated by reading with `TableError::TableDoesNotExist -> empty` (`store/broker_configs.rs`) and writes open_table create it. Precedent for "a flat key -> value table": `BROKER_CONFIGS: &str -> &str`. In-memory registry pattern for hot reads: `registry.rs` (`RwLock<Registry>`, `Arc<Topic>`). `arc-swap` is already a dependency of `kafka-storage`. |
| Weight | `weight.md`: 4,029,504 B, 107 packages, 4.8 MB idle (darwin arm64). CI (`.github/workflows/ci.yml:51-73`) builds `cargo build --release -p doze-kafka` with **default features** and gates **5,150,000 B (Linux x86_64) / 115 packages / 8192 KB idle**; `cargo tree -p doze-kafka --edges normal`. Neither `hmac`, `sha2`, `pbkdf2`, `subtle`, `digest`, `rustls`, `ring` nor `aws-lc-rs` is in `Cargo.lock` today (only `arc-swap`). Adding RustCrypto SCRAM (`hmac`+`sha2`+`pbkdf2`) pulls roughly 8-12 transitive packages (digest, crypto-common, block-buffer, typenum, generic-array/hybrid-array, cpufeatures, subtle, ...) **unmeasured**: 107 + that may exceed 115 even before TLS. Candidates to measure first: (a) `ring` only for `digest`/`hmac`/`pbkdf2` (one package, also the likely rustls backend), (b) RustCrypto with default-features off, (c) a small in-tree SHA-256/512 + HMAC + PBKDF2 (unsafe forbidden; ~250 lines; must be fuzzed/tested against RFC vectors and the recordings). Limits to keep: files soft 300 / hard 450 (`xtask/src/lint_size.rs:9-10`); deny `unwrap/expect/indexing_slicing/panic/todo`, `unsafe_code = forbid`, `too_many_lines`, `cognitive_complexity`, `too_many_arguments` (workspace `Cargo.toml`). |
| Console, registry | `kafka-console` and `kafka-registry` are one-line stubs (`lib.rs`). The allowed-hosts guard has nothing to guard until Phase 9; in Phase 8 it can only be a pure function plus its tests (and the flags), or Phase 8 must wait for an HTTP listener. |
| Tests available | `kafka-server/src/connection/tests.rs` (268 lines, in-memory streams), `kafka-server/tests/tcp.rs` (277), `kafka-testkit` is an empty stub (one doc line) — "a raw-protocol test client" promised by PLAN is not written, and a SASL/ACL test client is needed. Fuzz: `cargo xtask fuzz-check` fails a marked parser with no target; SCRAM message parsing, PLAIN token parsing and ACL filter matching are parsers to mark. |

## C1-C8. File-level breakdown

Sizes: all new files target under 200 lines; existing files named below at 250+ lines are flagged.

### (1) Session and the SASL handshake/authenticate state machine (`kafka-server`)

| File | Change | Notes |
|---|---|---|
| `kafka-server/src/session.rs` (new, ~150) | `enum Session { Unauthenticated, Handshaken(Mechanism), Authenticated(Principal) }` + `Session::admit(key) -> Verdict { Serve, CloseSilently, RefuseIllegalState }` encoding A2.3's order: before HS: one ApiVersions then HS, anything else (including a 2nd ApiVersions and AUTH without HS) closes; after HS: AUTH only; HS/ApiVersions in the middle -> 34 and close; `Authenticated` -> everything allowed except HS/AUTH which answer 34 (`AFTER_OK`) but keep the connection. Holds the in-flight `ScramExchange` inside `Handshaken`. | The "versions seen" bit: Go counts an ApiVersions only if it was answered with the list, not with UNSUPPORTED_VERSION (franz-go opens at v5). Not recorded: decide and test. |
| `kafka-server/src/session/steps.rs` (new, ~150) | `fn handshake(&mut Session, &Authenticator, SaslHandshakeRequest) -> (SaslHandshakeResponse, After)` and `fn authenticate(&mut Session, &Authenticator, SaslAuthenticateRequest) -> (SaslAuthenticateResponse, After)` where `After::{Continue, CloseAfterResponse, Authenticated(Principal)}`. Builds the exact answers of A2 (`ErrorMessage ""` on success, the message table, `mechs=null` on 34, list on 33 and 0). | pure (no I/O): testable with `Session` values |
| `kafka-server/src/connection.rs` (124 -> ~220) | Keep `serve`/`exchange` shape; add `session: Session` local state; `FrameDecoder::new(if ctx.auth.required() { Trust::Anonymous } else { Trust::Authenticated })` then `decoder.set_authenticated()` on success; between `header::split` and `Incoming::decode` consult `session.admit(head.key)` (so a stranger's body is never decoded); a new branch for SASL requests handled in the server (not `Broker::handle`); `After::CloseAfterResponse` -> write the answer then return an `End`. Pass a `Caller` (principal + host) to `broker.handle`. Likely split: move `exchange` to `connection/exchange.rs`. | file currently 124, tests in `connection/tests.rs` (268, near cap) |
| `kafka-server/src/connection.rs` `End` enum | add `Denied` / `AuthFailed` variants for logging (log the reason, never the client's bytes) | |
| `kafka-server/src/config.rs` (107 -> ~150) | `Config` gains `auth: Option<Arc<Authenticator>>` (or a `Security` struct: mechanisms, credentials, tls) and `Timeouts.pre_auth` (Go: 30 s from accept, not reset) | frames.rs must use it: `frames::next` has only `idle` and `frame` budgets (`frames.rs:20-60`) |
| `kafka-server/src/frames.rs` (87) | take a deadline "until" for the first byte: `pre_auth` budget counted from accept while anonymous | |
| `kafka-server/src/server.rs` (77) | `Features { sasl: config.security.sasl_enabled(), served, ..}`; `Context` gets the authenticator, authorizer handle | |
| `kafka-wire/src/api_versions.rs` | add `answer_illegal_state(version) -> ApiVersionsResponse` (error 34, no keys, as in A2 `versions-before-token-versions`) | small |
| `kafka-wire/src/api.rs` | nothing for the table (SaslHandshake/Authenticate rows exist, `when Sasl`) | `SaslAuthenticate` Kafka-1 row `v(0,0)` is fine |

### (2) SCRAM as a pure state machine (`kafka-broker`, sans-IO)

| File | New | Notes |
|---|---|---|
| `kafka-broker/src/auth.rs` (new, ~40) | module root: `Mechanism`, `Principal(String)` (`User:name`), re-exports | `Principal` defined here so the authorizer and Session share it |
| `kafka-broker/src/auth/scram.rs` (new, ~180) | `struct ScramExchange` + `fn first(&Credentials, mech, token) -> Result<(ServerFirst, Awaiting), Refusal>` and `fn last(Awaiting, token) -> Result<(ServerFinal, Principal), Refusal>`; `enum Refusal { Credentials(mech), WrongProofLength /* CLOSE, no answer */, AuthzId, ... }` | A2.3 says which refusals answer 58 with which text and which close silently |
| `kafka-broker/src/auth/scram/parse.rs` (new, ~180) | RFC 5802 client-first / client-final parsers with the recorded strictness (only `n` flag; `a=` authzid; `n=` user with `=2C` / `=3D` escapes only; `r=` non-empty; `m=` accepted at first; extensions after `r=` accepted and signed in first; final exactly `c=,r=,p=` with nothing else; proof base64 of exactly hash-length bytes else silent close). **Mark with the fuzz marker and add a fuzz target** (`kafka-wire/fuzz` is the pattern). | |
| `kafka-broker/src/auth/scram/keys.rs` (new, ~120) | `Verifier { salt, iterations (4096), stored_key, server_key }` derived from a password per mechanism; server signature; PBKDF2/HMAC calls behind one small `trait Digest`-less module so the crypto choice (see weight) is isolated | |
| `kafka-broker/src/auth/plain.rs` (new, ~80) | token split -> `Result<User, Refusal>` in the recorded order (parts, user, password, constant-time compare incl. unknown user, then authzid) | fuzz marker |
| `kafka-broker/src/auth/credentials.rs` (new, ~100) | `Credentials::from_flags(&[(user, password)], &[Mechanism])` -> plaintext for PLAIN (zeroize/secrecy per PLAN), verifiers for SCRAM; **no persistence** (Go parity; no recorded need); `Debug` that prints no secrets | redaction test (PLAN gate) |
| `kafka-broker/src/auth/authenticator.rs` (new, ~100) | `Authenticator { order: Vec<Mechanism>, credentials }`, `offered() -> &[Mechanism]` in flag order, `plain(token)`, `scram_first/last` | |
| `kafka-broker/Cargo.toml` | + crypto dep(s) (see weight row), + `arc-swap` (already in lock) | |

### (3) The authorizer (immutable snapshot + matching)

| File | New | Notes |
|---|---|---|
| `kafka-broker/src/authz.rs` (new, ~120) | `pub struct Authorizer { snapshot: ArcSwap<AclSet>, default_allow: bool, supers: HashSet<Principal> }`; `fn allowed(&self, who: &Principal, host: IpAddr, resource: Resource<'_>, op: Operation) -> bool`; `fn replace(&self, AclSet)`. One atomic load per check; empty set + `default_allow` = early return (Go's fast path). | |
| `kafka-broker/src/authz/acl.rs` (new, ~150) | `AclEntry` (`resource_type: i8, name, pattern: i8, principal, host, operation: i8, permission: i8` — raw wire codes like Go, so a client gets back exactly what it sent), `AclSet` (grouped by resource type for lookup), `AclFilter` (`matches`, `validate` -> `Unknown patternFilter.` / `Unknown entryFilter.`), `validate_creation`. | fuzz marker for filter matching |
| `kafka-broker/src/authz/rules.rs` (new, ~120) | the decision procedure: supers, DENY (exact op or ALL), ALLOW with implications (ALL; Read/Write/Delete/Alter -> Describe; AlterConfigs -> DescribeConfigs), default when no ACL covers the resource (note: *covers* = any ACL for that resource, not any ACL matching the principal), `writes_somewhere(principal, host)` and `denied_on_cluster` for InitProducerId | Kafka's `authorizeByResourceType` is subtler than Go's version: one recorded case only |
| `kafka-broker/src/caller.rs` (new, ~40) | `Caller<'a> { principal: &'a Principal, host: IpAddr }` (and `Listener`'s `client_host`/advertised data moved in or beside) so handlers get `who` without a signature explosion (lint `too_many_arguments`) | |

### (4) Tables in `kafka-meta`

| File | Change |
|---|---|
| `kafka-meta/src/acl.rs` (new, ~80) | `AclRecord` (the seven fields) with `encode/decode` as version byte + postcard (pattern of `record.rs`); key = fields joined by NUL (or a postcard tuple) so duplicates collapse |
| `kafka-meta/src/store/acls.rs` (new, ~100) | `MetaStore::acls() -> Result<Vec<AclRecord>>`, `add_acls(&[AclRecord])`, `remove_acls(&[AclRecord])` in one write transaction each, under `lock(&self.write)` (as `set_broker_configs`) |
| `kafka-meta/src/store.rs` (139) | `const ACLS: TableDefinition<&[u8], &[u8]>`; add `txn.open_table(ACLS)?` to `make()`; read path tolerates a missing table |
| `kafka-meta/src/lib.rs` | `mod acl; pub use acl::AclRecord;` |
| credentials | **No table.** Go stores none; recorded users come from flags; the oracle's AlterUserScramCredentials is unrecorded recorder-side setup. A credential table is needed only if the owner wants DescribeUserScramCredentials/AlterUserScramCredentials or runtime user management (decision, see Gaps). |

### (5) CreateAcls / DescribeAcls / DeleteAcls handlers (and optional SCRAM-credential APIs)

| File | New | Behaviour to implement |
|---|---|---|
| `kafka-broker/src/handlers/create_acls.rs` (~120) | any creation with a code 0 in any field -> `Outcome::Close` (before authorizing, before storing); else cluster Alter check (31 on each result); per-creation validation: code outside enum -> 42 `Invalid {resourceType\|patternType\|operation\|permissionType} UNKNOWN`; ANY/MATCH values -> `-1` null message; success -> `ErrorMessage ""`; store, then swap the authorizer snapshot | A3.2 |
| `kafka-broker/src/handlers/describe_acls.rs` (~120) | code 0 in a filter -> `Close`; response `ErrorMessage ""` always; cluster Describe check -> 31; out-of-enum filter code -> 0 / no entries; group matches by (type, name, pattern), sorted | A3.1, A2 fixture await |
| `kafka-broker/src/handlers/delete_acls.rs` (~130) | code 0 in any filter -> `Close`; cluster Alter check; out-of-enum -> 42 `Unknown patternFilter.` (resource/pattern type) or `Unknown entryFilter.` (operation/permission), null `MatchingACLs`; `MatchingACLs` with the removed entries; store then swap | A3.1 |
| `kafka-broker/src/handlers.rs` (40) | + `mod create_acls; mod describe_acls; mod delete_acls;` |
| `kafka-broker/src/broker.rs` (275, **near soft cap; `handle` already carries a cognitive-complexity `expect`**) | + three arms (DescribeAcls, CreateAcls, DeleteAcls out of the `Outcome::Close` arm); add the three ACL keys to `SERVED` (`broker.rs:22-55`). The two SASL keys are answered by the connection, not `Broker::handle`, but `Features.allows` (`profile.rs:84-92`) needs them in `Features.served` or ApiVersions will not list them and `profile.check` will reject them: either list them in `SERVED` (with their `handle` arm staying `Close`) or have `Server::bind` add them when SASL is on. **Split `handle` into `broker/dispatch.rs`** and pass `&Caller` |
| `kafka-broker/src/broker.rs` fields | + `authz: Authorizer` (loaded from `meta.acls()` in `open`), config: `acl_default_allow`, `super_users` (in `config.rs`, 112 lines) |
| SCRAM credential APIs 50/51 (optional) | `kafka-wire/src/api.rs` rows + `handlers/{describe,alter}_user_scram_credentials.rs`; only if the owner wants them (Go serves neither; nothing recorded) |
| `SECURITY_DISABLED` | decision: Go stores and serves ACLs on a broker with no SASL (anonymous principal) and `acl-default allow`; Kafka without an authorizer answers 54. The oracle requires only the `version-matrix` empty requests |

### (6) Authorization checks threaded through each served handler

Each handler gets `who: &Caller` (or a `Guard` built once per request) and the refusal is the API's own code, placed **before** existence checks as Kafka does
(only Metadata's order is recorded). Mapping from the Go `can*` sites (`docs/authorization.md` table) to the Rust files. Handler files at 250+ lines are flagged: they
need the check placed in an existing sub-file or split.

| Rust file (lines) | API | Check | Refusal |
|---|---|---|---|
| `handlers/metadata.rs` (153) | Metadata | Describe on each named topic (a list-all shows only describable topics); topic entry on refusal: name kept, `ErrorCode 29`, no partitions, topic id zero (A2); auto-create additionally needs Create on cluster or topic | 29 |
| `handlers/create_topics.rs` (**282**) | CreateTopics | Create on cluster **or** the topic | 29 per topic |
| `handlers/delete_topics.rs` (76) | DeleteTopics | Delete on the topic | 29 |
| `handlers/produce.rs` (**288**) + `produce/check.rs` (164) + `produce/request.rs` | Produce | Write on each topic (per partition error); a transactional batch also needs Write on the transactional id (today's rule "transactional bit with no transactional id -> 53" already exists and stays) | 29 / 53 |
| `handlers/fetch.rs` (171) + `fetch/round.rs` (270) | Fetch | Read on each topic (per partition) | 29 |
| `handlers/list_offsets.rs` (153) | ListOffsets | Describe on each topic | 29 |
| `handlers/find_coordinator.rs` (75) | FindCoordinator | key type 0: Describe on the group; type 1: Describe on the transactional id (v4+: per entry; before: top level) | 30 / 53 |
| `handlers/join_group.rs`, `sync_group.rs`, `heartbeat.rs`, `leave_group.rs` | classic group | Read on the group | 30 |
| `handlers/consumer_group_heartbeat.rs` (206) | KIP-848 heartbeat | Read on the group; subscribed topics the principal may not Describe are left out | 30 |
| `handlers/consumer_group_describe.rs` (125), `describe_groups.rs` (98) | describes | Describe on the group (Go: or on the cluster for DescribeGroups) | 30 |
| `handlers/list_groups.rs` (71) | ListGroups | shows only groups that may be described | n/a |
| `handlers/offset_commit.rs` (157) | OffsetCommit | Read on the group and on each topic | 30 / 29 |
| `handlers/offset_fetch.rs` (192) + `offset_fetch/check.rs` | OffsetFetch | Describe on the group and on each topic | 30 / 29 |
| `handlers/delete_groups.rs` (61), `offset_delete.rs` (75) | DeleteGroups, OffsetDelete | Delete on the group (OffsetDelete also Read on each topic) | 30 / 29 |
| `handlers/init_producer_id.rs` (64) | InitProducerId | no txn id: `IdempotentWrite` on cluster **or** Write on any topic (A4: 0 for alice, 31 for bob); txn id: Write on that id | 31 / 53 |
| `handlers/add_partitions_to_txn.rs` (134), `add_offsets_to_txn.rs` (54), `txn_offset_commit.rs` (130), `end_txn.rs` (22; the work is in `txn/`) | transactions | Write on the transactional id; AddPartitions Write on each topic; AddOffsets / TxnOffsetCommit Read on the group (TxnOffsetCommit Read on topics) | 53 / 30 / 29 |
| `handlers/describe_transactions.rs` (106), list_transactions (in `txn/`), `describe_producers.rs` (86) | txn describes | Describe on the transactional id (list shows describable); DescribeProducers Read on the topic | 53 / 29 |
| `handlers/delete_records.rs` (94) | DeleteRecords | Delete on the topic | 29 |
| `handlers/create_partitions.rs` (202) | CreatePartitions | Alter on the topic | 29 |
| `handlers/describe_configs.rs` (215) | DescribeConfigs | DescribeConfigs on the topic; on cluster for broker resources | 29 / 31 |
| `handlers/alter_configs.rs` (107) + `alter_configs/resource.rs` (208) | AlterConfigs, IncrementalAlterConfigs | AlterConfigs on the topic / cluster | 29 / 31 |
| (not served) DescribeCluster | Describe on cluster | Phase 9 |
| new ACL handlers (5) | Describe / Alter on cluster | 31 |

Where recorded: Metadata (A2) and InitProducerId (A4) only. **Every other row is Go behaviour backed by no recording** (and Kafka's source), so each needs
a test written from the Kafka source or a new recording; every non-security scenario runs with `--acl-default allow` and no SASL, so none can regress as long as
the allow-all fast path is exact.

### (7) TLS in the listener behind a cargo feature

| File | Change |
|---|---|
| `kafka-server/Cargo.toml` | `[features] tls = ["dep:tokio-rustls", "dep:rustls", "dep:rustls-pemfile"/"rustls-pki-types"]`; backend choice (ring vs aws-lc-rs vs a smaller provider) is the weight decision |
| `kafka-server/src/tls.rs` (new, ~120, `#[cfg(feature = "tls")]`) | `TlsConfig::load(cert_pem, key_pem) -> Result<Acceptor>` (server cert only, TLS 1.2 minimum, no client auth: Go parity; errors reported at start like Go) and `async fn accept(stream) -> io::Result<TlsStream<TcpStream>>` with a handshake timeout (counted inside the pre-auth budget) |
| `kafka-server/src/accept.rs` (65) | after `accept`, wrap the `TcpStream` in the acceptor inside the spawned task (a slow handshake must not hold the accept loop); `peer.ip()` is taken from the TCP socket before the handshake so ACL host matching works over TLS; `connection::serve` is already generic over the stream |
| `kafka-server/src/config.rs` | `tls: Option<TlsFiles>` (compiled in only with the feature; without it the field is absent and the CLI refuses the flags with a message that says to build with `--features tls`) |
| `doze-kafka/Cargo.toml` | `tls = ["kafka-server/tls"]`; **default?** The CI weight gate builds default features: if `tls` is default, its crates count against 115 packages and 5.15 MB. Decision for the owner |
| Tests | in-memory duplex for the Session; a loopback TLS test with a generated cert (rcgen is another dependency: dev-only, or commit a test cert) and the compat matrix's `scram-tls` shape |

### (8) Flags (`doze-kafka/src/cli.rs`, 238 lines; `cli/tests.rs` 118)

| Flag | Rust handling |
|---|---|
| `--sasl-mechanisms PLAIN,SCRAM-SHA-512,SCRAM-SHA-256` | `Vec<Mechanism>` (`ValueEnum`, upper-case exact; keep order); error on unknown (Go message) |
| `--sasl-user user:password` (repeatable) | split on the first `:`; empty name or password refused; duplicates refused; users without mechanisms (and mechanisms without users) refused as Go does (`config.go:467-487`); never printed (`Debug`) |
| `--acl-default allow\|deny` | currently `Option<String>`; becomes `ValueEnum`, default `allow`; harness always passes it |
| `--acl-super-user User:admin` | `Type:name` shape required; repeatable / comma list |
| `--tls-cert`, `--tls-key` | both or neither; with the cargo feature off: refuse naming the feature |
| `--console-allow-host` / `--console-unprotected` | belong to the console (Phase 9); the allowed-hosts guard is a pure function `host_allowed(hostport, extra) -> bool` (port the semantics in B4, minus `.doze`) that can be written and fuzzed in Phase 8 and mounted in Phase 9. The start-up rule "SASL/TLS on and a console beyond loopback is refused" needs the console flags |
| `--sasl-*` from the config file | the Rust CLI has no config file today; Go's `[credentials]` table existed to keep passwords out of `ps`: decide whether a `--sasl-user-file`/env is wanted |

Remove from `ignored()`/hidden-and-refused: `cli.rs:128-146`; rewrite `cli/tests.rs:64-73` and `tests/process.rs:254-271`; `docs/port/behaviours/binary.md:23-25` documents the refusal.

### Not previously listed but needed

- `kafka-server/src/session/…` tests, `kafka-testkit` (empty) needs a raw client with SASL helpers (PLAN promised one).
- Fuzz targets: PLAIN token, SCRAM client-first, SCRAM client-final, ACL filter/creation (follows `cargo xtask fuzz-check`).
- Secret-redaction tests (PLAN gate): a planted password must not appear in logs at any level, `Debug` output, or error text (Go has a suite to read: `cmd/doze-kafka/secrets*_test.go`).
- Docs: `docs/port/behaviours/{auth,authz}.md` (the catalogue), `oracle.md` per-phase result, `weight.md` entry, `PLAN.md` log.
- Console (Phase 9) will need an in-process "security facts" view (SASL on, mechanisms, TLS on, ACL count, default, supers) to replace the Access page's "Open" copy.

## C8. Suggested slice order (sizing is the owner's)

1. `Principal`, `Authenticator`, PLAIN, Session + connection gating -> `sasl-plain` passes except ACL-dependent `WHO` answers (PLAIN login with `--acl-default allow` is a smoke).
2. `kafka-meta` ACL table, `Authorizer`, the three ACL handlers, Metadata + InitProducerId checks -> `sasl-plain`, `acl-unknown-codes`, `idempotent-write...`, `version-matrix` pass.
3. SCRAM state machine -> `sasl-scram` passes.
4. The remaining handler checks (6) with tests from Kafka's source (no recordings), including the transaction APIs carried over from Phase 6.
5. TLS behind a feature + client-level security interop (kcat/franz-go over SASL_SSL, the Go `compat` shape).
6. allowed-hosts guard as a pure function (mounted in Phase 9).

---

# Gaps and questions

**Decisions the owner must make**

1. **Weight versus SCRAM/TLS.** Nothing in the lock for SHA/HMAC/PBKDF2/TLS today. Which crypto (ring for digest+hmac+pbkdf2 and as rustls backend; RustCrypto trio; hand-written)? Is `tls` a default feature (CI gates 115 packages and 5.15 MB Linux on default features) or opt-in? The cost is unmeasured and should be a spike before design.
2. **Mid-login requests.** Match Kafka (answer Metadata/ApiVersions in the request's own error shape with 34, which means decoding a stranger's body for those two APIs), or close like Go (Known-tolerated for the three Metadata `...-who` steps only)? ApiVersions-34 and handshake-34 are *not* Known and must be answered.
3. **SCRAM channel binding.** Kafka logs in on `c=eSws` after `n,,`; Go refuses (Known). Rust may do either; Kafka parity is the oracle's answer, RFC 5802 the stricter one.
4. **Authorization with SASL off.** Go enforces ACLs for the `User:ANONYMOUS` principal and serves ACL CRUD on an open broker; real Kafka answers SECURITY_DISABLED (54) without an authorizer. Keep Go's behaviour (needed for `--acl-default deny` without SASL)? What does `--acl-default allow` plus a created ACL do?
5. **`--kafka-version 1` and the ACL APIs.** Keep the pinned deviation (not offered) or decode v0 ourselves.
6. **Credential management.** Flags only (Go parity, SCRAM derived at start, no storage) or a stored credential table with DescribeUserScramCredentials/AlterUserScramCredentials (not served by Go; not recorded)? A config file / env / file option for passwords (Go's `[credentials]`) — the Rust CLI has no config file.
7. **Allowed-hosts guard scope.** It guards HTTP listeners only (Go); the console does not exist yet. Phase 8 can ship the pure function, its tests and the flags and mount it in Phase 9, or move it. Does the "beyond loopback with SASL/TLS is refused" start-up rule move with it?
8. **Transaction-API and other handler denials have no recording.** Write them from Kafka's source (and Go), or record new scenarios (e.g. a `authz-matrix` scenario) against `apache/kafka:4.3.1` first? Changing the Go oracle needs the `rust-oracle` branch.
9. **InitProducerId rule.** Kafka's `authorizeByResourceType(Write, Topic)` versus Go's `writesSomewhere` + `deniedOnCluster`: only the allow case and the no-ACL deny case are recorded.
10. **Pre-auth budget and caps.** Go: 30 s from accept, 1 MiB frames. Kafka's SASL pre-auth cap is `sasl.server.max.receive.size` (512 KiB default; not recorded); the 64 KiB token is under both.
11. **The ApiVersions "answered once" rule** for a client that opens at an unsupported version (franz-go v5): Go does not count a refusal as the one allowed ApiVersions; unrecorded for SASL listeners.

**Things the recordings do not cover** are listed in A7; the largest are TLS, every denial except Metadata-29 and InitProducerId-31, ACL CRUD with real entries, and SECURITY_DISABLED.

**Mistakes to avoid** (found while reading): the recorded JSON prints codes 99 and -1 as `UNKNOWN` (use the Go source for the numbers); `DescribeACLs` rows of `version-matrix` pass today only because unserved APIs close the connection; `Metadata` denial entries must have `Partitions:null`, `IsInternal:false`, zero topic id and a kept name; a failed SASL answer must be flushed before the close (the script reads the 58); `CreateACLs` success carries `ErrorMessage ""`, not null; `DescribeACLs` `ErrorMessage` is `""`; `ThrottleMillis 0` everywhere.
