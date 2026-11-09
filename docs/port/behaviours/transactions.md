# Idempotent producers and transactions

Phase 6. What Apache Kafka 4.3.1 does, and the test in this repository that holds each. The source is the recordings
(`conformance/recordings`, the oracle scenario in brackets) where there is one, and otherwise Kafka 4.3.1's own source,
read by item: **(source)** marks a behaviour taken from the source and not from a recording. Pure parts are tested where
they live (`producers/tests.rs`, `txn/rules/tests.rs`, `txn_index/tests.rs`, `kafka-wire/src/batch/marker.rs`,
`kafka-storage/src/log/tests/{history,transactional}.rs`); wire tests are in `crates/kafka-broker/tests/` (`idempotence`,
`transactions`, `transactions_{codes,crash,describe,expiry,race}`, `offset_fetch_checks`, `versions_txn`); real clients are
in `interop/` (`eos-franz-go`, `txn-franz-go`, `txn-sarama`, `txn-librdkafka`, `fence-franz-go`). The classic protocol
only: Produce is offered no newer than v9, so clients that would use KIP-890 transaction v2 fall back to it.

## TX-1. Idempotent produce: a repeat is not written twice
Per partition and producer id the broker keeps the epoch and the last five batches' sequence ranges with the offset each
was given. A batch whose range is one of the five is a duplicate and is answered with its first offset (and timestamp), not
written. A first batch from a producer nobody knows starts wherever it is. Sequences wrap at 2^31 - 1. Test:
`producers::tests::batches_in_order_are_accepted_and_a_repeat_is_answered_with_where_it_went`, `sequences_wrap_at_thirty_one_bits`,
`idempotence::a_batch_sent_again_is_answered_with_its_first_offset_and_written_once`.

## TX-2. Refusals (R-produce-validation) [produce-validation]
A gap, an overlap, and a repeat of a batch older than the five remembered are OUT_OF_ORDER_SEQUENCE_NUMBER (45) (source:
Kafka has DUPLICATE_SEQUENCE_NUMBER nowhere on this path; it only finds a duplicate among the five). An epoch lower than the
producer's is INVALID_PRODUCER_EPOCH (47); a higher epoch is a new session and must start at sequence 0 (else OUT_OF_ORDER).
A producer id the partition has no memory of is accepted wherever it starts: that batch is its baseline. Producers are
judged apart; a batch with no producer is not judged. Test: `idempotence::a_gap_a_stale_epoch_and_a_repeat_from_long_ago_are_refused`,
`producers_are_judged_apart_and_a_batch_with_no_producer_is_not_judged`.

## TX-3. The check, the append and the record are one step
The partition's writer does all three, so two retries of one batch cannot both append, and a batch that was refused or
written is never written again. Test: `producers::tests::a_batch_that_was_written_is_never_written_again`.

## TX-4. Expiry, restart, and what retention leaves
A producer silent for a day is forgotten (`producer.id.expiration.ms`), and the tracker holds at most 50,000 per partition.
A producer whose newest remembered batch is below the log start is forgotten too (Kafka drops a producer's state with its
batches) and starts afresh. After a restart the state is rebuilt from the last 16 MiB of the log, so a batch whose
acknowledgement was lost in the stop is still recognised. A transaction marker raises the producer's epoch on the partition
and its sequences start at 0 again, so a fenced producer is refused by the partition as well as by the coordinator. Test:
`producers::tests::producers_silent_for_a_day_are_forgotten_and_the_ceiling_holds`,
`a_producer_whose_batches_are_all_below_the_log_start_is_forgotten`, `a_marker_raises_the_producers_epoch_and_its_sequences_start_again`,
`idempotence::a_retry_after_a_restart_is_still_recognised`, `history::*`,
`transactions_describe::a_fenced_producers_epoch_is_raised_on_the_partition_by_the_marker_that_aborted_it`.

## TX-5. Transactional ids and fencing [txn-empty-id]
InitProducerId with a transactional id makes the caller the owner: a new producer id on first use, and after that the epoch
goes up and the previous owner is fenced; what it left open is aborted (markers at the raised epoch) before the answer. An
empty transactional id is INVALID_REQUEST; a timeout over 15 minutes (or not positive) is INVALID_TRANSACTION_TIMEOUT.
From version 3 a producer may say the producer id and epoch it holds (KIP-360) (source): they must be the id's, or the
epoch before the last bump (then the answer is the one it already got, not another bump), else PRODUCER_FENCED
(INVALID_PRODUCER_EPOCH below version 4); half a pair is INVALID_REQUEST; when the epochs run out (32,766) the producer id is
replaced, and the old one with its last epoch is still recognised as a retry. Test:
`transactions::a_new_owner_of_the_id_fences_the_old_and_aborts_what_it_left_open`,
`transactions_codes::a_producer_that_says_what_it_holds_is_bumped_asked_again_it_is_answered_alike_and_a_stranger_is_fenced`,
`rules::a_producer_says_the_producer_id_and_epoch_it_holds_and_is_fenced_if_they_are_not_the_ids`,
`rules::when_the_epochs_are_used_up_the_producer_id_is_replaced_and_the_old_one_is_still_known_for_a_retry`.

