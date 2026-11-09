# Compaction

Phase 7, slice 7d. What Apache Kafka 4.3.1's log cleaner does (read from `Cleaner`, `LogCleanerManager`, `CleanedTransactionMetadata`,
`MemoryRecords.filterTo`) and the test that holds each. Pure parts: `kafka-storage/src/compact/{plan,txn,offset_map,retain}.rs`
and `kafka-wire/src/batch/filter/tests.rs`; the log: `kafka-storage/src/log/tests/{compaction,compaction_txn,compaction_read}.rs`;
random workloads and stops: `kafka-storage/tests/compaction/`; through the broker: `kafka-broker/tests/{compaction,compaction_producers,compaction_crash}.rs`;
real clients: `interop/` scenario `compaction`.

**The property** (what a user of a compacted topic relies on, and what the simulation checks after every step and every stop):
the latest record of every key is in the log at its own offset; nothing is in the log that was not written; offsets do not move;
the log is read from every offset in it; and a round run again after a stop ends where an uninterrupted one does.

## CMP-1. When a log is due (source)
A topic whose `cleanup.policy` names `compact` is looked at every `--log-cleaner-interval` (15 s; `--log-cleaner-enable`).
The part below the checkpoint is clean; the rest, up to the first of the active segment, the first offset still open to a
transaction and the first segment newer than `min.compaction.lag.ms`, is dirty; the log is due when dirty bytes are strictly more
than `min.cleanable.dirty.ratio` (0.5) of all, or when the first dirty segment is older than `max.compaction.lag.ms`. The segment
that holds the first dirty offset is dirty whole. Test: `compact::plan::tests::*`.

## CMP-2. What stays (source)
A record stays if it is the latest for its key in the dirty part (or is past what the offset map covers). A record with no key goes.
A deletion (no value) stays until its batch's delete horizon: the first time a batch keeps one it is given `now + delete.retention.ms`,
written as Kafka writes it (attribute bit 6, the horizon in the first timestamp, the records' deltas against it), and the next round after
that time removes it. Test: `compact::retain::tests::*`, `kafka-wire` `filter::tests::*`,
`compaction::a_deletion_is_served_until_its_time_and_then_it_and_the_record_it_deleted_are_gone`.

## CMP-3. What the batches keep (source)
A batch keeps its base offset and the offsets it spans (`lastOffsetDelta`), its producer and sequence, whatever records are taken out;
a batch left with none is dropped, unless it is the last batch of its segment or a producer's last data batch (or the last marker of a
producer with no data), when it stays as an empty batch (no records, uncompressed). Test: `kafka-storage` `a_segment_all_superseded_keeps_one_empty_batch_*`,
`compaction_producers::a_producers_last_batch_is_kept_though_its_records_are_not_*`.

## CMP-4. Transactions (source)
Only below the first offset open to a transaction. The records of an aborted transaction go; its marker stays while a record of it was
dropped this round, is then given a delete horizon, and goes after it; a commit marker stays while a record of its transaction was
seen. Test: `compact::txn::tests::*`, `compaction_txn::the_marker_of_an_aborted_transaction_*`,
`compaction_producers::the_records_of_an_aborted_transaction_are_cleaned_away_*`, `a_transaction_still_open_holds_cleaning_back_*`.

## CMP-5. Replacing a segment is safe against a stop at any point
A round writes `.tmp` files beside the log and changes nothing; the writer then puts each in place: the index is removed, the log
renamed over, the index renamed in, the directory flushed between. A stop between any two steps leaves the segment whole, and an
index that is right or absent (made when the segment is opened). A segment retention removed meanwhile is not put back. The
checkpoint of how far cleaning got is written after. Test: `tests/compaction` (a stop after every file operation of a round, by a
kill and by each way the power can go, then the round run again), `compaction_crash::a_stop_at_any_file_operation_*`,
`compaction::a_segment_retention_removed_while_a_round_ran_*`, `a_round_put_in_place_twice_*`.

## CMP-6. Reading (source)
A batch of the log covers `lastOffsetDelta + 1` offsets however many records it holds; every walk of the log advances by that. A
read goes on past leading batches with no records to the records after them (found by librdkafka: a fetch answered with only
such a batch gives the client nothing to count as progress, and a run of them ends in "Message at offset N might be too large to
fetch"), and returns them only when nothing may follow. The log's own walks see them. Test:
`compaction_read::a_read_that_lands_on_a_batch_with_no_records_*`, `compaction::a_compacted_topic_keeps_the_latest_*`,
interop `compaction` (franz-go, sarama, librdkafka agree).

## CMP-7. Policies
`compact` alone: not trimmed by retention, DeleteRecords refused (ADM-2), a record with no key refused at Produce (INVALID_RECORD).
`compact,delete`: compacted, then retention applies. `delete`: never cleaned. Test: `compaction::a_topic_that_does_not_compact_*`.

## Decided differences
- **CMP-D1.** Each segment is rewritten under its own name, and segments are not merged into fewer; the last batch of every cleaned
  segment stays (Kafka keeps the last of the round and deletes a segment left empty). Reads skip the empty ones (CMP-6).
- **CMP-D2.** The offset map keeps the keys themselves (bounded to 16 MiB, `Config.cleaner_map_bytes`; the rest is cleaned in a
  later round), not a 16-byte digest in a 128 MiB buffer.
- **CMP-D3.** One cleaner for the broker, a segment at a time, with no I/O throttle and no `log.cleaner.threads`; the checkpoint is
  `cleaner-checkpoint` beside the segments.
- **CMP-D4.** A batch is kept in the codec it has; a topic's `compression.type` rewrites a batch at Produce (as Kafka), not when cleaning.
