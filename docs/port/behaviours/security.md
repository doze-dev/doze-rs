# Security: SASL login and ACLs (Phase 8)

Slice 8a: SASL/PLAIN and the login session (SEC-1 to SEC-15). Slice 8b: the authorizer, the three ACL APIs, and the first two
handler checks, Metadata and InitProducerId (SEC-16 to SEC-30). (SCRAM is slice 8c, the authorization matrix 8d, TLS 8e: their
rules join this file when they land.) What Apache Kafka 4.3.1 does, and the test that holds each. The source is the recordings
(`conformance/recordings/sasl-plain.json`, the step names are the oracle's) where there is one, else Kafka 4.3.1's own source,
read by item (`SaslServerAuthenticator` SSA, `PlainSaslServer`, `SocketServer`; the line numbers are in
`phase8/kafka-semantics.md` 3.1 to 3.9 and 6): **(source)** marks those. The pure parts are in `crates/kafka-auth`, the login
order in `crates/kafka-server/src/session*`, the flags in `crates/doze-kafka/src/cli/sasl.rs` and `cli/acl.rs`. The authorizer is
`crates/kafka-auth/src/authz*` (pure), its stored form `crates/kafka-meta/src/{acl.rs,store/acls.rs}`, and its use
`crates/kafka-broker/src/{authorizer.rs,caller.rs,handlers/*_acls.rs}`. Kafka source abbreviations are those of `phase8/kafka-semantics.md`
(SAD = `StandardAuthorizerData.java`, KA = `KafkaApis.scala`, ACM = `AclControlManager.java`).

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
(the table of kafka-semantics 3.8 and the other orders). Only PLAIN can be enabled in 8a.

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
(a dev-dependency). Unused by PLAIN: they exist for SCRAM (8c). Test: `kafka-auth crypto::tests::*`, `crypto::pbkdf2::tests::*`, `crypto::base64::tests::*`.

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
