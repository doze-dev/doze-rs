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

### S2. Offsets are assigned at append
The log-end offset (LEO) is the next offset. A batch's base offset is stamped to the LEO and the LEO advances by
its record count. The log is its own write-ahead log: there is no separate WAL.

## Append

### S3. A payload is appended whole or not at all
All batches are checked first. If the active segment is not empty and would pass `segment_bytes`, the roll comes
first and the whole payload goes in the new segment. A write that fails part-way takes back the batches before it:
bytes, index entries, LEO. A failed index append is a failed append: no trace is left, and the next append does
not write the same offset twice.

### S4. The index is sparse and derived
An entry (offset, byte position), 16 bytes, is written for a segment's first batch and whenever the bytes since the
last entry reach the interval. Lookup gives the position of the largest entry at or below the target, or 0. A
trailing partial entry is ignored when the file is read.

### S5. A roll seals the segment durably first
The segment being sealed (log and index) is flushed before its successor exists. The successor's name is not flushed
by the roll: nothing in it has been promised yet.

## Read

### S6. A read returns whole batches from the one holding the offset
Below the log-start offset or above the LEO is out of range; equal to the LEO is an empty answer (caught up). The
answer starts at the batch that contains the offset, and holds batches while they fit `max_bytes`, but at least one
if any exists. A `limit` (the last stable offset, for read-committed) excludes every batch that begins at or after it.
When a segment has nothing at or after the offset, the read carries on in the next one.

### S7. A reader never sees a partial append and never blocks the writer
Only fully appended payloads are visible. Reads are positioned reads of the file: the log is never pulled into the
heap, so idle memory does not grow with data.

### S8. A reader can wait for the log to grow
A reader can subscribe to the LEO and be woken when it changes, with no missed wake-up between reading and waiting.

## Durability

### S9. Sync makes everything appended so far durable
It flushes what is not known to be on disk: sealed segments this process has not flushed, the active log, the
active index if it changed, and the directory if its entries changed. Nothing dirty means no system call. When it
returns Ok, every record appended before the call is on disk.

### S10. One sync covers every append before it (group commit)
An append returns a ticket. A ticket is durable once a sync that began after its append completes. Waiting on a
ticket a concurrent sync already covered issues no second sync.

### S11. A failed fsync is never retried
The partition goes offline until restart: appends, syncs and `DeleteBefore` answer an offline error that names the
first failure; reads continue. The kernel may have dropped the pages and report success the second time.

### S12. A failed write is refused and nothing is left behind
Full disk or I/O error on a write: the call fails, its bytes are taken back, the partition stays online, and writes
resume when space returns. No offset is consumed by a refused write.

### S13. What is read back at open is not known to be on disk
Unless the last stop vouches for the partition (S19), the active segment and the directory start dirty, so the first
sync of the partition flushes them, whatever this process wrote.

## Recovery (open)

### S14. The active segment is scanned and cut at the first bad batch
A batch is good when its header is whole, magic 2, it fits in the file, its count is positive, its shape and CRC
check out. The CRC of every batch is verified unless the partition is vouched for, when only the last 8 MiB is. The
cut covers an interrupted append, a zero-filled tail after a power cut, and a body that never fully reached the disk.
The cut is flushed before open returns, so a second crash does not bring the torn bytes back.

### S15. The index follows the log
After the scan the index is compared with what the log should have and rewritten if they differ: an entry can outlive
the bytes it points at, be missing, or be partial.

### S16. A damaged sealed segment is cut, not deleted
A sealed segment is trusted (it was flushed before its successor), but a damaged one found anyway is cut at the first
bad batch and the segments after it are kept; there is no replica to fetch from, and what follows may have been
acknowledged. It is reported.

### S17. Leftovers of an interrupted operation are cleaned
A segment below the recorded log start is removed (finishing an interrupted delete); a `.index` with no `.log` is
removed; a `.log` with no `.index` gets one built; `*.tmp` files from an interrupted durable write are removed.

## Log start and DeleteRecords

### S18. The log start is the larger of the first segment and a recorded value
The recorded value (file `log-start`) is capped at the LEO. The file is replaced atomically and durably: a temporary
file is written and flushed, renamed over the name, and the directory flushed; a crash leaves the old value or the
new, never a torn file.

### S19. DeleteBefore moves the start, then deletes
`-1` means the LEO. At or below the start is a no-op; above the LEO or below -1 is out of range; an offline partition
refuses it before anything is recorded. Emptying a partition with data in its active segment starts a new segment at
the LEO so the old one can go. The new start is recorded durably before any file is removed, so a crash leaves
segments that are only invisible and the next open finishes the job. A segment that cannot be removed now does not
fail the call.

## Retention

### S20. Retention deletes whole old non-active segments
A policy has `max_bytes` and `max_age` (each optional). First by age: oldest sealed segments whose age is before the
cutoff; a segment's age is the largest timestamp of its records, or, if none has one, when it was last written. Then
by size: while the total exceeds `max_bytes`. The active segment is never deleted. The log start advances to the new
first segment. The same rule applies to a partition that is not open, from file sizes and times alone.

## Across a stop

### S21. A clean stop is recorded and vouches for what it flushed
The last act of a stop that flushed and closed every open partition is to write (durably) a marker file naming the
partitions it did not cover; the first act of the next start is to remove it (durably). Found, it vouches for every
partition not named; not found (killed, power cut, a failed flush on the way out), nothing is vouched for.

## Invariants the simulation checks

### S22. Power cut: every synced record survives, in order
For any workload and any cut (every unflushed byte dropped, zeroed, partly kept; every unflushed name gone or back),
reopening yields a log whose records up to the last synced append are all there, byte for byte, with no gaps and no
duplicates; whatever else is there is a prefix of the unsynced appends. A refused or failed operation at any single
file operation (a sweep over every operation of a workload) loses nothing acknowledged.

### S23. Errors cross the crate boundary as one kind
Every storage failure is `StorageError`; the broker maps all of them to `KAFKA_STORAGE_ERROR` except out-of-range.

## Deferred, and to whom

- Timestamp lookup (offset for a time) and the time index: Phase 3 (ListOffsets).
- Compaction and the cleaner's swap: Phase 7. Producer and transaction state in the log: Phase 6.
- Lazily opened files for thousands of partitions (Go keeps two descriptors per segment open): measured in Phase 3.
