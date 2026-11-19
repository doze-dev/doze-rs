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

## The admin APIs of debt K3 (2026-11-19)

Eleven APIs real brokers list, at the recorded versions of each major (`crates/kafka-wire/tests/data/api-versions.json`), written from Kafka
4.4.0's source and held to a real Kafka 4.4.0 (KRaft, one combined node, SASL and PLAINTEXT listeners, an authorizer, a token key) by
`crates/kafka-wire/examples/oracle_admin`: 132 steps, 0 differences, 1 written down (ADM-D9). Kafka's own tools give the same output on both
(2026-11-19, as alice over SASL/PLAIN): `kafka-topics.sh --describe` (one topic and all), `kafka-leader-election.sh` (preferred, all; unclean,
one), `kafka-reassign-partitions.sh --execute/--verify/--list`, `kafka-features.sh describe/downgrade --dry-run/upgrade`,
`kafka-delegation-tokens.sh --create/--describe/--renew/--expire`, and `kafka-topics.sh --list` logged in with a token; they differ only by
ids, HMACs, times, the log directory's path, the features' epoch and the topics' static configs (ADM-D4). Who may ask: SEC-340 to SEC-351.
Tests: `crates/kafka-broker/tests/admin_apis.rs`, `tests/delegation_tokens.rs`.

The oracle, by hand: Kafka 4.4.0 with `listeners` PLAINTEXT and SASL_PLAINTEXT (PLAIN and both SCRAMs; JAAS users admin, alice, bob with
passwords `<name>-pw`, SCRAM credentials added at format), `StandardAuthorizer`, `allow.everyone.if.no.acl.found=true`,
`super.users=User:admin;User:ANONYMOUS`, `delegation.token.secret.key` set; doze-kafka twice, one with `--sasl-mechanisms
PLAIN,SCRAM-SHA-256,SCRAM-SHA-512 --sasl-user admin:admin-pw --sasl-user alice:alice-pw --sasl-user bob:bob-pw --acl-super-user User:admin
--delegation-token-secret-key <key>`, one with no SASL; then `cargo run -p kafka-wire --example oracle_admin -- <kafka sasl> <doze sasl> <kafka
plaintext> <doze plaintext>`.

