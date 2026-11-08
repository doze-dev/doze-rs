# Behaviours: storage (kafka-storage)

What a partition log must do, distilled from the Go source (`internal/storage/log`, at `4081fa6`), its docs
(`docs/storage-and-durability.md`) and its power-cut model. Not translated: the Rust design is in
`PLAN.md`'s revision log. Each entry names the Rust test that covers it (filled in as the tests land).
Not in this phase: compaction and transaction/producer state (Phase 6/7), the Kafka-level Produce checks
(Phase 3: size limit, codec rewrite), listing partitions of a topic (Phase 3).

Dropped on purpose (fresh data, nothing older exists): Go's `unvalidated` list for logs from before the
"flush a segment before its successor" rule.

## Layout and offsets

### S1. A partition is a directory of segments
A segment is `<base offset, 20 digits, zero padded>.log` and `.index`. Listing reads the `.log` names, ignores
names that are not numbers, and orders by base offset. A new partition starts with one empty segment at 0.
Test: `log::tests::append::a_new_partition_is_empty_with_one_segment`, `segment::tests::names_are_twenty_digits_and_round_trip`

### S2. Offsets are assigned at append
The log-end offset (LEO) is the next offset. A batch's base offset is stamped to the LEO and the LEO advances by
its record count. The log is its own write-ahead log: there is no separate WAL.
Test: `log::tests::append::offsets_are_assigned_at_append_and_stamped_into_the_batches`, `log::tests::append::offsets_run_out_without_wrapping`

## Append

### S3. A payload is appended whole or not at all
All batches are checked first. If the active segment is not empty and would pass `segment_bytes`, the roll comes
first and the whole payload goes in the new segment. A write that fails part-way takes back the batches before it:
bytes, index entries, LEO. A failed index append is a failed append: no trace is left, and the next append does
not write the same offset twice.
Test: `log::tests::append::a_payload_is_appended_whole_or_not_at_all`, `log::tests::append::a_payload_that_does_not_fit_rolls_first_and_goes_in_whole`, `log::tests::append::a_full_disk_refuses_writes_and_they_resume_when_there_is_room`, `segment::tests::a_failed_write_leaves_no_trace`, `segment::tests::a_failed_index_write_takes_back_the_batch_it_belonged_to`, `segment::tests::a_write_cut_short_by_a_full_disk_is_taken_back`

### S4. The index is sparse and derived
An entry (offset, byte position), 16 bytes, is written for a segment's first batch and whenever the bytes since the
last entry reach the interval. Lookup gives the position of the largest entry at or below the target, or 0. A
trailing partial entry is ignored when the file is read.
Test: `segment::tests::the_index_gets_the_first_batch_and_then_one_per_interval`, `scan::tests::the_index_gets_an_entry_each_time_the_interval_is_reached`, `log::tests::recovery::an_index_that_disagrees_with_the_log_is_rebuilt`

### S5. A roll seals the segment durably first
The segment being sealed (log and index) is flushed, and its name with it (the directory), before its successor exists.
The successor's own name is not flushed by the roll: nothing in it has been promised yet. Flushing the sealed
segment's name too means the names left after a power cut are always a run of segments up to the newest: a newer
segment never outlives an older one, so the log has no hole in the middle. (Found by the simulation, under a model in
which each unflushed name survives or not on its own.)
Test: `log::tests::append::a_roll_flushes_the_segment_it_seals_before_the_next_exists`; the simulation sweeps.

## Read

### S6. A read returns whole batches from the one holding the offset
Below the log-start offset or above the LEO is out of range; equal to the LEO is an empty answer (caught up). The
answer starts at the batch that contains the offset, and holds batches while they fit `max_bytes`, but at least one
if any exists. A `limit` (the last stable offset, for read-committed) excludes every batch that begins at or after it.
When a segment has nothing at or after the offset, the read carries on in the next one. A read is otherwise served
from the one segment it starts in (as Kafka serves it); the consumer fetches again to go on.
Test: `log::tests::append::a_read_starts_at_the_batch_that_holds_the_offset`, `log::tests::append::a_read_holds_whole_batches_within_the_byte_limit_but_always_the_first`, `log::tests::append::a_limit_hides_the_batches_that_begin_at_or_after_it`

### S7. A reader never sees a partial append and never blocks the writer
Only fully appended payloads are visible. Reads are positioned reads of the file: the log is never pulled into the
heap, so idle memory does not grow with data.
Test: `segment::tests::nothing_is_visible_to_a_reader_until_it_is_published`, `tests/real_disk/main.rs::readers_never_see_a_partial_append_while_the_writer_writes_and_rolls`, `tests/flat_memory.rs`

