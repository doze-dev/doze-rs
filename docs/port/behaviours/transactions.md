# Idempotent producers and transactions

Phase 6. What Apache Kafka 4.3.1 does, and the test in this repository that holds each. The source is the recordings
(`conformance/recordings`, the oracle scenario in brackets) where there is one, and otherwise Kafka 4.3.1's own source,
read by item: **(source)** marks a behaviour taken from the source and not from a recording. Pure parts are tested where
they live (`producers/tests.rs`, `txn/rules/tests.rs`, `txn_index/tests.rs`, `kafka-wire/src/batch/marker.rs`,
`kafka-storage/src/log/tests/{history,transactional}.rs`); wire tests are in `crates/kafka-broker/tests/` (`idempotence`,
`transactions`, `transactions_{codes,crash,describe,expiry,race}`, `offset_fetch_checks`, `versions_txn`); real clients are
in `interop/` (`eos-franz-go`, `txn-franz-go`, `txn-sarama`, `txn-librdkafka`, `fence-franz-go`; the Java client's `txn-*`).

KIP-890 (TX-16 to TX-25) has no recording: it is held to **Apache Kafka 4.4.0 itself**. `tools/kip890-oracle/Probe.java`
puts raw requests at chosen versions to a broker and prints the answers with producer ids normalised; run against a real
4.4.0 (KRaft, one node, `transaction.state.log.replication.factor=1`, `transaction.abort.timed.out.transaction.cleanup.interval.ms=1000`)
its answers are `tools/kip890-oracle/kafka-4.4.0.txt`, and this broker's are the same lines (the probe's scenario is named
in brackets, e.g. [oracle `table`]). The wire tests are `transactions_v2{,_adds,_epochs}`; the rules are pure in
`txn/rules/{ending,adding}.rs`. Kafka 4 (`--kafka-version 4`, the default) speaks both protocols; Kafka 3 has the first part
only; Kafka 1 and 2 neither (TX-16).

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
and its sequences start at 0 again, so a fenced producer is refused by the partition as well as by the coordinator; the epoch is read
back from the markers in the log when the partition is opened again. When a marker found at open is all that is left of a
producer (DeleteRecords or retention took the batches it ended), its epoch is kept (a lower epoch is still fenced, a higher one
still starts at 0) but where its sequences stand is not known, so its next batch at that epoch is its baseline, as before the
reopen (Kafka's `ProducerAppendInfo.checkSequence`: a producer whose records retention or DeleteRecords deleted is accepted
with any sequence; Doze has no snapshots, so it matches its own answer from before the reopen). Test:
`producers_after_trim::*`, `producers::tests::*` (marker found in the history), `compaction_model` (random workloads),
`producers::tests::producers_silent_for_a_day_are_forgotten_and_the_ceiling_holds`,
`a_producer_whose_batches_are_all_below_the_log_start_is_forgotten`, `a_marker_raises_the_producers_epoch_and_its_sequences_start_again`,
`idempotence::a_retry_after_a_restart_is_still_recognised`, `history::*`,
`transactions_describe::a_fenced_producers_epoch_is_raised_on_the_partition_by_the_marker_that_aborted_it`.

## TX-5. Transactional ids and fencing [txn-empty-id]
InitProducerId with a transactional id makes the caller the owner: a new producer id on first use, and after that the epoch
goes up and the previous owner is fenced; what it left open is aborted (markers at the raised epoch) before the answer. An
empty transactional id is INVALID_REQUEST; a timeout over 15 minutes (or not positive) is INVALID_TRANSACTION_TIMEOUT.
From version 3 a producer may say the producer id and epoch it holds (KIP-360) (source): they must be the id's, or the
epoch before the last bump (then the answer is the one it already got, not another bump, and the id is made ready as for any
InitProducerId), else PRODUCER_FENCED (INVALID_PRODUCER_EPOCH below version 4); a fence by the broker (a timeout, or a new owner taking an open
transaction) is TX-21: at Kafka 1 to 3 it forgets the epoch before, so the producer that was fenced cannot take the id back
by saying the epoch it had, and at Kafka 4 it keeps it, as Kafka 4.4.0 does; half a pair is INVALID_REQUEST; when the epochs run out (32,766) the producer id is
replaced, and the old one with its last epoch is still recognised as a retry. Test:
`transactions::a_new_owner_of_the_id_fences_the_old_and_aborts_what_it_left_open`,
`transactions_codes::a_producer_that_says_what_it_holds_is_bumped_asked_again_it_is_answered_alike_and_a_stranger_is_fenced`,
`rules::a_producer_says_the_producer_id_and_epoch_it_holds_and_is_fenced_if_they_are_not_the_ids`,
`rules::when_the_epochs_are_used_up_the_producer_id_is_replaced_and_the_old_one_is_still_known_for_a_retry`,
`transactions_fence::*`.

