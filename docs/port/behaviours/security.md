# Security: SASL login and ACLs (Phase 8)

Slice 8a: SASL/PLAIN and the login session (SEC-1 to SEC-15). Slice 8b: the authorizer, the three ACL APIs, and the first two
handler checks, Metadata and InitProducerId (SEC-16 to SEC-30). Slice 8c: SASL/SCRAM-SHA-256 and SCRAM-SHA-512 (SEC-40 to SEC-51).
(The authorization matrix is slice 8d, TLS 8e: their rules join this file when they land.) What Apache Kafka 4.3.1 does, and the
test that holds each. The source is the recordings (`conformance/recordings/sasl-plain.json`, the step names are the oracle's) where
there is one, else Kafka 4.3.1's own source, read by item (`SaslServerAuthenticator` SSA, `PlainSaslServer`, `SocketServer`; the line
numbers are in `phase8/kafka-semantics.md` 3.1 to 3.9 and 6): **(source)** marks those. The pure parts are in `crates/kafka-auth`
(SCRAM: `scram.rs`, `scram/parse.rs`, `scram/keys.rs`), the login order in `crates/kafka-server/src/session*`, the flags in
`crates/doze-kafka/src/cli/sasl.rs` and `cli/acl.rs`. The authorizer is `crates/kafka-auth/src/authz*` (pure), its stored form
`crates/kafka-meta/src/{acl.rs,store/acls.rs}`, and its use `crates/kafka-broker/src/{authorizer.rs,caller.rs,handlers/*_acls.rs}`.
Kafka source abbreviations are those of `phase8/kafka-semantics.md` (SAD = `StandardAuthorizerData.java`, KA = `KafkaApis.scala`,
ACM = `AclControlManager.java`).

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
- **SEC-D11.** The ACL checks are in Metadata and InitProducerId only; every other handler is open to every principal until slice 8d. Metadata does
  not compute the authorized-operations bitfields (`IncludeTopicAuthorizedOperations` and the cluster one answer the schema default). A CreateAcls of
  more than 10,000 new ACLs is not limited (Kafka's outcome is unspecified, kafka-semantics U3). DescribeConfigs on the broker does not report
  `allow.everyone.if.no.acl.found` or `super.users` (the Go edition does; real Kafka lists neither, so the recorded answers would differ).
- **SEC-D12.** The server nonce is 18 random bytes in base64 (24 characters); Kafka's is a base-36 rendering of 130 random bits (up to 26). Both are
  unpredictable, printable and comma-free; no client depends on the alphabet (SEC-51).
- **SEC-D13.** With several extensions after the nonce Kafka signs them in the iteration order of an immutable `Map.copyOf`, which is random per JVM, so a client
  that sends two cannot be logged in reliably there; here they are signed in the order sent (a name twice counts once). `tokenauth=true` is the
  delegation-token path of Kafka, answered here as an unknown user (the generic 58 at client-first, which is what Kafka answers when the token does not exist);
  there are no delegation tokens (out of scope).
- **SEC-D14.** Every verifier has 4096 iterations and a random 16-byte salt. Kafka takes any count from 4096 to 16384 and the salt of whoever made the credential
  (its admin client makes about 26 ASCII bytes); that needs `AlterUserScramCredentials`, which is slice 8f. A stored count under 4096, which Kafka refuses at
  client-first, cannot occur.
- **SEC-D15.** The derived keys, the salted password and the proof are `Vec`s that are not zeroed on drop (extends SEC-D5); the verifiers are kept for the life of
  the process, the intermediate values for the length of a call.
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
