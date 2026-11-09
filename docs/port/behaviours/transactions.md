# Idempotent producers and transactions

Phase 6. What Apache Kafka 4.3.1 does, from the recordings (`conformance/recordings`), and the test in this repository that
holds each. Pure parts are tested where they live (`producers/tests.rs`, `txn/rules/tests.rs`, `txn_index/tests.rs`,
`kafka-wire/src/batch/marker.rs`, `kafka-storage/src/log/tests/history.rs`); wire tests are in
`crates/kafka-broker/tests/{idempotence,transactions,versions_txn}.rs`; the oracle scenario is in brackets; real clients are
in `interop/` (`eos-franz-go`, `txn-franz-go`, `txn-sarama`). The classic protocol only: Produce is offered no newer than
v9, so clients that would use KIP-890 transaction v2 fall back to it.

## TX-1. Idempotent produce: a repeat is not written twice
Per partition and producer id the broker keeps the epoch and the last five batches' sequence ranges with the offset each
was given. A batch whose range is one of the five is a duplicate and is answered with its first offset (and timestamp), not
written. A first batch from a producer nobody knows starts wherever it is. Sequences wrap at 2^31 - 1. Test:
`producers::tests::batches_in_order_are_accepted_and_a_repeat_is_answered_with_where_it_went`, `sequences_wrap_at_thirty_one_bits`,
`idempotence::a_batch_sent_again_is_answered_with_its_first_offset_and_written_once`.

## TX-2. Refusals (R-produce-validation) [produce-validation]
A gap or an overlap is OUT_OF_ORDER_SEQUENCE_NUMBER (45); a repeat older than the five remembered is
DUPLICATE_SEQUENCE_NUMBER (46) with no place to point to; an epoch lower than the producer's is INVALID_PRODUCER_EPOCH
(47); a higher epoch is a new session and must start at sequence 0 (else OUT_OF_ORDER). A producer id the partition has no
memory of (new, or from before an expiry) is accepted wherever it starts: that batch is its baseline.
Producers are judged apart; a batch with no producer is not judged. Test: `a_gap_a_stale_epoch_and_a_repeat_from_long_ago_are_refused`,
`producers_are_judged_apart_and_a_batch_with_no_producer_is_not_judged`.

## TX-3. The check, the append and the record are one step
The partition's writer does all three, so two retries of one batch cannot both append, and a batch that was refused or
written is never written again. Test: `producers::tests::a_batch_that_was_written_is_never_written_again`.