## TX-6. A transactional batch needs an open transaction that has the partition
A Produce with a transactional id is accepted only if the producer and epoch own the id, a transaction is open and the
partition was added to it by AddPartitionsToTxn; otherwise it would sit in the log with no marker ever coming and hold the
last stable offset still. The transaction is held for reading once for the whole request, from its first batch's check until
the request is answered, so an EndTxn, a new owner or the timeout cannot slip in between a check and its append. (Held once,
and not for each partition: a lock waited for again while an earlier hold is kept queues behind a writer that is itself
waiting for the first, and neither goes on. That was a deadlock.) Test:
`transactions::a_batch_needs_an_open_transaction_that_the_partition_was_added_to`,
`rules::a_batch_may_be_written_only_by_the_owner_to_a_partition_of_the_open_transaction`,
`transactions_race::a_new_owner_arriving_while_a_produce_spans_two_partitions_neither_waits_for_ever_nor_is_waited_for`,
`a_transaction_is_not_ended_in_the_middle_of_a_produce_request_that_writes_to_it`. (A mutant that holds a stale copy of the
record and not the lock survives on one thread: a log's queue is first in, first out, which already keeps a marker behind the
batch it follows. The hold is what rules out the gap between check and queue on several.)

## TX-7. EndTxn [endtxn-decisions, endtxn-while-preparing]
| state | the same decision | the other |
|---|---|---|
| Ongoing | carried out | carried out |
| PrepareCommit / PrepareAbort | carried out, then success | carried out, then INVALID_TXN_STATE |
| CompleteCommit / CompleteAbort | success | INVALID_TXN_STATE |
| Empty | INVALID_TXN_STATE | INVALID_TXN_STATE |