### S8. A reader can wait for the log to grow
A reader can subscribe to the LEO and be woken when it changes, with no missed wake-up between reading and waiting.
Test: `log::tests::append::a_reader_can_wait_for_the_end_to_move`

## Durability

### S9. Sync makes everything appended so far durable
It flushes what is not known to be on disk: the active log, the
active index if it changed, and the directory if its entries changed (a sealed segment was flushed whole by the roll
that sealed it). Nothing dirty means no system call. When it
returns Ok, every record appended before the call is on disk.
Test: `log::tests::durability::a_ticket_is_durable_once_a_sync_covers_it`, `log::tests::durability::a_sync_with_nothing_dirty_makes_no_system_call`, `segment::tests::sync_flushes_only_what_changed`, `log::tests::durability::close_flushes_everything`

### S10. One sync covers every append before it (group commit)
An append returns a ticket. A ticket is durable once a sync that began after its append completes. Waiting on a
ticket a concurrent sync already covered issues no second sync.
Test: `log::tests::durability::one_sync_covers_every_append_before_it`

### S11. A failed fsync is never retried
The partition goes offline until restart: appends, syncs and `DeleteBefore` answer an offline error that names the
first failure; reads continue. The kernel may have dropped the pages and report success the second time.

A limit no code can remove: after a failed fsync the kernel has marked the dropped pages clean, so a new *process* on the
same machine finds them in the page cache and a later fsync of them succeeds without writing anything. Only the
machine starting again clears it. The broker's answer to an offline partition has to say so (restart the machine, or at
least do not trust the process restart), and the simulation models an offline log's restart as a reboot.
Test: `log::tests::durability::a_failed_fsync_takes_the_partition_offline_and_is_never_retried`, `log::tests::durability::a_failed_directory_flush_takes_the_partition_offline_too`, `log::tests::durability::a_retry_that_would_have_succeeded_proves_nothing_and_so_is_not_made`, simulation `a_failed_flush_is_never_retried_and_nothing_it_covered_is_lost_that_was_acknowledged`

### S12. A failed write is refused and nothing is left behind
Full disk or I/O error on a write: the call fails, its bytes are taken back, the partition stays online, and writes
resume when space returns. No offset is consumed by a refused write.
Test: `log::tests::append::a_payload_is_appended_whole_or_not_at_all`, `log::tests::append::a_full_disk_refuses_writes_and_they_resume_when_there_is_room`

### S13. What is read back at open is not known to be on disk
Unless the last stop vouches for the partition (S21), the active segment's log, its index and the directory each start
dirty, so the first sync of the partition flushes all three, whatever this process wrote. (They are three claims: a
process that restarts before it flushed the index it rewrote must still flush it, or a power cut after the next roll
leaves a sealed segment with a stale index. Found by the simulation.)
Test: `log::tests::durability::what_is_read_back_at_open_is_flushed_by_the_first_sync_unless_the_last_stop_vouches`, `log::tests::durability::what_is_read_back_at_open_is_flushed_log_index_and_directory_each_by_itself`

## Recovery (open)

### S14. The active segment is scanned and cut at the first bad batch
A batch is good when its header is whole, magic 2, it fits in the file, its count is positive, its shape and CRC
check out. The CRC of every batch is verified unless the partition is vouched for. A vouched log was flushed whole by
the stop that vouches, so only the stretch from the last index entry to the end is walked (checksums and all, and the
index is taken as it is); if that stretch is not whole after all, nothing is trusted and the whole segment is walked.
The cut covers an interrupted append, a zero-filled tail after a power cut, and a body that never fully reached the disk.
The cut is flushed before open returns, so a second crash does not bring the torn bytes back.
Test: `log::tests::recovery::a_torn_tail_is_cut_and_the_cut_is_flushed`, `log::tests::recovery::a_zero_filled_tail_after_a_power_cut_is_cut`, `log::tests::recovery::a_damaged_batch_in_the_middle_ends_the_log_there`, `log::tests::open::a_vouched_open_walks_only_the_end_of_the_log_and_an_unvouched_one_checks_all_of_it`, `log::tests::open::a_vouched_open_that_finds_the_end_not_whole_falls_back_to_checking_everything`, `scan::tests::*`, `segment::tests::adopting_a_scan_cuts_the_log_durably_and_rebuilds_the_index`

### S15. The index follows the log
After the scan the index is compared with what the log should have and rewritten if they differ: an entry can outlive
the bytes it points at, be missing, or be partial.
Test: `log::tests::recovery::an_index_that_disagrees_with_the_log_is_rebuilt`, `log::tests::recovery::the_index_of_a_sealed_segment_is_rebuilt_when_it_is_gone`

