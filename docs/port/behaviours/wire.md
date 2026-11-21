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
`--kafka-version` 1 to 4 selects the ranges advertised per API. Each major stands for the newest release of it there is a
recording of: Kafka 1.1, 2.8, 3.9 and 4.4 (owner's decision, 2026-11-19; `KafkaMajor::release`, the CLI's help says so). The table lists every served API once with
its range in each profile (none where the profile does not offer it). SASL APIs are offered only when SASL
is configured; telemetry APIs only when client telemetry is on. The advertised list is sorted by API key.
Test: `profile::tests::advertised_is_sorted_and_conditional`

### W9. No profile offers more than a real broker of that major
Held to the recorded ApiVersions of a real broker of each major
(`conformance/recordings/api-versions.json`).
Test: `tests/recorded_api_versions.rs::no_profile_offers_more_than_a_real_broker`. Narrower is held too: W36.

### W10. Every profile starts where its recorded broker starts
Every API of every profile starts at the recorded broker's floor (debt K2, 2026-11-19; before it the crate's floors raised
eight at majors 1 to 3). The crate reads what Kafka 4.0 removed (KIP-896) again: v0 of the ACL APIs, OffsetForLeaderEpoch,
DescribeLogDirs (vendor/kafka-protocol PATCHES.md 5), AlterReplicaLogDirs and the token APIs (7), and the oldest versions of
Produce, Fetch, ListOffsets, OffsetCommit, OffsetFetch, CreateTopics, DeleteTopics and DescribeConfigs (8, regenerated with
the fields only those versions have). Produce v0 to v2 and Fetch v0 to v3 carry message formats v0 and v1 (W38). Kafka 4
lists Produce from v0 and closes the connection on v0 to v2, as 4.4.0 does (its schema starts at v3): `Profile::check`.
Test: `tests/every_offered_version.rs::every_offered_request_is_read`, `tests/recorded_api_versions.rs::every_profile_starts_where_the_real_broker_starts`, `profile::tests::each_major_offers_what_it_should`, `tests/old_versions.rs`, recorded `acl-v0` (tools/kafka-replay, Apache Kafka 3.9.1)

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

### W12d. Every answer carries the throttle time
Whatever the API, an answer whose version has a `throttle_time_ms` carries the time the client was throttled for (0 unless a quota
was exceeded, `quotas.md`); at a version without the field it is not written (Kafka marks it ignorable).
Test: `api::throttle::tests::the_throttle_is_written_where_the_version_has_it_and_left_out_where_it_has_not`

### W12c. The two telemetry APIs are the only extension to what real brokers list
Offered only at Kafka 4 and only when client telemetry is on (`--client-telemetry`, on by default: W35); the recorded brokers have no reporter, so they
do not list them.
Test: `tests/recorded_api_versions.rs::telemetry_apis_are_the_only_extension`

### W36. Every recorded API and range is offered as recorded, or is a written-down deviation
W9 is one direction; this is both. For each major, every API the recorded broker lists is offered at exactly its range,
and nothing else is offered, unless the pair (major, API) is in the deviations table of the test with its reason and its
row: inter-broker and controller APIs real brokers list on the client listener (LeaderAndIsr,
StopReplica, UpdateMetadata, ControlledShutdown, WriteTxnMarkers, AlterPartition, UnregisterBroker, DescribeQuorum, the
Raft voter APIs, UnregisterController and the share-state APIs 83 to 87), left out by the owner's decision of 2026-11-18: clients never
send them, a single node has no other broker, and serving them would only make the list match (debt K4). Nothing else: the
older columns and the floors left the table on 2026-11-19 (K2, W10, W38). A table entry that no longer differs fails as well, so the list cannot go
stale. The admin APIs of debt K3 left the table on 2026-11-19: every one is offered as recorded (ADM-10 to ADM-17); and so did the
newer versions at Kafka 4 (K1, W37): Kafka 4 serves every version 4.4.0 lists of every API it serves. SaslHandshake is 0 to 1 in
every major, as every recorded broker lists it
(v0 is raw tokens, SEC-401).
Test: `tests/recorded_api_versions.rs::every_recorded_range_is_offered_as_recorded_or_is_a_written_down_deviation`

