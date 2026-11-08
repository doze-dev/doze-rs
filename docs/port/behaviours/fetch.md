# Behaviours: Fetch and ListOffsets

From the Go source (`internal/broker/{fetch,listoffsets}.go`, `internal/storage/log/timestamps.go` at `4081fa6`) and
the recordings `fetch-above-the-end`, `listoffsets`. Not here: read-committed isolation beyond "the last stable
offset is the end" (Phase 6), fetch sessions (never), compressed batches' records for timestamp lookups (3f).

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

### F8. Read-committed
Carries an empty list of aborted transactions (null for read-uncommitted); the last stable offset is the end until
transactions exist.
Test: `a_read_committed_fetch_carries_an_empty_list_of_aborted_transactions_*`

### L1. ListOffsets
Latest (-1) is the log end, earliest (-2) the log start; max timestamp (-3, v7+) is the first record with the largest
timestamp; a real timestamp is the first record from the log start whose timestamp is at least it, to the record (the
batch's own records are read when it is uncompressed; a compressed batch answers at the grain of its header). Nothing
qualifies: offset -1, timestamp -1. A partition named twice is answered twice (the console relies on it).
Test: `tests/list_offsets::*`; `kafka-storage::log::tests::times::*` (against a list of every record)

### L2. Refusals
UNKNOWN_TOPIC_OR_PARTITION; an isolation level that is neither 0 nor 1 is UNKNOWN_SERVER_ERROR; a leader epoch above 0
is UNKNOWN_LEADER_EPOCH (a single node has only epoch 0); -3 before v7 and anything below -3 is UNSUPPORTED_VERSION.
Test: `list_offsets_refuses_*`, `list_offsets_holds_the_isolation_level_and_the_leader_epoch_*`

### Known cost
A timestamp lookup reads the log from its start (no time index). Fine for a development broker's logs; a time index is
a Phase 7 candidate if the weight baseline shows it matters. ListOffsets does not open a partition whose directory is empty. A response is held to 50 MiB whatever the client asks
(Kafka's `fetch.max.bytes`).

### F9. A response is held to 50 MiB
Whatever `max_bytes` says (zero or negative included), a Fetch response holds at most 50 MiB (Kafka's `fetch.max.bytes`
default), so a request for "everything" does not build a response the size of the log. The first batch of the first
partition with data still comes back if it alone is larger.
Test: `round::tests::a_request_is_held_to_what_it_asks_and_to_the_most_a_response_may_hold`

### F10. A long poll wakes on a topic made or deleted, once
The wait for records re-subscribes to the topic-change signal before it looks again, so a change that has been seen is
not seen again (a receiver kept from before the change reports it on every look, and the poll spins).
Test: `fetch::tests::*`, `tests/races`