## ADM-10. DescribeTopicPartitions v0 (KIP-966; source, oracle)
`kafka-topics.sh --describe` from Kafka 3.7. No topic named is every topic; a name is asked once; only names at or after the cursor's topic count
(Java's `String` order, by UTF-16 unit). A cursor whose topic is not named, or whose partition is negative, is INVALID_REQUEST for every topic
named. The topics go in name order until `min(response_partition_limit, 2000)` partitions (at least 1) are in: a topic cut short ends the page
with a cursor at its next partition, a topic reached with the page full ends it with a cursor at its partition 0. A missing topic named is
UNKNOWN_TOPIC_OR_PARTITION (INVALID_TOPIC_EXCEPTION for a name no topic could have), takes no room and has a zero id. Each partition: led by this
broker at epoch 0, which is its one replica and in-sync replica, no replica offline, empty ELR and last known ELR (empty lists, not null).
Test: `admin_apis::describe_topic_partitions_pages_in_name_order_and_says_where_to_go_on`

## ADM-11. ElectLeaders v0-2 (KIP-460; source, oracle)
One replica is always the leader and the preferred one, so no election is needed: each partition named is ELECTION_NOT_NEEDED with Kafka's text,
an unknown topic or partition UNKNOWN_TOPIC_OR_PARTITION ("No such topic as t", "No such partition as t-9"); a null list answers every topic with
no partition (Kafka leaves out the partitions that need nothing); an empty list is answered empty; an election type other than 0 or 1 is
INVALID_REQUEST "Unknown election type N" at the top and on every partition named (from v1, which has the field). Nothing changes.
`kafka-leader-election.sh` prints "Valid replica already elected for partitions ..." on both. Test: `admin_apis::no_election_is_ever_needed_on_one_node`

## ADM-12. AlterPartitionReassignments v0-1 and ListPartitionReassignments v0 (KIP-455; source, oracle)
The only replica set a partition can have is this broker, which it has: a target of exactly `[node]` succeeds at once with no record (Kafka's
controller completes a reassignment to the replicas there are immediately), so nothing is ever in progress, a cancellation (null replicas) is
NO_REASSIGNMENT_IN_PROGRESS with no text, and the list is always empty. Refused as the controller refuses (`validateManualPartitionAssignment`, in
its order over the sorted ids): an empty list, a broker that is not this one ("... includes broker 2, but no such broker is registered."), this
broker twice; an unknown topic "Unable to find a topic named t.", partition "Unable to find partition t:7.". An empty request answers the
schema's defaults. `kafka-reassign-partitions.sh --verify` says each is completed. Test:
`admin_apis::a_reassignment_to_this_broker_is_done_at_once_and_any_other_is_refused_in_kafkas_words`

## ADM-13. AlterReplicaLogDirs v0-2 (KIP-113; source, oracle)
The broker has one log directory (ADM-9), and every log is already in it: a move there succeeds and does nothing. In
`ReplicaManager.alterReplicaLogDirs`'s order: a topic name too long for a future log's directory INVALID_TOPIC_EXCEPTION, any other directory
LOG_DIR_NOT_FOUND (compared as the absolute path, exactly), a partition the broker does not have UNKNOWN_TOPIC_OR_PARTITION. A partition named twice
is answered once, for the last directory. Test: `admin_apis::a_log_moves_only_to_where_it_is`

## ADM-14. ListConfigResources v0-1 (KIP-1142; v0 is 3.x's ListClientMetricsResources; source, oracle)
v0 knows CLIENT_METRICS alone, v1 TOPIC, BROKER, BROKER_LOGGER, CLIENT_METRICS and GROUP; none named is all the version knows; a type it does not
know is UNSUPPORTED_VERSION with nothing listed. Listed by type in Kafka's order: the groups with a config set, client-metrics subscriptions (none:
SEC-D100), the broker as a logger, the broker, every topic. Test: `admin_apis::config_resources_are_listed_by_type_and_an_unknown_type_is_unsupported`

## ADM-15. UpdateFeatures v0-2 (KIP-584; source, oracle)
Each update is checked as Kafka 4.4.0's controller checks it, in the order of Kafka's hash map of them, against the features the major supports:
an unknown upgrade type, a level below 0, a level outside the supported range ("Local controller 1 only supports versions 0-2", "... does not
support this feature."), a downgrade asked as an upgrade, an upgrade asked as a downgrade, a `metadata.version` downgrade (always refused from
4.4-IV2, which changed metadata), a `kraft.version` downgrade, an unknown feature, ELR's dependency on `metadata.version`. The first failure fails
the request: INVALID_UPDATE_VERSION "The update failed for all features since the following feature had an error: ...". A request that passes and
changes no level, or validates only, succeeds, with a result per feature ("NONE") before v2. v0 has neither `validate_only` nor an upgrade type
(Kafka 4.4.0 reads every v0 update as an upgrade). Test: `admin_apis::feature_updates_are_checked_as_kafka_checks_them_and_levels_do_not_move`

## ADM-16. The features ApiVersions lists (KIP-584; oracle, `kafka-features.sh describe`)
From v3, every feature the recorded broker of the major supports (`Profile::features`): at Kafka 4 `eligible.leader.replicas.version` 0-1,
`group.version` 0-1, `kraft.version` 0-1, `metadata.version` 7-33 (3.3-IV3 to 4.4-IV2), `share.version` 0-2, `streams.version` 0-1,
`transaction.version` 0-2, each finalized at 4.4.0's default (kraft.version 0, so not listed as finalized); at Kafka 3 3.9.1's `kraft.version` 0-1
and `metadata.version` 1-21; none at Kafka 1 and 2. Below v4 a supported row whose least level is 0 is left out (KAFKA-17492). A broker that does not
serve share or streams groups lists neither feature. The epoch is 0 (Kafka's is its metadata log's offset). Test:
`kafka-wire api_versions::tests::each_major_lists_the_features_of_its_recorded_broker`

