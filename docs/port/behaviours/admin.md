# Topic administration, configs, retention

Phase 7 (slices 7a to 7c). What Apache Kafka 4.3.1 does, and the test in this repository that holds each. The source is the
recordings (`conformance/recordings`, the oracle scenario in brackets) where there is one, and otherwise Kafka 4.3.1's own
source, read by item (`ConfigAdminManager`, `ConfigHelper`, `ControllerConfigurationValidator`, `DynamicBrokerConfig`,
`ReplicationControlManager`, `KafkaApis`): **(source)** marks those. Wire tests are in `crates/kafka-broker/tests/`
(`delete_records`, `log_retention`, `describe_configs`, `alter_configs`, `broker_configs`, `create_partitions`,
`create_partitions_crash`); the pure parts live where they are (`configs/tests.rs`, `topic_config/tests.rs`,
`kafka-meta` `store/tests.rs`, `kafka-storage` `topic_dirs/tests.rs`).

## ADM-1. Retention acts (source)
Every `log.retention.check.interval.ms` (flag `--log-retention-check-interval`, 5 minutes) each partition of a topic whose
policy contains `delete` is trimmed by the topic's `retention.bytes` and `retention.ms` (else the broker's), oldest whole
segments first. As in Kafka (`UnifiedLog.deletableSegments`), a non-empty active segment that is past a bound is rolled first
and then deleted with the rest, an empty active segment is never deleted, and a segment goes by size only while what remains
is still at least `retention.bytes`. A topic that only compacts is not trimmed. A closed partition
is opened for it, and closed again right after if the sweep is what opened it (it used to stay open until the limit on open
partitions closed it: the idle audit of 2026-11-14 measured +6 descriptors and +2.5 MB for three partitions). Each pass notes when
retention could next take something from the partition with nothing written to it: an age bound takes the oldest segment first, so
that is when the oldest segment outlives `retention.ms` (`PartitionLog::retention_due`); a size bound changes only with writes, and
an empty partition has nothing. Until then, and until the partition changes (an append, a trim, a new config; its writer counts
them), the sweep does not open it or list its directory. Test: `log_retention::*`,
`compaction::a_topic_that_does_not_compact_is_left_as_it_is_and_one_that_only_compacts_is_not_trimmed`,
`idle_upkeep::the_retention_sweep_closes_what_it_opened_and_leaves_a_partition_alone_until_something_in_it_can_expire`,
`idle_upkeep::the_retention_sweep_comes_back_when_the_oldest_segment_has_aged_out`,
`kafka-storage log::tests::retention::retention_due_is_when_the_oldest_segment_outlives_the_age_bound_and_never_without_one`,
`partitions::upkeep::tests::a_verdict_holds_until_its_time_a_change_or_other_settings`.

## ADM-2. DeleteRecords v0-2 (source)
Per partition, in order: unknown topic or partition (3, low watermark -1); a topic that only compacts (POLICY_VIOLATION 44);
an offset of -1 is the high watermark; below -1 or past the high watermark OFFSET_OUT_OF_RANGE (1); at or below the start
changes nothing; otherwise the start moves, durably, and the answer is the low watermark now in force. A topic naming no
partition has no entry; a partition named twice is answered once, for the last offset. The last stable offset never falls below
the start. Test: `delete_records::*`.