### S16. An empty leftover segment that begins inside its predecessor is removed
A roll that fails half way (the log created, its index not) cleans up after itself, but the removal is only in memory
until the directory is flushed, and a power cut can bring the empty file back while the old segment went on taking
appends. Left alone it would make the log end at the leftover's base and hide the records after it. So a failed segment
creation marks the directory changed (the next sync flushes the cleanup), and open removes an empty last segment whose
base is below the end of the one before it. (Found by the simulation.) A damaged sealed segment is not a case: a sealed
segment was flushed whole before its successor existed.
Test: `log::tests::recovery::an_empty_segment_a_failed_roll_left_behind_cannot_hide_the_records_that_followed_it`.

### S17. Leftovers of an interrupted operation are cleaned
A segment below the recorded log start is removed (finishing an interrupted delete); a `.index` with no `.log` is
removed; a `.log` with no `.index` gets one built; `*.tmp` files from an interrupted durable write are removed.
Test: `log::tests::recovery::leftovers_of_interrupted_operations_are_removed`, `log::tests::recovery::segments_below_the_recorded_start_are_removed_at_open`, `log::tests::recovery::a_roll_that_died_after_creating_the_next_segment_leaves_an_empty_active_segment`

## Log start and DeleteRecords

### S18. The log start is the larger of the first segment and a recorded value
The recorded value (file `log-start`) is replaced atomically and durably: a temporary file is written and flushed, renamed
over the name, and the directory flushed; a crash leaves the old value or the new, never a torn file. A recorded start
beyond the end of the records that survived (DeleteRecords is durable, the records it covered were not) is not capped
and not forgotten: the log carries on from it, with a new segment beginning there, so an offset below a start that
was recorded is never handed out again. (Found by the audit; the simulation had hidden it by capping.)
Test: `log::tests::open::a_recorded_start_beyond_the_records_that_survived_is_carried_on_from_never_reissued`, `log::tests::open::a_directory_with_only_a_recorded_start_begins_its_first_segment_there`, `fs::tests` (the durable write), `log::tests::trim::the_new_start_survives_a_restart`

### S19. DeleteBefore moves the start, then deletes
`-1` means the LEO. At or below the start is a no-op; above the LEO or below -1 is out of range; an offline partition
refuses it before anything is recorded. Emptying a partition with data in its active segment starts a new segment at
the LEO so the old one can go. The new start is recorded durably before any file is removed, so a crash leaves
segments that are only invisible and the next open finishes the job. A segment that cannot be removed now does not
fail the call. The next open records the start again before it removes anything on its account: it read the value
from the page cache, where a flush that failed or was interrupted left it, and a power cut after the removals must not
take the start back and leave a hole. (The new segment's name is not flushed by the delete: a power cut that loses it
is the case the carry-on rule of S18 covers, and the simulation agrees that a flush there is not needed.)
Test: `log::tests::trim::delete_before_moves_the_start_and_removes_whole_segments_below_it`, `log::tests::trim::deleting_to_the_end_empties_the_partition_and_starts_a_fresh_segment`, `log::tests::trim::the_start_is_recorded_before_any_file_is_removed`, `log::tests::trim::deleting_to_the_end_starts_by_flushing_and_a_failure_there_changes_nothing`, `log::tests::trim::a_start_that_reached_only_the_page_cache_is_made_durable_before_segments_are_removed_on_its_account`

## Retention

### S20. Retention deletes whole old non-active segments
A policy has `max_bytes` and `max_age` (each optional). First by age: oldest sealed segments whose age is before the
cutoff; a segment's age is the largest timestamp of its records, or, if none has one, when it was last written. Then
by size: while the total exceeds `max_bytes`. The active segment is never deleted. The log start advances to the new
first segment, and, as for DeleteRecords, it is recorded durably before any file is removed: removals are only in
memory until the directory is flushed, and a power cut may keep a later one and lose an earlier one, which would leave a
hole in the log; with the start recorded, whatever comes back is below it and the next open removes it. The same rule
applies to a partition that is not open, from file sizes and times alone.
Test: `log::tests::trim::retention_by_size_deletes_the_oldest_whole_segments_and_never_the_active_one`, `log::tests::trim::retention_by_age_goes_by_the_newest_record_in_a_segment_not_by_when_it_was_written`, `log::tests::trim::a_segment_with_no_timestamps_is_as_old_as_its_last_write`, `log::tests::trim::the_decision_is_a_pure_function_of_sizes_and_ages`, `log::tests::trim::a_partition_that_is_not_open_is_trimmed_the_same_way`