## TX-6. A transactional batch needs an open transaction that has the partition
A Produce with a transactional id is accepted only if the producer and epoch own the id, a transaction is open and the
partition was added to it (by AddPartitionsToTxn, or from Produce v12 by the request itself, TX-18); otherwise it would sit
in the log with no marker ever coming and hold the last stable offset still. The code a refusal carries depends on the
Produce version (TX-17). The transaction is held for reading once for the whole request, from its first batch's check until
the request is answered, so an EndTxn, a new owner or the timeout cannot slip in between a check and its append. (Held once,
and not for each partition: a lock waited for again while an earlier hold is kept queues behind a writer that is itself
waiting for the first, and neither goes on. That was a deadlock.) Test:
`transactions::a_batch_needs_an_open_transaction_that_the_partition_was_added_to`,
`rules::a_batch_may_be_written_only_by_the_owner_to_a_partition_of_the_open_transaction`,
`transactions_race::a_new_owner_arriving_while_a_produce_spans_two_partitions_neither_waits_for_ever_nor_is_waited_for`,
`a_transaction_is_not_ended_in_the_middle_of_a_produce_request_that_writes_to_it`. A mutant that holds a stale copy of the
record and not the lock is killed by the schedule on which it matters: the request lists its partitions in the opposite order
to the one they were added in, with the first listed one's queue full, so the end of the transaction could otherwise mark the
second before the request has reached it (`a_transaction_is_not_ended_in_the_middle_of_a_produce_request_that_lists_its_partitions_the_other_way`).