## ADM-17. Delegation tokens: CreateDelegationToken v0-3, Renew and Expire v0-2, DescribeDelegationToken v0-3 (KIP-48, KIP-373; source, oracle, tools)
On only with `--delegation-token-secret-key` (or `DOZE_KAFKA_DELEGATION_TOKEN_SECRET_KEY`), and asked for over SASL only (SEC-347). A token's id is
a random Kafka `Uuid` (22 characters, no `-`), its HMAC HMAC-SHA512 of the id under the key. Its maximum is now plus the lifetime asked
(at most `--delegation-token-max-lifetime`, 7 days; the most when none is asked), its expiry now plus `--delegation-token-expiry-time` (1 day),
never past the maximum. A renewal moves the expiry to now plus the period asked (the configured one when none, never more), an expiry to now plus
the period asked, or removes the token at once for a negative period; an expired token is DELEGATION_TOKEN_EXPIRED to a renewal and to an expiry
with a period, not to one that removes it; an unknown HMAC is DELEGATION_TOKEN_NOT_FOUND. Expired tokens stay, are described and can log in until
the sweep every `--delegation-token-expiry-check-interval` (1 hour) removes them, as in Kafka. Tokens are rows of `meta.redb` and outlive a
restart; a new key makes every old token's HMAC another (as in Kafka). A token logs in over SCRAM (SEC-349). Test:
`delegation_tokens::a_token_is_made_with_kafkas_times_and_an_hmac_of_its_id_under_the_key`,
`delegation_tokens::an_expired_token_lives_until_the_sweep_and_tokens_outlive_a_restart`

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
- **ADM-D6.** (Withdrawn 2026-11-19: ListConfigResources is served, ADM-14.) The quota-throttled error of CreatePartitions v3 is: `quotas.md` QT-10.
- **ADM-D9.** UpdateFeatures changes no level. A request Kafka would apply (`transaction.version` down to 1, `group.version` to 0, `kraft.version`
  up to 1, ...) passes every check of ADM-15 and is then refused INVALID_UPDATE_VERSION, saying the levels are the Kafka major's and change with
  `--kafka-version`. Decided: each level is behaviour across the broker (the transaction coordinator, the group protocols, which APIs a client
  meets), fixed per profile; a level that moved at run time would need every one of those to follow it. A dry run (`validate_only`,
  `kafka-features.sh --dry-run`) answers as Kafka does. At Kafka 2 the checks are 4.4.0's controller's, not 2.8's ZooKeeper path.
- **ADM-D10.** Where Kafka's answer comes from a hash map or set (DescribeTopicPartitions' refused topics, ElectLeaders of every partition,
  AlterReplicaLogDirs' topics and partitions, ListConfigResources within a type, DescribeDelegationToken's tokens) this broker answers in name or
  request order. Kafka's internal topics are not listed by DescribeTopicPartitions, ElectLeaders or ListConfigResources (ADM-D8).
- **ADM-D8.** DescribeLogDirs is offered to v4 at Kafka 4: v5 (Kafka 4.2's `IsCordoned`) is not in the wire crate. Kafka's internal
  topics (`__consumer_offsets`, `__transaction_state`) are not listed: this broker keeps offsets and transactions in its metadata
  store, which has no partitions.
- **ADM-D7.** `segment.ms` is the wall-clock age of the active segment since its first write, judged when the next append
  comes (an idle partition keeps its segment until then). Kafka's `LogSegment.timeWaitedForRoll` measures the incoming batch's
  largest timestamp against the first batch's timestamp, and a reopened segment starts from that timestamp, not from the file's
  time. The two differ for backfilled or skewed record timestamps, and for the first roll after a restart. Decided: a clock the
  producers cannot move keeps a topic's segments from growing without bound whatever timestamps arrive; the cost is a roll that
  may come earlier or later than Kafka's for such data. Test: `log::tests::retention::a_segment_older_than_segment_ms_is_rolled_by_the_next_append_and_an_idle_one_is_left_alone`.
