# Behaviours: Fetch and ListOffsets

From the Go source (`internal/broker/{fetch,listoffsets}.go`, `internal/storage/log/timestamps.go` at `4081fa6`) and
the recordings `fetch-above-the-end`, `listoffsets`. Not here: read-committed isolation beyond "the last stable
offset is the end" (Phase 6), fetch sessions (never).

### F1. A fetch returns whole batches from the one that holds the offset
Up to the byte limit; always the first batch however large. Caught up (offset = end) is an empty answer with no error.
High watermark = last stable offset = the log end; the answer carries the log start.
Test: `tests/fetch::a_fetch_returns_whole_batches_from_the_one_that_holds_the_offset`

### F2. An offset outside the log is OFFSET_OUT_OF_RANGE
With -1 for the high watermark, last stable offset and log start, and empty (not null) records.
Test: `an_offset_outside_the_log_is_out_of_range_with_no_offsets_and_empty_records`

### F3. Unknown topics and partitions
UNKNOWN_TOPIC_OR_PARTITION; by topic id (v13), UNKNOWN_TOPIC_ID.
Test: `an_unknown_topic_or_partition_is_unknown_and_by_id_it_is_an_unknown_id`

### F4. Byte limits
A partition reads up to its own limit, else the request's, else 1 MiB, and no more than the request has left; the first
batch of the first partition with data comes back even if larger, a later partition whose first batch does not fit gets
none. A read is never asked for more than 64 MiB at once.
Test: `the_requests_byte_limit_is_shared_out_and_the_first_batch_always_comes`, `round::tests::*`

### F5. Long poll
Not enough data (`min_bytes`, at least 1) waits up to `max_wait_ms` (never more than 30 s) for a produce to any of the
partitions, then answers with what has accumulated. An error is an answer and does not wait. The wait is subscribed before
the first read, so a produce at any moment is seen.
Test: `a_fetch_with_nothing_to_read_waits_for_a_produce_*`, `a_fetch_that_nothing_satisfies_answers_empty_after_the_wait_*`, `a_fetch_waits_until_it_has_min_bytes_*`

### F6. A topic deleted under a waiting fetch
The waiting fetch is woken and answers UNKNOWN_TOPIC_OR_PARTITION at once. (A waiter that is dropped between polls is no
longer woken: the waits keep their streams alive. A first version re-created them and slept the whole time.)
Test: `a_fetch_waiting_on_a_topic_that_is_deleted_is_told_it_is_unknown_at_once`

### F7. Fetch sessions are not kept
A request that continues one is FETCH_SESSION_ID_NOT_FOUND; every response says session 0.
Test: `fetch::a_request_that_continues_a_fetch_session_is_told_there_is_none_*`

### F8. Read-committed
Carries an empty list of aborted transactions (null for read-uncommitted); the last stable offset is the end until
transactions exist.
Test: `a_read_committed_fetch_carries_an_empty_list_of_aborted_transactions_*`

### F11. OffsetForLeaderEpoch v2-4: epoch 0 ends at the log end (source)
Every partition here has had one leader epoch, 0, from its log start (Metadata says so), which is the leader-epoch cache Kafka
holds for a single-node partition (`Partition.makeLeader` assigns epoch 0 at the start; `LeaderEpochFileCache.truncateFromStart`
moves it up with the log start). The answer is `LeaderEpochFileCache.endOffsetFor` over that cache: epoch 0 ends at the log end
offset; an epoch after it, or -1, is (-1, -1) with no error; an epoch below 0 other than -1 is that epoch ending at the log start.
A partition never written to starts and ends at 0. `current_leader_epoch` -1 is not checked, 0 matches, above is
UNKNOWN_LEADER_EPOCH (75), below -1 is FENCED_LEADER_EPOCH (74); a topic or partition the broker does not have is
UNKNOWN_TOPIC_OR_PARTITION, checked first (`Partition.checkCurrentLeaderEpoch`, `ReplicaManager.lastOffsetForLeaderEpoch`).
`replica_id` (v3) changes nothing. Rights: SEC-295. Offered at Kafka 2 to 4 at v2 to v4 (Kafka 1.1 has v0 only, which the wire
crate does not read: W10). Same as real Kafka 4.4.0 at v2, v3 and v4 for every case of `kafka-wire/examples/oracle_diff`
(epochs 0, 1, -1, -2, -7; current -1, 0, 1, -2, -9; an unknown partition and topic; the follower form; after DeleteRecords).
Every batch is stored under epoch 0, as Kafka's leader stamps its epoch into each batch it appends (`LogValidator`): a client's
(written with -1), a transaction marker, one the topic's `compression.type` re-encodes; the field is outside the CRC (W21). A
consumer learns epoch 0 from the records and, as Kafka's does (KIP-320), validates its position with this API after a leader change
or when it restores a committed offset; the cleaner keeps a batch's epoch, as Kafka's does. A log an older build wrote holds batches
with -1: they are served as stored (a consumer that read them has no epoch to validate) and the broker reads the whole log as epoch 0
from its start, so the answers above are the same for it.
Test: `handlers::offset_for_leader_epoch::tests::*`, `tests/offset_for_leader_epoch.rs::*`, `tests/leader_epoch_stamp.rs::*`;
interop `consumer-validates-position` (against doze-kafka and Kafka 4.4.0)