## Across a stop

### S21. A clean stop is recorded and vouches for what it flushed
The last act of a stop that flushed and closed every open partition is to write (durably) a marker file naming the
partitions it did not cover; the first act of the next start is to remove it (durably). Found, it vouches for every
partition not named; not found (killed, power cut, a failed flush on the way out), nothing is vouched for.
Test: `marker::tests::*`, `tests/real_disk/main.rs::the_clean_stop_marker_works_on_the_real_disk`

## Invariants the simulation checks

### S22. Power cut: every synced record survives, in order
For any workload and any cut (every unflushed byte dropped, zeroed, partly kept; every unflushed name gone or back),
reopening yields a log whose records up to the last synced append are all there, byte for byte, with no gaps and no
duplicates; whatever else is there is a prefix of the unsynced appends. A refused or failed operation at any single
file operation (a sweep over every operation of a workload) loses nothing acknowledged.
Test: the simulation suite: `the_power_cut_after_each_step_of_a_workload_loses_nothing_acknowledged`, `stopping_the_process_after_every_file_operation_loses_nothing_acknowledged`, `any_single_failing_file_operation_loses_nothing_acknowledged`

### S23. Errors cross the crate boundary as one kind
Every storage failure is `StorageError`; the broker maps all of them to `KAFKA_STORAGE_ERROR` except out-of-range.
Test: the type itself (`StorageError`); `OutOfRange` in `log::tests::append::a_read_starts_at_the_batch_that_holds_the_offset`, `Offline` in `log::tests::durability::a_failed_fsync_takes_the_partition_offline_and_is_never_retried`.

### S24. Appending copies nothing
A batch is written from the buffers it already is: the 61-byte header with the broker's changes (the base offset, and
under LogAppendTime the clock bit, the newest timestamp and a checksum extended over the unchanged body) is built on the
stack, and the body is borrowed from the request. The body is written first and the header last, because the header is
what makes it a batch. Looking at a header while reading, and comparing an index, allocate nothing; a fetch allocates
about the bytes it returns. Test: `kafka-wire batch::tests::stamping_does_not_copy_the_body`, `tests/flat_memory.rs`,
`log::tests::append::a_log_append_time_topic_stamps_the_brokers_clock_and_keeps_the_batches_valid`.

### S25. A read is one range of the file, and damage is an error
A read asks the file for the bytes it will return (and up to 16 KiB before them, to step over the batches ahead of the
offset) in one read, walks the headers in memory, and cuts the answer out of that buffer without copying it. Committed
bytes that are not a batch are damage and an error, never a gap to read past (sealed segments are not re-checked at
open). If retention moved the start past the offset while the bytes were being read, the reader is told it is out of
range rather than handed what follows.
Test: `log::tests::read::a_read_of_many_small_batches_asks_the_file_for_a_few_ranges_not_one_per_batch`, `log::tests::read::damage_in_committed_bytes_is_an_error_and_never_a_gap_to_read_past`, `tests/real_disk/main.rs::readers_never_see_a_partial_append_while_the_writer_writes_and_rolls`

### S26. Only canonical names are segments
A segment's files are twenty digits and `.log` or `.index`. `5.log`, `+5.log` or `007.log` are not segments (parsing them
loosely would name a file that the canonical path does not reach, and the partition would not open).
Test: `segment::tests::names_are_twenty_digits_and_round_trip`

### S27. A ticket never waits for ever
If the log is dropped, or a flush fails, a ticket that was not covered learns it (an error), even while a reader still
holds the shared state; one that was covered stays good.
Test: `log::tests::tickets::a_ticket_does_not_wait_for_ever_for_a_log_that_is_gone`, `log::tests::durability::a_failed_fsync_takes_the_partition_offline_and_is_never_retried`

## Deferred, and to whom

- Timestamp lookup (offset for a time) and the time index: Phase 3 (ListOffsets).
- Compaction and the cleaner's swap: Phase 7. Producer and transaction state in the log: Phase 6.
- Lazily opened files for thousands of partitions. Each segment holds two descriptors (5,000 partitions of 10 segments
  each would need 100,000), which Phase 3 must bound with lazy open or a descriptor cache.
- Go behaviours not built yet, because Phase 3 is what needs them: a closed partition's start and end without opening it
  (`boundsFromDisk`: ListOffsets, DescribeLogDirs, the console), destroying a partition (DeleteTopics: `Fs` has no
  `remove_dir`), changing `segment.bytes` at runtime, and the broker's own reading of `max_bytes` of 0 (here it means
  "the first batch", where Go returned the whole remainder).
