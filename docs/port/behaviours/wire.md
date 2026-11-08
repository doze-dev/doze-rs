# Behaviours: the wire (kafka-wire)

What a Kafka-compatible broker must do between the socket and the handlers, distilled from the Go
source, its docs and the recorded real-Kafka scenarios. Each entry names the Rust test that covers it.
Test names are `crate::module::tests::name` unless a file is given.

## Framing

### W1. A frame is a 4-byte big-endian length and that many bytes
A zero length is an error. The payload is returned without the prefix.
Test: `frame::tests::a_frame_is_length_prefixed`, `frame::tests::a_zero_length_frame_is_refused`

### W2. The size cap depends on the API
Produce 100 MiB; PushTelemetry 1 MiB + 4096; every other request 1 MiB. A connection that has not
authenticated may send at most 1 MiB whatever the API. The cap is picked from the API key, which is the
first two bytes of the payload, before any of the body is read.
Test: `frame::tests::the_cap_follows_the_api_key`, `frame::tests::the_caps_are_exact` (the literal sizes, one under and one over), `frame::tests::an_unauthenticated_connection_is_capped_at_one_mib`

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
Test: `header::tests::parses_a_plain_header`, `header::tests::parses_a_flexible_header`

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
Test: `profile::tests::floors_are_never_below_what_can_be_decoded`, `profile::tests::the_apis_dropped_because_the_crate_cannot_decode_them_are_known`

### W11. A request outside the advertised range closes the connection
An API outside the profile, or a version outside its range, is refused (the connection is closed). The one
exception is ApiVersions, below.
Test: `profile::tests::outside_the_range_is_not_served`, `api::tests::an_unserved_key_is_refused`

### W12. ApiVersions at a version the broker does not serve is answered in v0
The answer is UNSUPPORTED_VERSION with one row: the versions of ApiVersions itself. The body of such a
request is not read. The client asks again at one of those versions.
Test: `api_versions::tests::an_unserved_version_is_answered_in_v0`, `incoming::tests::the_body_of_an_unserved_api_versions_version_is_not_read`, `tests/recorded_api_versions_exchanges.rs` (all 31 recorded exchanges)

### W13. From ApiVersions v3 the client names itself
A software name or version that is empty, or is not letters and digits with dots and dashes between them,
is INVALID_REQUEST with no rows; the connection lives on.
Test: `api_versions::tests::bad_client_software_is_invalid_request`, `api_versions::tests::software_names`

### W14. The ApiVersions response header is always v0
No tagged fields in the header, even at flexible versions.
Test: `response::tests::api_versions_uses_the_v0_response_header_at_every_version` (reads each frame back with the crate's decoders and
requires every byte used); `tests/every_offered_version.rs::every_offered_response_is_written_and_read_back`

## Response

### W15. A response is length, correlation id (and tagged fields when flexible), body
Test: `response::tests::a_response_is_a_framed_header_and_body`

### W12b. An ApiVersions body that cannot be read is answered all the same
At a served version, the list is returned (nothing is checked about the client).
Test: `incoming::tests::api_versions_with_an_unreadable_body_is_still_answered`, `api_versions::tests::an_unreadable_body_at_a_served_version_gets_the_list`

### W12c. The two telemetry APIs are the only extension to what real brokers list
Offered only at Kafka 4 and only when client telemetry is on; the recorded brokers have no reporter, so they
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
Test: `batch::tests::shape_rules`, `batch::tests::the_count_limits_are_exact`, `batch::tests::checks_run_in_kafkas_order_so_the_first_fault_is_the_one_reported`, `batch::tests::refusals_are_answered_as_kafka_answers_them`

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
Test: `batch::produce::tests` (eleven); `batch::tests::a_run_of_batches_is_walked_one_at_a_time` covers the other use,
walking a segment

### W23. Control batches are identified, not decided on
The wire layer reports the control and transactional bits; refusing a client's control batch is the
broker's rule.
Test: `batch::tests::attribute_bits`