### W37. Kafka 4 serves every version Kafka 4.4.0 lists, each handled (debt K1, 2026-11-19)
The Kafka 4 column of `api.rs` is 4.4.0's for every API served: Produce 3-13, Fetch 4-18, ListOffsets 1-11, Metadata 0-13,
OffsetCommit 2-10, OffsetFetch 1-10, FindCoordinator 0-6, DescribeGroups 0-6, ApiVersions 0-5, DeleteGroups 0-3,
TxnOffsetCommit 0-6, ListTransactions 0-2, DescribeLogDirs 1-5 (InitProducerId 6 only with `--unstable-api-versions`,
TX-27). The crate reads them all (vendor/kafka-protocol PATCHES.md 6). What each version changed, and what is done with it,
as a real Kafka 4.4.0 answered the same raw requests (`tools/wire-tail-oracle/Probe.java`, files `kafka-4.4.0*.txt`):

- **Fetch 14-18.** 14 adds an error only tiered storage raises; 15 moves the follower's id into the `ReplicaState` tagged
  field (KIP-903), so a follower (CLUSTER_ACTION, no quota) is read from there from v15 (`fetch::replica_of`); 16 adds
  `NodeEndpoints` and the current leader for NOT_LEADER and FENCED_LEADER_EPOCH, which one node never answers; 17 and 18
  add follower-only tagged fields (directory id, KIP-853; high watermark, KIP-1166), read and not used. A known id with a
  partition the topic has not is UNKNOWN_TOPIC_OR_PARTITION, as by name; only an id that stands for no topic is
  UNKNOWN_TOPIC_ID (it was 100 for both before). Tests: `handlers::fetch::tests::the_replica_is_read_from_its_state_from_v15`,
  `tests/fetch.rs::an_unknown_topic_or_partition_is_unknown_and_by_id_it_is_an_unknown_id`.
- **ListOffsets 8-11.** -4 (local start, v8) is the log's start; -5 (newest tiered, v9) and -6 (first not uploaded, v11)
  are offset -1 at leader epoch -1, as for a topic with no tiered storage; v10's `TimeoutMs` is for tiered reads and not
  needed. A special time below the version that brought it, or one no version has (-7), is UNSUPPORTED_VERSION, judged
  before the topic is looked for (`ReplicaManager.fetchOffset`; the topic was looked for first here before).
  Test: `tests/versions_4_4.rs::the_tiered_storage_times_are_answered_as_kafka_4_4_answers_them_with_none`.
- **Metadata 13** adds a top-level error code, which a broker sets on none of its answers: 0.
- **OffsetCommit 10 and OffsetFetch 10** name topics by id (KIP-848): an id that stands for no topic (the nil id
  included) is UNKNOWN_TOPIC_ID on each partition, before its right is asked and without asking the group; the answer
  names each topic by id; fetching every offset at v10 leaves out a topic with no id. OffsetCommit answers in the order of
  Kafka's builder at every version: the partitions `KafkaApis` answers itself (unknown id, barred topic, unknown topic or
  partition) first, then the group's; and an unknown partition is 3 even where the group would refuse the committer.
  Test: `tests/versions_4_4.rs::offset_commit_and_fetch_v10_name_topics_by_id`.
- **FindCoordinator 5-6.** 5 lets a broker answer TRANSACTION_ABORTABLE, which this one never does; 6 adds the share
  coordinator's key type (2), groups.md / security.md SEC-337. Test: `tests/versions_4_4.rs::find_coordinator_v6_answers_a_share_partition_key`.
- **DescribeGroups 6** (KIP-1043): a group not found, or one that is not a classic group, is Dead with GROUP_ID_NOT_FOUND
  and `Group <id> not found.` or `Group <id> is not a classic group.`; up to v5 the same group is Dead with no error.
  Test: `tests/versions_4_4.rs::describe_groups_v6_says_a_group_is_not_found`.
