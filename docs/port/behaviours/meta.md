# Behaviours: metadata (kafka-meta)

What the broker's metadata store must do, distilled from the Go source (`internal/storage/meta`, at `4081fa6`).
Phase 3 holds topics and producer ids; committed offsets, group liveness, transactions and ACLs join in the
phases that need them. Each entry names the Rust test that covers it.

### M1. A topic name is Kafka's, parsed once
1 to 249 characters from `[a-zA-Z0-9._-]`, and not `.` or `..`. A name is a directory name under the data
directory, so the alphabet (no separator, no NUL, no non-ASCII) is also what keeps a topic inside it. Only
creation validates; there is no data from before this rule (fresh start).
Test: `kafka-wire::ids::topic::tests::a_name_is_what_kafka_allows_and_nothing_that_could_leave_a_directory`

### M2. Names that would share a metric name or a directory may not both exist
`a.b` against `a_b` (Kafka's rule) and names that differ only in letter case (ours: a case-insensitive file system
would give them one directory). Letter case is judged first; of several names that collide under the dot rule, the one that sorts last is named (Kafka
names the one created last, which only differs for names made by one request, since the others cannot coexist). A request's own earlier topics
are let off the dot rule and not the case rule (Kafka checks each name of a request against the topics that existed
before it). The same name is "already exists", not a collision. Deleting a topic frees its spellings.
Test: `store::tests::a_name_may_not_live_beside_one_that_differs_only_in_case_or_in_dot_for_underscore`

### M3. A created or deleted topic is durable before the call returns
Whatever the power does after the call, the topic (name, id, partitions, configs) is there, or is gone, as told. A
topic whose creation was never acknowledged may or may not be there; one that was never attempted is not.
Test: `tests/crash`: the three sweeps (power cut after each step in six ways, process stopped after every file
operation, any single failing operation including a flush that loses its pages); `store::tests::what_was_acknowledged_is_there_after_the_power_goes_and_comes_back`

### M4. A write that fails changes nothing the broker can see
The in-memory registry is updated only after the commit. After a failed commit the database refuses everything
until the process starts again (redb's rule), as a partition goes offline on a failed flush.
Test: `store::tests::a_write_that_fails_leaves_the_topics_as_they_were`

### M5. The database is made whole before it has its name
Created under a temporary name, committed, closed (by name: a flush that fails at close stops the file being put in
place, because a dropped database swallows it and a file whose flush failed can report success without writing),
renamed, and the directory flushed. A crash while it is being made leaves no file for redb to make sense of.
Test: `tests/crash` (stopping after every operation, and failing every operation, cover each step of making it)

### M6. Records are ours and versioned
A version byte then postcard; a record of a version this build does not know is refused, not misread.
Test: `store::tests::a_record_written_by_a_version_this_build_does_not_know_is_refused`

### M7. A producer id is never handed out twice, and a flush is paid once for a block
A block of 1000 is reserved durably at a time; ids left in a block when the process stops are never used.
Test: `store::tests::producer_ids_never_repeat_across_restarts_and_cost_a_flush_once_a_block`; the crash sweeps check no id repeats after any cut.

### Not in Phase 3
Locking: redb is given no file locks (our file seam has none); one process per data directory is the binary's job
(a lock file in the data directory, slice 3g). Config updates on a topic and partition growth (Phase 7), committed
offsets with the topic's lifetime (Phase 4: deleted in the same transaction as the topic).