### L1. ListOffsets
Latest (-1) is the log end, earliest (-2) the log start; max timestamp (-3, v7+) is the first record with the largest
timestamp; a real timestamp is the first record from the log start whose timestamp is at least it, to the record. The
candidate batch's records are read, through its codec if it is compressed (streamed, nothing inflated is held), as
Kafka's `FileRecords.searchForTimestamp` and `RecordBatch.offsetOfMaxTimestamp` iterate them; a record's time is the
one Kafka's `DefaultRecordBatch` gives it: first timestamp plus delta, or, in a LogAppendTime batch, the batch's newest
timestamp (the broker's clock) for every record. Only a batch whose records cannot be read at all answers at the grain
of its header. Nothing qualifies: offset -1, timestamp -1. A partition named twice is answered twice (the console relies on it).
Test: `tests/list_offsets::*`; `kafka-storage::log::tests::times::*` (against a list of every record;
`a_compressed_batch_is_inflated_to_find_the_record_as_kafka_does`, `under_log_append_time_every_record_is_at_its_batchs_append_time`);
`kafka-wire::batch::records::tests::*`, `kafka-wire::batch::look::tests::under_log_append_time_every_record_has_the_batchs_newest_timestamp`

### L2. Refusals
UNKNOWN_TOPIC_OR_PARTITION; an isolation level that is neither 0 nor 1 is UNKNOWN_SERVER_ERROR; a leader epoch above 0
is UNKNOWN_LEADER_EPOCH (a single node has only epoch 0). A special time asked below the version that brought it (-3 v7,
-4 v8, -5 v9, -6 v11) or one no version has (below -6) is UNSUPPORTED_VERSION, before the topic is looked for, as
`ReplicaManager.fetchOffset` orders it. -4 is the log start; -5 and -6 are offset -1 at leader epoch -1, there being no
tiered storage (wire.md W37).
Test: `list_offsets_refuses_*`, `list_offsets_holds_the_isolation_level_and_the_leader_epoch_*`

### L3. ListOffsets v0 answers from segments (debt K2, 2026-11-19)
v0 (`max_num_offsets`, a list of offsets newest first) is Kafka's `Log.legacyFetchOffsetsBefore`: not records but segments.
Each segment's first offset (no lower than the log start) stands with the time its file was last written, and the end with
now if the last segment holds anything; LATEST counts back from the end, EARLIEST is the first alone, a time counts back from
the last whose time is at or before it (none: an empty list). A partition never written is one empty segment as old as now.
Test: recorded `old-offsets` (tools/kafka-replay, Apache Kafka 3.9.1); `tools/old-profiles-oracle` `list-offsets` (1.1, 2.8, 3.9.1)

### F12. Fetch v0 to v3 read message formats v0 and v1
Down-converted from the v2 batches the log holds, cut and filled as the release the broker stands for does (wire.md W38):
Kafka's slice of the log (a hard limit up to v2, the first batch whole from v3), control batches dropped, overflow filler from
Kafka 2.0, zstd refused. From Kafka 2.0 a zstd topic is UNSUPPORTED_COMPRESSION_TYPE below v10 and one with
`message.downconversion.enable=false` UNSUPPORTED_VERSION below v4, refused with the release's shape (2.8: empty records, no
list of aborted transactions; 3.9 on: null records). Kafka 1.1 gives a read-uncommitted v4 to v7 no last stable offset.
Test: `batch::legacy::tests`, recorded `old-formats-fetch`, `tools/old-profiles-oracle` `fetch-old`, `fetch-old-txn`, `zstd`

### Known cost
A timestamp lookup reads the log from its start (no time index). Fine for a development broker's logs; a time index is
a Phase 7 candidate if the weight baseline shows it matters. ListOffsets does not open a partition whose directory is empty. A response is held to 50 MiB whatever the client asks
(Kafka's `fetch.max.bytes`).

### F9. A response is held to 50 MiB
Whatever `max_bytes` says (zero or negative included), a Fetch response holds at most 50 MiB (Kafka's `fetch.max.bytes`
default), so a request for "everything" does not build a response the size of the log. The first batch of the first
partition with data still comes back if it alone is larger.
Test: `round::tests::a_request_is_held_to_what_it_asks_and_to_the_most_a_response_may_hold`

### F10. A long poll notices a topic deleted, once
Only a deletion changes what a waiting fetch is watching (a topic made is not one it asked for), and the broker counts
them. The wait re-subscribes to that count before it looks again, so a deletion that has been seen is not seen again (a
receiver kept from before it reports it on every look, and the poll spins).
Test: `fetch::tests::*` (the wait does not spin); `tests/races` (a delete and a create racing). Removing the
re-subscribe is *not* caught by a test: `from_changes` also consumes each change, which prevents the spin on its own.