- **ApiVersions 5** (KIP-1242): a client may name the cluster and node it means. Both or neither, else INVALID_REQUEST;
  another cluster, or another node with this cluster, is REBOOTSTRAP_REQUIRED (129) with no rows. A login in progress is
  held to the first rule only (Kafka's authenticator answers that ApiVersions, unchecked). Test:
  `api_versions::tests::a_v5_client_that_means_another_cluster_or_node_is_told_to_rebootstrap`.
- **DeleteGroups 3** adds an error message to each result, which Kafka sets only for a streams group's topology
  description plugin (none here): null. The empty group id is GROUP_ID_NOT_FOUND, as 4.4.0 looks it up like any other (it
  was INVALID_GROUP_ID here). Test: `tests/group_admin.rs::deleting_a_group_needs_it_empty_takes_its_offsets_and_says_when_there_was_nothing`.
- **ListTransactions 2** (KIP-1152): a pattern that is neither null nor empty keeps the ids it matches whole;
  one that does not compile is INVALID_REGULAR_EXPRESSION (128). Read with `regex-lite`, not Java's `Pattern`: what only
  Java has (look-around, back-references) does not compile here (W37-D1). Test:
  `tests/versions_4_4.rs::list_transactions_v2_keeps_the_ids_its_pattern_matches_whole`.
- **DescribeLogDirs 5** adds `IsCordoned` to each directory (KIP-1066); no directory is cordoned: false.

Every new version is also written and read back by `tests/every_offered_version.rs`. The oracle's lines that still differ
are the all-apis list (W36's table) and the order DescribeGroups answers groups in, which follows each group's offsets
partition and so the run's random names: no other line differs (2026-11-19).

- **W37-D1.** A ListTransactions v2 pattern is `regex-lite` syntax, not `java.util.regex`: the common syntax matches alike;
  look-around, back-references and possessive quantifiers are refused with 128 where Kafka accepts them.

### W38. Message formats v0 and v1, converted as each release converts them (debt K2, 2026-11-19)
The log stores record batch format v2 only. `kafka_wire::batch::legacy` converts between it and the older formats:

- **Up, Produce v0 to v2** (`legacy::up`, Kafka's `Log.analyzeAndValidateRecords` and `LogValidator`). The request's
  records are not held to one batch (Kafka checks that only from v3). Each whole entry is looked at first: over
  `max.message.bytes` is MESSAGE_TOO_LARGE, a CRC-32 (v0, v1) or CRC-32C (v2) that is not its contents' CORRUPT_MESSAGE, a
  size under 14 or an unknown magic CORRUPT_MESSAGE; a codec number Kafka does not know is UNKNOWN_SERVER_ERROR. A last
  entry cut short is not read; no whole entry appends nothing and is answered 0 with offset -1. From Kafka 2.0 (profiles 2
  and 3) a compressed set, or one led by a v2 batch, must be one entry, and every entry must have the first one's format
  (INVALID_RECORD); Kafka 1.1 converts any mix into one batch, in the codec of its compressed entries. One v2 batch is taken
  as from v3 (zstd's v7 floor is not asked). Otherwise each message (a wrapper's inner messages through its codec: each of
  the wrapper's format, with no codec of its own, INVALID_RECORD, and a valid CRC, CORRUPT_MESSAGE; zstd in a wrapper is
  INVALID_RECORD, unknown to 1.1) becomes a record of one uncompressed v2 batch: the broker's offsets, no producer, no
  headers, v0's time -1 (exempt from the timestamp bounds), v1's own (a LogAppendTime wrapper's time for its inner messages).
  That batch is then held to the topic's rules as any is (keys on a compacted topic, the timestamp bounds), written in the
  topic's codec or under `producer` the set's, and held to `max.message.bytes` as stored. An LZ4 frame of format v0 has
  Kafka's mistaken header checksum (KAFKA-3160), put right before it is read.
- **Down, Fetch v0 to v3** (`legacy::down`, `RecordsUtil.downConvert`). v0 and v1 read format v0, v2 and v3 format v1.
  The read is Kafka's slice of the log: from the batch that holds the offset, the partition's limit in bytes (up to v2 a hard
  limit, the first batch possibly cut; from v3, KIP-74, at least the first batch whole), the last batch cut where the slice
  ends. A control batch has no form there and is dropped; an aborted transaction's records are not told apart (an old
  consumer reads uncommitted); zstd cannot be written (UNSUPPORTED_COMPRESSION_TYPE). An uncompressed batch becomes one
  message per record from the fetch offset on; a compressed one one wrapper of all its records in its codec (v1 numbers its
  inner messages from 0, v0 by offset; the wrapper's offset is the last record's). Headers are lost; under LogAppendTime the
  batch's time is every message's and the type is marked. How the answer is filled is the release's: Kafka 1.1 converts it
  all and answers with what came out, or with the read as it is if nothing converts; from 2.0 (KIP-283,
  `LazyDownConversionRecords`) the answer is as long as the read, or as the first batch that converts if that is longer,
  what is converted past that is cut, what falls short is filled with overflow messages (offset -1, a length one past the
  end), the first batch that converts is converted twice when a batch that did not came before it (Kafka's iterator steps past
  one batch for it), and one that converts to nothing is one 12-byte overflow message; a read with no whole batch is answered
  as read by 2.8 and with nothing by 3.9. From Kafka 2.0 a topic that stores zstd is UNSUPPORTED_COMPRESSION_TYPE below
  Fetch v10, and one with `message.downconversion.enable=false` UNSUPPORTED_VERSION below v4.
- **Kafka 1.1 alone** closes the connection of a Produce v3 to v5 whose records are not one v2 batch (it fails the read of
  the request), answers INVALID_RECORD as CORRUPT_MESSAGE (INVALID_RECORD came with 2.4) and gives a read-uncommitted Fetch v4
  to v7 no last stable offset (-1).

Test: `batch::legacy::tests` (the down-conversion of an uncompressed batch byte for byte as Kafka's own `RecordsUtil` wrote it,
`tests/data/legacy-vectors.txt`; Kafka's message sets of every codec read up), recorded `old-formats-produce` and
`old-formats-fetch` (tools/kafka-replay, Apache Kafka 3.9.1), `tools/old-profiles-oracle` against Kafka 1.1, 2.8 and 3.9.1
(`produce-old`, `produce-old-refused`, `fetch-old`, `fetch-old-txn`, `zstd`), and old clients (interop/README.md).

What `tools/old-profiles-oracle` still finds different from the real brokers (2026-11-19), each decided here:

- **W38-D1.** A wrapper this broker writes is compressed by its own codecs, not Java's, so its bytes and size differ; where an
  answer is cut at its length, the cut falls elsewhere (a consumer fetches the rest next time). The records are the same.
- **W38-D2.** Every profile has Kafka 4.4's topic configs and their defaults (`compression.type=zstd` is taken at 1.1, 
  DescribeConfigs lists 4.4's set, `segment.bytes` is this broker's own); only `message.timestamp.after.max.ms`'s default
  is the release's (no bound before 4.0, KIP-937), as it decides which records are refused.
- **W38-D3.** Kafka 2.8 and 3.9 start a new generation when a static member is replaced while its group waits for SyncGroup
  (3.9 not on every run: it is a race with the join it completes); this broker answers with the current one.
- **W38-D6.** DescribeProducers v0 at Kafka 3.9 gives an idempotent producer coordinator epoch -1 and its batch's own time, and an
  unknown topic the text "This server does not host this topic-partition."; this broker answers as at Kafka 4 (0, its clock, no text).
- **W38-D4.** As at Kafka 4 (and decided elsewhere): no internal topics in Metadata, an authorizer is always there (the
  recorded brokers have none and answer the ACL APIs SECURITY_DISABLED), member ids are `doze-…`, and DescribeGroups,
  DeleteGroups and DeleteRecords answer in the order asked, where Kafka's follows its hash maps.
- **W38-D5.** The inter-broker APIs (K4).

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
The base offset is assigned at append time and is not covered by the CRC. The partition leader epoch is set to the leader's
(0, `kafka_storage::LEADER_EPOCH`) on every batch appended, as Kafka's `LogValidator` does; it is not covered by the CRC either.
LogAppendTime stamping sets the timestamp-type bit and the maximum timestamp and recomputes the CRC. Nothing else in a batch is
touched.
Test: `batch::tests::stamping_the_base_offset_changes_only_the_base_offset`, `batch::tests::the_leader_epoch_is_written_outside_the_crc_and_nothing_else_moves`, `batch::tests::log_append_time_changes_only_the_clock_bit_the_max_timestamp_and_the_crc` (byte for byte)

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
(Telemetry's own gate, per client instance and with no quota behind it; it does not touch the client quotas, and they do not touch
it. The other THROTTLING_QUOTA_EXCEEDED, of the controller mutation quota, is `quotas.md` QT-10.)
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
