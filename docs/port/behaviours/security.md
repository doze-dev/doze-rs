# Security: SASL login, ACLs, TLS and the allowed-hosts guard (Phase 8)

All seven slices of Phase 8 are built (status: **built, independent audits pending**, 2026-11-12; the ledger is in `phase8.md`). This file is the
catalogue: what Apache Kafka 4.3.1 does (or, where the Go edition or this broker differs on purpose, what is done instead), and the
test that holds each rule. The source is the recordings (`conformance/recordings/sasl-plain.json`, `sasl-scram.json`, `acl-unknown-codes.json`,
`idempotent-write-from-topic-write.json`; the step names are the oracle's) where there is one, else Kafka 4.3.1's own source, read by
item (`SaslServerAuthenticator` SSA, `PlainSaslServer`, `SocketServer`; the line numbers are in `phase8/kafka-semantics.md` 3.1 to 3.9 and 6):
**(source)** marks those. Rules without a recording are written from the source and are only as right as the reading is (decision D9; the
proposal to record them is in `oracle.md`). The pure parts are in `crates/kafka-auth` (PLAIN `plain.rs`; SCRAM `scram.rs`, `scram/parse.rs`,
`scram/keys.rs`; the authorizer `authz*`; the guard `hostguard.rs`), the login order in `crates/kafka-server/src/session*`, TLS in
`crates/kafka-server/src/tls.rs` (feature `tls`), the flags in `crates/doze-kafka/src/cli/{sasl,acl,tls,console}.rs`. The authorizer's stored form is
`crates/kafka-meta/src/{acl.rs,store/acls.rs}`, and its use `crates/kafka-broker/src/{authorizer.rs,caller.rs,guard.rs,handlers/*}`. Kafka source
abbreviations are those of `phase8/kafka-semantics.md` (SAD = `StandardAuthorizerData.java`, KA = `KafkaApis.scala`, ACM = `AclControlManager.java`,
CA = `ControllerApis.scala`, SCM = `ScramControlManager.java`, KC = `KafkaChannel.java`).

The numbers are checked by `tools/check-security-catalogue.ts` (`bun tools/check-security-catalogue.ts`; a CI step): no SEC-n or SEC-Dn is defined
twice, none is referenced (here, in the other docs or in a source comment) without a definition, every deviation is in the index below, and every
test name quoted in this file is a `fn` in the Rust sources.

## Index

| Rules | Topic | Slice | Where |
|---|---|---|---|
| SEC-1 to SEC-15 | SASL opt-in, the login session, PLAIN, redaction, credentials, the crypto boundary | 8a | below |
| SEC-16 to SEC-30 | the authorizer, the three ACL APIs, Metadata and InitProducerId checks, the model test | 8b | "The oracle, slice 8b" |
| SEC-40 to SEC-51 | SCRAM-SHA-256 and SCRAM-SHA-512 | 8c | after SEC-15 |
| SEC-60 to SEC-72 | ACL enforcement on the topic APIs | 8d-1 | "The topic APIs" |
| SEC-100 to SEC-123 | ACL enforcement on the group and transaction APIs | 8d-2 | "Slice 8d-2" |
| SEC-140 to SEC-154 | TLS behind the `tls` feature | 8e | "TLS, slice 8e" |
| SEC-160 to SEC-179 | the SCRAM credential APIs (50, 51), the stored table, the users file and variable | 8f | after SEC-154 |
| SEC-180 to SEC-189 | the allowed-hosts guard, `--console-allow-host`, `--console-unprotected`, the start-up rule and notes | 8g | "The allowed-hosts guard" |

Deviations from Apache Kafka (and one from the Go edition) by number. Every one is a decision, not a bug; each has a row in `debt.md` section I.

- **SEC-D1** a request in the middle of a login (other than ApiVersions and SaslHandshake) closes unanswered
- **SEC-D2** the 30 s login budget from the accept
- **SEC-D3** SaslHandshake v0 is not served
- **SEC-D4** no 100 ms delay before a failed login is answered
- **SEC-D5** passwords are not zeroed on drop
- **SEC-D6** `Session::Unauthenticated` carries a `versions_answered` flag
- **SEC-D7** the ACL APIs and authorizer work with SASL off, for `User:ANONYMOUS`
- **SEC-D8** no long message on a CLUSTER_AUTHORIZATION_FAILED
- **SEC-D9** a super user must be written `Type:name` with both parts non-empty
- **SEC-D10** DescribeAcls and DeleteAcls results are sorted
- **SEC-D11** no 10,000-ACL limit; DescribeConfigs does not report the two ACL settings
- **SEC-D12** the server nonce is 18 random bytes in base64
- **SEC-D13** several SCRAM extensions are signed in the order sent; no delegation tokens
- **SEC-D14** bootstrap users have 4096 iterations and a random 16-byte salt
- **SEC-D15** SCRAM intermediate values are not zeroed
- **SEC-D16** results in the order asked where Kafka's order is a hash map's or a shuffle
- **SEC-D17** CreateTopics refused for want of CREATE answers empty `configs`; `__cluster_metadata` and DeleteTopics shape errors not added
- **SEC-D18** unsupported resource types are checked for the right, then answered 42
- **SEC-D19** a follower Fetch with CLUSTER_ACTION is read like a consumer's; a nameless Metadata entry has no operations field
- **SEC-D20** the topic-API rules are read from the source, not recorded
- **SEC-D30** DeleteGroups with a repeated allowed name answers it twice
- **SEC-D31** SyncGroup v5+ with no protocol type or name is 23 only for a caller also refused
- **SEC-D32** AddPartitionsToTxn on the internal topics is 3, not 29
- **SEC-D33** InitProducerId's two-phase-commit check is unreachable (v6 not served)
- **SEC-D34** unsure: a DescribeTransactions entry refused with 53 carries schema defaults
- **SEC-D35** unsure: ConsumerGroupDescribe and a deleted topic's empty name
- **SEC-D36** unsure: FindCoordinator before v4 puts the error's own message in the answer
- **SEC-D37** share groups, streams groups and AddPartitionsToTxn v4+ are not served
- **SEC-D40** client certificates and mutual TLS are out of scope
- **SEC-D41** `ssl.principal.mapping.rules` has nothing to map
- **SEC-D42** protocol versions and cipher suites are rustls's with ring
- **SEC-D43** no certificate generation and no reload
- **SEC-D44** PEM only, one certificate, no SNI, ALPN or tickets
- **SEC-D45** a failed handshake may send a TLS alert record
- **SEC-D46** the pre-auth budget applies to the handshake with SASL off
- **SEC-D47** the advertised host is not checked against the certificate
- **SEC-D48** a `tls` build needs a C compiler
- **SEC-D50** two layers of users, bootstrap and stored; the API refuses a bootstrap user with 93
- **SEC-D51** the credential APIs work with SASL off: an open broker lets anyone manage users
- **SEC-D52** credential results are in the request's order, or by name
- **SEC-D53** `metadata.version` is not modelled
- **SEC-D54** Describe with a null user list from a denied caller answers 31 where Kafka throws
- **SEC-D55** a changed credential applies to new logins; no re-authentication
- **SEC-D56** credentials are a table in `meta.redb`, not records in a metadata log
- **SEC-D57** an unusable stored row is left and named at warn
- **SEC-D58** the environment variable is not cleared
- **SEC-D59** file and variable passwords are ASCII and held for the life of the process
- **SEC-D60** the guard has no `.doze` special case (the Go edition's guard has one)
- **SEC-D61** the guard is stricter than the Go edition's in five shapes
- **SEC-D62** a configuration that cannot start exits 2 (it exited 1 before slice 8g)
- **SEC-D63** the console flags have nothing to act on until Phase 9

## SEC-1. SASL is opt-in; without it nothing changes
No `--sasl-*` flag: no `Session` is made, the decoder trusts every connection, the connection loop has no extra branch, ApiVersions
does not list SaslHandshake or SaslAuthenticate, and a request for either closes the connection like any API not offered.
With the flags, both APIs are listed (v1; v0 to v2) and every connection starts as a stranger. Test:
`tests/sasl::the_sasl_apis_are_listed_only_by_a_broker_that_has_sasl`,
`tests/sasl::a_broker_without_sasl_serves_from_the_first_request_and_closes_on_a_handshake`,
`connection::tests::login::without_sasl_a_handshake_is_not_a_request_the_server_offered`,
`kafka-wire profile::tests::the_two_sasl_apis_need_both_the_feature_and_a_handler`; every pre-existing test is the rest of the proof.

## SEC-2. What a stranger may send, in order
Before the handshake: one ApiVersions (answered in full), then SaslHandshake. Anything else is closed unanswered: a
SaslAuthenticate without a handshake (`no-handshake`), any other API (`no-login`), a second ApiVersions (`versions-twice`).
After the handshake only SaslAuthenticate is welcome. Once logged in every API is served, ApiVersions included (`versions-first`),
except the two SASL ones (SEC-10). Test: `session::tests::a_stranger_may_ask_apiversions_once_and_then_handshake_and_nothing_else`
(every state against every served API), `..::after_the_handshake_only_a_token_is_welcome_and_two_apis_are_answered_then_closed`,
`..::a_logged_in_connection_is_served_everything_but_the_two_sasl_apis`, `tests/sasl::a_request_before_the_login_is_closed_without_an_answer`.

## SEC-3. The verdict is on the header; a stranger's body is never decoded
`header::split`, then `Session::admit(key)`, then (only if served) `Incoming::decode`. A Metadata with a body that does not decode
is "out of order" from a stranger and "a bad body" from a logged-in client. Test:
`connection::tests::login::a_strangers_body_is_not_decoded_but_a_logged_in_clients_is`.

## SEC-4. Limits before the login
A stranger's frame is capped at 512 KiB (Kafka's `sasl.server.max.receive.size`; it was 1 MiB), whatever the API; a logged-in
client's cap is the API's own (Produce 100 MiB). From the accept, a connection has 30 s to finish logging in, counted once and
not restarted by each request (**deviation SEC-D2**: Kafka has no such limit; the Go edition has this one). Test:
`frame::tests::an_unauthenticated_connection_is_capped_at_half_a_mib`, `frame::tests::the_caps_are_exact`,
`connection::tests::login::a_stranger_frame_is_held_to_512_kib_and_a_logged_in_one_is_not`,
`..::a_connection_that_does_not_log_in_within_the_budget_is_closed_however_it_trickles`, `..::the_budget_ends_with_the_login`.

## SEC-5. A request in the middle of a login
After a successful handshake and before the token: ApiVersions is answered ILLEGAL_SASL_STATE (34) with no APIs, at the request's
own version (`versions-before-token`); a second SaslHandshake is answered 34 with no mechanisms (`handshake-twice`); in both the
connection is then closed. Any other API is closed unanswered (**deviation SEC-D1**: Kafka answers it in its own shape, each
topic 34 and no brokers; the Go edition closes, as this does; needs the stranger's body decoded, which SEC-3 forbids). Test:
`kafka-wire api_versions::tests::the_answer_to_an_apiversions_in_the_middle_of_a_login_is_illegal_sasl_state_with_no_apis`,
`tests/sasl::apiversions_before_the_handshake_is_answered_once_and_after_it_is_illegal_state`,
`session::steps::tests::a_handshake_in_any_other_state_is_34_with_no_mechanisms_and_changes_nothing`.

## SEC-6. SaslHandshake
The mechanism must be offered, exactly: case-sensitive, so `plain`, `""`, `BOGUS` and an unconfigured `SCRAM-SHA-256` fail
(`unknown-mechanism`, `lowercase-mechanism`, `empty-mechanism`, `scram-not-enabled`). Success: error 0 and the list of offered
mechanisms. Failure: UNSUPPORTED_SASL_MECHANISM (33) with the same list, then the connection is closed, so a following handshake
or token gets no answer (`refused-then-right`). **Served at v1 only** (v0 means "raw tokens follow" and is not served; **deviation SEC-D3**).
Test: `session::steps::tests::a_handshake_for_an_offered_mechanism_is_answered_with_the_list_and_moves_on`,
`..::a_mechanism_that_is_not_offered_is_33_with_the_list_and_then_closed`, `tests/sasl::an_unknown_mechanism_is_answered_with_the_list_then_closed`.

## SEC-7. The list is in Java `HashSet` order, not the configured order (source: SSA:181)
Kafka answers `new ArrayList<>(new HashSet<>(configured))`: a 16-bucket table, a name's bucket being its `String.hashCode` spread
(`h ^ (h >>> 16)`) masked to four bits, a bucket keeping insertion order. `PLAIN` and `SCRAM-SHA-512` share bucket 0, `SCRAM-SHA-256`
is in bucket 6: the full list is `PLAIN, SCRAM-SHA-512, SCRAM-SHA-256` (recorded in `sasl-scram`) however it was configured, unless
`SCRAM-SHA-512` was written before `PLAIN` (then `SCRAM-SHA-512, PLAIN, SCRAM-SHA-256`: the bucket keeps insertion order. The
task text said "whatever the flag order"; the source says otherwise for that one order, and the source is followed). Test:
`kafka-auth authenticator::tests::the_buckets_are_the_ones_java_computes`, `..::the_mechanism_list_is_in_java_hash_set_order`
(the table of kafka-semantics 3.8 and the other orders); `doze-kafka cli::tests::scram_mechanisms_are_offered_in_javas_order_whatever_order_the_flag_has`
(the flag). All three mechanisms can be enabled since 8c.

## SEC-8. The PLAIN token, checked in this order (recorded `sasl-plain`; source: `PlainSaslServer`)
The token is `authzid NUL user NUL password`, bytes (not text). 1. Parts: exactly three, else `Invalid SASL/PLAIN response: expected 3
tokens, got N` where N is the number of parts counted up to four (no NUL or an empty token: 1; one NUL: 2; three NULs or a trailing
NUL or more: 4). 2. Empty user: `Authentication failed: username not specified` (checked first, so both empty gives this).
3. Empty password: `Authentication failed: password not specified`. 4. The password, compared in constant time and compared for an
unknown user too (against itself, then refused): `Authentication failed: Invalid username or password` for a wrong password, another
user's, a prefix, a longer one, an unknown user, a capitalised user and non-UTF-8 bytes alike. 5. Only then the authzid: empty or the
user is fine, anything else (an existing user, `admin`, a stranger) is `Authentication failed: Client requested an authorization id
that is different from username`. A wrong password hides the authzid refusal. A 64 KiB token is judged like any other. Test:
`kafka-auth plain::tests::*` (one test per recorded group, steps named in comments), fuzz target `plain` (`crates/kafka-auth/fuzz`,
`// fuzz:parser plain`, in the CI fuzz job).

## SEC-9. A failed login is answered, flushed, then the connection is closed
SaslAuthenticate error 58 (SASL_AUTHENTICATION_FAILED), `ErrorMessage` the SEC-8 text, empty bytes; the answer is written and the stream
shut down before the connection ends, so the client reads the text and not a reset (the harness reads the 58). The next request gets
nothing. Test: `session::steps::tests::every_recorded_refusal_is_58_with_kafkas_text_and_closes`,
`connection::tests::login::a_failed_login_is_flushed_before_the_close_and_ends_the_connection`,
`tests/sasl::a_wrong_password_is_answered_then_the_connection_is_closed`. Unlike Kafka there is no 100 ms delay before the answer
(`connection.failed.authentication.delay.ms`; not observable in any recording).

## SEC-10. A login is final: afterwards both SASL APIs are 34 and the connection stays
After success, SaslAuthenticate (the same token, another user's valid one, a wrong one, none) is error 34 with `SaslAuthenticate
request received after successful authentication`, and SaslHandshake (any mechanism, even `BOGUS`) is 34 with no mechanisms. The
connection stays open and the principal never changes (`twice`, `authenticate-v0/v1`). Both are answered without decoding the body.
Test: `session::steps::tests::a_second_token_after_a_login_is_34_whoever_it_names_and_changes_nobody`,
`tests/sasl::a_second_login_is_illegal_state_and_the_connection_and_its_user_stay`.

## SEC-11. The success answer
Error 0, `ErrorMessage` the empty string (not null), empty `AuthBytes`, `SessionLifetimeMillis` 0, at SaslAuthenticate v0, v1 and v2
(re-authentication is off). The principal is `User:<user>`; the authzid cannot change it. Test:
`session::steps::tests::the_right_token_logs_in_with_an_empty_message_not_an_absent_one`, `tests/sasl::a_client_that_logs_in_with_plain_can_produce_and_fetch`.

## SEC-12. An ApiVersions answered with an error does not use up the stranger's one (source: SSA:572-586, kafka-semantics 6.2)
UNSUPPORTED_VERSION (35, a client opening at a version not served) and INVALID_REQUEST (bad software name) leave the state as it was;
only the answer with the list uses the slot. Test: `connection::tests::login::an_apiversions_answered_with_an_error_does_not_use_up_the_one_a_stranger_has`.

## SEC-13. Nothing the client sent is logged, printed or put in an error
Not the token, not a password, not a user name from a failed login (a user name is logged only for a successful login, at debug, as
`principal=User:<name>`). Refusal texts are fixed strings; `End` values name an API key, never bytes; `Secret` has no `Display` and a
redacting `Debug`, `Credentials` and `Authenticator` print user names only; `--sasl-user` is parsed from text clap cannot echo, and the
CLI errors leave out everything after the colon. Test (a planted password through a failed login, a stranger's token and a good
login with TRACE logging on, in a test binary of its own so the logging is the process's): `tests/redaction::no_password_or_token_is_logged_at_any_level`;
`kafka-auth credentials::tests::debug_output_names_the_users_and_never_a_password`, `authenticator::tests::debug_output_shows_no_password`,
`plain::tests::no_refusal_repeats_what_the_client_sent`, `doze-kafka cli::tests::the_password_of_a_user_is_in_no_debug_output_of_the_arguments_or_the_settings`,
`cli::tests::sasl_flags_that_cannot_work_are_refused_with_the_go_editions_words`,
`connection::tests::login::nothing_a_connection_ends_with_says_what_the_client_sent`, and the process test
`sasl::a_broker_started_with_sasl_logs_a_client_in_and_serves_it_and_prints_no_password` (the real binary at `--log-level trace`).

## SEC-14. Credentials
Built from `(user, password)` at start, nothing stored. Refused: an empty name, a NUL in a name, an empty password, a password that
is not ASCII or holds a NUL (no SASLprep, so only the bytes every client sends the same way), a name given twice (the messages name the
user, never the password). Names are compared as bytes, exactly (`ALICE` is not `alice`). Test: `kafka-auth credentials::tests::*`.

## SEC-15. The crypto is one file wide
`kafka-auth/src/crypto.rs` is the only code that names `hmac`, `sha2` or `subtle`; PBKDF2 (one output block) and base64 (strict,
canonical, padded) are in-tree on top of it, pinned by RFC 4231 (HMAC), FIPS 180 (SHA), RFC 7914 and OpenSSL-made PBKDF2 vectors for
SHA-256 and SHA-512, the RFC 7677 example's salted password, RFC 4648 base64 vectors, and a differential test against the `pbkdf2` crate
(a dev-dependency). PLAIN uses only the comparison; the rest is SCRAM's (8c, SEC-40 to SEC-51). Test: `kafka-auth crypto::tests::*`, `crypto::pbkdf2::tests::*`, `crypto::base64::tests::*`.

## SEC-40. SCRAM users are verifiers derived at start, and nothing is stored (slice 8c; source: ScramCredential, recorded `i=4096`)
`--sasl-mechanisms` may name `SCRAM-SHA-256` and `SCRAM-SHA-512` (exact case; the flag order is kept, the handshake answer follows SEC-7).
For every `--sasl-user` and every SCRAM mechanism that is on, `Credentials::with_scram` derives, once at start, a verifier: a
random 16-byte salt (new for every user and every mechanism, and new at every start), 4096 PBKDF2 rounds, `StoredKey = H(HMAC(salted,
"Client Key"))` and `ServerKey = HMAC(salted, "Server Key")`. The password is the same one PLAIN checks. Nothing is written to disk. A user
has a verifier for exactly the mechanisms that are on, so a hash that is off is an unknown user (the `Kind` filter in `scram::first`). Test:
`kafka-auth scram::tests::every_user_has_its_own_salt_for_each_hash`, `..::the_verifier_of_the_other_hash_is_not_used`,
`..::an_iteration_count_is_what_the_client_is_told`, `scram::keys::tests::the_kinds_map_to_mechanisms_and_sizes`,
`authenticator::tests::scram_is_offered_and_logs_in_with_the_mechanism_named`, `doze-kafka cli::tests::a_scram_user_logs_in_with_the_password_of_the_flag_and_with_no_other`.

## SEC-41. The exchange, and the vectors (RFC 5802, RFC 7677; recorded `good-256`, `good-512`, `scram-authenticate-v0/v1`)
SaslHandshake, then two SaslAuthenticate round trips. The first token `n,,n=user,r=<cn>` is answered error 0, message `""`,
`AuthBytes = r=<cn><sn>,s=<base64 salt>,i=4096` (exactly these three fields, in this order), lifetime 0; the second
`c=<b64>,r=<cn><sn>,p=<b64 proof>` is answered error 0, message `""`, `AuthBytes = v=<base64 ServerSignature>` and logs the connection in as
`User:<name>`; the harness checks the signature itself. The same bytes at SaslAuthenticate v0, v1 and v2. `AuthMessage = client-first-bare
"," server-first "," client-final-without-proof`, and the machine is pure (`scram::first`, `Awaiting::finish`), the server nonce an
argument. Vectors: the RFC 7677 exchange for SHA-256 (user `user`, password `pencil`: salted password, StoredKey, ServerKey, the proof
`dHzbZapWIk4jUhN+Ute9ytag9zjfMHgsqmmiz7AndVQ=` and the signature `6rriTRBi23WpRR/wtup+mMhUZUn/dB5nLTJRsjl95G4=`), and the same inputs
for SHA-512, which the RFC has none for: `tools/scram-vector.ts` (`bun tools/scram-vector.ts`, Node's OpenSSL PBKDF2 and HMAC) prints both,
and its SHA-256 output is the RFC's byte for byte, which is what vouches for the SHA-512 output beside it. A client written separately
from RFC 5802 (`kafka-auth::scram::client`, feature `client`, test-only) drives every exchange. Test: `scram::keys::tests::the_stored_and_server_keys_are_those_of_rfc_7677`,
`..::the_proof_and_the_server_signature_are_those_of_the_rfc_and_of_openssl`, `scram::tests::rfc_7677_through_the_machine`,
`..::a_right_password_logs_in_with_both_hashes`, `kafka-server session::steps::tests::scram::two_tokens_log_a_scram_user_in_with_the_challenge_and_the_signature_as_auth_bytes`,
`connection::tests::scram::a_scram_login_takes_two_tokens_and_then_the_connection_is_served`, `tests/scram::a_client_that_logs_in_with_scram_can_produce_and_fetch`,
`interop/run.sh -sasl` (four client libraries, both hashes), and the oracle (below).

## SEC-42. The client-first message is read as Kafka's regular expression reads it (source: `ClientFirstMessage.PATTERN`; recorded `first-*`, 19 connections)
`n,[a=<saslname>],[m=<value>,]n=<saslname>,r=<nonce>[,<alpha>=<value>]...`, the whole message and nothing else. The GS2 flag is `n`
only: `y,,`, `p=tls-unique,,`, `x,,`, a lower-case `n`, and no header at all are refused. The name is one or more characters of
`\x01-\x7F` except `,` and `=`, where `=` may only be `=2C` or `=3D` (exact case): `n=al=ice`, `n=alic=65` and an empty name are refused, `n=al=2Cice`
is a user called `al,ice` (unknown). The nonce is one or more visible ASCII characters, no comma and no space; an empty nonce, a nonce before the
name, no nonce, and `,,` after it are refused. `m=` takes one or more characters and is read and dropped (SEC-44). An extension is letters,
`=`, one or more characters (the value may hold `=`). A NUL or any byte over `\x7F` anywhere refuses the message, so a PLAIN token
and non-UTF-8 text are refused. Every refusal is the one generic 58 (SEC-47). Test: `scram::parse::tests::the_recorded_malformed_firsts_are_refused`
(the 17 refused shapes and more), `..::the_header_the_name_and_the_nonce_are_as_strict_as_the_regex`, `..::a_plain_client_first_is_read`,
`scram::tests::firsts::every_recorded_malformed_first_is_the_generic_refusal`; fuzz target `scram_first` (`// fuzz:parser scram_first`).

## SEC-43. The checks at client-first run in Kafka's order, and an unknown user is refused here (recorded `unknown-user-*`, `authzid-*`)
The shape (SEC-42), a `tokenauth=true` extension (SEC-D13), the user's verifier, and only then the authorization id. An unknown user, a known user
with another hash off, and a delegation-token login are answered the generic 58 **at client-first**, and a wrong password only at client-final (SEC-45):
the stage in which the refusal comes tells whether a user exists, exactly as in Kafka (the recording shows it, so it is kept). With the user
found, an `a=` that is not empty and not the user is `Authentication failed: Client requested an authorization id that is different from
username` (the one SCRAM text that is not generic); an unknown user with an `a=` gets the generic text, not this one (`authzid-other-user-unknown-user`).
The id is compared **as sent** (escapes and all) with the **unescaped** name, as Kafka does: for a user called `a,b=c`, `a=a=2Cb=3Dc` is refused,
and so is every other id; with no `a=` the user logs in by its escaped name. Own name (`a=alice`, with `c=bixhPWFsaWNlLA==` after) logs in.
Test: `scram::tests::an_unknown_user_is_refused_at_the_first_message_with_the_same_text`, `..::an_authzid_must_be_the_user_and_is_judged_after_the_user_exists`,
`..::a_name_with_a_comma_and_an_equals_sign_logs_in_by_its_escapes`, `..::the_verifier_of_the_other_hash_is_not_used`,
`kafka-server session::steps::tests::scram::a_refused_first_message_is_58_and_closes_before_a_challenge`, `tests/scram::an_unknown_user_is_answered_at_the_first_message_then_the_connection_is_closed`.

## SEC-44. What the client signs is rebuilt, not echoed (source: `clientFirstMessageBare`; recorded `first-mandatory-extension`, `first-optional-extension`)
The server signs `n=<saslname as sent>,r=<client nonce>` and then each extension as `,name=value`; the gs2 header and a reserved `m=` are not in it.
So a client that signs `m=ext,n=alice,...` (the harness does) is accepted at client-first and **fails its proof** at client-final (`first-mandatory-extension`),
and a client that signs `,x=ext` after the nonce logs in (`first-optional-extension`). A name sent twice counts once, with its last value, as in
Kafka's map. `AuthMessage`'s last part is `c=<base64 of the decoded binding, re-encoded>,r=<nonce>`: the binding is decoded and written back
canonically, which is what Kafka signs. Test: `scram::tests::firsts::a_reserved_extension_is_not_signed_and_a_plain_extension_is`, `scram::parse::tests::a_reserved_extension_and_extensions_after_the_nonce_are_read`.

## SEC-45. The client-final message, and the order of its checks (source: `ClientFinalMessage.PATTERN`, `ScramSaslServer`; recorded `final-*`)
`c=<base64>,r=<nonce>[,<alpha>=<value>]...,p=<base64>` and nothing else: the proof is last, so `,x=ext` after it is refused (`final-trailing-attribute`) while
`,x=ext` before it is read and dropped (SEC-46). Base64 is the padded alphabet (no URL alphabet, no white space, padding only at the end, as Java's
regex says) and, as Java's decoder, accepts bits set past the end of the data (`crypto::base64::decode_lax`); `c=` may be empty and `p=` too. Checks in
order: the shape; the nonce equals the server-first nonce exactly (`final-wrong-nonce`, `final-client-nonce-only`, `final-nonce-with-suffix`, and a
message made for another exchange, `replay-elsewhere`); the proof's length (SEC-48); the proof. A final message in the first state and a first message in
the second are not messages of the other's shape: the generic 58 (`final-without-first`, `first-twice`, `plain-token-as-final`, `final-empty`, `final-garbage`).
The channel binding is **never compared** with the first message's header: `c=eSws` (`y,,`) after `n,,` logs in (`final-wrong-channel-binding`), as do `c=` and
any valid base64; one that is not base64 is refused (`final-channel-binding-not-base64`). Decision D6, and Go's refusal of it is its known difference.
Test: `scram::parse::tests::the_recorded_malformed_finals_are_refused`, `..::the_channel_binding_is_decoded_and_nothing_more`, `scram::tests::finals::*`,
`crypto::base64::tests::the_lax_decoder_accepts_spare_bits_and_nothing_else`; fuzz target `scram_final` (`// fuzz:parser scram_final`).

## SEC-46. An extension before the proof is not signed, so it fails a client that signed it (recorded `final-extension-before-proof`)
`c=biws,r=<nonce>,x=ext,p=<proof>` is read; `x=ext` is dropped and the server signs `c=biws,r=<nonce>`, so a proof made over the message with the extension (the
harness's) fails: generic 58. A client whose proof does not cover it is logged in (the pattern lets it through; no recording tells, the source does).
Test: `scram::tests::finals::an_extension_the_client_signed_before_the_proof_fails_the_proof`, `..::an_extension_before_the_proof_that_the_proof_does_not_cover_is_dropped`,
`scram::parse::tests::extensions_before_the_proof_are_read_and_dropped`.

## SEC-47. Refusals: the generic 58, flushed, then closed (recorded `SCRAM_BAD`, `AUTHZID`)
Every refusal but two is SaslAuthenticate error 58 with `Authentication failed during authentication due to invalid credentials with SASL mechanism SCRAM-SHA-256`
(or `-512`, the mechanism of the handshake) and empty bytes; the text says nothing of why. The reason (`scram::Why`) goes to the debug log as an enum name and is
never put in the answer. The exceptions are the authzid text (SEC-43) and the silent close (SEC-48). The answer is written and the stream shut down before
the connection ends (SEC-9), whether the refusal came at the first token or the second; the next request gets nothing. Test:
`scram::tests::a_wrong_password_is_refused_at_the_final_message_with_the_generic_text`, `kafka-server session::steps::tests::scram::a_wrong_password_is_58_naming_the_mechanism_and_closes`,
`..::a_token_of_the_wrong_kind_for_the_step_is_58`, `connection::tests::scram::a_wrong_password_is_flushed_before_the_close`,
`tests/scram::a_wrong_password_is_answered_at_the_final_message_then_the_connection_is_closed`.

## SEC-48. A proof that is not as long as the hash gets no answer, only a close (source: `ScramFormatter.xor`; recorded A14: `final-proof-too-short`, `final-proof-empty`, `proof-of-512-under-256`)
Kafka's XOR of the signature with the proof throws `IllegalArgumentException` when the lengths differ, which is not a failed login, so no response is
sent and the connection closes. Here: after the nonce is judged (so a short proof with a wrong nonce is answered 58), a proof whose decoded length is
not 32 (SHA-256) or 64 (SHA-512) is `Refusal::ProofLength`, `steps::authenticate` returns `Hangup`, and the connection ends with `End::LoginAbandoned` and nothing written:
`AAAA`, an empty proof, 31 and 33 bytes, 4 KiB, and the proof of the other hash. Test: `scram::tests::finals::a_proof_that_is_not_a_hash_long_closes_without_an_answer`,
`..::the_nonce_is_checked_before_the_length_of_the_proof`, `kafka-server session::steps::tests::scram::a_proof_the_wrong_length_hangs_up_without_an_answer`,
`connection::tests::scram::a_proof_the_wrong_length_ends_the_connection_with_nothing_written`, `tests/scram::a_proof_that_is_not_a_hash_long_closes_the_connection_without_an_answer`.

## SEC-49. Nothing of a SCRAM login is printed (extends SEC-13)
The plaintext password, the salted password, `ClientKey`, `StoredKey`, `ServerKey`, the proof, the signature, both nonces and an unknown user's name appear in
no log line at TRACE, in no refusal text, and in no `Debug` output: `Verifier` prints its kind and cost only, `Awaiting` its mechanism only, `Session::Challenged`
the same, `Refusal` an enum. Test: `tests/redaction::no_password_or_token_is_logged_at_any_level` (a wrong password, a good login, an unknown user and a short
proof, both hashes, with the derived secrets computed from the salt the broker sent and searched for in hex and base64),
`scram::keys::tests::a_verifier_prints_neither_a_key_nor_the_salt`, `scram::tests::a_refusal_and_an_exchange_in_flight_print_no_secret`,
`session::steps::tests::scram::a_session_in_flight_prints_no_nonce`, and the process test
`sasl::a_broker_started_with_scram_logs_a_client_in_with_either_hash_and_prints_no_password`.

## SEC-50. In the middle of a SCRAM login (recorded `switch-to-plain`, `switch-to-512`, `handshake-256-then-512`, `versions-before-final`, `request-before-final`)
After the first token the session is `Challenged`, which `Session::admit` treats as `Handshaken`: only SaslAuthenticate is welcome; SaslHandshake (any mechanism,
a different one or the same) and ApiVersions are answered 34 in their shapes and the connection closes (SEC-5); any other API is closed unanswered (SEC-D1). The
30 s login budget covers both rounds. After the login SEC-10 applies unchanged: a replayed final message, a new first message, a null token and a handshake
are 34 and the user stays (`replay`, `reauth`, `third-message`). Test: `session::tests::after_the_handshake_only_a_token_is_welcome_and_two_apis_are_answered_then_closed`
(both states), `session::steps::tests::scram::a_handshake_in_the_middle_of_a_scram_login_is_34_and_closes`, `..::a_token_after_a_scram_login_is_34`,
`connection::tests::scram::between_the_two_tokens_a_strangers_body_is_still_not_decoded`, `..::the_login_budget_covers_both_rounds`,
`tests/scram::a_request_between_the_two_tokens_is_not_served_and_a_second_login_is_illegal`.

## SEC-51. The proof is checked in constant time; the server nonce is unpredictable
`ClientKey' = ClientSignature XOR proof`, `H(ClientKey')` compared with `StoredKey` by `crypto::ct_eq` (the same time wherever they differ). The server nonce is
18 bytes from the operating system (`getrandom`, already in the build through the broker and the metadata store), written in base64: 24 characters, never a comma.
A salt need only be unique and is random too. If the system gives no random bytes, the login is refused (generic 58) and the start fails for the salts. Test:
`scram::tests::server_nonces_are_fresh_printable_and_without_commas`, `scram::keys::tests::a_wrong_proof_or_a_proof_of_another_length_is_never_right`
(one flipped bit anywhere), `..::a_right_proof_with_bytes_added_is_wrong`.

## The oracle, slice 8a (2026-11-12; superseded by 8b below)
`sasl-plain` could not be replayed whole then: its fixture needs CreateACLs/DescribeACLs (slice 8b). With the fixture's ACL step made a no-op
in a scratch copy of the harness, every login step of the 41 connections matches the recording (handshake, token, 33, 34, 58 with the
exact texts, every close) except: `fixture-acls` (ACL API), and the 13 `*-who` Metadata probes that expect `for-bob` (or `for-alice`) to be
error 29 (needs ACLs). The steps `request-before-token-who` and the three ApiVersions list steps are the Go edition's own known
differences (SEC-D1 and the API list) and are not reported for a foreign binary.

## The oracle, slice 8b (2026-11-12)
`sasl-plain` now passes whole (its fixture's CreateAcls works and the 13 `*-who` probes answer 29, in Kafka's order of authorized topics
first), as do `acl-unknown-codes` (all 77 steps: the closes, the -1 answers, the 42 texts) and `idempotent-write-from-topic-write`.
`version-matrix` differs in no ACL row: DescribeAcls v1 to v3 with a 0 in the filter closes, and CreateAcls and DeleteAcls with an empty
list answer an empty list; it still differs on DescribeCluster v0 to v2, GetTelemetrySubscriptions v0 and PushTelemetry v0 (Phase 9).

## SEC-16. Who is checked, and when there is no SASL (decision D4; **deviation SEC-D7**)
Every request is authorized as the connection's principal: `User:<user>` after a login, `User:ANONYMOUS` for a connection that never logged
in (SASL off, or a broker started without it). The ACL APIs work and ACLs are enforced on a broker with no SASL; real Kafka with no
authorizer answers SECURITY_DISABLED (54) and enforces nothing. With the default `--acl-default allow` and no ACL stored, nothing is
refused, so a broker that uses none of this behaves as it did at the close of Phase 7 (a check is one atomic load and a length test:
`Authorizer::allows`). Test: `kafka-server tests/acl::without_sasl_the_connection_is_the_anonymous_user_and_the_acls_bind_it`,
`tests/acl::what_a_login_proves_is_what_the_acls_are_checked_against`, `kafka-broker tests/acls_access::with_no_sasl_the_anonymous_principal_is_checked_like_any_other`;
the allow-all path is proved by every pre-existing broker, server and interop test passing unchanged.

## SEC-17. The principal and the host that are matched (SAD:463-469, 492)
An ACL's principal matches the session principal exactly (type and name, case-sensitive) or is the string `User:*`, which matches a
principal of any type; `Group:*` is no wildcard. An ACL's host is `*` or the same text as the client's address: no CIDR, no prefix, no
hostname. The address is written as Java's `getHostAddress` writes it (an IPv4-mapped IPv6 address is the IPv4 one; IPv6 is eight
unshortened hexadecimal groups, `0:0:0:0:0:0:0:1`, so an ACL written `::1` never matches, as in Kafka). A connection with no peer address
(a test) matches only `*`. Test: `kafka-auth authz::tests::the_wildcard_principal_is_exactly_user_star`, `..::names::a_host_is_star_or_the_same_text`,
`kafka-broker caller::tests::*`, `tests/authz_metadata_rules::an_acl_for_a_host_applies_to_that_address_only`.

## SEC-18. Which ACLs cover a resource, and the default (SAD:310-364, 422, 584-594)
An ACL covers a resource of its type whose name it names exactly (LITERAL), or starts with its name (PREFIXED), or every resource of the type
(LITERAL `*`). A LITERAL ACL whose name is only a prefix of the resource's covers nothing. Types are separate namespaces. **The default
(`--acl-default`, Kafka's `allow.everyone.if.no.acl.found`) applies only when no ACL covers the resource, whoever it is for, whatever
host or operation it names**; one ACL for anybody on a topic (or a prefix or the wildcard covering it) turns the default off for that topic,
and the rest are refused. Test: `authz::tests::the_default_applies_only_to_a_resource_no_acl_covers`, `..::names::literal_prefixed_and_wildcard_names`,
`..::names::the_cluster_resource_has_its_own_namespace`, `kafka-broker tests/authz_metadata_rules::the_default_applies_only_to_a_topic_nothing_covers`,
`tests/authz_metadata_rules::prefixes_wildcards_and_the_any_user_are_honoured`.

## SEC-19. DENY wins, and super users bypass everything (SAD:236-238, 424-429, 584-594)
In order: a super user (`--acl-super-user`, the whole principal string) is allowed, DENY ACLs and the default notwithstanding; else any
covering DENY that matches the principal, host and operation refuses, wherever it is found (a specific name, a prefix, or the wildcard,
before or after the ALLOW); else any matching ALLOW permits; else the default if no ACL covers the resource; else refused. Test:
`authz::tests::deny_wins_over_allow_whatever_the_order`, `..::a_super_user_bypasses_every_acl_including_a_deny`, the model test (SEC-29).

## SEC-20. Operations: ALLOW implies, DENY is exact (SAD:495-520)
ALL matches every operation, in an ALLOW and in a DENY. An ALLOW of READ, WRITE, DELETE, ALTER or DESCRIBE grants DESCRIBE; an ALLOW of
ALTER_CONFIGS grants DESCRIBE_CONFIGS; any other operation must be equal (CREATE, IDEMPOTENT_WRITE and the rest imply nothing). A DENY is
exact: DENY READ does not deny DESCRIBE, and an ALLOW READ still describes. Test: `authz::tests::a_deny_is_exact_and_an_allow_implies`,
`kafka-broker tests/authz_metadata_rules::read_write_delete_and_alter_imply_describe_but_a_deny_is_exact`.

## SEC-21. Kafka's asterisk quirk, kept (SAD:354-362, 398-405)
Kafka's walk of the ACLs skips a LITERAL ACL whose name is a prefix of the resource's name, and that includes the wildcard `*` for a
resource whose name starts with an asterisk and is not just `*` (a group or transactional id called `*x`): the wildcard does not cover
it. Topic names cannot start so; a group id or transactional id can. Found by the model test (SEC-29) and kept for parity. Test:
`authz::tests::names::literal_prefixed_and_wildcard_names` (the `*x` lines).

## SEC-22. `authorizeByResourceType`, for InitProducerId (Authorizer.java 184-293)
May the principal do an operation on at least one resource of a type? First the operation on a resource called `hardcode` is authorized
as usual (so super users pass, and so does everyone when the default allows and no ACL covers that name). Then the ACLs of the type for
this principal and host whose operation is equal or ALL (nothing implied) are collected: a DENY of `*` refuses at once; an ALLOW of `*`
permits; an ALLOW of a name or prefix permits unless a DENY of the same name (literal against literal) or of a prefix of it (itself
included) dominates it. Test: `authz::tests::by_type::*` (one test per sentence), the model test (SEC-29).

## SEC-23. The ACL APIs: a 0 closes first, then who may use them
In every one of CreateAcls, DeleteAcls and DescribeAcls a raw code of 0 (UNKNOWN) in a resource type, pattern type, operation or
permission closes the connection before anything is authorized or stored (recorded: `*-0` steps); only the number 0 counts, so 99 and -1
are read later. Then CreateAcls and DeleteAcls need ALTER on the cluster `kafka-cluster`, DescribeAcls DESCRIBE: a caller without it is
CLUSTER_AUTHORIZATION_FAILED (31) on every result (DescribeAcls: top level; its `ErrorMessage` stays `""`). Test:
`kafka-broker tests/acls_access::a_caller_with_nothing_on_the_cluster_is_refused_with_31_everywhere`,
`..::describe_and_alter_on_the_cluster_open_the_apis_one_by_one_and_a_deny_closes_them`, `tests/acls::the_recorded_filter_steps_describe_and_delete`.

## SEC-24. CreateAcls (recorded `acl-unknown-codes`; ACM:95-177, AclApis:84-138)
After the close and the cluster check: a code only a filter can have (resource type ANY, pattern type ANY or MATCH, operation ANY,
permission ANY) in any creation fails every result with -1 (UNKNOWN_SERVER_ERROR) and no message, as a request Kafka cannot build. Then each
creation in turn, answered 42 (INVALID_REQUEST): CLUSTER with a name other than `kafka-cluster` ("The only valid name for the CLUSTER
resource is kafka-cluster"); an empty name ("Invalid empty resource name"); a resource type, pattern type, operation or permission that is no
value ("Invalid resourceType UNKNOWN", "Invalid patternType UNKNOWN" (only LITERAL and PREFIXED), "Invalid operation UNKNOWN", "Invalid
permissionType UNKNOWN"); a principal with no colon ("Could not parse principal from \`x\` (no colon is present separating the principal type
from the principal name)"). Nothing else is checked (the host may be empty, the principal `User:`, an operation meaningless for the type).
A success is 0 with the **empty** message; one stored already or twice in the request is stored once and succeeds; the stored ACLs are one
write, and the next request sees them. Test: `kafka-auth authz::tests::validate::{a_code_of_zero_is_found_in_each_field,any_and_match_cannot_be_created_at_all,a_creation_is_refused_in_kafkas_words_and_order,nothing_else_is_validated}`,
`kafka-broker tests/acls::{the_recorded_create_steps_good_and_the_zeros,the_recorded_create_steps_any_and_the_codes_that_are_none,one_bad_item_in_a_request_decides_as_kafka_decides,the_other_refusals_of_a_creation_are_kafkas_words}`,
`tests/acls_use::acls_are_listed_by_resource_and_come_back_as_sent`.

## SEC-25. DeleteAcls (ACM:179-226)
Each filter is judged against the ACLs as they were before the request: two filters that select the same ACL both list it and it is
removed once, in one write. A filter whose resource type or pattern type is no value is 42 "Unknown patternFilter."; whose operation or
permission is none, 42 "Unknown entryFilter." (the pattern half first). A filter that selects nothing succeeds with an empty list and **no**
message (null, unlike CreateAcls and DescribeAcls). More than 10,000 ACLs removed by one request fail every filter with 42 "Cannot remove
more than 10000 acls in a single delete operation." Test: `kafka-auth authz::tests::validate::a_filter_is_checked_in_two_halves`,
`kafka-broker tests/acls::the_recorded_filter_steps_describe_and_delete`, `tests/acls_use::delete_lists_what_each_filter_matched_and_removes_it_once`.

## SEC-26. DescribeAcls and the filter (ResourcePatternFilter.matches, AccessControlEntryFilter.matches)
The answer is grouped by resource (type, name, pattern) and sorted (Kafka's order is a hash map's; a client sorts; tests compare sorted).
`ErrorMessage` is `""`. A filter with a code that is no value (99, -1) matches nothing and is not an error. The filter: ANY and null match
everything; strings are exact and case-sensitive (a filter host of `*` matches only ACLs whose host is the literal `*`); a name compares
with equals when the filter pattern is ANY or the ACL's own (a PREFIXED filter does no prefix matching on PREFIXED ACLs); MATCH
means "which ACLs apply to a resource of that name": the literal, the `*` literal, and the prefixes it starts. Test:
`authz::tests::validate::{the_any_filter_matches_everything_and_nulls_are_any,a_name_filter_is_exact_unless_the_pattern_is_match,a_host_filter_star_matches_only_acls_whose_host_is_star}`,
`tests/acls_use::a_filter_selects_by_each_field_and_match_means_which_apply`; fuzz target `acl_filter` (`// fuzz:parser acl_filter`, in
`crates/kafka-auth/fuzz`; 1.6 million runs of it were clean when it was written).

## SEC-27. Metadata (KA:880-1002; recorded `sasl-plain` identity probe)
DESCRIBE on each topic asked for, checked **before** the topic is looked for, so a topic that does not exist is hidden exactly like one
that does. A topic the caller may not describe is answered 29 with its name kept, no partitions, `IsInternal` false and a zero topic id
(by id: no name, the id the cache knows); a list of all topics (null; at v0 also empty) holds only the topics it may describe, with no
denied entries. Order of the answer, as Kafka's: what the caller may see in the order asked, then topics it may describe but not create,
then those it may not describe (Kafka's order beyond four topics is a Scala set's; this one stays in request order within a group).
Auto-creation of a missing topic that may be described needs CREATE on the cluster or on the topic, else the entry is 29 and nothing is
made. Test: `kafka-broker tests/authz_metadata::the_recorded_identity_probe`, `..::a_topic_that_does_not_exist_is_hidden_the_same_way`,
`..::a_list_of_all_topics_shows_only_those_the_caller_may_describe`, `..::by_id_a_denied_topic_is_29_with_the_id_and_no_name`,
`tests/authz_metadata_rules::a_missing_topic_is_made_only_for_a_caller_that_may_create_it`.

## SEC-28. InitProducerId (KA:1603-1666; recorded `idempotent-write-from-topic-write`)
Checked first, before the id is judged. With a transactional id: WRITE on that id, else 53. With none: IDEMPOTENT_WRITE on the cluster, or
`authorizeByResourceType(WRITE, TOPIC)` (SEC-22), else 31, so a principal that may write to any topic may be given a producer id. The
refusal is the whole answer (producer id -1, epoch -1) and leaves no mark on the transactional id. Test: `kafka-broker tests/authz_init_producer_id::*`.

## SEC-29. The authorizer is checked against a transcription of Kafka's
`kafka-auth/tests/authz_model`: `StandardAuthorizerData.authorize`/`findAclRule`/`checkSection`/`findResult`/`MatchingRuleBuilder` and `authorizeByResourceType`
transcribed (the ACLs kept in `StandardAcl.compareTo` order and walked from an exemplar with the jump to a shorter prefix), run against
the real one on 6,000 random ACL sets of up to 13 ACLs, 30 random requests each, for both procedures (`AUTHZ_MODEL_SEEDS` sets the number); a
second test counts that the random sets reach each branch. It found SEC-21. Test: `the_authorizer_agrees_with_kafkas_algorithm_on_random_acl_sets`,
`the_random_sets_reach_every_branch_of_the_decision`.

## SEC-30. Where ACLs live
The seven fields as the wire carried them (so a client reads back what it stored), a version byte and postcard, in the table `acls` of the
metadata database, keyed by the fields so a repeat lands on the same row; a database made before the table existed reads as no ACLs. A change
is built from the set in force, written in one transaction under the store's write lock, then swapped in (an `ArcSwap` snapshot) while the
broker's ACL lock is held, so two changes at once lose nothing and a failed write leaves the set as it was. The set is loaded at open. Test:
`kafka-meta store::acls::tests::*`, the crash workload (`tests/crash`, adds and removes under every power cut),
`kafka-broker tests/acls_access::{acls_outlive_a_restart_and_still_decide,changes_at_once_lose_nothing}`. Flags: `--acl-default allow|deny` (default
`allow`), `--acl-super-user Type:name` (see `binary.md` B4).

## The oracle, slice 8c (2026-11-12)
`sasl-scram` (69 connections after the fixture), replayed against the release binary in a scratch copy of the harness with the DescribeACLs wait of the
fixture removed (ACL APIs are slice 8b), flags as the harness gives them (`--sasl-mechanisms PLAIN,SCRAM-SHA-512,SCRAM-SHA-256`, three users).
**Every handshake, SaslAuthenticate (v0, v1, v2) and close step matches the recording**: the 58 texts for the 256 and the 512 mechanism, the AUTHZID text, the
34 answers, the silent closes (wrong-length proofs, `unknown-user` followed by a final message, `switch-to-*`), the server-first shape and the server signature
(the harness verifies it). What differs, and why: `fixture-acls` (the ACL API closes the admin connection: slice 8b), and the `*-who` Metadata probes after a
successful login (14 of them: `good-256`, `good-512`, `authzid-same-*`, `first-optional-extension`, `replay` twice, `reauth` four times, `third-message`,
`scram-authenticate-v0/v1`): they expect `for-bob` to be error 29 and need ACLs. The probes after a refusal are `CLOSED` and match. `final-wrong-channel-binding` and the two
`request-before-*-who` probes are in the Go edition's known differences, so the harness does not report them for a foreign binary: the first is pinned by
`scram::tests::finals::the_channel_binding_is_not_compared_with_the_header`, the others are SEC-D1. `sasl-plain` is unchanged from 8a.

## Mutation check, slice 8c
Twenty-three hand-made mutations of `scram.rs`, `scram/parse.rs`, `scram/keys.rs`, `credentials.rs`, `session.rs` and `session/steps.rs` (nonce check, proof-length check and
guard, authzid compared with the escaped name, token-auth case, hash-kind filter, binding re-encoding, nonce class, `=3D`, NUL, the last-field rule, the
server key, iterations, nonce size, salt randomness, empty `m=`, empty extension values, duplicate extensions, the silent close, `Challenged` admitted,
the close after a refused first message, the stored exchange, the hash of the recovered key), each checked as applied, in a copy of the tree. First run: 20 killed, 3 survived
(the kind filter was redundant with the keyed lookup, a NUL in a name was caught only by the nonce class, a last field that merely looked like base64 was
accepted); a test was added for each and the three are killed. Final: 23 of 23.

## The topic APIs, slice 8d-1 (2026-11-12)
Every rule below is **(source)**: none of these denials is recorded (only Metadata and InitProducerId are), so each is written from Kafka 4.3.1's
`KafkaApis.scala` (KA), `ControllerApis.scala` (CA), `ConfigHelper.scala` (CH), `ConfigAdminManager.scala` (CAM), `AuthHelper.scala` (AH) and
`ReplicationControlManager.java` (RCM), with the line it was read at (re-read against the tag, 2026-11-12; the matrix of `phase8/kafka-semantics.md`
2.1 was right in every row used here, and its line numbers are the old ones, so the ones below are the tag's). The tests are from the same
reading, not from a run of Kafka: **the one real risk is a reading that differs from the JVM**, which `authz-matrix` (plan D9) would settle.
The checks are in `crates/kafka-broker/src/guard.rs` (the resource type, the cluster's name and the denial texts, written once) and in each
handler; each is asked **before** the topic is looked for (a topic that is not there is hidden like one that is) and, with no ACL and the
default allowing, costs one atomic load and allocates nothing (`Authorizer::allows`), asked once per topic of a request and never per record
batch.

## SEC-60. The five things every topic row does (tests: `kafka-broker tests/authz_topics_matrix.rs`, one table over the eleven APIs)
For each API, with the operation of SEC-61 to SEC-71 on the topic: a caller with the ACL is served; one with none is refused 29 (the item's own
code, in its own place); an ACL for another topic or for an operation that does not imply the needed one is no use (`an_acl_on_another_topic_is_not_enough`,
`an_operation_that_does_not_imply_the_needed_one_is_not_enough`, `the_weaker_right_that_the_stronger_one_implies_is_not_enough`); a super user is
served though a DENY names it; a DENY beats an ALLOW of the same operation, or an ALLOW of ALL against a DENY of the operation, in either order;
`User:*`, a PREFIXED ACL and the name `*` grant, and a PREFIXED DENY beats a literal ALLOW; the default (`--acl-default`) applies only to a topic no
ACL covers, and an ACL for another host covers it (turns the default off) without allowing this host; a topic that does not exist is 29 to a caller
that may not use it and the API's "unknown" to one that may; and a refused request changes nothing.

## SEC-61. CreateTopics: CREATE on the cluster, else on the topic (source: CA:391-449; RCM:815-843; tests `authz_topics_create.rs`)
The cluster is asked once (CREATE on `kafka-cluster`); if it is not allowed, each topic is asked for CREATE. A name listed twice is set aside as
42 "Duplicate topic name." before anyone is asked (CA:395-408). A topic the caller may not create is taken out before it is looked at, so a name
that exists (36) or is invalid (17) is not told to a caller that may not create it: **29 with "Authorization failed."**, `NumPartitions` and
`ReplicationFactor` -1, no id; these answers come after all the others (CA:427-445). `validate_only` asks the same. A caller that may create but
may not DESCRIBE_CONFIGS the topic (ALTER_CONFIGS implies it) is told it was made without the configs: `TopicConfigErrorCode` 29 (a tagged field of
v5 and later), `NumPartitions` and `ReplicationFactor` -1, an empty config list, and the topic id (RCM:840-842). Because the default is per resource, a
broker with the default allowing and no ACL on the cluster lets anyone create anything (the cluster is "covered by no ACL"); an ACL for anyone on the
cluster turns that off. Test: `an_unauthorized_topic_is_29_with_kafkas_text_and_no_numbers_and_comes_last`, `a_name_that_is_taken_or_invalid_is_not_revealed_to_a_caller_that_may_not_create_it`,
`a_duplicate_name_is_answered_42_before_authorization_is_asked`, `without_describe_configs_the_topic_is_made_and_its_configs_are_withheld`,
`create_on_the_cluster_covers_every_name_and_deny_on_the_topic_does_not_undo_it`, `authz_topics_matrix::with_the_default_allowing_a_cluster_no_acl_covers_is_open_and_covers_every_topic`.

## SEC-62. DeleteTopics: DELETE on the cluster, else DESCRIBE and DELETE on the topic (source: CA:198-345; test `authz_topics_create.rs`)
CLUSTER DELETE is asked once and grants both. Otherwise, **by name**: a topic the caller may not DESCRIBE is 29 with the name (id zero) whether it
exists or not; one it may describe, present, and may not DELETE is 29 with the name and no id; one it may describe and that is absent is 3. **By id**
(v6): a present topic it may not DELETE is 29 with the id the caller sent, and the name only if it may DESCRIBE it (CA:307-345). The message is
"Topic authorization failed." (v5 and later carry it). DELETE implies DESCRIBE, but a DENY of DESCRIBE hides the topic from the deleter. A DENY of
DELETE on the topic does not undo a cluster DELETE. Test: `delete_topics_by_name_hides_what_a_caller_may_not_describe`,
`delete_topics_by_id_shows_the_name_only_to_a_caller_that_may_describe_it`, `delete_on_the_cluster_grants_both_describing_and_deleting`,
`delete_implies_describe_but_a_deny_of_describe_hides_the_topic_from_the_deleter`.

## SEC-63. Produce: WRITE on each topic; WRITE on the transactional id if a batch is transactional (source: KA:396-451, 507-521; test `authz_topics_data.rs`)
First, if the first batch of any partition is transactional: WRITE on the request's transactional id, a missing id being denied, as a whole-request
error: 53 on **every** partition (KA:398-405; this is the existing "transactional bit and no id" rule, now also "no right to the id"). Then each
topic entry is asked for WRITE once, before the topic is looked for: 29 on each of its partitions, no offsets (KA:431-441). **Produce asks for nothing
else**: an idempotent producer needs no IDEMPOTENT_WRITE here, and IDEMPOTENT_WRITE on the cluster does not let a caller write a topic. With `acks=0` a
refusal closes the connection and a success is not answered. A request with no transactional batch asks nothing of the id. Test:
`produce_answers_each_topic_by_its_own_right_and_stores_only_what_was_allowed`, `every_partition_of_a_refused_topic_is_refused_and_a_missing_one_is_hidden`,
`produce_needs_only_topic_write_not_idempotent_write`, `a_transactional_batch_needs_write_on_the_transactional_id_before_any_topic_is_looked_at`,
`a_batch_that_is_not_transactional_asks_nothing_of_the_transactional_id`, `with_no_acks_a_refusal_closes_the_connection_and_a_success_is_not_answered`.

## SEC-64. Fetch: READ on each topic; a follower needs CLUSTER_ACTION (source: KA:563-620; test `authz_topics_data.rs`)
A consumer (`replica_id` -1) is asked READ per topic, before the topic is looked for: 29 on each partition, in the order asked; the answer is
immediate (an error ends a long wait) and the topic's records do not leave. From v13 the check is on the name the id stands for, and an id that stands
for nothing is UNKNOWN_TOPIC_ID to everyone, before authorization (KA:607). A request from a follower (`replica_id` 0 or more) is asked
CLUSTER_ACTION on the cluster once, and then needs no topic right; without it every partition is 29 (KA:586-600; this broker has no followers, so such a
request is read like a consumer's once allowed: deviation SEC-D19). Fetch sessions (KIP-227) are not kept, so the session bookkeeping that Kafka does
before authorization is the existing "session not found" answer. Test: `fetch_answers_each_partition_by_the_topics_right_in_every_version`,
`a_refused_fetch_is_answered_at_once_and_does_not_wait_for_data`, `a_follower_fetch_needs_cluster_action_and_no_topic_right`,
`fetch_by_topic_id_judges_the_name_the_id_stands_for_and_an_unknown_id_is_unknown_to_all`.

## SEC-65. ListOffsets: DESCRIBE on each topic (source: KA:781-821; test `authz_topics_data.rs`)
A topic the caller may not DESCRIBE (READ, WRITE, DELETE and ALTER each grant it, a DENY of READ does not take it away) is answered 29 on each
partition with timestamp and offset -1; those entries come **after** the answers for the topics it may (KA:805-807). Test:
`list_offsets_puts_the_refused_topics_after_the_others_with_no_offset`, `list_offsets_is_granted_by_any_right_that_implies_describe_and_denied_only_by_describe`.

## SEC-66. DeleteRecords: DELETE on each topic (source: KA:1538-1601; test `authz_topics_data.rs`)
29 on each partition of a topic the caller may not DELETE, low watermark -1, before the topic is looked for; nothing is deleted. Test:
`delete_records_refuses_a_topic_per_partition_with_no_low_watermark_and_deletes_nothing`.

## SEC-67. CreatePartitions: ALTER on each topic (source: CA:826-861; test `authz_topics_create.rs`)
Names listed twice are set aside first (42 "Duplicate topic name.", both removed, one answer); then the topics the caller may not ALTER (DESCRIBE and
READ do not do) are 29 with **no message**, whether they exist or not; then the rest. The answers are in that order. Test:
`create_partitions_needs_alter_and_answers_the_refused_after_the_duplicates_with_no_message`, `create_partitions_with_describe_or_read_only_is_refused_and_changes_nothing`.

## SEC-68. DescribeConfigs: DESCRIBE_CONFIGS on the resource (source: CH:49-80; test `authz_topics_configs.rs`)
A topic: DESCRIBE_CONFIGS on it (ALTER_CONFIGS grants it, DESCRIBE does not). BROKER, BROKER_LOGGER and CLIENT_METRICS: DESCRIBE_CONFIGS on the cluster.
GROUP: on the group. A resource refused is answered 29, 31 or 30 with that error's own words ("Topic authorization failed." and so on), no configs and
its type and name; it comes **after** the answers for the resources the caller may see; this is asked before the topic is looked for. A resource type
nothing knows is left to the request (42), not to the ACLs. Test: `describe_configs_answers_the_refused_resources_after_the_others_with_no_configs_and_the_errors_words`,
`describe_configs_is_granted_by_alter_configs_and_a_topic_that_is_not_there_is_hidden`, `describe_configs_leaves_an_unknown_resource_type_to_the_request_and_not_to_the_acls`.

## SEC-69. AlterConfigs and IncrementalAlterConfigs: ALTER_CONFIGS, after the request is found well-formed (source: CAM:100-160, 209-261; CA:472-554, 715-794; test `authz_topics_configs.rs`)
The order, per resource: a resource listed twice (both copies 42 "Each resource must appear at most once."); duplicate config keys ("Error due to
duplicate config keys"); null values ("Null value not supported for : a, b"); for a broker its name ("Node id must be an integer, but it is: x",
"Unexpected broker id, expected 1, but received 7") and its configs' validity, all **before authorization**, so a caller with no right sees these and
never a 29 or 31 for the resources they affect (the broker preprocesses before forwarding to the controller); then ALTER_CONFIGS: a topic on the
topic (29 "Topic authorization failed."), a broker or client metrics on the cluster (31), a group on the group (30), and, in the incremental API only,
BROKER_LOGGER on the cluster; only then the topic's name, its existence and its configs. The answers are in the order asked. Test:
`a_resource_the_caller_may_not_alter_is_refused_with_that_resources_error_and_words`, `the_loggers_of_a_broker_are_the_clusters_in_the_incremental_api_only`,
`preprocessing_errors_are_said_before_authorization_is_asked`, `a_topic_the_caller_may_not_alter_is_29_even_when_it_is_missing_or_named_badly`,
`altering_needs_alter_configs_and_nothing_else_grants_it`.

## SEC-70. DescribeProducers: READ on each topic, after its name (source: KA:2485-2529; test `authz_topics_data.rs`)
Per topic, in Kafka's order: a name that cannot be a topic's (INVALID_TOPIC_EXCEPTION, the same words as CreateTopics), then READ (29 with
"Topic authorization failed." on each requested partition), then whether the topic is there. (Before this slice an invalid name was answered as an
unknown topic.) Test: `describe_producers_refuses_each_partition_with_the_error_text_and_judges_the_name_first`.

## SEC-71. Metadata: the authorized-operations fields (source: KA:960-981, AH:62-76; test `authz_metadata_ops.rs`)
From v8, with `IncludeTopicAuthorizedOperations`, every topic entry has a bit `1 << code` for each of READ, WRITE, CREATE, DELETE, ALTER, DESCRIBE,
DESCRIBE_CONFIGS and ALTER_CONFIGS that the caller is allowed on the topic of that name (ALL is never a bit; the implications of SEC-20 count, a
DENY is exact), also for the entries that are refused, by name. With `IncludeClusterAuthorizedOperations` (v8 to v10 only) the cluster's: 0 if the
caller may not DESCRIBE the cluster, else a bit for each of CREATE, ALTER, DESCRIBE, CLUSTER_ACTION, DESCRIBE_CONFIGS, ALTER_CONFIGS and
IDEMPOTENT_WRITE it may. A field not asked for, or not in the version, is the schema default (-2147483648). A super user, and a broker with no ACLs and the default
allowing, have every bit. The auto-creation check (CREATE on the cluster or the topic, 29 instead of 3) was already SEC-27. Test: all of
`authz_metadata_ops.rs`.

## SEC-72. How these are held
`authz_topics_matrix.rs` (SEC-60), `authz_topics_create.rs`, `authz_topics_data.rs`, `authz_topics_configs.rs`, `authz_metadata_ops.rs` (broker handlers,
a real ACL set each), and on a real socket `kafka-server tests/acl_topics.rs::a_user_is_refused_on_a_topic_it_has_no_acl_for_and_served_on_one_it_has`
(a SASL PLAIN user: served on a topic it has an ACL for, refused 29 on another, on Produce, Fetch, ListOffsets and CreateTopics). The allow-all path is
proved by every earlier broker, server and interop test passing unchanged except the 8b tests that made their topics as the anonymous caller of a
closed broker (they now make them as the super user: `Rig::create_as_admin`).

Mutation check (25 hand-made mutations of these checks, each applied to a copy of the tree and checked as applied, 2026-11-12): a check removed (CreateTopics,
ListOffsets), a wrong operation (DeleteRecords ALTER for DELETE; Fetch DESCRIBE for READ; DescribeConfigs ALTER_CONFIGS for DESCRIBE_CONFIGS; DescribeProducers
DESCRIBE for READ; CreatePartitions CREATE for ALTER), a wrong resource type (Produce asked of the cluster; the topic's bits asked of the cluster), a check after
the change (AlterConfigs), a check before the validation it must follow (a broker's AlterConfigs), a refusal on the wrong place or in the wrong order
(ListOffsets loses the topic's name; DescribeConfigs and CreateTopics put the refused first), any transactional id accepted (Produce), the wrong code
(Produce answers 3), existence leaked (DeleteTopics by name, and the name shown by id), the cluster's DELETE grant lost, the cluster's bits told without
DESCRIBE or in v11, a follower asked for READ, replica 0 read as a consumer, configs told without DESCRIBE_CONFIGS, a name judged after READ in DescribeProducers.
**25 of 25 were killed**. (The first table asked a caller with only an unrelated operation, which would not have killed the weaker-operation
mutations; that was seen by reading before the run, and `the_weaker_right_that_the_stronger_one_implies_is_not_enough` was added first.)

## TLS, slice 8e (SEC-140 to SEC-159)

A cargo feature (`tls`, off by default) in `kafka-server` and `doze-kafka`: `crates/kafka-server/src/tls.rs` (the certificate, the key, the handshake),
`accept.rs` (where the handshake runs), `crates/doze-kafka/src/cli/tls.rs` (the flags). rustls 0.23 with the ring provider, tokio-rustls 0.26, PEM through
`rustls-pki-types` (decision D2; the weight is in `weight.md`). The Kafka references are `kafka-semantics.md` section 4 (KC = `KafkaChannel.java`); the
Go edition's are `go-oracle-rust.md` B3. There is no recording of TLS (the claims list it as not recorded), so the tests below, and the real clients of
`interop/run.sh -tls` (sarama, franz-go, kafka-go and librdkafka over SASL_SSL and over SSL), are the evidence. The fixtures are `kafka-server/tests/tls-fixtures/`.

## SEC-140. TLS is a build choice and a start choice; neither changes a broker that does not use it
Without `--features tls` no TLS code is compiled, `Config` has no `tls` field and the listener is plaintext. With it, no `--tls-cert` and `--tls-key` means
plaintext still. Both flags together put every connection of the listener behind a handshake. Test: `cli::tests::tls_is_refused_*` (default build),
`cli::tests::tls::a_certificate_and_a_key_turn_tls_on_and_none_leaves_it_off`; every pre-existing test is the rest of the proof (the default build's tests are unchanged).

## SEC-141. The flags: both or neither, refused without the feature, in the Go edition's words
`--tls-cert FILE` and `--tls-key FILE`. One without the other stops the start with `config: TLS needs both --tls-cert and --tls-key, or neither`. In a build without the
feature either flag is refused with a message that says to rebuild with `--features tls` (and that it needs a C compiler), exit non-zero; they are never ignored,
because a broker that ignored them would send in the clear. Test: `cli::tests::tls::one_of_the_two_is_refused_in_the_go_editions_words`,
`cli::tests::tls_is_refused_and_not_ignored_*` (default), `tests/process sasl::flags_that_cannot_be_honoured_stop_the_start_and_say_why`.

## SEC-142. A certificate or key that cannot be used stops the start, before anything is bound, and never falls back to plaintext
The files are read once, at start. A missing or unreadable file, a file with no certificate, a file with no key (including a certificate where the key goes and the reverse),
an encrypted key, bad PEM, a key that does not belong to the first certificate, a key this build cannot use: each is an error that names the file (or both, for a mismatch) and says
what is wrong. The words of the stop are the Go edition's: `config: cannot load the TLS certificate and key (--tls-cert X, --tls-key Y): <reason>`. No listener is left behind.
Test: `tls::tests::{a_missing_file_is_named, an_empty_file_or_the_wrong_kind_of_file_says_what_is_missing_and_where, a_key_that_belongs_to_another_certificate_is_a_mismatch_naming_both_files, an_encrypted_key_is_recognised_and_garbage_is_not_echoed}`,
`cli::tests::tls::a_file_that_cannot_be_used_stops_the_start_naming_both_flags_and_no_key_bytes`, `tests/process tls::a_certificate_that_cannot_be_used_stops_the_start_and_no_plaintext_listener_is_left`.

## SEC-143. What is accepted: a PEM chain, leaf first, and a PEM key of any of three kinds
The certificate file holds the leaf and then, if the clients do not have them, its intermediates; all are sent. The key is PKCS#8 (`PRIVATE KEY`), SEC1 (`EC PRIVATE KEY`) or
PKCS#1 (`RSA PRIVATE KEY`), unencrypted; ring signs with P-256, P-384, Ed25519 and RSA keys. Not accepted: JKS, PKCS#12, DER files, an encrypted key (the error says to decrypt it with
`openssl pkey`). Test: `tls::tests::a_chain_loads_with_a_key_in_any_of_the_three_formats`, `tests/tls::every_key_format_serves_and_the_chain_is_sent_whole` (a client that trusts only the root
verifies the chain; with the leaf alone it cannot).

## SEC-144. The protocol: TLS 1.2 at least, ring's suites, the server proves itself and asks nothing of the client
TLS 1.3 and 1.2 are offered, nothing older (rustls has no older). The cipher suites are ring's defaults. No client certificate is requested: Kafka's `ssl.client.auth=none` default
(DefaultSslEngineFactory:192-203), and the Go edition's `MinVersion: TLS 1.2` with no client authentication. Test: `tests/tls::tls_one_two_and_one_three_are_both_offered_and_the_address_may_be_an_ip`
(a TLS 1.2-only and a TLS 1.3-only client are both served; the address 127.0.0.1 is in the certificate).

## SEC-145. The handshake completes before the first Kafka byte; a plaintext client is closed and is answered nothing in Kafka's protocol (KC:177-178)
A connection on a TLS listener speaks TLS first. A client that sends Kafka (or HTTP) in the clear is closed; the most it receives is a TLS alert record (content type 21, a few bytes), never a
response frame. Test: `tests/tls::a_plaintext_client_on_the_tls_port_is_closed_and_gets_no_kafka_answer`, `tests/process tls::a_broker_started_with_a_certificate_serves_tls_refuses_plaintext_and_prints_no_key`.

## SEC-146. A failed handshake closes the connection with no Kafka bytes, says what kind of failure at debug level, and costs the server nothing
Half a hello, a record of a type that does not exist, a client that does not trust the certificate and hangs up, a TLS version in common none: the connection ends and the task goes. The
debug line (`connection closed`) carries only a kind of ours (`not a TLS handshake`, `the client sent an alert`, `no protocol version or cipher suite in common`, `a protocol
violation`, `closed during the handshake`, `an i/o error`), never rustls's own text (which can quote the peer) and never a byte the client sent. The next client is served as before.
Test: `tests/tls::a_broken_handshake_closes_quietly_and_the_server_goes_on`, `tls::tests::what_went_wrong_in_a_handshake_is_named_without_quoting_the_peer`, `tests/tls_redaction`.

## SEC-147. The handshake runs in the connection's own task; a slow one holds a place in the connection limit and nothing else
`accept` returns at once; the handshake is the first thing the spawned task does. A client that connects and says nothing does not delay the accept loop or any other connection.
Test: `tests/tls_lifecycle::a_connection_that_never_handshakes_is_cut_by_the_pre_auth_budget_and_does_not_block_others`.

## SEC-148. The handshake is inside the pre-auth budget, with SASL or without (extends SEC-4; **deviation SEC-D2** for the idea)
The budget (`Timeouts.pre_auth`, 30 s) counts from the accept. With TLS on, the handshake must finish inside it whether or not SASL is on; with SASL on the login has what is left,
so a slow handshake does not extend the time a stranger may hold the connection. Test: `tests/tls_lifecycle::the_handshake_counts_against_the_login_budget_it_does_not_extend_it`,
`tests/tls_lifecycle::a_connection_that_never_handshakes_is_cut_by_the_pre_auth_budget_and_does_not_block_others`.

## SEC-149. The peer's address is the TCP socket's, taken before the handshake
An ACL on a host (SEC-17) is matched against the address `accept` returned, so ACLs by host work over TLS as over a plain socket. Test:
`tests/tls_lifecycle::an_acl_by_host_sees_the_peers_address_over_tls` (an allow for 127.0.0.1 matches, an allow for another address does not, a deny for 127.0.0.1 beats an allow for all).

## SEC-150. TLS and SASL are independent: the four listener shapes all work
PLAINTEXT (no flags), SSL (TLS only: the principal is `User:ANONYMOUS`, Kafka's `SSL` listener without client authentication, kafka-semantics 4), SASL_PLAINTEXT (the earlier slices)
and SASL_SSL (both: the login of SEC-2 to SEC-12 runs inside the tunnel, PLAIN and both SCRAM hashes). Test: `tests/tls::sasl_and_tls_are_independent_and_both_work_together`,
`tests/tls_lifecycle::an_acl_by_host_sees_the_peers_address_over_tls` (the anonymous principal over TLS), `interop/run.sh -tls` (both shapes, four client libraries).

## SEC-151. A stop says close_notify, and an unfinished handshake does not hold it up
Stopping the server ends every TLS connection with a close_notify (bounded to 1 s each, all in parallel), so a client reads a clean end and not a truncation; a connection still in its
handshake is dropped at once. Test: `tests/tls_lifecycle::a_stop_ends_tls_connections_with_close_notify_and_does_not_wait_for_a_stalled_handshake`, and `tests/process tls::a_broker_started_with_a_certificate_serves_tls_*` (SIGTERM with a TLS connection open exits 0).

## SEC-152. The private key is printed nowhere (extends SEC-13)
Not in an error (each is one of ours, built from the file name and a reason, never from the parser's text, which quotes lines), not in the `Debug` of `TlsConfig` or of `Config` (the file
names are shown), not in a log line at any level, not in the startup line (which names the certificate file only). The key bytes are read once and held by ring. Test:
`tests/tls_redaction::the_key_and_a_clients_broken_bytes_are_in_no_log_error_or_debug_output` (every line of every key fixture is searched for), `tls::tests::the_key_is_in_no_debug_output_*`,
`tests/process tls::*` (the broker's stderr at trace level).

## SEC-153. The start says that TLS is on
`TLS is on: clients must connect with TLS (1.2 or newer)` at info, with `cert=<path>`. Test: `tests/process tls::a_broker_started_with_a_certificate_serves_tls_*`.

## SEC-154. The weight is behind the feature
The default build has no `rustls`, `ring` or `tokio-rustls` (the package count is unchanged), and a build with the feature needs a C compiler and an assembler (ring's build script). The numbers and the CI
jobs are in `weight.md`. Test: the `weight` and `weight-tls` jobs of `.github/workflows/ci.yml`; `cargo tree -p doze-kafka --prefix none --edges normal | sort -u | wc -l` with and without `--features tls`.

## Slice 8d-2: the group and transaction APIs (SEC-100 to SEC-123)

Every rule here is **(source)**: none of these denials is recorded (the recordings hold only the Metadata and InitProducerId ones, SEC-27, SEC-28),
so each is written from Kafka 4.3.1's `KafkaApis.scala` (KA) and the request and response classes it uses (`FindCoordinatorRequest`,
`OffsetFetchResponse.groupError`, `DescribeGroupsResponse.groupError`, `OffsetDeleteResponse.Builder`, `ApiError.fromThrowable`), and the test
that holds it cites the same lines. A recording of `authz-matrix` against `apache/kafka:4.3.1` (decision D9) would confirm or correct them.
The checks are in `crates/kafka-broker/src/handlers/*.rs` through `handlers/group_guard.rs` (`Guard`); the tests are
`kafka-broker tests/authz_{groups_classic,groups_848,offsets,transactions,txn_offsets}.rs` with their harness in `tests/support/authz_{groups,calls,txn}.rs`,
and `kafka-server tests/acl_groups.rs` on a real socket.

## SEC-100. Where the checks are made, and the questions every API is asked
Each check is made in the request handler, once per request (once per group, topic or id the request names), **outside the group's actor**, before
the coordinator is asked anything, as `KafkaApis` makes them before `groupCoordinator`/`txnCoordinator`. With the default allowing and no ACL
stored a check is `Authorizer::allows`' one atomic load and a length test, and nothing is allocated. The shape of a refusal is the API's own code
(30 group, 29 topic, 53 transactional id) in the response of the version asked. Every API is held to the same five questions by `five_ways!`
(`tests/support/authz_groups.rs`), run on a closed broker (`--acl-default deny`) unless said: the named right is granted (passes); it is missing
(refused); a super user with a DENY of ALL on the resource passes (SEC-19); a DENY beats an ALLOW to the same user; each right the API needs is
left out in turn (refused with that right's own code, which also fixes the order of the checks); only DESCRIBE where more is needed (refused:
implication runs one way, SEC-20); granted to `User:*`, to a PREFIXED name, to the LITERAL `*` (all pass) and a prefix that is not the name's
(refused); on a broker whose default allows, a resource nothing covers passes and one covered by an ACL for somebody else is refused (SEC-18).
Names are authorized as sent: an empty group id or transactional id is authorized like any other (no ACL can name it, since CreateAcls refuses an
empty name: only a super user or the default passes). Test: `five_ways!` in each test below.

## SEC-101. FindCoordinator (KA:1244-1250)
Key type 0: DESCRIBE on the group; key type 1: DESCRIBE on the transactional id; any other key type is asked nothing. Before the coordinator is
looked for. v4 and later: that key's entry has 30 or 53, node -1, port -1, host `""` and the entry's default message `""`, and the other keys are
served. Before v4 the answer is the top-level `prepareOldResponse`: the code, node -1, port -1, host `""`, and from v1 the error's own words
(`Group authorization failed.`, `Transactional Id authorization failed.`; v0 has no message). Test: `authz_groups_classic::find_coordinator_*`,
`describe_is_implied_by_read_and_a_deny_of_read_does_not_take_it_away`.

## SEC-102. JoinGroup (KA:1376-1398)
READ on the group, before the group id is judged (an empty id is 30, not INVALID_GROUP_ID, to one who may not read it). The refusal is
`getErrorResponse`: code 30, generation -1, empty member id and leader, no members, and the protocol type and name `""` before v7 and null from v7.
The caller is made no member. Test: `join_group_needs_read_on_the_group`, `join_group_refusal_has_the_shape_of_each_version`,
`a_member_with_read_is_taken_in_and_one_without_is_not_made_a_member`, `authorization_comes_before_the_group_id_is_judged`.

## SEC-103. SyncGroup (KA:1400-1426)
READ on the group, with Kafka's one ordering exception: from v5 a SyncGroup without a protocol type or name is INCONSISTENT_GROUP_PROTOCOL (23)
**before** the group is authorized, so a caller that is refused and also nameless is told 23; before v5, or with the names, it is told 30 (**deviation
SEC-D31**). Test: `sync_group_needs_read_on_the_group`, `sync_group_looks_for_the_protocol_before_it_authorizes`.

## SEC-104. Heartbeat (KA:1466-1484) and SEC-105. LeaveGroup (KA:1486-1504)
READ on the group, first, once per request in the handler (not per member event in the group). Heartbeat answers 30. LeaveGroup answers 30 **at the
top** in every version (`getErrorResponse`), with no member list. Test: `heartbeat_needs_read_on_the_group`, `leave_group_needs_read_on_the_group`,
`leave_group_refusal_is_at_the_top_in_every_version`.

## SEC-106. ConsumerGroupHeartbeat (KA:2604-2642)
READ on the group, then DESCRIBE on every topic in `SubscribedTopicNames` when it is non-empty (a heartbeat that names none checks none). Group
refused: 30; any one subscribed topic not describable: 29 for the whole request (a subscription is all or nothing). Both are the code alone: no
message (the default-message exceptions leave it out), no member id, epoch 0, no assignment; and no member is made. Both before the request itself is
judged (an empty member id is 30, not 42, to a caller that may not use the group). Test: `the_heartbeat_needs_read_on_the_group_and_describe_on_each_subscribed_topic`,
`a_refused_heartbeat_is_the_code_alone_in_both_versions_and_makes_no_member`, `one_topic_the_caller_may_not_describe_among_several_refuses_the_subscription`,
`authorization_comes_before_the_request_is_judged`.

## SEC-107. DescribeGroups (KA:1302-1348)
DESCRIBE on each group. A refused group is an entry of its own: code 30, state `""`, protocol type and protocol `""`, no members, authorized operations
`Int.MIN_VALUE`; the refused come **first**, in the order asked, then the others in the coordinator's order. Test: `describe_groups_needs_describe_on_each_group`,
`describe_groups_puts_the_refused_first_and_serves_the_rest`.

## SEC-108. ConsumerGroupDescribe (KA:2644-2728)
DESCRIBE on each group (a refused group is an entry with 30 and nothing else, ahead of the others). Then, for what is left, every topic named in a
member's assignment or target assignment must be DESCRIBE-able: a group with one that is not is replaced by an entry with 29, the message `The group
has described topic(s) that the client is not authorized to describe.`, and no members. Test: `the_describe_needs_describe_on_the_group`,
`refused_groups_come_first_and_the_rest_follow`, `a_group_holding_a_topic_the_caller_may_not_describe_becomes_a_refusal_with_kafkas_words`.

## SEC-109. ListGroups (KA:1350-1374)
Never an error. A caller with DESCRIBE on the **cluster** is shown every group, with no per-group check (so a group DENY does not narrow it); any other
is shown the groups it may DESCRIBE. On a broker whose default allows, the cluster is describable until an ACL covers it. Test: `list_groups_shows_only_the_groups_the_caller_may_describe`,
`describe_on_the_cluster_lists_every_group_and_a_group_deny_does_not_narrow_it`, `list_groups_by_any_user_prefix_and_the_default`.

## SEC-110. OffsetCommit (KA:273-374)
READ on the group (30 on **every partition of every topic**, topics as sent), then READ on each topic (29 on that topic's partitions, the others committed),
the group before the topics, both before the topic is looked for: a topic that is not there is 29 to one who may not read it and UNKNOWN_TOPIC_OR_PARTITION
to one who may. A refused topic's offsets are not stored. When no topic is left to commit to the group is not asked about the committer. Test:
`offset_commit_needs_read_on_the_group_and_on_the_topic`, `a_refused_group_fails_every_partition_of_every_topic_in_each_version`,
`a_refused_topic_fails_its_partitions_and_the_others_are_committed`, `offset_commit_checks_the_group_before_the_topics`,
`a_topic_that_is_not_there_is_not_revealed_to_one_who_may_not_read_it`.

## SEC-111. OffsetFetch (KA:1018-1185; `OffsetFetchResponse.groupError`)
DESCRIBE on the group, then DESCRIBE on each topic. Group refused: v8 and later, that group's entry has 30; v2 to v7, 30 at the top; v1 (no code at the
top), each partition asked about has 30, offset -1, metadata `""`, leader epoch -1. Topics asked for by name that may not be described: their partitions
29 with offset -1, **after** the topics found. Asked for every offset (topics null): the topics not describable are dropped with no word. A topic that
is not there is 29 to one who may not describe it and offset -1, no error, to one who may. Test: `offset_fetch_needs_describe_on_the_group_and_the_topic`,
`a_refused_group_is_answered_in_the_shape_of_the_version`, `a_refused_topic_asked_for_by_name_is_29_with_offset_minus_one_after_the_others`,
`asked_for_everything_the_topics_not_described_are_dropped_without_a_word`, `a_topic_that_is_not_there_is_not_revealed_to_offset_fetch_either`.

## SEC-112. DeleteGroups (KA:1428-1464)
DELETE on each group (READ, DESCRIBE or ALL-less rights do not do). The refused are answered 30, **once each**, after the answers for the others
(Kafka's `authorized results then unauthorized`); a refused group is not deleted and keeps its offsets (**deviation SEC-D30** on repeated names). Test:
`delete_groups_needs_delete_on_each_group`, `delete_groups_serves_the_allowed_first_then_the_refused_once_each`, `delete_needs_delete_and_describe_or_read_do_not_do`.

## SEC-113. OffsetDelete (KA:2363-2427; `OffsetDeleteResponse.Builder.merge`)
DELETE on the group (30 at the top), then READ on each topic (29 on its partitions), before the group or topic is looked for; a group that is not there is 30 to
one who may not delete it and GROUP_ID_NOT_FOUND to one who may; a top-level error of the coordinator discards the topic entries, as Kafka's merge does.
Nothing of a refused topic is deleted. Test: `offset_delete_needs_delete_on_the_group_and_read_on_the_topic`,
`offset_delete_refuses_the_group_at_the_top_and_a_topic_on_its_partitions`, `offset_delete_does_not_reveal_a_topic_or_a_group_either`.

## SEC-114. AddPartitionsToTxn (KA:1856-1966; v0 to v3, the client shape)
WRITE on the transactional id (53 on every partition asked, whether or not the topic is there), then WRITE on each topic, before the topic is looked for:
a refused topic's partitions are 29, an unknown one 3, and **every other partition OPERATION_NOT_ATTEMPTED (55)**; the transaction is not looked at. Test:
`add_partitions_needs_write_on_the_id_and_on_the_topic`, `a_refused_id_fails_every_partition_of_every_topic_even_one_that_is_not_there`,
`one_refused_topic_leaves_the_others_not_attempted`, `a_topic_that_is_not_there_is_not_revealed_to_one_who_may_not_write_it`, `the_id_is_judged_before_the_topics`.

## SEC-115. AddOffsetsToTxn (KA:1968-2016)
WRITE on the transactional id (53), then READ on the group (30), both before the transaction is looked at. Test: `add_offsets_needs_write_on_the_id_and_read_on_the_group`,
`add_offsets_judges_the_id_before_the_group`.

## SEC-116. TxnOffsetCommit (KA:2018-2116)
WRITE on the id (53 on every partition of every topic asked, there or not), else READ on the group (30 on every partition), else READ on each topic (29 on that
topic's partitions; a topic that is not there is 29 to one who may not read it, 3 to one who may). The offsets of a refused topic are not staged, so they are not
committed when the transaction commits. Test: `txn_offset_commit_needs_write_on_the_id_read_on_the_group_and_read_on_the_topic`,
`a_refused_id_or_group_fails_every_partition_asked_whether_or_not_it_is_there`, `a_refused_topic_is_29_on_its_partitions_and_hidden_existence_holds`,
`offsets_of_a_refused_topic_are_not_staged_with_the_transaction`.

## SEC-117. EndTxn (KA:1668-1709)
WRITE on the transactional id, 53 at the top in every version, before the id is judged (an empty id is 53 to one who may not write it and INVALID_REQUEST to one who
may). Test: `end_txn_needs_write_on_the_id`, `end_txn_authorizes_the_id_as_sent_before_it_is_judged`.

## SEC-118. DescribeTransactions (KA:2540-2566)
DESCRIBE on each transactional id; a refused one is an entry with its id, 53 and the schema defaults for the rest. For one that may be seen, the topics the caller
may not DESCRIBE are removed from the entry (`__consumer_offsets`, which stands for the groups of the transaction, is such a topic). Test:
`describe_transactions_needs_describe_on_the_id`, `describe_transactions_shows_a_refused_id_as_an_entry_of_53_and_only_the_topics_described`.

## SEC-119. ListTransactions (KA:2568-2593)
Never an error: only the ids the caller may DESCRIBE are listed. Test: `list_transactions_shows_only_the_ids_the_caller_may_describe`.

## SEC-120. InitProducerId, the transactional half (KA:1603-1614), checked against the matrix
WRITE on the transactional id, and TWO_PHASE_COMMIT on it as well when the request has `Enable2Pc` (added in this slice; the field is a v6 one, which is not served, so it
is reached only by a caller of the handler); either missing is 53, top level, with producer id -1 and epoch -1. The no-id half is SEC-28. Test:
`init_producer_id_with_a_transactional_id_needs_write_on_it`, `a_two_phase_commit_needs_that_right_on_the_id_as_well`.

## SEC-121. The authorized-operations bits (AH:62-76; KA:1326-1335, 2675-2684)
For a group: one `authorize` per operation a group supports (READ 3, DELETE 6, DESCRIBE 8, DESCRIBE_CONFIGS 10, ALTER_CONFIGS 11), bit `1 << code` when allowed, ALL never a bit.
DescribeGroups gives them from v3 and ConsumerGroupDescribe at any version, when the request asks and only for a group that is not an error; otherwise the schema
default (`Int.MIN_VALUE`). A READ grant is bits 3 and 8; ALL, and a super user, all five (3400); a DENY of DESCRIBE takes the group away. Test:
`describe_groups_says_which_operations_the_caller_has`, `the_operations_are_given_when_asked_and_only_for_a_group_that_is_not_an_error`.

## SEC-122. On a real socket
Two users logged in by PLAIN, one allowed to join a group and read a topic and one refused (FindCoordinator, JoinGroup, OffsetCommit, ListGroups), and a transactional
producer with and without WRITE on its transactional id through InitProducerId, AddPartitionsToTxn and EndTxn. Test: `kafka-server tests/acl_groups.rs`.

## SEC-123. Mutation check, slice 8d-2
Forty hand-made mutations of the checks in `handlers/*.rs`, `offset_fetch/guard.rs` and `group_guard.rs`, each checked as applied, run in a copy of the tree against the four
`authz_*` test files: a check removed (FindCoordinator's transactional half, JoinGroup, SyncGroup, Heartbeat, LeaveGroup, AddPartitionsToTxn's id and its topics,
TxnOffsetCommit's group, OffsetCommit's group and its topics), the wrong operation (READ for DESCRIBE, DESCRIBE for READ, WRITE for READ, DELETE for READ, a cluster
DESCRIBE read as READ), the authorized-operations bits shifted, a list not filtered (ListGroups, ListTransactions, DescribeTransactions' topics, OffsetFetch for every
offset), a refused topic's entries not appended, staged offsets of a refused topic kept, "any" turned into "all" (the subscription), a per-item error put on the wrong
item (DescribeGroups, ConsumerGroupDescribe, AddPartitionsToTxn), the check moved after the state check (EndTxn's id after the empty-id test, JoinGroup's after the
group-id test), the v1 shape of OffsetFetch, the v1 message of FindCoordinator, the two-phase-commit right ignored. First run of 30: 28 killed, 2 survived (the offsets
of a refused topic were still staged, which only a committed transaction shows; AddOffsetsToTxn's group right read as DESCRIBE, which a user holding READ cannot show);
a test was added for each, and `five_ways!` gained the "only DESCRIBE" caller for every API that needs more. Second run of 10 new ones: 9 killed, 1 survived (the wrong
item was the first partition of a refused topic, which a single-partition test cannot tell from the right one); a test was added. Final: 40 of 40.
(The mutation copy builds in a target directory of its own: sharing one with the worktree let the last mutant's build be taken for the worktree's.)

## SEC-160. DescribeUserScramCredentials (50) and AlterUserScramCredentials (51): versions and profile
Both exist at v0 only (`msg/DescribeUserScramCredentials*.json`, `AlterUserScramCredentials*.json`: `validVersions 0`, flexible). The profile offers them at 0 to 0 for
`--kafka-version` 2, 3 and 4 and not at 1, as the recorded brokers do (`crates/kafka-wire/tests/data/api-versions.json`: cp-kafka 6.2, 3.9.1 and 4.3.1 list both at 0..0, the 1.x broker neither).
They are served with SASL off too (SEC-D51). The crate decodes both at v0 (`vendor/kafka-protocol/src/messages/*_user_scram_credentials_*.rs`). Test: `kafka-wire profile::tests::the_scram_credential_apis_are_offered_from_kafka_2_at_version_0_and_not_at_1`,
`tests/every_offered_version` (every offered request is read and every default response written and read back), `kafka-server connection::tests::served::*`, `tests/tcp`, `kafka-server tests/users_api::the_two_apis_are_advertised_after_login`.

## SEC-161. Who may call them (KA:2444-2455, CA:890-900)
Describe needs DESCRIBE on the cluster, Alter needs ALTER (which implies DESCRIBE, SEC-19), both through the authorizer of SEC-16 to SEC-19, so a super user, an ALLOW on `kafka-cluster`, or (on a broker with
the default allow and no ACL) anybody. **Denied:** Describe answers the top-level `ErrorCode 31` and one entry with 31 for each name asked for (`DescribeUserScramCredentialsRequest.getErrorResponse`,
DescribeUserScramCredentialsRequest.java:63-76); Alter answers 31 for each distinct user name in the request, sorted (`AlterUserScramCredentialsRequest.getErrorResponse`,
AlterUserScramCredentialsRequest.java:67-83), and nothing is changed. The check precedes every rule below. Messages are null (**deviation SEC-D8**, the long message of `authorizeClusterOperation` is not reproduced). Test:
`kafka-broker tests/user_scram_use::a_caller_without_the_right_is_refused_in_the_shape_kafka_answers`, `..::alter_needs_alter_on_the_cluster_and_describe_needs_describe_which_alter_implies`,
`kafka-server tests/users_api::a_user_made_by_an_admin_logs_in_*` (a user who is not a super user gets 31 on a real socket).

## SEC-162. The client sends the salted password; the broker does no PBKDF2 (SCM:260-282)
An upsertion carries `Salt`, `Iterations` and `SaltedPassword = Hi(password, salt, iterations)`, computed by the client. The broker derives `StoredKey = H(HMAC(SaltedPassword, "Client Key"))` and
`ServerKey = HMAC(SaltedPassword, "Server Key")` and keeps salt, iterations and the two keys (`Verifier::from_salted_password`); the password and the salted password are never kept, and the keys are in no `Debug`
output. A SCRAM login for that user then proves the password with the salt and iteration count the client chose (the server sends them in server-first). Test: `kafka-broker tests/user_scram::a_user_is_made_for_each_mechanism_and_logs_in_with_the_password_the_client_salted`,
`kafka-auth scram::keys::tests::*` and `users::tests::a_verifier_made_from_parts_must_have_keys_the_size_of_the_hash`, `interop/run.sh -sasl` (franz-go's kadm salts with Go's PBKDF2; all four libraries then log in).

## SEC-163. Validation, in Kafka's order, with Kafka's words (SCM:284-322)
Per user, an upsertion is judged: the name (empty: `UNACCEPTABLE_CREDENTIAL` 93 `Username must not be empty`), the mechanism (not 1 or 2, including 0: `UNSUPPORTED_SASL_MECHANISM` 33 `Unknown SCRAM mechanism`), then
the iteration count: below 4096 `UNACCEPTABLE_CREDENTIAL` `Too few iterations`, above 16384 `Too many iterations` (both mechanisms; 4095 and 16385 are refused, 4096 and 16384 taken; negatives are too few). A deletion is judged by
the name and the mechanism the same way. Mechanism numbers: 1 SCRAM-SHA-256, 2 SCRAM-SHA-512. (The refusal of a user configured at start sits after the mechanism and before the count: SEC-D50.) Test: `tests/user_scram::the_iteration_count_must_be_from_4096_to_16384`,
`..::an_empty_name_and_an_unknown_mechanism_are_refused_with_kafkas_words`.

## SEC-164. Deleting what is not there is RESOURCE_NOT_FOUND (SCM:298-309)
The key is the user **and** the mechanism: `RESOURCE_NOT_FOUND` 91 `Attempt to delete a user credential that does not exist` for a user with no credential of that mechanism, for a user who was never there, and for the
second deletion of the same credential. Test: `tests/user_scram::deleting_what_is_not_there_is_resource_not_found`, `..::a_deleted_credential_no_longer_logs_in_and_the_other_mechanism_still_does`.

## SEC-165. A user touched twice in one request loses both operations (SCM:188-226)
The key is the user's name, not the name and mechanism: two deletions, a deletion and an upsertion (either mechanism), or two upsertions of one user are `DUPLICATE_RESOURCE` 92 `A user credential cannot be altered twice in the same request`;
the earlier operation is cancelled with the later one, nothing is changed for that user, and a third mention is ignored. Once a user has an error (any), every later operation on the name is ignored and the first error stands.
Test: `tests/user_scram_requests::a_user_touched_twice_in_one_request_is_refused_whatever_the_operations`, `..::a_user_whose_first_operation_failed_keeps_that_error_and_a_repeat_is_ignored`, `..::a_deletion_that_failed_ends_the_users_part_of_the_request`.

## SEC-166. The answer: deletions that happened, upsertions that happened, then the refusals (SCM:229-257)
One result per user: `User`, `ErrorCode`, `ErrorMessage` (null on success). Kafka's order inside each group is a hash map's; here it is the order of the request (**deviation SEC-D52**). An empty request answers an empty list. Test:
`tests/user_scram_requests::a_mixed_request_answers_deletions_then_upsertions_then_errors`, `..::an_empty_request_changes_nothing_and_answers_nothing` (both in `tests/user_scram_requests`).

## SEC-167. Salt and salted password are not judged, except that a key cannot be empty (SCM:260-282, ApiError.java:35-41)
Any salt is taken, empty included (Kafka validates nothing: kafka-semantics 3.6), and a salted password of any length is a key. A salted password of no bytes makes Java's `SecretKeySpec` throw
`IllegalArgumentException("Empty key")` inside `finishUpsertion`, which the controller catches and `ApiError.fromThrowable` turns into `UNKNOWN_SERVER_ERROR` (-1) with a null message; no record is written. Here the same: -1, null, nothing stored.
Test: `tests/user_scram_requests::the_salt_and_the_salted_password_are_not_judged_except_that_a_key_cannot_be_empty`.

## SEC-168. A request is one change: written, then swapped in, under a lock
The whole request is applied in one table transaction (deletions and upsertions together: Kafka's `ControllerResult.atomicOf`, SCM:257), durable before the answer, and only then is the new snapshot of stored users swapped in (an `ArcSwap`), so a
login never sees a credential the table does not have, and two requests at once never start from the same snapshot (`Broker::user_admin`; the table's own write lock serialises the writers, `kafka-meta`). A write that fails answers `KAFKA_STORAGE_ERROR` (56) for each
operation that would have happened (the refusals keep their own answers) and changes neither the table nor the snapshot. Test: `tests/user_scram_use::a_write_that_fails_changes_nothing_and_says_so`,
`tests/user_scram::two_requests_at_once_lose_neither`, `kafka-meta store::credentials::tests::*` (a write that fails leaves them as they were; writers at once lose nothing; an old database takes a write), the crash sweep `tests/crash` (`credentials` in the workload: a power cut, a failing operation or a stopped process after any step loses nothing acknowledged).

## SEC-169. DescribeUserScramCredentials answers (ScramImage.java:79-133)
No list, or an empty list: every user that has a credential (here by name). Otherwise each name asked for, once, in the order first seen: a name given more than once is one entry `DUPLICATE_RESOURCE` 92
`Cannot describe SCRAM credentials for the same user twice in a single request: <user>` (whether or not it exists); a name with no credential is `RESOURCE_NOT_FOUND` 91 `Attempt to describe a user credential that does not exist: <user>`
(the empty name too); else `CredentialInfos` of mechanism and iterations, in mechanism order. Never a salt or a key. A success has `ErrorCode 0` and the empty message `""` at the top and in each entry, as Kafka's generated classes leave them.
Test: `tests/user_scram_use::describe_lists_every_user_with_its_mechanisms_and_iterations_and_nothing_secret`, `..::describe_by_name_answers_each_name_once_with_kafkas_words_for_the_failures`.

## SEC-170. A stored credential logs a user in with SCRAM, and only SCRAM; the change applies to the next login
A SCRAM login finds the verifier in the bootstrap users or, if the name is not one of them, in the stored ones (SEC-172), by user and mechanism: a credential for SCRAM-SHA-512 does not log in SCRAM-SHA-256, and one for a mechanism the broker does not offer
(`--sasl-mechanisms`) cannot log in. **PLAIN can log in bootstrap users only**: a stored SCRAM verifier cannot give a password back to be compared, and Kafka's PLAIN uses its JAAS file only. A replaced, deleted or added credential applies to the logins
that begin after the answer; **a connection that is already logged in stays logged in** with the same principal (Kafka checks credentials again only at re-authentication, `connections.max.reauth.ms`, which this broker does not do, SEC-D55). Test:
`kafka-auth authenticator::tests::a_stored_user_logs_in_with_scram_and_never_with_plain`, `..::a_stored_credential_of_a_mechanism_that_is_off_cannot_log_in`, `kafka-server tests/users_api::a_user_made_by_an_admin_logs_in_at_once_and_after_a_restart` and `..::a_deleted_credential_is_refused_at_the_next_login_and_an_open_connection_stays_logged_in`
(a deleted user is refused with the generic 58 and the connection closes; the connection that was open before the deletion still answers), `interop/run.sh -sasl` (`users/` cells).

## SEC-171. The stored users outlive the process
The table (`kafka-meta`, key = mechanism byte then user name, value = a version byte and postcard of salt, iterations, the two keys) is read when the broker opens and swapped into the snapshot before the server is bound; the table is made with the database and
a database made before it existed reads as none and takes the first write. A row this program cannot use (an unknown mechanism, keys not the size of the hash, no iterations) is left in the table, unused, and named at warn level (user and mechanism, no key). Test:
`tests/user_scram_use::stored_users_come_back_after_a_restart_and_log_in`, `kafka-meta store::credentials::tests::credentials_survive_a_restart_and_a_power_cut_after_the_answer`, `..::a_database_made_before_credentials_existed_reads_as_none_and_takes_a_write`,
`kafka-server tests/users_api` (restart on the same disk), `tests/process users::users_from_a_file_and_the_environment_log_in_and_one_made_over_the_api_outlives_the_process` (the real binary, SIGTERM, started again).

## SEC-172. Two layers of users: bootstrap and stored (**deviation SEC-D50**)
The **bootstrap** users are those of `--sasl-user`, `--sasl-users-file` and `DOZE_KAFKA_SASL_USERS`: derived at every start, never written to the table, and for PLAIN and for each SCRAM mechanism that is on. The **stored** users are those the API made. A name in both
is the bootstrap user's, for every mechanism and every login: the stored entries under that name are never consulted (they stay in the table, and come back if the bootstrap user is removed from the flags). An Alter or a Delete aimed at a bootstrap user is refused with
`UNACCEPTABLE_CREDENTIAL` (93) `The user is configured at start (--sasl-user, --sasl-users-file or DOZE_KAFKA_SASL_USERS) and cannot be changed over the API`, whichever the mechanism (it is judged after the mechanism is known to be 1 or 2 and before the count); Describe lists
a bootstrap user with the mechanisms that are on and 4096 iterations (SEC-D14). Test: `tests/user_scram_use::a_user_configured_at_start_is_listed_and_cannot_be_changed_over_the_api`,
`..::a_name_stored_and_then_configured_at_start_is_the_configured_one_and_the_stored_row_stays`, `kafka-auth users::tests::a_user_in_both_is_the_bootstrap_one_whatever_is_stored`, `kafka-server tests/users_api::the_user_configured_at_start_cannot_be_taken_over_and_is_listed`.

## SEC-173. The users file (`--sasl-users-file PATH`)
One `user:password` a line, split at the first colon (so a password may hold colons); ASCII, as the flag; the user and the password are checked as the flag's are (an empty user or password, a NUL, a non-ASCII password). A line that is empty or only blanks, a line whose first non-blank character is `#`,
a CRLF ending and a leading byte order mark are skipped; nothing else is trimmed (a space in a password is a character). A line that is not an entry stops the start with `--sasl-users-file: line N: not a valid entry (expected user:password)`, a bad user or password with
`--sasl-users-file: line N: SASL user "alice" has an empty password`: the line number and the user, never the line. A file that cannot be read or is not UTF-8 stops the start (`cannot be read: <os error>`, `is not valid UTF-8 text`), naming the path. Test: `cli::tests::users::a_users_file_holds_one_user_and_password_a_line_split_at_the_first_colon`,
`..::a_bad_line_is_named_by_its_number_and_its_content_is_not_repeated`, `..::a_file_that_cannot_be_read_is_refused_without_a_guess`.

## SEC-174. The environment variable (`DOZE_KAFKA_SASL_USERS`)
`user:password` entries separated by commas, with the same rules as a line; the space around an entry is ignored and an empty entry is skipped (`a:1, b:2,,`). **A password with a comma cannot be written there** (the text after the comma is another entry): use the file. A variable that is empty or only blanks is
not set. A bad entry stops the start with `DOZE_KAFKA_SASL_USERS: entry N: ...` (N counts the commas, empty entries included), never repeating it; a value that is not UTF-8 says so. The variable is visible to the same user's other processes (`/proc/<pid>/environ`, `ps eww`) and is not cleared: it keeps the password out of the command line, no more (**SEC-D58**). Test: `cli::tests::users::an_environment_list_is_comma_separated_user_and_password`,
`..::an_empty_variable_is_the_same_as_none`, `..::a_bad_line_is_named_by_its_number_and_its_content_is_not_repeated`.

## SEC-175. The three sources add up, once
Flag, then file, then variable; a user given twice anywhere (within a source or across them) stops the start with `SASL user "alice" is given more than once`, naming the user and no password. Users from the file or the variable without `--sasl-mechanisms` stop the start (`no mechanism is enabled`), as
`--sasl-user` does, and so does `--sasl-allow-empty-users` (it needs `--sasl-mechanisms`). Test: `cli::tests::users::the_three_sources_add_up_in_the_order_flag_file_environment`, `..::a_user_given_twice_across_sources_is_refused_naming_the_user_and_never_a_password`,
`..::users_from_the_file_or_the_environment_without_a_mechanism_are_refused`.

## SEC-176. SASL with nobody to log in: refused, unless a stored user exists or it was meant (`--sasl-allow-empty-users`)
Mechanisms with no user in any source still stop the start (`SASL is enabled but no credentials are configured; give --sasl-user, --sasl-users-file or DOZE_KAFKA_SASL_USERS, or start with --sasl-allow-empty-users ...`), but a stored credential counts as a user, and that is known only when the database is open: so with no database in the data
directory the refusal is immediate and nothing is created, and with one it comes after the stored users are loaded (`Authenticator::ensure_users`). `--sasl-allow-empty-users` starts it anyway: the way to run a secured broker whose first user is made over the API by a super user (`--acl-super-user`); with no super user and no stored user nobody can log in, and the start says nothing more. **The bootstrap story:** the first user comes
from a flag, a file or the variable, or (with the flag) from the API as a super user, which itself needs a login or the open-broker posture (SEC-D51). Test: `cli::tests::users::mechanisms_without_a_user_are_refused_unless_a_stored_user_may_exist_or_it_was_meant`, `kafka-auth authenticator::tests::a_start_with_nobody_to_log_in_is_refused_unless_it_was_meant`,
`tests/process users::a_secured_broker_with_no_user_starts_only_when_that_was_asked_for`.

## SEC-177. A users file others can read is warned about
On Unix, if `mode & 0o077` is not zero the start logs at warn `--sasl-users-file PATH is readable by group or others (mode 644); run chmod 600 on it`: the path and the mode, nothing of the content. It is only a warning: the broker starts. Not on other platforms.
Test: `cli::tests::users::a_file_others_can_read_is_warned_about_and_a_private_one_is_not`, `tests/process users::a_users_file_others_can_read_is_warned_about_without_its_content`.

## SEC-178. No password from any source is shown (extends SEC-13)
The passwords of the flag, the file and the variable are in no `Debug` output of the arguments, the settings or the authenticator, in no error (a bad line, a duplicate, a bad entry), and in no log line at trace level; the keys of a stored credential are in no `Debug` output
(`CredentialRecord`, `Verifier`, `Stored`, `Users` print names and counts), and a rejected SCRAM login logs an enum name. The salted password a client sends is not logged: the connection logging shows no request bodies. Test: `cli::tests::users::no_password_from_any_source_is_in_a_debug_output`,
`..::a_user_given_twice_across_sources_is_refused_naming_the_user_and_never_a_password`, `kafka-meta credential::tests::a_record_prints_no_secret`, `kafka-auth users::tests::debug_output_names_users_and_shows_no_key`, `tests/process users::users_from_a_file_and_the_environment_log_in_and_one_made_over_the_api_outlives_the_process` (the log at trace level is searched for all three passwords and the API user's).

## SEC-179. Real clients
franz-go's kadm makes the user (its own salt and PBKDF2), then sarama, franz-go, kafka-go and librdkafka log in as it with SCRAM-SHA-256 and SCRAM-SHA-512 and write and read 20 records; PLAIN with the right password is refused in each; the user configured at start cannot be taken over (93); the credentials are deleted one mechanism at a time and
each deletion is refused at the next login by every library, the other mechanism still working until its own deletion; a second deletion is `RESOURCE_NOT_FOUND`. Test: `interop/run.sh -sasl` (the `users/` lines after the twelve cells), `interop/go/users.go`.

## The allowed-hosts guard, slice 8g (SEC-180 to SEC-189)

The Go edition has a DNS-rebinding guard for its two HTTP listeners (console and schema registry): `internal/hostguard`, `go-oracle-rust.md` B4. Kafka has none, and the Kafka protocol has no `Host`
header, so there is no Kafka behaviour to copy: these rules are the Go edition's domain facts (what a rebinding page does and what a guard must refuse), written for this broker in Rust, with the
Go edition's `.doze` special case removed (PLAN.md, "an explicit allowed-hosts guard ... with no `.doze` special case"). The guard is `crates/kafka-auth/src/hostguard.rs` (100 lines, no
dependency), its tests `hostguard/tests.rs`, its fuzz target `crates/kafka-auth/fuzz/fuzz_targets/hostguard.rs`. **Nothing is mounted on it until Phase 9** (decision D8): the console and
the registry do not exist yet.

## SEC-180. A pure function: `allowed(host_header, extra) -> bool`
No I/O, no allocation, no dependency; the same answer for the same text. `extra` is the operator's list (`--console-allow-host`). A listener will call it with the request's `Host` header and answer
403 when it returns false. It guards the HTTP listeners only, never the Kafka port. Marked `// fuzz:parser hostguard`; `cargo xtask fuzz-check` fails without the target. Test:
`hostguard::tests::ip_literals_localhost_and_names_under_it_are_allowed`, `..::the_shapes_a_rebinding_page_presents_are_refused`.

## SEC-181. What is allowed with an empty list
An IPv4 literal, an IPv6 literal in brackets, `localhost`, and any name under it (`app.localhost`); case is ignored. An IP literal can never be a rebinding name, so a LAN address works. Everything
else is refused: `evil.example`, `localhost.evil.example`, `127.0.0.1.evil.example`, `xlocalhost`, `kafka.doze` (**deviation SEC-D60**). Test:
`hostguard::tests::the_shapes_a_rebinding_page_presents_are_refused`, `..::a_header_in_any_case_is_judged_alike`.

## SEC-182. A port is only trailing digits; brackets are only a pair around an IPv6 literal
The last colon splits off a port, which is ASCII digits (and may be empty: `localhost:`). Anything else after it is part of what the header names, so `localhost:pw@evil.example`,
`localhost:80a` and `localhost: 80` are refused. Text in brackets must parse as an IPv6 address and the closing bracket must be the end or be followed by `:` and digits. `::1` without brackets is refused
(the colons would be a port), as are `[localhost]`, `[127.0.0.1]`, `[::1]x`, `[[::1]]` and a zone id (**deviation SEC-D61**). Test: `hostguard::tests::an_ipv6_literal_is_written_in_brackets_and_only_an_ipv6_literal_is`,
`..::a_port_is_only_trailing_digits`.

## SEC-183. A name is letters, digits, hyphens and underscores in sane labels
Labels of ASCII letters, digits, `-` and `_`, none empty or over 63 bytes, 253 bytes in all; anything else (a space, `@`, `/`, `#`, `?`, `%`, a control character, a non-ASCII character) is no name and is
refused whatever the list holds, so `a@localhost` and `a b.localhost` are not under `localhost`. A browser sends punycode, so non-ASCII text never names anyone (**SEC-D61**). A very long host is refused in
bounded time. Test: `hostguard::tests::a_name_is_letters_digits_hyphens_and_underscores_in_labels_that_are_not_empty_or_too_long`, `..::non_ascii_names_are_refused_because_a_browser_sends_punycode`.

## SEC-184. One trailing dot is the same name; an address has none
`localhost.` and `Dev.Example.COM.` are the names without the dot (the list's entries too: `dev.example.com.` matches `dev.example.com`). Two dots make nobody's name. An IP literal with a dot after it is not an
IP literal and is refused. Test: `hostguard::tests::a_trailing_dot_is_the_same_name_but_one_is_all_and_an_address_has_none`.

## SEC-185. The list adds exact names; `*` turns the check off, except for an empty host
A name in `extra` (trimmed, case ignored, one trailing dot ignored) is allowed; a pattern such as `*.example.com` is an exact (and so never matching) name; an entry with a port matches nothing. A `*` anywhere in
the list allows any non-empty host, including text that is no name. An empty host (an empty header, `:80`, `.`) is refused even with `*`, as the Go edition does. Test:
`hostguard::tests::the_list_adds_exact_names`, `..::a_star_in_the_list_turns_the_check_off_but_an_empty_header_still_fails`, `..::an_empty_header_is_never_allowed`.

## SEC-186. The fuzz target
`hostguard` takes any text (4 KiB at most) and checks, with no list, one listed name and `*`: no panic; what is let in with no list is ASCII with no control character, space, `@`, `/`, `\`, `#`, `?` or `%` and is an address, a
bracketed host or ends in `localhost`; what only the list lets in is exactly the listed name (case, one trailing dot, a port); the list only ever adds; `*` never lets in an empty host. Seeds are in
`fuzz/seeds/hostguard`; the CI `fuzz` job runs it for 30 seconds beside the others. Run for 40 seconds on a laptop: 11 million executions, no finding.

## SEC-187. The flags: `--console-allow-host HOST` and `--console-unprotected`
`--console-allow-host` is repeatable or comma separated (blanks dropped, case and dots left to the guard); `--console-unprotected` is a switch. Both are parsed, checked and kept in `Settings.console`; **the
console does not exist before Phase 9, so nothing acts on them** (**deviation SEC-D63**): the help text of each says so, the start logs `--console-allow-host is accepted and ignored` like the other flags of
later parts, and the values are logged at debug. Test: `cli::tests::console::the_flags_are_parsed_and_kept_and_say_in_their_help_that_nothing_serves_them_yet`.

## SEC-188. The start-up rule: SASL or TLS on and a console beyond loopback is refused (exit 2)
With SASL or TLS on, a `--console-addr` that other machines can reach (a wildcard or empty host, an address that is not loopback, a name other than `localhost`) stops the start, before the data directory is
touched, with the Go edition's words: `config: the console has no login and is plain HTTP, and SASL is on: serving it on 0.0.0.0:9093 would let anyone who can reach that address act on the broker as a super
user, without a password. Keep --console-addr on loopback (and put a proxy that authenticates in front of it), turn it off with --console=false, or accept this with --console-unprotected` (`SASL and TLS` or `TLS` for
the other cases; with TLS and no SASL the consequence is `send topics and messages unencrypted to anyone who can reach that address`). `--console-unprotected` accepts it. A console turned off (`--console=false`), a
loopback address (`127.0.0.1`, `[::1]`, `localhost`) or a broker with neither SASL nor TLS is not refused. An address that is not `host:port` is refused with `config: console address "x" is not host:port (for example
127.0.0.1:9093)`. Every start refusal now exits **2** (**deviation SEC-D62**). Test: `cli::tests::console::sasl_and_a_console_beyond_loopback_is_refused_in_the_go_editions_words`,
`..::loopback_a_turned_off_console_and_no_protection_all_start`, `..::a_console_address_that_is_not_host_and_port_is_refused`,
`tests/process console::sasl_and_a_console_beyond_loopback_stop_the_start_with_exit_2_unless_unprotected`, `console::a_console_address_that_is_no_host_and_port_exits_2_too`.

## SEC-189. Start-up notes: what the operator's choices leave open
Said once at warn before the broker listens: **the console served beyond loopback although SASL or TLS is on** (`--console-unprotected`: `the console is served on ADDR with no login and over plain HTTP although
SASL is on (--console-unprotected): that will ...`), and **SASL without TLS on a listen address other machines can reach** (`SASL is on without TLS and --listen ADDR can be reached from other machines: logins
(a PLAIN password is sent as it is) and messages cross the network unencrypted`). No note for a loopback listener, or for no SASL. Test:
`cli::tests::console::unprotected_accepts_the_start_and_the_start_says_what_it_left_open`, `..::sasl_without_tls_on_an_address_other_machines_reach_is_noted`,
`tests/process console::unprotected_starts_the_broker_and_the_log_says_what_is_open`.

## Deviations
- **SEC-D1.** A request in the middle of a login other than ApiVersions and SaslHandshake closes unanswered; Kafka answers it in its
  own shape with 34 and then closes. (Recorded `request-before-token-who`; the Go edition has the same difference.)
- **SEC-D2.** The 30 s login budget from the accept. Kafka has none.
- **SEC-D3.** SaslHandshake v0 (raw tokens) is not served; the request closes the connection.
- **SEC-D4.** No `connection.failed.authentication.delay.ms` (100 ms) before a failure is answered.
- **SEC-D5.** Passwords are not zeroed on drop (no `zeroize` dependency; the weight rule). They live in the process for its life and are
  never printed.
- **SEC-D6.** `Session::Unauthenticated` carries a flag (`versions_answered`) that the plan's enum did not (SEC-12 needs it).
- **SEC-D7.** The ACL APIs and the authorizer work with SASL off, enforced for `User:ANONYMOUS` (decision D4, the Go edition's behaviour).
  Kafka without an authorizer answers SECURITY_DISABLED (54) and enforces nothing.
- **SEC-D8.** A CLUSTER_AUTHORIZATION_FAILED on the ACL APIs carries no long message. Kafka's is `Request(processor=N, connectionId=...) needs
  ALTER permission.`, which embeds a request dump and cannot be reproduced; here DescribeAcls says `""` and the results of CreateAcls and
  DeleteAcls say nothing. The code is Kafka's.
- **SEC-D9.** A super user must be written `Type:name` with both parts non-empty (the Go edition's rule); Kafka only needs a colon, so it
  accepts `:x` and `User:`.
- **SEC-D10.** DescribeAcls and DeleteAcls list resources and ACLs sorted; Kafka's order is a hash map's (unspecified).
- **SEC-D11.** (This once also listed the topic, group and transactional-id APIs as unchecked; they are checked, SEC-60 to SEC-72 and SEC-100 to SEC-123.) A CreateAcls of
  more than 10,000 new ACLs is not limited (Kafka's outcome is unspecified, kafka-semantics U3). DescribeConfigs on the broker does not report
  `allow.everyone.if.no.acl.found` or `super.users` (the Go edition does; real Kafka lists neither, so the recorded answers would differ).
- **SEC-D12.** The server nonce is 18 random bytes in base64 (24 characters); Kafka's is a base-36 rendering of 130 random bits (up to 26). Both are
  unpredictable, printable and comma-free; no client depends on the alphabet (SEC-51).
- **SEC-D13.** With several extensions after the nonce Kafka signs them in the iteration order of an immutable `Map.copyOf`, which is random per JVM, so a client
  that sends two cannot be logged in reliably there; here they are signed in the order sent (a name twice counts once). `tokenauth=true` is the
  delegation-token path of Kafka, answered here as an unknown user (the generic 58 at client-first, which is what Kafka answers when the token does not exist);
  there are no delegation tokens (out of scope).
- **SEC-D14.** Every verifier has 4096 iterations and a random 16-byte salt. Kafka takes any count from 4096 to 16384 and the salt of whoever made the credential
  (its admin client makes about 26 ASCII bytes); a user made over `AlterUserScramCredentials` (slice 8f) keeps the count and salt its client chose (4096 to 16384, SEC-163). A stored count under 4096, which Kafka refuses at
  client-first, cannot occur.
- **SEC-D15.** The derived keys, the salted password and the proof are `Vec`s that are not zeroed on drop (extends SEC-D5); the verifiers are kept for the life of
  the process, the intermediate values for the length of a call.
- **SEC-D16.** Where Kafka answers from an unordered map or a shuffle, the answer here is in the order asked: DeleteTopics (shuffled on purpose, CA:353),
  Produce and DeleteRecords (maps merged, KA:458 and 1580), and Fetch's partitions. Where Kafka's order is a stated one it is kept: the topics a
  caller may not use come after the others in CreateTopics, ListOffsets, DescribeConfigs and Metadata, and after the duplicates in CreatePartitions. In
  CreateTopics a name listed twice is answered where it first appears, as before this phase, not after the others (Kafka puts the duplicates after the
  controller's answers); clients match results by name.
- **SEC-D17.** CreateTopics refused for want of CREATE answers `configs` as an empty list, not null (the existing "never null" rule, see `refused` in
  `create_topics.rs`); `__cluster_metadata` is not special-cased (Kafka answers 42 "Creation of internal topic __cluster_metadata is prohibited.");
  DeleteTopics has none of Kafka's request-shape errors ("Neither topic name nor id were specified.", "You may not specify both topic name and topic id.",
  duplicate ids) and no `delete.topic.enable`; those are not authorization and were not added here.
- **SEC-D18.** A resource type this broker does not take (BROKER_LOGGER, CLIENT_METRICS and GROUP in DescribeConfigs and the alter APIs) is checked first
  for the right Kafka asks (31 or 30 when the caller lacks it) and then answered 42 "Unsupported resource type" as before; the legacy AlterConfigs says
  that for BROKER_LOGGER where Kafka says "Unknown resource type 8" and does not ask for a right. A broker's configs are validated by this broker's own
  table (`configs::apply_to_broker`) where Kafka uses `DynamicBrokerConfig.validate`, so which config names fail before authorization is this table's.
- **SEC-D19.** A Fetch from a follower (`replica_id` 0 or more) that has CLUSTER_ACTION is read like a consumer's, without READ on the topic: there is no
  replication to serve, and the caller is a broker or a tool that asked for it. Metadata's topic entry that has no name (a topic asked for by an id
  nobody knows or may not describe) has no authorized operations field; Kafka builds a resource with a null name for it, which throws.
- **SEC-D20.** The tests of these rules read the Kafka source and not a run of it (D9: `authz-matrix` is not recorded). The rows most likely to differ are
  the ones whose order Kafka leaves to a hash map or a set (the order of the refused among themselves in CreateTopics and Metadata) and the texts of
  the 29 answers for which Kafka's message is the default one (set from `Errors.TOPIC_AUTHORIZATION_FAILED.message`, checked against the source only).

- **SEC-D40.** Client certificates and mutual TLS are out of scope: the listener never asks for one (Kafka's `ssl.client.auth` `requested` and `required`), so on a TLS
  listener without SASL the principal is always `User:ANONYMOUS` (what Kafka answers with `ssl.client.auth=none`), and no principal is read from a certificate subject. The Go edition
  has the same limit.
- **SEC-D41.** `ssl.principal.mapping.rules` has nothing to map (SEC-D40). Out of scope.
- **SEC-D42.** The protocol versions and cipher suites are rustls's with ring (TLS 1.3 and 1.2; AEAD suites only). Kafka's `ssl.enabled.protocols`, `ssl.cipher.suites`, `ssl.protocol`,
  `ssl.provider` and `ssl.endpoint.identification.algorithm` have no counterpart; a client that can only speak TLS 1.1 or a CBC-only suite fails its handshake (SEC-146).
- **SEC-D43.** The broker does not generate a certificate and does not reload one. The files are read at start; a renewed certificate needs a restart. The documentation gives the
  two-step `openssl ecparam` recipe of the Go edition (LibreSSL-safe), and `interop/run.sh -tls` makes a throwaway CA and leaf the same way.
- **SEC-D44.** PEM only: no JKS, PKCS#12 or DER, no encrypted key, no key password. One certificate for the one listener; no SNI-based choice of certificate, no ALPN, no session tickets.
- **SEC-D45.** A failed handshake may send the client a TLS alert record (rustls writes the alert before it closes); the JVM's `SSLEngine` does the same. No Kafka bytes are sent (SEC-145).
- **SEC-D46.** The pre-auth budget applies to the handshake even with SASL off (SEC-148); Kafka bounds neither (extends SEC-D2).
- **SEC-D47.** The advertised host (`--advertised-host`, or the listen address) must be a name or address in the certificate for clients that verify host names; the broker checks nothing of the
  kind (Kafka's broker does not either).
- **SEC-D48.** ring is built from C and assembly by its build script, so a build with the `tls` feature needs a C compiler (and, where ring does not ship the object file for the target, an
  assembler): clang or gcc on Linux and macOS, the MSVC build tools on Windows (ring ships prebuilt Windows objects, so NASM is not normally needed; not verified here, the `windows` CI job checks
  the default build only). The default build needs neither. Where it matters: Windows developers and minimal CI images that have no compiler.

- **SEC-D30.** DeleteGroups with the same allowed name twice answers it twice (the existing behaviour, pinned by `group_admin`: deleted, then not found); Kafka de-duplicates
  the request first (KA:1431). The refused names are de-duplicated, as Kafka's are.
- **SEC-D31.** A SyncGroup of v5 or later with no protocol type or name is 23 up front (KA:1406-1411) only for a caller that is also refused; an allowed caller's goes to the group
  as before, which judges only the names it is given. Kafka answers 23 to everyone.
- **SEC-D32.** AddPartitionsToTxn: Kafka removes the internal topics (`__consumer_offsets`, `__transaction_state`) from the topics it authorizes, so they are 29 even with no
  authorizer (KA:1912). Here those topics do not exist as topics: they are 3 (or 29, if the caller may not WRITE them). v4 and later (a broker's, CLUSTER_ACTION) is not served.
- **SEC-D33.** InitProducerId's `Enable2Pc` and `KeepPreparedTxn` are version 6 fields; version 6 is not served, so the TWO_PHASE_COMMIT check (SEC-120) cannot be reached by a client.
- **SEC-D34.** Unsure, from the source alone: a DescribeTransactions entry refused with 53 carries the schema defaults for the rest (producer id 0, start time 0), as Kafka's
  `new TransactionState().setTransactionalId().setErrorCode()` does; the entry for an id that is not found keeps the -1 and -1 this broker already gave it.
- **SEC-D35.** Unsure: ConsumerGroupDescribe authorizes the topic names it has filled in. A topic deleted since a member held it has the name `""` there, which no ACL
  can name, so a caller that is not a super user and not covered by the default is told 29 for such a group. Kafka's coordinator names a missing topic from its metadata
  image; whether it too gives `""` was not confirmed.
- **SEC-D36.** Unsure: FindCoordinator before v4 puts the error's own message in the answer (`Errors.message()` is the exception's, `Group authorization failed.`; the `NONE` of a success
  is its `toString`), read from `FindCoordinatorResponse.prepareOldResponse`. Not recorded.
- **SEC-D37.** Not served, so not checked: share groups (key type 2), the share and streams group APIs, AddPartitionsToTxn v4 and later.

- **SEC-D50.** Two layers of users (SEC-172). Kafka has one store, the cluster metadata, and its PLAIN users live in a JAAS file outside it; here the users of the flags, the file and the variable are derived at every start and never stored. If a name is in both layers the bootstrap user wins for every
  mechanism and the stored entries under the name are shadowed, not deleted. An Alter or Delete aimed at a bootstrap user is refused with `UNACCEPTABLE_CREDENTIAL` (93) and a message that names the three sources: Kafka has no such case, and 93 (a credential it will not accept) is the closest of its codes; the check sits after the
  name and the mechanism and before the iteration count and the existence check, and, like every error, it ends the user's part of the request (SEC-165). The alternative, letting the API override a bootstrap user until the next start, would make the broker's login depend on the order of events.
- **SEC-D51.** The two APIs work with SASL off (decision D4): the authorizer then governs by `User:ANONYMOUS`, and with `--acl-default allow` (the default) and no ACL **anybody who can connect may make, replace and delete users** and list them. That is the same open-broker posture as the ACL APIs (SEC-D7) and as
  every other API of a broker with no flags: it matters only if SASL is on, and then the users made are the only ones SASL checks. A broker that must not allow it needs `--acl-default deny` with a super user. Kafka without an authorizer answers `SECURITY_DISABLED` (54) to the ACL APIs, but its credential APIs have no such case (ControllerApis checks `authorizeClusterOperation`, which succeeds with no authorizer).
- **SEC-D52.** The order of the results of Alter and of Describe is the request's (Alter: deletions that happened, upsertions that happened, refusals) or by name (Describe with no list), not a Java hash map's, which no client depends on (SCM:229-257, ScramImage.java:96-127).
- **SEC-D53.** Metadata.version is not modelled: Kafka's `UNSUPPORTED_VERSION` `The current metadata.version does not support SCRAM` (SCM:195, 216) is unreachable here, as a broker of 4.x always supports SCRAM.
- **SEC-D54.** Describe with a **null** user list from a caller who is denied: Kafka's `getErrorResponse` iterates `data.users()` without a null check (DescribeUserScramCredentialsRequest.java:70), so the denial of a describe-all request throws a NullPointerException there and the client sees whatever the error path then does. Here it is the top-level 31 with no entries (what the same code does for an empty list). Not verified against a running Kafka.
- **SEC-D55.** A changed or deleted credential applies to new logins; a connection that is logged in stays logged in, as in Kafka with `connections.max.reauth.ms` at its default of 0. Kafka with re-authentication on checks again at the next re-login; this broker has no re-authentication (kafka-semantics 3.7, out of scope).
- **SEC-D56.** The credentials are kept in a table of the broker's own database (`meta.redb`), key mechanism then user, value salt, iterations and the two keys, not as `UserScramCredentialRecord`s in a replicated metadata log. A data directory made by a build without this slice has no such table and reads as none; one written by this build and opened by an older one has a table the older one ignores. The keys are not encrypted at rest (a stolen `meta.redb` lets someone check
  passwords offline, not log in: SEC-162).
- **SEC-D57.** A stored row the broker cannot use is left in the table and named at warn (SEC-171); Kafka would fail to replay the log. Deleting such a row over the API is `RESOURCE_NOT_FOUND`, because the snapshot does not have it.
- **SEC-D58.** The environment variable is read once and not cleared (`set_var` is unsafe on edition 2024 and the process is multi-threaded): it is visible to other processes of the same user for the life of the broker. `--sasl-users-file` with mode 600 is the one that does not leave the password in `ps`, in `/proc/<pid>/cmdline` or in `environ`.
- **SEC-D59.** Passwords in the file and the variable are ASCII without NUL, as the flag's (no SASLprep: Phase 8 "Not in this phase"), and are held in memory for the life of the process (SEC-D5). A stored credential holds no password at all.

- **SEC-D60.** The guard has no `.doze` special case: the Go edition's allows the shared `.doze` zone, this one allows exactly what SEC-181 lists and a name the operator needs is listed with
  `--console-allow-host` (PLAN.md, the allowed-hosts guard "with no `.doze` special case").
- **SEC-D61.** The guard is stricter than the Go edition's in five shapes, all of which no browser sends in a `Host` header: a bare IPv6 literal (`::1`), brackets around anything but an IPv6 literal (`[localhost]`,
  `[127.0.0.1]`, `[dev.example.com]:1`), an IP literal with a trailing dot, a name with a character outside letters, digits, `-` and `_` (or any non-ASCII one: a space, `@` or a control character under `.localhost`,
  or in a listed name), and a name over 253 bytes or a label over 63. A refusal costs a flag; an acceptance costs the defence.
- **SEC-D62.** A configuration that cannot start (every refusal of the flags: SASL, TLS, ACL, numbers, the console rule) exits 2, as clap's own error for a bad flag does and as the Go edition's configuration errors
  do; before slice 8g it exited 1 through `main`'s `Result`. Failures after the start (the data directory in use, an I/O error) still exit 1. No test or script read the code 1.
- **SEC-D63.** `--console-allow-host` and `--console-unprotected` are parsed and kept and nothing acts on them (SEC-187): there is no console or registry listener before Phase 9. The rule on their inputs (SEC-188)
  is built now. A console is taken as on unless `--console=false` (the Go edition's default), so an address given alone is judged; this broker has no default console address, so with no `--console-addr` no rule applies.
