# Behaviours: the wire (kafka-wire)

What a Kafka-compatible broker must do between the socket and the handlers, distilled from the Go
source, its docs and the recorded real-Kafka scenarios. Each entry names the Rust test that covers it.
Test names are `crate::module::tests::name` unless a file is given.

## Framing

### W1. A frame is a 4-byte big-endian length and that many bytes
A zero length is an error. The payload is returned without the prefix.
Test: `tests/frame_chunking.rs::any_split_gives_the_same_frames`, `frame::tests::a_zero_length_frame_is_refused`

### W2. The size cap depends on the API
Produce 100 MiB; PushTelemetry 1 MiB + 4096; every other request 1 MiB. A connection that has not
authenticated may send at most 128 KiB whatever the API (Kafka's `sasl.server.max.receive.size` is 512 KiB; it was 1 MiB before Phase 8, the audit made it 64 KiB and the replay raised it to 128 KiB, because the recorded Kafka takes a 64 KiB token: SEC-4, SEC-194, SEC-D71). The cap is picked from the API key, which is the
first two bytes of the payload, before any of the body is read.
Test: `frame::tests::the_caps_are_exact` (the literal sizes, one under and one over), `frame::tests::an_unauthenticated_connection_is_capped_at_128_kib`

### W3. A length over every cap is refused on the length alone
No waiting for a key that may never come. A length under 2 cannot hold a key and is refused.
Test: `frame::tests::a_length_over_every_cap_needs_no_key`, `frame::tests::a_length_too_short_for_a_key_is_refused`

### W4. Memory follows what the peer sends
A frame claiming 100 MiB and sending nothing costs one 64 KiB chunk, not 100 MiB.
Test: `frame::tests::memory_follows_the_bytes_that_arrive`

### W5. Frames are the same however the bytes arrive in pieces
Splitting the input at any points gives the same frames.
Test: `tests/frame_chunking.rs::any_split_gives_the_same_frames` (property)

## Request header

### W6. The header is api key, api version, correlation id, nullable client id
Flexible versions (request header v2) carry tagged fields after the client id. Which header version applies
depends on the API and its version.
Test: `tests/every_offered_version.rs::every_offered_request_is_read` (every API at every offered version, plain and
flexible headers, its correlation id and client id)

### W7. An unknown API key is an error, not a panic
Test: `header::tests::an_unknown_key_is_an_error`; `tests/no_panics.rs::arbitrary_bytes_never_panic`

## Versions and ApiVersions

### W8. Four profiles, one table
`--kafka-version` 1 to 4 selects the ranges advertised per API. The table lists every served API once with
its range in each profile (none where the profile does not offer it). SASL APIs are offered only when SASL
is configured; telemetry APIs only when client telemetry is on. The advertised list is sorted by API key.
Test: `profile::tests::advertised_is_sorted_and_conditional`

### W9. No profile offers more than a real broker of that major
Held to the recorded ApiVersions of a real broker of each major
(`conformance/recordings/api-versions.json`).
Test: `tests/recorded_api_versions.rs::no_profile_offers_more_than_a_real_broker`

### W10. Floors are the crate's where the crate's are higher
Kafka 4.0 removed the oldest versions of nine APIs (KIP-896) and the wire crate does not decode them. A
profile never advertises below what the crate can decode. (Deviation from Go, which advertised v0 of these
at majors 1 to 3: no client of the last eight years negotiates them.) One consequence is a removal, not a
raised floor: Kafka 1.x has only v0 of DescribeAcls, CreateAcls and DeleteAcls, so those three are not
offered at `--kafka-version 1`. The list of such removals is pinned, so a new one needs a reason.
Test: `tests/every_offered_version.rs::every_offered_request_is_read` (every advertised version is decoded), `profile::tests::the_apis_dropped_because_the_crate_cannot_decode_them_are_known`

### W11. A request outside the advertised range closes the connection
An API outside the profile, or a version outside its range, is refused (the connection is closed). The one
exception is ApiVersions, below.
Test: `profile::tests::outside_the_range_is_not_served`, `incoming::tests::a_version_outside_the_range_is_refused`

### W12. ApiVersions at a version the broker does not serve is answered in v0
The answer is UNSUPPORTED_VERSION with one row: the versions of ApiVersions itself. The body of such a
request is not read. The client asks again at one of those versions.
Test: `incoming::tests::the_body_of_an_unserved_api_versions_version_is_not_read`, `tests/recorded_api_versions_exchanges.rs` (all 31 recorded exchanges)

### W13. From ApiVersions v3 the client names itself
A software name or version that is empty, or is not letters and digits with dots and dashes between them,
is INVALID_REQUEST with no rows; the connection lives on.
Test: `tests/recorded_api_versions_exchanges.rs` (the 25 client-software steps Apache Kafka 4.3.1 answered, names it
accepts and names it refuses)

### W14. The ApiVersions response header is always v0
No tagged fields in the header, even at flexible versions.
Test: `response::tests::api_versions_uses_the_v0_response_header_at_every_version` (reads each frame back with the crate's decoders and
requires every byte used); `tests/every_offered_version.rs::every_offered_response_is_written_and_read_back`

## Response

### W15. A response is length, correlation id (and tagged fields when flexible), body
Test: `tests/every_offered_version.rs::every_offered_response_is_written_and_read_back` (every API at every offered
version, read back at the header version the protocol fixes, every byte used)

### W12b. An ApiVersions body that cannot be read is answered all the same
At a served version, the list is returned (nothing is checked about the client).
Test: `incoming::tests::api_versions_with_an_unreadable_body_is_still_answered`, `api_versions::tests::an_unreadable_body_at_a_served_version_gets_the_list`

### W12c. The two telemetry APIs are the only extension to what real brokers list
Offered only at Kafka 4 and only when client telemetry is on (`--client-telemetry`, on by default: W35); the recorded brokers have no reporter, so they
do not list them.
Test: `tests/recorded_api_versions.rs::telemetry_apis_are_the_only_extension`

## Hostile input

### W16. A count the buffer cannot hold does not size an allocation
An array reserves room for at most 1024 elements before they decode, whatever the count says (a count equal to the
bytes left is believable and still a lie: elements are tens of bytes). Test: `tests/hostile_input.rs` (five tests,
including `a_count_equal_to_the_bytes_left_allocates_about_the_body`); vendor/kafka-protocol/PATCHES.md

### W17. No panics on arbitrary bytes
Frames, headers and request bodies of every served API at every version.
Test: `tests/no_panics.rs`; fuzz targets `fuzz_frame`, `fuzz_request`, `fuzz_batch`

## Record batches (v2)

### W18. A batch is magic 2, a 61-byte header, and records
The checks run in Kafka's order, which decides which fault a client hears about: shape, CRC, codec, then count.
Declared length matches the bytes present; the record count is positive and agrees with `lastOffsetDelta + 1`.
A count that cannot fit the bytes that carry it (uncompressed: more records than body bytes; compressed:
more than 16 Mi) is refused: the log-end offset advances by the count, so a lying count would skip offsets.
Test: `batch::tests::shape_rules`, `batch::tests::the_count_limits_are_exact`, `batch::tests::checks_run_in_kafkas_order_so_the_first_fault_is_the_one_reported`, `batch::produce::tests::a_bad_batch_is_answered_by_its_own_fault`

### W19. The CRC is CRC32C over everything from the attributes field on
A batch with the wrong CRC is refused. A codec number that is not one of Kafka's five (0 to 4) is refused as `UNKNOWN_SERVER_ERROR`.
Test: `batch::tests::a_wrong_crc_is_refused`

### W20. Our parser agrees with an independent encoder
Batches written by the `kafka-protocol` crate's encoder parse here, with the same fields.
Test: `tests/batch_agrees_with_the_crate.rs`

### W21. Stamping changes only what it must
The base offset is assigned at append time and is not covered by the CRC. LogAppendTime stamping sets the
timestamp-type bit and the maximum timestamp and recomputes the CRC. Nothing else in a batch is touched.
Test: `batch::tests::stamping_the_base_offset_changes_only_the_base_offset`, `batch::tests::log_append_time_changes_only_the_clock_bit_the_max_timestamp_and_the_crc` (byte for byte)

### W22. A Produce partition carries exactly one batch
Kafka takes the first batch of a partition's records, refuses a second whole batch (`INVALID_RECORD`), and drops bytes
after a batch that were never a whole one. No whole batch at all is `INVALID_RECORD`. A first batch that declares fewer
than 14 bytes, or a magic byte outside 0 to 2, is `CORRUPT_MESSAGE` for the whole request; magic 0 or 1 is
`INVALID_RECORD`. A producer's base offset is 0 and it may not write a control batch; a producer id with no sequence
number is refused. The size limit and the zstd-needs-Produce-v7 rule are the broker's, which knows the topic.
Test: `batch::produce::tests`; `batch::tests::a_run_of_batches_is_walked_one_at_a_time` covers the other use,
walking a segment

### W23. Control batches are identified, not decided on
The wire layer reports the control and transactional bits; refusing a client's control batch is the
broker's rule.
Test: `tests/batch_agrees_with_the_crate.rs::transactional_and_control_bits_are_read`,
`batch::marker::tests::a_marker_is_a_valid_control_batch_of_the_producer_with_no_sequence`

## DescribeCluster and client telemetry (Phase 9, slice 9i)

The three APIs the replay still failed on at the close of Phase 8 (the ten `telemetry-*` scenarios and `version-matrix`). The recordings are Apache
Kafka 4.3.1's (`conformance/recordings/telemetry-*.json`, `version-matrix.json`); where this broker answers otherwise on purpose the rule says so
(**deviation SEC-D100**, **SEC-D101**). The authorization row of all three is in `security.md` (SEC-290). The handlers are
`crates/kafka-broker/src/handlers/{describe_cluster,get_telemetry_subscriptions,push_telemetry}.rs`, the store and the gate are
`crates/kafka-broker/src/telemetry.rs` and `telemetry/instance.rs`.

### W24. DescribeCluster v0 to v2 is the broker as Metadata says it
The cluster id, the controller (this node), one broker with the advertised host and port the connection's listener gives, no rack, never fenced
(`include_fenced_brokers` changes nothing), throttle 0. Offered at Kafka 3 (v0 to v1) and Kafka 4 (v0 to v2), by the profile table. Recorded: the three
`DescribeCluster v*` steps of `version-matrix`.
Test: `kafka-broker tests/describe_cluster.rs::the_default_request_is_answered_as_the_recorded_kafka_does_at_every_version`, `::it_names_the_cluster_and_the_broker_that_metadata_does`, `::fenced_brokers_are_never_listed_because_this_one_is_never_fenced`; `kafka-server connection::tests::served::api_versions_lists_only_what_the_broker_serves`

### W25. The endpoint type is checked first (source)
BROKER (1, the default) is answered. CONTROLLER (2) is `MISMATCHED_ENDPOINT_TYPE` (114) from v1 and `INVALID_REQUEST` (42) at v0, with the text `The request was sent to
an endpoint of type BROKER, but we wanted an endpoint of type CONTROLLER`, and no cluster in the answer. Any other number is `UNSUPPORTED_ENDPOINT_TYPE` (115) with `Unsupported endpoint
type N`. Not recorded (the matrix sends the default); read from `AuthHelper.computeDescribeClusterResponse` (`phase8/kafka-semantics.md` row 60).
Test: `tests/describe_cluster.rs::a_controller_endpoint_is_asked_of_the_wrong_node`, `::an_endpoint_type_that_is_neither_is_unsupported_and_says_which`

### W26. The cluster's authorized operations are the only thing an ACL changes in DescribeCluster
Not asked for: the schema's default (`i32::MIN`). Asked for: `0` when the caller may not DESCRIBE the cluster (whatever else it may do), else the bit `1 << code` of each of the seven operations an ACL on
the cluster can name that it may do. The answer is given to every caller (SEC-290).
Test: `tests/describe_cluster.rs::no_acl_is_needed_to_be_answered_and_the_operations_are_what_describe_gates`, `::an_open_broker_tells_every_operation`

### W27. A client with no instance id is given one; the same subscription is given to every client
GetTelemetrySubscriptions v0 with a zero `client_instance_id` is answered with a fresh version 4 UUID; with one, the same. The subscription is the standard metric set (`org.apache.kafka.`), every
5000 ms, cumulative (`delta_temporality` false), `telemetry_max_bytes` 1,048,576 (Kafka's default, recorded), compression types `[4, 3, 1, 2]` (recorded). Kafka with nothing configured subscribes a
client to nothing every 300,000 ms with deltas; the replay gives Kafka the same five seconds with a CLIENT_METRICS resource and holds the rest to the recording (**deviation SEC-D100**).
Test: `kafka-broker tests/telemetry.rs::a_client_with_no_id_is_given_one_and_the_subscription_the_recordings_show`

### W28. The subscription id is a checksum of the instance, so it survives a restart
CRC-32C over the instance id, the requested metrics and the interval, as a signed 32-bit number, as Kafka derives it. A push that holds the id a client was given before a broker restart is therefore taken
(recorded: `telemetry-broker-restart`); one that holds another instance's is `UNKNOWN_SUBSCRIPTION_ID` (117).
Test: `tests/telemetry.rs::a_subscription_id_is_the_same_after_a_restart_and_a_push_that_holds_it_is_taken`, `telemetry::tests::the_subscription_id_is_a_function_of_the_instance_alone`

### W29. The gate: a request inside the interval of the last one taken is `THROTTLING_QUOTA_EXCEEDED`
An instance remembers when a subscription was last given and a push last taken. A subscription request is early within an interval of the last request of either kind. A push is on time when it is the
instance's first, or a subscription was given since the last push that set the clock, or that push is an interval old; one on time sets the clock whatever becomes of it after, one that is early (89)
does not postpone the next, and a terminating push is taken though early without setting the clock. Recorded: `telemetry-throttle`, `-interval` (half an interval, nine tenths and a whole one, in real time), `-instance-binding-spaced`.
The answer to an early subscription request has no other field set.
Test: `telemetry::tests::the_first_push_is_on_time_the_next_is_early_until_an_interval_has_passed`, `::a_subscription_given_since_the_last_push_makes_the_next_push_on_time`, `::a_subscription_request_shares_the_clock_with_pushes_and_a_refused_one_postpones_nothing`, `tests/telemetry.rs::asking_again_at_once_is_throttled_with_nothing_else_in_the_answer`, `::a_push_straight_after_another_is_early_whatever_else_is_wrong_with_it`

### W30. What follows a terminating push
The instance remembers whether the last push it was sent said it was terminating, whatever became of that push (taken, or refused for another reason): a push that follows is `INVALID_REQUEST` (42) once, and its own
word replaces the memory, so terminating pushes in a row are refused from the second on and an ordinary push after one is refused once; the next is judged by the clock again. A subscription request does not clear it.
Recorded: `telemetry-terminating`, `-throttle`.
Test: `telemetry::tests::a_terminating_push_is_taken_early_and_closes_the_instance_for_the_next_push_only`, `::terminating_pushes_in_a_row_are_refused_from_the_second_on`, `::a_subscription_request_does_not_clear_a_terminating_push`, `tests/telemetry.rs::a_terminating_push_is_taken_when_early_and_the_next_is_refused_once`

### W31. A push refused for its subscription id or its compression type tells the client to ask again
The next subscription request of that instance is answered though it is early, once, and sets no clock, so the push straight after it is still early. A push that is refused for anything else, or any
push at all, cancels it. Recorded: `telemetry-ask-again`.
Test: `telemetry::tests::the_request_after_a_refusal_that_says_to_ask_again_is_answered_once_and_sets_no_clock`, `::a_push_that_comes_next_cancels_the_ask_again`, `tests/telemetry.rs::a_refusal_that_tells_a_client_to_ask_again_lets_its_next_request_through_once`

### W32. A push is judged in Kafka's order, and an instance belongs to no connection and no principal
No instance id (42); the gate (89 and 42, above, before anything else is looked at); the subscription id (117); the compression type, which is 0 to 4 (76); the size, over `telemetry.max.bytes` (118); the payload
inflating (87: corrupt under any codec, or inflating past 1 MiB). A payload that is not OpenTelemetry metrics, or is empty, is taken: no plugin reads it. An instance id the broker has never seen is adopted by
the first request that names it. Any connection, of any principal, may speak for any instance id (recorded: `telemetry-instance-binding`, `-spaced`).
Test: `tests/telemetry.rs::the_refusals_are_the_recorded_ones`, `::when_a_push_has_several_faults_the_first_in_kafkas_order_is_the_answer`, `::any_connection_may_speak_for_any_instance_and_no_acl_is_asked`

### W33. A payload is read to its end through a fixed buffer, never inflated whole
`kafka_wire::batch::inflates_within(codec, bytes, limit)` reads a compressed payload and keeps nothing; it fails the moment more than `limit` has come out, or the bytes are not what the codec says. A 64 MiB
gzip of zeros that weighs 64 KiB costs one 8 KiB buffer and the decompressor's window. It is a parser, with a place in `fuzz_batch`.
Test: `kafka-wire batch::inflate::tests::payload::a_payload_is_read_to_its_end_under_every_codec_and_the_limit_is_exact`, `::a_payload_that_claims_far_more_than_it_weighs_is_refused_at_the_limit_not_inflated`, `::bytes_that_are_not_a_stream_are_refused_under_every_codec`

### W34. What is kept of a client is bounded, allocates nothing until a client asks, and is never read
The last payload of each instance, as received (compressed or not, copied so that the frame it came in is not held): at most 128 instances and 4 MiB of payloads. A new instance past 128 evicts a
terminating one first, then the one heard from longest ago; payloads past 4 MiB are let go from the instance heard from longest ago (its gate state stays). An instance quiet for ten minutes (a minute if it
terminated) is forgotten the next time anyone calls; there is no timer. An evicted or forgotten instance is simply unknown again (W32). Nothing prints a payload or an id (SEC-292).
Test: `telemetry::tests::an_idle_store_holds_nothing`, `::bytes_are_bounded_and_the_payloads_heard_longest_ago_go_first`, `::entries_are_bounded_and_a_terminating_client_goes_before_the_one_heard_longest_ago`, `::evicting_an_instance_frees_its_bytes`, `::a_client_that_went_quiet_is_forgotten_the_next_time_anyone_calls`

### W35. `--client-telemetry` (default on) decides whether the two telemetry APIs are served and listed
On, ApiVersions at Kafka 4 lists GetTelemetrySubscriptions and PushTelemetry (as the recorded `version-matrix` has them); `--client-telemetry=false` serves and lists neither, and a client that sends one is
disconnected as for any API the broker does not serve (W11). DescribeCluster does not depend on it. Clients that support KIP-714 report every five seconds once it is on.
Test: `doze-kafka cli::tests::client_telemetry_is_on_unless_told_otherwise`, `kafka-server connection::tests::served::telemetry_is_listed_unless_the_broker_is_told_not_to_serve_it`, `connection::tests::an_api_the_broker_does_not_serve_closes_the_connection_without_an_answer`, `kafka-broker tests/telemetry.rs::the_two_apis_are_served_unless_the_broker_is_told_not_to`