The decision is stored first. The answer is success only when every marker is on disk and, for a commit, the offsets are
stored. Nothing is added while a transaction is being ended (CONCURRENT_TRANSACTIONS). Kafka answers CONCURRENT_TRANSACTIONS
to a second request for the milliseconds the markers take, and the client asks again; here the second request waits for
them (the coordinator's lock), so the answer it gets is the later one. The recording `endtxn-while-preparing` is evidence
only (the harness does not replay it: it relied on a setting that does not hold the markers back). The owner checks are
Kafka's (source): a producer id that is not the id's is INVALID_PRODUCER_ID_MAPPING; any other epoch, lower or higher, is
PRODUCER_FENCED (INVALID_PRODUCER_EPOCH below version 2). Test: `rules::each_end_is_answered_as_the_recording_says`,
`transactions::each_end_is_answered_as_kafka_answers_it`,
`transactions_crash::a_decision_stored_and_not_carried_out_is_carried_out_by_whoever_asks_next`,
`transactions_codes::an_epoch_that_is_not_the_owners_is_fenced_and_a_version_without_that_code_is_told_the_epoch_is_invalid`.

## TX-8. Markers, the last stable offset and aborted ranges
A marker is a control, transactional batch carrying the producer's id and epoch, one record keyed COMMIT or ABORT. A
partition keeps an index of open transactions (producer to first offset) and aborted ranges, from batch headers: the **last
stable offset** is the first offset of the oldest open transaction, else the log end. An abort is remembered until the log
start passes it; a commit is not. A marker for a producer with no open transaction changes nothing, so a marker written
twice (Kafka may write one twice too, after a retry) is harmless. The index is shared between the partition's writer and its
readers, so a read-committed Fetch or ListOffsets does not queue behind appends and flushes; the writer holds its lock from
before a transactional batch is appended until the index knows of it, so a reader that sees the batch in the log finds it in
the index. A log is read for its transactions once, when it is opened, and only if a note on disk (`transactional`, made
durably before the first transactional batch or marker) says it may hold any: a log that never held one is not read to find
that out. Test: `txn_index::*`, `partitions::tests::txns::{open_and_aborted_transactions_are_seen_as_they_are_appended_and_after_a_restart,
a_partition_that_never_held_a_transaction_is_not_read_to_find_that_out,
a_transactional_batch_is_not_visible_to_a_reader_before_the_index_knows_it_is_open}`, `log::tests::transactional::*`, `marker::*`.

## TX-9. Offsets in a transaction
AddOffsetsToTxn names the group; TxnOffsetCommit stores the offsets with the transaction. They are applied to the group when
it commits and forgotten when it aborts. A group that was not added to the open transaction is INVALID_TXN_STATE. The
committer's place in the group is checked as KIP-447 says (source): no generation, no member and no instance is accepted
whatever the group; a member must be in the group (UNKNOWN_MEMBER_ID) at its generation or epoch (ILLEGAL_GENERATION, for a
consumer group's member as well: STALE_MEMBER_EPOCH is never answered here); a group that does not exist is found for no
generation and not for one; the group waiting for its leader does not refuse them. The order is Kafka's: a partition that is
not there is UNKNOWN_TOPIC_OR_PARTITION whatever else is wrong; then the group id, the producer's right to the transaction (an
id nobody owns, including the empty one, is INVALID_PRODUCER_ID_MAPPING; a replaced producer INVALID_PRODUCER_EPOCH), then the
place in the group. Test: `transactions::the_offsets_a_transaction_carries_are_stored_if_it_commits_and_forgotten_if_it_aborts`,
`transactions_codes::a_transactions_offsets_are_fenced_by_the_committers_place_in_the_group_and_by_nothing_if_it_names_none`.

## TX-10. Reading
`read_committed` Fetch is capped at the last stable offset and lists the aborted transactions in range; a plain fetch sees
everything, markers included. The last stable offset in a Fetch response is the real one at every isolation level (source).
OffsetFetch with `require_stable` answers UNSTABLE_OFFSET_COMMIT, offset -1, for a partition that a transaction has offsets
for and has not ended (source). ListOffsets latest under `read_committed` is the last stable offset; a lookup by time, or for
the newest record, that lands at or past it finds nothing (offset and time -1) (source); below v4 the leader epoch is -1.
A read bounded by a limit short of the log end asks the file only for what is near the limit, and a small answer is copied
out of a large buffer. Test: `transactions::a_committed_transaction_is_read_after_it_ends_and_not_before`,
`an_aborted_transaction_is_listed_so_that_a_consumer_skips_its_records`,
`offset_fetch_checks::offsets_a_transaction_is_still_deciding_are_unstable_for_a_fetch_that_asks_for_stable_ones`,
`transactions_codes::a_lookup_by_time_that_lands_in_what_a_committed_reader_may_not_yet_read_finds_nothing_to_it`,
`log::tests::read::a_read_with_a_limit_just_ahead_does_not_ask_the_file_for_what_max_bytes_would_allow`.

## TX-11. Timeout
A transaction open longer than its timeout, counted from when it began (adding partitions or offsets does not start it again)
and not from its last change, is aborted by the broker (checked every 500 ms) and its producer fenced (epoch raised) (source).
A restart does not start the clock again. Test: `transactions::a_transaction_that_outlives_its_timeout_is_aborted_and_its_producer_fenced`,
`transactions_codes::a_transaction_times_out_from_when_it_began_whatever_is_added_to_it_since`,
`rules::a_transaction_outlives_its_timeout_from_when_it_began_not_from_its_last_change`.

## TX-12. Restart and faults
The coordinator's record is written before the broker acts. At start, before the listener binds, a transaction whose decision
was stored is finished (markers, offsets, Complete); one still open is left open, its producer may be alive. A stop at every
operation of an EndTxn, by a kill or a power cut, for a commit and for an abort, leaves the transaction open or decided
everywhere, never half, never the other decision, and the producer asking again finishes it once; one failed operation
(disk full, device error) anywhere in it is survived the same way. Test: `transactions_crash::{a_stop_at_any_point_of_a_commit_leaves_the_transaction_open_or_committed_everywhere,
a_stop_at_any_point_of_an_abort_leaves_the_transaction_open_or_aborted_everywhere, one_operation_failing_anywhere_in_a_commit_is_survived}`,
`transactions::an_open_transaction_is_left_open_by_a_restart_and_a_committed_one_stays_committed`, the metadata crash sweep
(`kafka-meta/tests/crash`, which writes and forgets transaction records).

## TX-13. Describing
DescribeProducers (per partition: the producers, their epoch and where their open transaction began), DescribeTransactions
(by id, or TRANSACTIONAL_ID_NOT_FOUND; the topics include the offsets partition of each group the transaction commits for)
and ListTransactions (filters by state, producer id and, from version 1, how long it has run) answer at every served version.
Test: `transactions_describe::*`, `versions_txn::a_transaction_is_run_through_every_version_of_each_api`.

## TX-14. Transactional ids that are not used again
An id whose transaction is over and that has not changed for `transactional.id.expiration.ms` (seven days) is forgotten,
in memory and in the store; one with a transaction open never is (the timeout deals with it first). A request that still
holds the entry when it is forgotten looks the id up again. Test: `transactions_expiry::*`, the metadata crash sweep.

## Not done, and why that is not a debt
- KIP-890 transaction v2 (AddPartitionsToTxn v4, verification in Produce, EndTxn v5): not offered, so clients use the classic
  protocol, which is what every recorded scenario and every real client run here exercises. A scope decision, revisited if
  the Java client's default moves.
- `idempotent-write-from-topic-write` needs ACLs and SASL: it moves to Phase 8 (`debt.md`).
- kafka-go: its ReadCommitted ignores the aborted list and returns markers as messages, so it is not among the transaction
  readers (franz-go, sarama and kcat are).
- A batch that compaction has thinned (fewer records than its offset span) is not representable by the batch header reader,
  which insists that the record count agrees with the last offset delta: Phase 7 (compaction) relaxes that and walks by the
  last offset delta (`debt.md`).