## ADM-3. DescribeConfigs v1-4: topics (source) [topic-config-keys]
A topic lists the 33 configs Kafka lists, in Kafka's order, each with value, type, `read_only` false, `is_sensitive` false.
A key named that is not a config is not listed and not an error; no keys, or an empty list, is all. The source of a value is
the topic (1) when the topic sets it, else the first place the broker has it: dynamic broker (2), dynamic default (3), static
(4: a flag whose value is not Kafka's default, such as `--segment-bytes`) or default (5). With synonyms asked for, the topic's
own value first, then those places. An unknown topic is UNKNOWN_TOPIC_OR_PARTITION with no message; an invalid name is
INVALID_TOPIC_EXCEPTION with Kafka's text. Documentation is given only when asked for. Test: `describe_configs::*`.

## ADM-4. DescribeConfigs: the broker (source) [group-coordinator-defaults]
Resource `""` lists what is set for all brokers (nothing, to start with); `"1"` this broker's configs: the five that can be
changed (below), and the settings it holds that cannot (`num.partitions`, `offsets.retention.minutes`,
`group.initial.rebalance.delay.ms`, the `group.consumer.*` intervals, ...), each read-only with its place and, with synonyms
asked for, the places it could be from (the layout of the recording). Another id: INVALID_REQUEST "Unexpected broker id,
expected 1 or empty string, but received 2"; not a number: "Broker id must be an integer, but it is: x". Test:
`describe_configs::the_broker_*`, `a_broker_that_is_not_this_one_*`.

## ADM-5. AlterConfigs v0-2 and IncrementalAlterConfigs v0-1: the checks, in Kafka's order and words (source) [list-config-append]
A resource named twice (type and name): INVALID_REQUEST "Each resource must appear at most once." for both. Then, per
resource: a config named twice "Error due to duplicate config keys"; a missing value on anything but a delete "Null value not
supported for : a, b"; an operation number that does not exist "Unknown operations type N"; APPEND or SUBTRACT on what is not
a list INVALID_CONFIG "Can't APPEND to key X because its type is not LIST."; an unknown key "Unknown topic config name: X";
a bad value "Invalid value V for configuration K: ..."; a minimum compaction lag above the maximum "conflict topic config
setting min.compaction.lag.ms (X) > max.compaction.lag.ms (Y)"; only then the topic: UNKNOWN_TOPIC_OR_PARTITION "The topic 'x'
does not exist."; an empty topic name "Default topic resources are not allowed."; an unknown resource type "Unknown resource
type N". Each resource is answered in the order asked; a refusal of one does not stop the others; a validate-only runs every
check and changes nothing. The legacy AlterConfigs replaces everything the resource has; the incremental one changes it
(SET, DELETE, APPEND, SUBTRACT; an APPEND starts from the default when the topic has none). Test: `alter_configs::*`,
`configs/tests.rs`.

## ADM-6. A change reaches the log at once
`segment.bytes` and `index.interval.bytes` of a topic, and the broker's defaults for them, go to the open partitions' writers
and apply from the next append; `max.message.bytes`, `message.timestamp.*`, `compression.type` and `cleanup.policy` are
read for each request; retention and compaction read the configs each pass. Test: `broker_configs::a_topic_changed_while_open_*`,
`a_segment_size_set_for_the_broker_*`, `a_larger_batch_than_the_brokers_limit_*`.

## ADM-7. The broker's dynamic configs (source)
`log.segment.bytes` (at least 1 MiB), `log.index.interval.bytes`, `message.max.bytes`, `log.retention.bytes`, `log.retention.ms`
can be set for all brokers (resource `""`) or for this one (its id; this one's wins), are kept durably and are there after a
restart. A config Kafka has that cannot be changed while it runs: INVALID_REQUEST "Cannot update these configs dynamically:
[num.partitions]"; one it does not have "Unknown Dynamic Configuration: X."; a bad value "Invalid value V for configuration K: ...";
another broker id "Unexpected broker id, expected 1, but received 2"; not a number "Node id must be an integer, but it is: x".
Test: `broker_configs::*`; the store: `kafka-meta` `broker_configs_are_kept_by_scope_replaced_whole_and_survive_the_power_going`.

## ADM-8. CreatePartitions v0-3 (source)
A name listed twice is INVALID_REQUEST "Duplicate topic name." (once, first in the answer; neither entry is acted on). Per topic,
in order: unknown topic (3, no message); the same count "Topic already has N partition(s)."; a smaller one "The topic T
currently has N partition(s); M would not be an increase." (INVALID_PARTITIONS); an assignment list whose length is not the
number of new partitions INVALID_REPLICA_ASSIGNMENT "Attempted to add A additional partition(s), but only L assignment(s) were
specified."; each assignment: an empty list, a broker that is not registered, a broker twice, a replica count that is not the
topic's. The new partitions are there at once for Metadata, producers and consumers (a KIP-848 member is given them); they
start empty; a validate-only changes nothing. The directories are made and flushed before the count is recorded, so a stop at
any point leaves the old count or the new with every partition on disk. Test: `create_partitions::*`,
`create_partitions_crash::a_stop_at_any_point_*`, `topic_dirs::directories_made_or_removed_from_a_number_up_*`.

## ADM-9. DescribeLogDirs v1-4 (source)
One result: the data directory, as an absolute path (Java's `toAbsolutePath`, not resolved), error 0, and from v4 the volume's
size and the room this process may still write (`FileStore.getTotalSpace` / `getUsableSpace`; -1 where the platform does not
say). Every partition of every topic, or those asked for (`topics` null: all; asked partitions that do not exist are left out, and a
topic none of whose asked partitions exist): its size is its segments' log files (`UnifiedLog.size`, read by the same walk the
Data page reads), 0 for a partition never written to; lag 0; never a future log. A topic's partitions are by number and the topics
by name (Kafka's come from a hash map). Directories that belong to no topic (strays) are not listed. Without DESCRIBE on the
cluster: CLUSTER_AUTHORIZATION_FAILED and no directory (SEC-296). `kafka-log-dirs.sh --describe` prints the same as against
Kafka 4.4.0 (sizes to the byte for the same records); every case of `oracle_diff` (all, some, none, at v1 to v4) is the same.
Test: `tests/describe_log_dirs.rs::*`, `kafka-storage usage::tests::every_byte_is_counted_once_under_the_kind_it_belongs_to`

## Decided differences
- **ADM-D1.** BROKER_LOGGER, CLIENT_METRICS and GROUP resources are refused (INVALID_REQUEST "Unsupported resource type: X"); Kafka
  serves them. There are no loggers to set, and no client metrics or group configs.
- **ADM-D2.** The broker's config list is the ones it has (about twenty), not Kafka's several hundred; their documentation is
  null. Of Kafka's dynamic broker configs only five can be set; a config Kafka allows and this broker has no use for is answered
  "Unknown Dynamic Configuration" where Kafka would accept it.
- **ADM-D3.** A topic config's broker synonym is the highest-priority name (`log.segment.bytes`, `log.retention.ms`), and the
  default Kafka lists is the hours form for `retention.ms` and `segment.ms` (`log.retention.hours` 168); the other
  names a synonym has (minutes) are not listed.
- **ADM-D4.** The flags' defaults are not Kafka's for `segment.bytes` (128 MiB), `retention.ms` and `retention.bytes` (none),
  and `group.initial.rebalance.delay.ms` (200 ms): they are shown as static broker configs, with the value in force.
- **ADM-D5.** CreatePartitions answers the topics that are not a duplicate in the order asked (Kafka's comes from a hash set).
  The partition count is held to `--max-partitions` (a limit of this broker, with its own text). Replication factor is one.
- **ADM-D6.** ListConfigResources is not offered. (The quota-throttled error of CreatePartitions v3 is: `quotas.md` QT-10.)
- **ADM-D8.** DescribeLogDirs is offered to v4 at Kafka 4: v5 (Kafka 4.2's `IsCordoned`) is not in the wire crate. Kafka's internal
  topics (`__consumer_offsets`, `__transaction_state`) are not listed: this broker keeps offsets and transactions in its metadata
  store, which has no partitions.
- **ADM-D7.** `segment.ms` is the wall-clock age of the active segment since its first write, judged when the next append
  comes (an idle partition keeps its segment until then). Kafka's `LogSegment.timeWaitedForRoll` measures the incoming batch's
  largest timestamp against the first batch's timestamp, and a reopened segment starts from that timestamp, not from the file's
  time. The two differ for backfilled or skewed record timestamps, and for the first roll after a restart. Decided: a clock the
  producers cannot move keeps a topic's segments from growing without bound whatever timestamps arrive; the cost is a roll that
  may come earlier or later than Kafka's for such data. Test: `log::tests::retention::a_segment_older_than_segment_ms_is_rolled_by_the_next_append_and_an_idle_one_is_left_alone`.