## TX-4. Expiry and restart
A producer silent for 15 minutes is forgotten (Kafka's `producer.id.expiration.ms`) and the tracker holds at most 50,000
per partition. After a restart the state is rebuilt from the last 16 MiB of the log, so a batch whose acknowledgement was
lost in the stop is still recognised. Test: `producers_silent_for_fifteen_minutes_are_forgotten_and_the_ceiling_holds`,
`idempotence::a_retry_after_a_restart_is_still_recognised`, `history::*`.

## TX-5. Transactional ids and fencing [txn-empty-id]
InitProducerId with a transactional id makes the caller the owner, with a new producer id on first use and a raised epoch
after; the previous owner is fenced, and what it left open is aborted (markers written) before the answer. An empty
transactional id is INVALID_REQUEST. A timeout over 15 minutes is INVALID_TRANSACTION_TIMEOUT. Test:
`transactions::a_new_owner_of_the_id_fences_the_old_and_aborts_what_it_left_open`, `txn::rules::the_owner_is_the_producer_id_and_epoch_on_the_record_and_a_lower_epoch_is_fenced`.

## TX-6. A transactional batch needs an open transaction that has the partition
A Produce with a transactional id is accepted only if the producer and epoch own the id, a transaction is open and the
partition was added to it by AddPartitionsToTxn; otherwise it would sit in the log with no marker ever coming and hold the
last stable offset still. The coordinator's lock is held across the append, so an EndTxn cannot slip between check and
write. A fenced epoch is answered INVALID_PRODUCER_EPOCH by a partition. Test:
`transactions::a_batch_needs_an_open_transaction_that_the_partition_was_added_to`, `rules::a_batch_may_be_written_only_by_the_owner_to_a_partition_of_the_open_transaction`.

## TX-7. EndTxn [endtxn-decisions, endtxn-while-preparing]
| state | the same decision | the other |
|---|---|---|
| Ongoing | carried out | carried out |
| PrepareCommit / PrepareAbort | CONCURRENT_TRANSACTIONS until carried out | likewise, then INVALID_TXN_STATE |
| CompleteCommit / CompleteAbort | success | INVALID_TXN_STATE |
| Empty | INVALID_TXN_STATE | INVALID_TXN_STATE |

The decision is stored first. The answer is success only when every marker is on disk and, for a commit, the offsets are
stored. Nothing is added while a transaction is being ended (CONCURRENT_TRANSACTIONS). Test: `rules::each_end_is_answered_as_the_recording_says`,
`transactions::each_end_is_answered_as_kafka_answers_it`.

## TX-8. Markers, the last stable offset and aborted ranges
A marker is a control, transactional batch carrying the producer's id and epoch, one record keyed COMMIT or ABORT. A
partition keeps an index of open transactions (producer to first offset) and aborted ranges, from batch headers: the **last
stable offset** is the first offset of the oldest open transaction, else the log end. An abort is remembered until the log
start passes it; a commit is not. A marker for a producer with no open transaction changes nothing, so a marker written
twice (after a crash) is harmless. The index is rebuilt after a restart from the log, and fed batch by batch gives the same
index. Test: `txn_index::*`, `partitions::tests::txns::open_and_aborted_transactions_are_seen_as_they_are_appended_and_after_a_restart`,
`marker::*`.

## TX-9. Offsets in a transaction
AddOffsetsToTxn names the group; TxnOffsetCommit stores the offsets with the transaction. They are applied to the group when
it commits and forgotten when it aborts. A group that was not added to the open transaction is INVALID_TXN_STATE. Test:
`transactions::the_offsets_a_transaction_carries_are_stored_if_it_commits_and_forgotten_if_it_aborts`.

## TX-10. Reading
`read_committed` Fetch is capped at the last stable offset and lists the aborted transactions in range; a plain fetch sees
everything, markers included. ListOffsets latest under `read_committed` is the last stable offset; below v4 the leader epoch
is -1. Test: `transactions::a_committed_transaction_is_read_after_it_ends_and_not_before`,
`an_aborted_transaction_is_listed_so_that_a_consumer_skips_its_records`.

## TX-11. Timeout
A transaction open longer than its timeout (counted from when its record last changed, checked every 500 ms) is aborted by
the broker and its producer fenced (epoch raised). Test: `transactions::a_transaction_that_outlives_its_timeout_is_aborted_and_its_producer_fenced`,
`rules::a_transaction_outlives_its_timeout_from_when_the_record_last_changed`.

## TX-12. Restart
The coordinator's record is written before the broker acts. At start, before the listener binds, a transaction whose decision
was stored is finished (markers, offsets, Complete); one still open is left open, its producer may be alive. Test:
`transactions::an_open_transaction_is_left_open_by_a_restart_and_a_committed_one_stays_committed`, the metadata crash sweep
(`kafka-meta/tests/crash`, which now writes transaction records).

## TX-13. Describing
DescribeProducers (per partition: the producers and their open transaction), DescribeTransactions (by id, or
TRANSACTIONAL_ID_NOT_FOUND) and ListTransactions (filters by state and producer id) answer at every served version. Test:
`versions_txn::a_transaction_is_run_through_every_version_of_each_api`.

## Not done (recorded)
- KIP-890 transaction v2 (AddPartitionsToTxn v4, verification in Produce): not offered.
- `idempotent-write-from-topic-write` needs ACLs and SASL (Phase 8); the `full-disk-*` scenarios inject faults the replay
  does not drive (the same is covered with `SimFs`).
- kafka-go: its ReadCommitted ignores the aborted list and returns markers as messages, so it is not in the transaction
  readers (franz-go, sarama and kcat are).