## TX-7. EndTxn [endtxn-decisions, endtxn-while-preparing]
EndTxn v0 to v4 (the classic protocol; v5 is TX-19):

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
it commits and forgotten when it aborts. A group that was not added to the open transaction is INVALID_TXN_STATE
(TRANSACTION_ABORTABLE at v4; from v5 TxnOffsetCommit adds it, TX-20). The
committer's place in the group is checked as KIP-447 says (source): no generation, no member and no instance is accepted
whatever the group (`transactions_offsets`: and a classic group waiting for its leader does not refuse them either); a member must be in the group (UNKNOWN_MEMBER_ID) at its generation or epoch (ILLEGAL_GENERATION, for a
consumer group's member as well: STALE_MEMBER_EPOCH is never answered here); a group that does not exist is found for no
generation and not for one; the group waiting for its leader does not refuse them. The order is Kafka's: a partition that is
not there is UNKNOWN_TOPIC_OR_PARTITION whatever else is wrong; then the group id, the producer's right to the transaction (an
id nobody owns, including the empty one, is INVALID_PRODUCER_ID_MAPPING; a replaced producer INVALID_PRODUCER_EPOCH), then the
place in the group. Test: `transactions::the_offsets_a_transaction_carries_are_stored_if_it_commits_and_forgotten_if_it_aborts`,
`transactions_codes::a_transactions_offsets_are_fenced_by_the_committers_place_in_the_group_and_by_nothing_if_it_names_none`,
`transactions_offsets::*` (the producer's right to the transaction, and a classic group).

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
and not from its last change, is aborted by the broker and its producer fenced (epoch raised) (source).
A restart does not start the clock again. The coordinator sleeps until the first moment it has something to do on its own (a
timeout, an idle id's expiry, a decision whose markers could not be written tried again after 500 ms) and is woken when a
change makes that sooner; with no transactional id it does not wake (it used to look every 500 ms, an idle broker's only
wake-ups). Test: `transactions::a_transaction_that_outlives_its_timeout_is_aborted_and_its_producer_fenced`,
`idle_upkeep::the_transaction_coordinator_does_not_wake_with_nothing_to_do_and_aborts_a_timed_out_transaction_on_time`,
`rules::the_next_step_is_the_first_moment_the_timeout_or_the_expiry_holds`,
`alarm::tests::the_alarm_only_moves_earlier_and_rings_when_it_does`,
`transactions_codes::a_transaction_times_out_from_when_it_began_whatever_is_added_to_it_since`,
`rules::a_transaction_outlives_its_timeout_from_when_it_began_not_from_its_last_change`.

## TX-12. Restart and faults
The coordinator's record is written before the broker acts. At start, before the listener binds, a transaction whose decision
was stored is finished (markers, offsets, Complete); one still open is left open, its producer may be alive. A stop at every
operation of an EndTxn, by a kill or a power cut, for a commit and for an abort, leaves the transaction open or decided
everywhere, never half, never the other decision, and the producer asking again finishes it once; one failed operation
(disk full, device error) anywhere in it is survived the same way, and with a power cut after a failed flush that loses its
pages an answer of success holds only because a marker is waited for until it is on disk. The same holds for EndTxn v5 and
for an end that hands over to a new producer id (TX-23). Test: `transactions_crash::{a_stop_at_any_point_of_a_commit_leaves_the_transaction_open_or_committed_everywhere,
a_stop_at_any_point_of_an_abort_leaves_the_transaction_open_or_aborted_everywhere, one_operation_failing_anywhere_in_a_commit_is_survived}`,
`transactions::an_open_transaction_is_left_open_by_a_restart_and_a_committed_one_stays_committed`, the metadata crash sweep
(`kafka-meta/tests/crash`, which writes and forgets transaction records).

## TX-13. Describing
DescribeProducers (per partition: the producers, their epoch and where their open transaction began), DescribeTransactions
(by id, or TRANSACTIONAL_ID_NOT_FOUND; the topics include the offsets partition of each group the transaction commits for)
and ListTransactions (v0 to v1: filters by state, producer id and, from version 1, how long it has run; a transaction that is not
open counts as begun at -1, Kafka's way, so it is listed whatever the duration) answer at every served version.
Test: `transactions_describe::*`, `versions_txn::a_transaction_is_run_through_every_version_of_each_api`.

## TX-14. Transactional ids that are not used again
An id whose transaction is over and that has not changed for `transactional.id.expiration.ms` (seven days) is forgotten,
in memory and in the store; one with a transaction open never is (the timeout deals with it first). A request that still
holds the entry when it is forgotten looks the id up again. Test: `transactions_expiry::*`, the metadata crash sweep.

## Not done, and why that is not a debt
- `idempotent-write-from-topic-write` needs ACLs and SASL: it moves to Phase 8 (`debt.md`).
- kafka-go: its ReadCommitted ignores the aborted list and returns markers as messages, so it is not among the transaction
  readers (franz-go, sarama and kcat are).
- A batch that compaction has thinned (fewer records than its offset span) is not representable by the batch header reader,
  which insists that the record count agrees with the last offset delta: Phase 7 (compaction) relaxes that and walks by the
  last offset delta (`debt.md`).

## TX-15. Authorization is asked before any of the rules above (Phase 8, slice 8d-2; rules SEC-114 to SEC-120 in `security.md`) (source)
In InitProducerId, AddPartitionsToTxn, AddOffsetsToTxn, TxnOffsetCommit and EndTxn the caller's rights are judged first, in Kafka's order: WRITE on the transactional id
(53), then for AddOffsetsToTxn and TxnOffsetCommit READ on the group (30), then for AddPartitionsToTxn WRITE and for TxnOffsetCommit READ on each topic (29, before the topic
is looked for). So the empty-id rules of TX-5 (INVALID_REQUEST, INVALID_PRODUCER_ID_MAPPING) and the unknown-topic and state rules of TX-6, TX-7 and TX-9 are the answer only for a
caller that is allowed. With the default allowing and no ACL stored nothing is refused, so every rule above holds unchanged. DescribeTransactions and ListTransactions
show only what the caller may DESCRIBE (TX-13). AddPartitionsToTxn v4 and later is a broker's request and asks CLUSTER_ACTION instead (TX-24, SEC-124).

## TX-16. Which protocol a request speaks, and what each Kafka major offers [oracle `features`]
The version of the request decides, as in Kafka (`TransactionVersion.transactionVersionFor*`, `AddPartitionsToTxnManager`):
Produce v12 and later, TxnOffsetCommit v5 and later and AddPartitionsToTxn v4 and later add what they name at the second
transaction version; EndTxn v5 ends at it; AddOffsetsToTxn is always classic. A client may change from one to the other
between two transactions, and two clients of different versions share partitions (`a_classic_producer_and_one_of_the_second_version_share_a_partition`).
A client learns it may speak the second from ApiVersions v3 and later: Kafka 4 finalizes `transaction.version` 2 (supported
0 to 2; at v3 the supported row is left out, its least level being 0, KAFKA-17492; the finalized epoch is not negative). The
Java client (4.x) and franz-go (1.22) turn the second version on from that finalized level. Only that feature is listed:
Kafka lists `metadata.version`, `group.version` and four more, which no client decides anything by here. The profiles
(`profile.rs`, each column no wider than the recorded broker of the major): Kafka 4 Produce 3 to 13, InitProducerId 0 to 5
(0 to 6 with the unstable versions on, TX-27), AddPartitionsToTxn 0 to 5, AddOffsetsToTxn 0 to 4, EndTxn 0 to 5, TxnOffsetCommit 0 to 6
(Produce 13 and TxnOffsetCommit 6 name topics by id, TX-26), as 4.4.0 offers them; Kafka 3 the first part only (Produce 11, EndTxn 4, TxnOffsetCommit 4,
AddPartitionsToTxn 5, as 3.9.1 offered) and no feature; Kafka 1 and 2 as before. Test:
`api_versions::tests::kafka_4_finalizes_transaction_version_2_and_the_older_majors_list_no_feature`,
`profile::tests::only_kafka_4_offers_the_second_part_of_kip_890`, `cli::tests::the_transaction_version_follows_the_kafka_major`.

## TX-17. A write to a partition the transaction does not have (KIP-890's first part) [oracle `unadded`, `refusals`]
Refused, with the code the Produce version knows: INVALID_TXN_STATE up to v10, TRANSACTION_ABORTABLE (120, the client may
abort and go on) at v11, whether or not a transaction is open; a transaction being ended (a decision stored and not yet
carried out) NOT_ENOUGH_REPLICAS before v12 (retriable for every client, Kafka's translation of CONCURRENT_TRANSACTIONS)
and CONCURRENT_TRANSACTIONS from v12. The owner checks are as before: another producer id, or an id nobody owns,
INVALID_PRODUCER_ID_MAPPING; another epoch, lower or higher, INVALID_PRODUCER_EPOCH. Test:
`rules::tests::an_unverified_write_is_refused_with_the_code_its_version_knows`,
`transactions_v2::{a_write_to_a_partition_not_added_is_refused_by_version_and_added_from_v12, the_refusals_of_a_produce_of_v12}`.

## TX-18. Produce v12 adds its partitions [oracle `basic`, `refusals`, `unadded`]
Every partition of the request with a transactional batch (of a topic that exists and the caller may write, read from the
batch's header alone) that the open transaction does not have is added, and the record stored, before any batch is
appended; a transaction that is not open is begun. It is done in the request's one hold of the transaction (TX-6): taken
for writing, changed and stored, then turned into the hold for reading without letting go, so nothing can end the
transaction between the add and the appends. Kafka adds before it holds the records to the rules, so a batch refused then
(a first batch not at sequence 0, a corrupt one) leaves its partition added. The coordinator's refusal is every
transactional batch's answer: PRODUCER_FENCED as INVALID_PRODUCER_EPOCH, a record that could not be stored as
NOT_ENOUGH_REPLICAS (`ReplicaManager.handleProduceAppend`; `an_add_of_a_produce_of_v12_that_cannot_be_stored_is_retriable`). A transactional batch of v12 from a producer the partition knows
no batch of (none at all, or only a marker) must start at sequence 0, else OUT_OF_ORDER_SEQUENCE_NUMBER
(`ProducerAppendInfo.checkSequence` for a verification at `TV_2`). Test: `transactions_v2::{a_produce_of_v12_adds_its_partition_and_an_end_of_v5_raises_the_epoch,
the_refusals_of_a_produce_of_v12}`, `producers::tests::a_transaction_of_the_second_version_starts_its_sequences_at_zero_where_its_producer_wrote_nothing`,
`transactions_race::*` (the hold, unchanged).

## TX-19. EndTxn v5 raises the epoch on every end [oracle `table`, `basic`]
Every end raises the epoch, so the epoch says which end a request is: the current one asks for the next end, the one
before is a retry of the end that raised it (`TransactionCoordinator.endTransaction` for `TV_2`):

| state | current epoch | the epoch before (a retry) |
|---|---|---|
| Ongoing | commit or abort decided | PRODUCER_FENCED |
| Empty | abort decided; commit INVALID_TXN_STATE | PRODUCER_FENCED |
| CompleteCommit | abort decided; commit INVALID_TXN_STATE | commit success; abort INVALID_TXN_STATE |
| CompleteAbort | abort decided; commit INVALID_TXN_STATE | abort success; commit INVALID_TXN_STATE |
| PrepareCommit / PrepareAbort | PRODUCER_FENCED | carried out, then as Complete |

Another producer id is INVALID_PRODUCER_ID_MAPPING, any other epoch PRODUCER_FENCED. The decision is stored with the epoch
raised and the one it had kept as the last; the markers carry the raised epoch (the partition's producer is at it after, so
the old epoch is refused there too); the answer carries the producer id and epoch to go on with (-1 and -1 for a refusal).
An abort where nothing was begun (Empty, a Complete state) raises the epoch and writes no marker. A classic EndTxn (v4)
after one of v5 keeps the epoch, and an EndTxn v5 at that same epoch is no retry. Test:
`ending::tests::each_end_of_the_second_transaction_version_is_answered_as_kafka_4_answers_it`,
`transactions_v2::{each_end_of_v5_is_answered_as_kafka_4_answers_it, a_classic_end_after_one_of_v5_keeps_the_epoch}`.

## TX-20. TxnOffsetCommit v5 adds its group [oracle `offsets`]
A group not added to the open transaction is INVALID_TXN_STATE at v0 to v3 and TRANSACTION_ABORTABLE at v4; from v5 the group
is added (its offsets partition is then among the transaction's topics) and stored before the committer's place in the group
is judged, as Kafka's group coordinator asks the transaction coordinator first; a transaction that is not open is begun. A
stale epoch is INVALID_PRODUCER_EPOCH. AddOffsetsToTxn (v4 included) still adds at the classic version. Test:
`transactions_v2_adds::a_txn_offset_commit_of_v5_adds_its_group_and_older_versions_need_it_added`.

## TX-21. The broker's own abort follows the transaction version it finalizes [oracle `timeout`, `owninit`, `zombie`]
A transaction that outlives its timeout, or that a new owner finds open, is aborted by the broker, which raises the epoch
first. Kafka's coordinator does that at its own finalized level, not the client's. At Kafka 4 (level 2) the abort is EndTxn
v5's: raised once, the epoch before kept as the last, and when that was the last of the producer id, a new one handed over to
(TX-22). So the producer that timed out may take its id back: InitProducerId with the epoch it had is answered the raised
one, unbumped (KIP-360), and its EndTxn v5 abort is a retry, answered with it; the Java client recovers so from a timeout
(`txn-v2-timeout`). Kafka answers the owner that asks InitProducerId with its own epoch while its transaction is open
CONCURRENT_TRANSACTIONS and then the raised epoch; here the abort is done before the answer, so the first answer is that one.
A new owner (no epoch said) is bumped once more, the epoch before forgotten: the old producer is fenced for good. At Kafka 1
to 3 (level 0) the epoch before is forgotten at the abort too (`endTransactionWithTV1`), as A11 left it (`debt.md` A13).
An abort of v5 stored and not carried out is carried out as decided when a new owner arrives, not raised again: the new owner
is two epochs on, as after a classic one. Test: `transactions_fence::{a_producer_the_broker_timed_out_takes_its_id_back_at_kafka_4_and_is_fenced_at_the_classic_level,
an_owner_asking_while_its_transaction_is_open_is_given_the_raised_epoch_at_kafka_4_and_fenced_at_the_classic_level}`,
`transactions_v2_epochs::a_new_owner_fences_a_producer_of_the_second_version_for_good`,
`ending::tests::a_decision_raises_the_epoch_at_the_second_version_and_the_brokers_own_abort_follows_its_level`,
`transactions_crash::a_new_owner_after_a_stored_abort_of_v5_is_two_epochs_on_as_after_a_classic_one`.

## TX-22. The epochs of a producer id run out [oracle `exhaust`]
An end at the second version (EndTxn v5, or the broker's own abort at Kafka 4) at epoch 32,766 chooses a new producer id,
stores it with the decision (`next_producer_id`), writes the markers at 32,767 with the old id, and completes at the new id,
epoch 0, the old one kept for a retry. The old id with 32,766 is then a retry (the same decision success with the new id,
the other INVALID_TXN_STATE); another epoch of the old id INVALID_PRODUCER_ID_MAPPING; InitProducerId with the old id and
32,766 is answered the new id at 0, unbumped. Test: `ending::tests::an_end_that_used_up_the_epochs_hands_over_to_a_new_producer_id_and_its_retry_is_known`,
`transactions_v2_epochs::{an_end_that_uses_up_the_epochs_hands_over_to_a_new_producer_id, a_timeout_at_the_last_epoch_hands_over_to_a_new_producer_id}`.

## TX-23. The record, and a stop anywhere in an end of the second version
The record is version 2 of `TxnRecord`: Kafka's `TransactionLogValue` v1 fields, the client's transaction version at the last
change and the producer id to hand over to; a record of version 1 is read as a classic one with none. A stop at every
operation of an EndTxn v5, by a kill and by a power cut, for a commit and an abort, and of one that hands over to a new
producer id, leaves the transaction open or decided everywhere, at the raised epoch, and the producer asking again with the
epoch it had finishes it once and is told the producer to go on with, the same after a second restart; one failed operation
anywhere is survived in all three. Test: `txn::tests::a_record_of_the_first_version_is_read_as_a_classic_one_with_no_producer_id_to_hand_over`,
`transactions_crash::{a_stop_at_any_point_of_a_commit_of_v5_leaves_it_open_or_committed_at_the_raised_epoch,
a_stop_at_any_point_of_an_abort_of_v5_leaves_it_open_or_aborted_at_the_raised_epoch,
a_stop_at_any_point_of_a_commit_that_uses_up_the_epochs_hands_over_once, a_stop_at_any_point_of_an_abort_that_uses_up_the_epochs_hands_over_once,
one_operation_failing_anywhere_in_a_commit_is_survived}`, the metadata crash sweep (it writes records of both versions' fields).

## TX-24. AddPartitionsToTxn v4 and v5 (KIP-890's first part: a broker's request) [oracle `batched`]
Several transactions in one request, each verified or added to. CLUSTER_ACTION on the cluster is asked and nothing else, or
the answer is CLUSTER_AUTHORIZATION_FAILED at the top with no transaction (SEC-124). Per transaction, as for v3: an unknown
partition is UNKNOWN_TOPIC_OR_PARTITION and the others OPERATION_NOT_ATTEMPTED. Verify-only answers each partition NONE if
the transaction has it and TRANSACTION_ABORTABLE if not, and changes nothing; an empty id is INVALID_REQUEST, an id nobody owns
or another producer id INVALID_PRODUCER_ID_MAPPING, another epoch PRODUCER_FENCED, an ending CONCURRENT_TRANSACTIONS, on every
partition. Adding adds at the second version, with the coordinator's codes on every partition (PRODUCER_FENCED stays
PRODUCER_FENCED). Test: `transactions_v2_adds::{add_partitions_of_v4_and_v5_verify_or_add_for_several_transactions,
add_partitions_of_v5_that_is_not_verify_only_adds_and_begins}`, `authz_transactions::{add_partitions_of_v4_and_later_needs_cluster_action,
add_partitions_of_v4_and_later_asks_for_no_right_on_the_id_or_the_topic}`,
`rules::tests::an_add_is_refused_for_another_owner_or_an_ending_and_is_nothing_when_all_is_there`.

## TX-25. Where this broker differs from Kafka 4.4.0, decided
- Kafka writes the markers after it answers EndTxn, and a Produce at the old epoch that comes before them is appended to a
  partition that still has the transaction open (seen once in the probe before it waited for the markers). Here EndTxn is
  answered once the markers are on disk, so that write is INVALID_PRODUCER_EPOCH. Kafka answers CONCURRENT_TRANSACTIONS where
  here the request waits for the transaction's lock (TX-7), and does not retry the adds of a Produce of v12 for it.
- A request whose transactional batches carry two producers is failed whole by Kafka (INVALID_PRODUCER_ID_MAPPING); here each
  partition is judged by its own producer, the add by the first (as before KIP-890).
- Kafka refuses a TV2 marker at an epoch not above the partition's; this broker writes its markers itself, always at the
  record's epoch, so there is no other marker to refuse.

Mutation check (2026-11-15) of TX-16 to TX-25: 28 hand-made mutations of the new branches (the codes by version, the EndTxn v5 table's retry
conditions, the epoch kept and forgotten by each level's fence, the hand-over and its choice, the stored abort not raised
again, the add of Produce v12 and of TxnOffsetCommit v5 and their refusals, the start at sequence 0, the verify-only
answer, CLUSTER_ACTION, the EndTxn v5 answer, the feature and its v3 form, the record of version 1, the level by major),
each applied and its tests run: 27 killed at once; the one that survived (the coordinator's refusal of a v12 add ignored,
which the owner checks of the batch answer alike except for a store that fails) is killed by
`an_add_of_a_produce_of_v12_that_cannot_be_stored_is_retriable`, added for it. Not mutated, because nothing can observe it: the transaction
version a record stores (Kafka keeps it for its own checks; DescribeTransactions does not show it).

## TX-26. Produce v13 and TxnOffsetCommit v6 name their topics by id (KIP-516) [wire-tail oracle `produce-ids`, `produce-ids-txn`, `offsets-ids`]
Kafka 4.4.0 offers both and its Java client sends them once Metadata has given the topics' ids: a transactional producer of 4.4.0
writes with Produce v13 and commits offsets with TxnOffsetCommit v6 (seen on the wire against both brokers: `interop/java`
`txn-topic-ids`, `txn-send-offsets`). An entry's id is turned into the name it stands for; one that stands for no topic, the zero
id included, is UNKNOWN_TOPIC_ID (100) on each of its partitions, before any right on a topic is asked (SEC-334, SEC-335), and a name
on the struct is never used (v13 and v6 carry none on the wire). A partition the topic has not is UNKNOWN_TOPIC_OR_PARTITION as
before. Everything else is as at v12 and v5: Produce v13 adds its partitions to the transaction (TX-18) by the name the id stands
for, an unknown id's to nothing; TxnOffsetCommit v6 adds its group (TX-20) and its offsets are applied by the commit. The answers
name each topic by the id it was asked by. v12 and v5 name topics by name, unchanged. The console's recorder shows the id where a
request named a topic by id, as it does for Fetch. The vendored crate reads TxnOffsetCommit v6 since patch 5
(`vendor/kafka-protocol/PATCHES.md`). Test: `topic_ids_in_writes::{produce_v13_writes_to_the_topic_its_id_stands_for_and_an_id_that_stands_for_none_is_unknown_topic_id,
produce_v12_names_by_name_and_v13_by_id, a_transactional_produce_of_v13_adds_the_partitions_its_ids_stand_for,
txn_offset_commit_v6_stages_the_offsets_of_the_topic_its_id_stands_for, txn_offset_commit_v5_names_by_name_and_v6_by_id}`,
`produce_checks::from_v13_a_topic_is_named_by_its_id_and_the_answer_says_which`, `kafka-wire tests/kip516_kip939_versions.rs`,
`profile::tests::only_kafka_4_offers_the_second_part_of_kip_890`.

## TX-27. InitProducerId v6: the two-phase commit (KIP-939) [wire-tail oracle `twopc`, `twopc-open`]
v6 is `latestVersionUnstable` in Kafka 4.4.0: with its defaults the broker lists InitProducerId 0 to 5 and closes a connection that
sends v6, and `transaction.two.phase.commit.enable` alone changes neither; only `unstable.api.versions.enable` offers it (recorded:
`crates/kafka-wire/tests/data/api-versions-4.4.0-unstable.json`). No feature level is involved (`transaction.version` stays 2;
4.4.0 has no level 3). The Java client 4.4.0 has the producer setting `transaction.two.phase.commit.enable` but no two-phase API:
it never sets `Enable2Pc`, and its request builder leaves the unstable v6 out, so it sends v5 to either broker (`interop/java`
`txn-two-phase-commit-setting`). So, as in Kafka: `--unstable-api-versions` (off) offers v6, and `--transaction-two-phase-commit`
(off) lets a request with `Enable2Pc` through. In the coordinator, in Kafka's order (`TransactionCoordinator.handleInitProducerId`):
no transactional id, a producer id whatever the flags; the empty id INVALID_REQUEST; `Enable2Pc` with the setting off
TRANSACTIONAL_ID_AUTHORIZATION_FAILED; `KeepPreparedTxn` UNSUPPORTED_VERSION (4.4.0 has not built it); then the timeout: with
`Enable2Pc` any timeout is accepted and Int.MaxValue is stored (DescribeTransactions shows 2147483647), and such a transaction
never times out (`isDistributedTwoPhaseCommitTxn`); without it, as before (INVALID_TRANSACTION_TIMEOUT out of range). An open
transaction is aborted and its producer fenced as by any new owner. The ongoing-transaction fields of the answer are -1 always.
The rights are SEC-336. Kafka 1 to 3 do not change with either flag. Test:
`transactions_two_phase::{without_the_broker_setting_a_two_phase_commit_is_refused_and_a_plain_v6_is_served,
with_the_broker_setting_a_two_phase_commit_has_no_timeout, a_two_phase_commit_fences_an_open_transaction_like_any_owner,
the_transaction_of_a_two_phase_commit_never_times_out, enable2pc_needs_two_phase_commit_on_the_id_as_well_as_write}`,
`rules::tests::a_two_phase_commit_has_no_timeout_and_never_expires`,
`profile::tests::init_producer_id_v6_is_offered_only_with_the_unstable_versions`, `recorded_unstable_api_versions::*`,
`cli::tests::unstable_versions_and_two_phase_commit_are_off_unless_asked_for`, `kafka-server tests/unstable_versions.rs` (on a socket: v6
closed unanswered with the defaults, answered with the unstable versions).

The wire-tail oracle (`tools/wire-tail-oracle/Probe.java`), run against Kafka 4.4.0 in four settings (the defaults, the unstable
versions, both, the two-phase setting alone) and against doze-kafka with the same flags, answers the same lines but one, in every
setting: ApiVersions lists Produce from v3 (the deliberate floor of `api.rs`).

## TX-28. ListTransactions v2 filters by a pattern (KIP-1152) [wire-tail oracle `list-transactions`]
A `TransactionalIdPattern` that is neither null nor empty keeps the transactional ids it matches whole (Java's
`Matcher.matches`), after the state, producer and duration filters; one that does not compile is INVALID_REGULAR_EXPRESSION
(128) for the request, with no rows (`TransactionStateManager.listTransactionStates`). The pattern is read by `regex-lite`
(wire.md W37-D1). Test: `kafka-broker tests/versions_4_4.rs::list_transactions_v2_keeps_the_ids_its_pattern_matches_whole`.
