# Behaviours: Produce and InitProducerId

From the Go source (`internal/broker/{produce,producerid}.go`, `internal/storage/log/produce.go` at `4081fa6`) and
the recordings `produce-validation`, `produce-acks-zero`, `full-disk-produce`, `full-disk-init-producer-id`.
Not in this slice: record-level validation, decompression and the codec rewrite (3f); idempotent sequence checks and
transactions (Phase 6): a batch with a producer id is stored without checking its sequence for now.

### P1. Offsets are the log's
Each partition's batch is appended at the log end and the answer says where (`base_offset`) and the partition's log
start. Partitions of a request are written at once, each by its own writer.
Test: `tests/produce::batches_are_given_the_next_offsets_in_order_and_stored_as_they_came`

### P2. Acks
`acks=-1`: answered when the batch is flushed (and told so if the flush fails). `acks=1`: answered when written; the
flush follows `fsync_interval` (500 ms) after the first unflushed write, and nothing is armed while the log is clean.
`acks=0`: not answered, and a refusal is told by closing the connection. Any other value is INVALID_REQUIRED_ACKS on
every partition.
Test: `an_acknowledged_acks_all_produce_survives_*`, `an_acks_all_produce_is_flushed_when_it_arrives_*`, `acks_zero_is_not_answered_*`, `tests/produce_checks::*` (acks 2)

### P3. Group commit
Producers that arrive together for one partition share one flush.
Test: `producers_that_arrive_together_share_a_flush`

### P4. What a request is refused for before a partition is looked at
Kafka looks at every partition's first batch first: one it cannot take apart (an impossible size, a format byte that is
no format) fails every partition, with Kafka's text; a transactional batch with no transactional id (TRANSACTIONAL_ID_AUTHORIZATION_FAILED) too.
Test: `tests/produce_checks::a_batch_the_first_look_cannot_make_sense_of_fails_the_other_partitions_too`, `a_transactional_batch_needs_a_transactional_id_*`

### P5. What a partition's batch is refused for
In Kafka's order: shape (one whole v2 batch), zstd before Produce v7 (UNSUPPORTED_COMPRESSION_TYPE), a second batch,
a base offset that is not 0, `max.message.bytes` (MESSAGE_TOO_LARGE, the topic's else the broker's), the header and checksum
(CORRUPT_MESSAGE), counts, control batches, a producer id with no sequence (INVALID_RECORD). A refusal made before the log was asked
answers log start -1; one made after it carries the log start.
Test: `tests/produce_checks::*`; `kafka-wire::batch::produce::tests::*`

### P6. Unknown topics and partitions
UNKNOWN_TOPIC_OR_PARTITION with base offset -1. From Produce v13 a topic is named by id.
Test: `an_unknown_topic_or_partition_is_unknown_*`, `tests/produce_checks::from_v13_*`

### P7. Time
A topic with `message.timestamp.type=LogAppendTime` has its batches stamped with the broker's clock and the answer says when.
Test: `tests/produce_checks::a_topic_that_stamps_with_the_brokers_clock_says_when`

### P8. Storage failures
A write or flush that fails is KAFKA_STORAGE_ERROR with no offsets and no message; a failed flush takes the partition
offline until restart (never retried).
Test: `a_write_that_fails_is_a_storage_error_*`

### P9. A topic's partitions follow its life
A partition is opened when first used (its directory made if a cut lost it), and closed, after flushing, before a
deleted topic's files are removed.
Test: `a_topic_deleted_while_open_*`, `a_topic_made_but_never_given_its_directories_*`, `partitions::tests::*`

### P10. InitProducerId
A fresh id (epoch 0) from a durable block; a transactional id is refused until Phase 6.
Test: `producer_ids_are_handed_out_once_each_*`; `kafka-meta` (blocks survive restarts)
