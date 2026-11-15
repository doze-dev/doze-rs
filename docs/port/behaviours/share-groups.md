# Share groups (KIP-932, "Queues for Kafka")

What Apache Kafka 4.4.0 does with share groups, and the test in this repository that holds each rule. There is no
recording: every rule is read from the 4.4.0 source (the release's `kafka-4.4.0-src.tgz`; paths below are relative to it,
KA = `core/src/main/scala/kafka/server/KafkaApis.scala`, SP = `core/src/main/java/kafka/server/share/SharePartition.java`,
SPM = `.../share/SharePartitionManager.java`, GMM = `group-coordinator/.../GroupMetadataManager.java`) and held to a real
Kafka 4.4.0 (KRaft, one node in combined mode, formatted with its defaults, so `share.version` 2): the Java scenarios of
`interop/java/Share.java` pass against both brokers unchanged, and Kafka's own tools (`kafka-share-groups.sh`,
`kafka-console-share-consumer.sh`, `kafka-configs.sh --entity-type groups`) print the same against both (see "The oracle").

The parts: the share-partition, a pure state machine (`crates/kafka-broker/src/share/partition{,/acquire,/ack,/lapse}.rs`);
the share sessions (`share/sessions.rs`); the broker's side of them (`share/load.rs`: loading, storing, initializing) and
the lock task (`share/locks.rs`); the group (`groups/share.rs`, its assignor `groups/share/assign.rs`); the seven handlers
(`handlers/share_*.rs`); the stored state (`crates/kafka-meta/src/{share.rs,store/shares.rs}`) and the group configs
(`group_configs.rs`, `kafka-meta/src/store/group_configs.rs`).

## Versions and features

### SG-1. Seven APIs, at Kafka 4.4.0's versions, in the Kafka 4 profile only
ShareGroupHeartbeat (76) v1, ShareGroupDescribe (77) v1, ShareFetch (78) v1-2, ShareAcknowledge (79) v1-2,
DescribeShareGroupOffsets (90) v0-1, AlterShareGroupOffsets (91) v0, DeleteShareGroupOffsets (92) v0. Version 0 of 76-79
was 4.0's early access and is gone. Kafka 1 to 3 offer none. The share coordinator's internal APIs (83-87) are not served:
the state they carry is kept in the metadata database. The vendored `kafka-protocol` was patched to 4.4.0's ShareFetch,
ShareAcknowledge and DescribeShareGroupOffsets versions (`vendor/kafka-protocol/PATCHES.md`, patch 3).
Test: `kafka-wire profile::tests::only_kafka_4_offers_share_groups_at_kafka_4_4_versions`, `tests/share_versions.rs`,
`tests/every_offered_version.rs`, `tests/recorded_api_versions.rs` (no wider than the recorded 4.3.1).

### SG-2. `share.version` 2 is finalized
A 4.4.0 formatted with its defaults finalizes `share.version` 2 (`ShareVersion.SV_2`: share groups, level 1, and their
dead-letter queue, level 2). ApiVersions lists it as it lists `transaction.version`: supported 0 to 2 from v4, finalized
2..2 from v3. Share groups are on; `group.share.enable` no longer exists in 4.4.0 (E3).
Test: `kafka-wire api_versions::tests::kafka_4_finalizes_transaction_version_2_and_the_older_majors_list_no_feature`.

## The handlers' checks (KA)

### SG-3. ShareGroupHeartbeat: READ on the group, a member id, DESCRIBE on the topics, then the request's words
In order: READ on GROUP (30); a member id present, not empty and at most 36 characters (42); DESCRIBE on every subscribed
topic (29). These are code only, no message (Kafka's `getErrorResponse`). Then the group coordinator's checks, with words:
"MemberId can't be empty." (blank), "GroupId can't be empty.", "RackId can't be empty.", "SubscribedTopicNames must be set
in first request." (epoch 0), "MemberEpoch is invalid." (below -1).
Test: `tests/share_groups.rs::the_heartbeats_own_checks_are_kafkas_in_its_order_and_words`,
`a_request_without_a_usable_member_id_is_refused_whole`.

### SG-4. ShareFetch and ShareAcknowledge: the request-wide checks
In order: a group id (null is 42), READ on the group (30), a member id as SG-3 (42), and for ShareFetch v2 with IsRenewAck
MaxBytes, MinBytes, MaxRecords and MaxWaitMs all 0 (42). Code only. A ShareFetch with MaxRecords or MaxBytes 0 fetches
nothing and answers at once (what a client sends to carry acknowledgements to another broker). A ShareAcquireMode other
than 0 or 1 is UNKNOWN_SERVER_ERROR once there is something to fetch.
Test: `tests/share_groups.rs::a_request_without_a_usable_member_id_is_refused_whole`.

### SG-5. A share-partition the group never initialized cannot be fetched
Kafka's share coordinator refuses to read one ("Read operation on uninitialized share partition not allowed."), which the
fetch answers as UNKNOWN_SERVER_ERROR on the partition. The share fetch is not checked against the group's membership.
Test: `tests/share_groups.rs::a_share_partition_never_initialized_cannot_be_fetched`.

## Share sessions (B)

### SG-10. Epochs: 0 opens, -1 closes, every other request carries the next
A ShareFetch at epoch 0 opens a session for (group, member), replacing any it had (without letting go of what it holds),
and may carry no acknowledgement (42). The next request carries 1, then 2; after `i32::MAX` comes 1. A session not there is
SHARE_SESSION_NOT_FOUND (122), a wrong epoch INVALID_SHARE_SESSION_EPOCH (123), both top-level with no partitions. A
ShareAcknowledge moves the same epoch but never opens a session: at epoch 0 it is 123.
Test: `share::sessions::tests::a_session_opens_at_zero_and_each_request_must_carry_the_next_epoch`,
`epochs_count_from_one_and_wrap_to_one`, `tests/share_groups.rs::the_share_session_is_opened_at_zero_and_each_request_must_carry_its_next_epoch`.

### SG-11. The cache holds 2000 sessions and makes room for nobody
`group.share.max.share.sessions` (2000): a new session past it is SHARE_SESSION_LIMIT_REACHED (133), answered after the
request's MaxWaitMs as Kafka's idle timer does; a member replacing its own session needs no room.
Test: `share::sessions::tests::the_cache_is_full_at_its_limit_but_a_member_may_replace_its_own_session`.

### SG-12. An incremental fetch reads the whole session and reports only what has something to say
A continued session fetches every partition in it (the request's are added, its forgotten ones removed). The answer has a
partition only if it has records, an error (reported once more after the error clears), or is new to the session and not
yet reported; one with records goes to the back of the order. The order of a fetch is turned by the epoch. An epoch 0 fetch
reports every partition read. Every acknowledged partition is in the answer, with its acknowledgement's code.
Test: `share::sessions::tests::an_incremental_response_reports_new_partitions_once_and_errors_until_they_clear`,
`tests/share_groups.rs::acknowledgements_ride_on_the_next_fetch_and_the_start_moves_past_what_is_done`.

### SG-13. A session ends when it is closed, replaced, or its connection goes
Epoch -1 (on a ShareFetch, which then fetches nothing, or a ShareAcknowledge) closes it once the acknowledgements are done,
and what the member held in its partitions is let go of (available again, or archived at the delivery limit). A
connection that goes closes the sessions it made the same way; a group left with no session on the broker has its
share-partitions let go of from memory (read again from the store when next fetched). The group's own member expiry does
not touch sessions or what they hold.
Test: `tests/share_groups.rs::closing_the_session_lets_go_of_what_the_member_held_and_another_member_gets_it_again`,
`a_connection_that_goes_closes_its_sessions_and_lets_go_of_what_they_held`.

### SG-14. Acknowledgement batches are checked before anything else (KA `validateAcknowledgementBatches`)
On each partition: a batch with first after last, one beginning before the previous one's last, one with no type, one with
several types that are not one per offset, a type below 0 or above 3 (4 at version 2), or RENEW (4) in a request without
IsRenewAck: INVALID_REQUEST on the partition, and none of its batches is taken. Then: an unknown topic id UNKNOWN_TOPIC_ID,
READ on the topic (29), the partition (3), and a share-partition not in memory (3). A partition with no batches is answered
only if another went as far as its share-partition.
Test: `handlers::share_acks::tests::batches_are_held_to_kafkas_shape`,
`tests/share_groups.rs::an_acknowledgement_of_what_the_member_does_not_hold_is_refused_on_its_partition`.

## The group (D)

### SG-20. Epochs
A new share group is at epoch 0. A member joining, a subscription that changes, and the subscribed topics changing (made,
deleted, grown) move the group epoch by one; a member leaving or expiring moves it too, and when the last goes the
assignment epoch is set to it. A member is at the epoch of the targets it was last given.
Test: `groups::share::tests::a_member_joins_at_the_first_epoch_with_every_partition_and_is_told_only_changes`,
`leaving_and_a_silent_member_move_the_epoch_and_the_last_one_leaves_the_targets_at_it`,
`a_topic_that_grows_moves_the_epoch_at_the_next_heartbeat_and_the_new_partition_is_given_out`.

### SG-21. Targets are computed by a heartbeat at least the assignment interval after the last computation
`share.assignment.interval.ms` (1000): until then the targets of the last epoch stand, and a member that joined meanwhile has
none. The first computation of a group runs whenever it is asked. There is nothing to revoke: a member holds its target as
soon as it is told it, filtered to the topics it subscribes to.
Test: `groups::share::tests::a_second_member_shares_after_the_assignment_interval_and_partitions_are_spread`,
`a_subscription_that_drops_a_topic_drops_its_partitions_at_once`.

### SG-22. The simple assignor
Each partition goes to `ceil(M/P)` members; member i gets `ceil(x(i+1)) - ceil(xi)` of them where `x = sharing * P / M`.
Kafka's steps: what is no longer subscribed is taken back, an over-filled member and an over-shared partition give up the
excess, and the rest is dealt to members with room. A member keeps what it had where it can. Members of different topics are
assigned topic by topic. P=3, M=2 is 2 and 1; P=1, M=3 all share; P=2, M=3 is 2/1/1 with each partition shared by two.
Test: `groups::share::assign::tests::*`.

### SG-23. The epoch checks of a known member
Epoch 0 rejoins (the member keeps its epoch and assignment and is told it whole). Its epoch, or the one before it (an
answer lost), go on. A greater one, or a smaller one that is not the one before, is FENCED_MEMBER_EPOCH (110) in Kafka's
words. An unknown member at another epoch is UNKNOWN_MEMBER_ID "Member <m> is not a member of group <g>.". A group that is
not there: GROUP_ID_NOT_FOUND "Share group <g> not found." (a leave: "Group <g> not found.").
Test: `groups::share::tests::the_checks_of_a_known_members_epoch_are_kafkas`,
`a_smaller_epoch_that_is_not_the_previous_one_is_fenced`, `tests/share_groups.rs::the_heartbeats_own_checks_are_kafkas_in_its_order_and_words`.

### SG-24. A share group holds 200 members
`group.share.max.size`: a new member past it is GROUP_MAX_SIZE_REACHED (81) "The share group has reached its maximum
capacity of 200 members."; a member already in it is not refused.
Test: `groups::share::tests::the_group_is_full_at_its_maximum_size_but_a_member_already_in_it_is_not_refused`.

### SG-25. The answer: epoch, interval, and the assignment when it changed
`share.heartbeat.interval.ms` (5000). The assignment is sent when the member joins (epoch 0), says what it subscribes to, or
holds something new; else null. A leave is answered with epoch -1 and no interval. A topic missing from the metadata stays
in the subscription and gives nothing.
Test: `groups::share::tests::a_member_joins_at_the_first_epoch_with_every_partition_and_is_told_only_changes`,
`a_member_reading_a_topic_that_is_not_there_is_given_nothing_of_it`.

### SG-26. A share group and the other kinds refuse each other
A share heartbeat to a classic or consumer group (one with committed offsets, or a record of members) is GROUP_ID_NOT_FOUND
"Group <g> is not a share group."; a JoinGroup to a share group is INCONSISTENT_GROUP_PROTOCOL; a ConsumerGroupHeartbeat is
GROUP_ID_NOT_FOUND "Group <g> is not a consumer group."; an OffsetCommit GROUP_ID_NOT_FOUND. ListGroups lists it with type
and protocol type `share`, state Empty or Stable (the type filter matches without regard to case: the Java Admin sends
"Share"). DescribeGroups (to v5) shows it Dead. Nothing converts a share group.
Test: `tests/share_groups.rs::a_share_group_and_the_other_kinds_refuse_each_others_requests`.

### SG-27. ShareGroupDescribe
Per group: DESCRIBE on it or 30 (no words, ahead of the rest); "Group <g> not found."; "Group <g> is not a share group.";
else its state, epochs, assignor `simple`, and each member with its epoch, client id and host, rack, subscription and
assignment by topic id and name (a deleted topic left out). A member holding a topic the caller may not DESCRIBE replaces
the group by 29 "The group has described topic(s) that the client is not authorized to describe.".
Test: `tests/share_crash.rs::a_share_groups_epochs_and_configs_outlive_a_restart`, interop `share-admin`, `share-one-partition`.

### SG-28. DeleteGroups of a share group
With members: NON_EMPTY_GROUP. Empty: gone, with every share-partition state it had.
Test: `tests/share_groups.rs::deleting_a_share_group_needs_it_empty_and_takes_its_share_partitions_with_it`.

### SG-29. The share-partitions of what the group reads are initialized
When a member joins or the epoch moves, every partition of the subscribed topics that has no stored state gets one: a topic
the group has never read with no start (-1: set by `share.auto.offset.reset` at its first fetch), a new partition of a topic
it reads at 0. Until a state is written by an acknowledgement, DescribeShareGroupOffsets shows no start and no lag.
Test: `tests/share_groups.rs::a_member_joins_reads_every_partition_and_its_partitions_are_initialized_with_no_start`.

## The share-partition (C)

### SG-30. States and their moves
Available, Acquired, Acknowledged, Archived (Kafka's ids 0, 1, 2, 4; its ARCHIVING is the dead-letter queue's, SG-D6). Only
available becomes acquired; only acquired becomes anything else; acknowledged and archived are final. A record made
available after its delivery count reached `share.delivery.count.limit` is archived instead.
Test: `share::partition::tests::the_state_machine_moves_a_record_only_as_kafka_does`.

### SG-31. Whole batches are acquired, cut at the client's batch size
New records are acquired from the first offset read to the end of the last log batch read, as ranges of at least BatchSize
cut where a log batch begins; MaxRecords is soft (the batch holding the last record asked for is taken whole). In
record-limit mode (ShareFetch v2, ShareAcquireMode 1) exactly MaxRecords, the batch split. The response carries whole log
batches from the one holding the first acquired offset; offsets acquired with no record (control records, compacted ones)
come back from the client as gaps.
Test: `share::partition::tests::new_records_are_acquired_whole_batch_by_batch_and_cut_at_the_batch_size`,
`the_record_limit_is_soft_in_batches_and_exact_in_record_limit_mode`.

### SG-32. The start offset (SPSO) moves past what is done with
After a change is stored, the start moves past the unbroken run of acknowledged and archived records; batches wholly behind
it are let go of. A released record holds it.
Test: `share::partition::tests::an_accepted_run_moves_the_start_and_lets_go_of_whole_batches`.

### SG-33. Acknowledge types
Accept: acknowledged. Release: available again (the delivery count is kept; it rises on the next acquisition). Reject and
gap: archived. Renew (4, KIP-1222): the lock starts again, nothing is stored, unless the group's
`share.renew.acknowledge.enable` is false ("Renewing acquisition locks is not enabled for the group.").
Test: `share::partition::tests::a_release_redelivers_with_one_more_delivery_and_a_reject_never_again`,
`per_offset_types_and_gaps_split_a_batch`, `renewing_restarts_the_lock_and_moves_nothing_else`.

### SG-34. What is not held is not acknowledged, in Kafka's words, and nothing of the request is taken
INVALID_RECORD_STATE (121) for a batch or offset not acquired, or held by another member ("Member is not the owner of batch
record", "...of offset", "The offset cannot be acknowledged. The offset is not acquired.", "Batch record not found. The
request batch offsets are not found in the cache."); INVALID_REQUEST for offsets past what was acquired ("Batch record not
found. The first offset in request is past acquired records." / "...last offset...") or an unknown type ("Invalid acknowledge
type: [9]"). One failing batch fails the partition's whole acknowledgement. Batches behind the start are skipped.
Test: `share::partition::tests::acknowledging_what_is_not_held_is_refused_in_kafkas_words_and_changes_nothing`.

### SG-35. The delivery count limit
`share.delivery.count.limit` (5, 2 to 10): a record released, or whose lock runs out, after that many deliveries is archived.
Test: `share::partition::tests::a_record_released_after_its_last_delivery_is_archived`,
`a_lock_that_runs_out_after_the_last_delivery_archives`, interop `share-delivery-limit`.

### SG-36. A record delivered often is delivered with fewer others, and on its last delivery alone
From a delivery count of half the limit (at least 2) a batch is acquired per offset: `size >> (count - half + 1)` records
at most, and no record after one acquired; a record whose count is one short of the limit goes alone (Kafka's throttling,
which is in the code and not in the KIP).
Test: `share::partition::tests::a_record_delivered_often_is_delivered_with_fewer_others_and_on_its_last_delivery_alone`.

### SG-37. The in-flight limit
`share.partition.max.record.locks` (2000): records from the start to the end are in flight; past the limit nothing new is
acquired, but records made available inside the span are.
Test: `share::partition::tests::acquisition_stops_at_the_in_flight_limit_but_released_records_inside_it_are_taken_again`.

### SG-38. `read_committed`
`share.isolation.level=read_committed`: reads stop at the last stable offset, and an aborted transaction's records are
archived when acquired and left out of what the member is told; the transaction markers are acquired and come back as gaps.
Test: `share::partition::tests::under_read_committed_an_aborted_transactions_records_are_archived_not_delivered`,
`tests/share_groups.rs::under_read_committed_an_aborted_transaction_is_archived_and_not_delivered`, interop `share-read-committed`.

### SG-39. Where a share-partition starts
`share.auto.offset.reset`: latest (the default, the log's end), earliest (its start), or `by_duration:<ISO-8601>` (the first
record no older). Evaluated at the share-partition's first fetch.
Test: `tests/share_groups.rs::a_share_partition_starts_at_the_end_by_default_and_at_the_start_when_the_group_says_earliest`,
`group_configs::tests::the_reset_strategy_takes_kafkas_three_forms`.

### SG-40. A log start past the start offset
Records deleted under the share-partition (retention, DeleteRecords) move its start: what is available before the log's new
start is archived, the fetch of that round answers nothing, and the next reads from the log's start.
Test: `share::partition::tests::a_log_start_past_the_start_archives_what_is_available_before_it`.

### SG-41. Acquisition locks run out on time and nothing wakes otherwise
`share.record.lock.duration.ms` (30000, 15000 to 60000), told as AcquisitionLockTimeoutMs. A lock that runs out makes its
records available again (archived at the limit or behind the start), the count unchanged, and the change is stored. One task
for the broker sleeps until the soonest lock is due (the alarm every acquisition and renewal sets); with nothing held it has
no timer at all.
Test: `share::partition::tests::a_lock_that_runs_out_makes_the_records_available_again_at_the_same_delivery_count`,
`tests/idle_upkeep.rs::the_share_lock_task_does_not_wake_while_nothing_is_held_and_wakes_once_when_a_lock_runs_out`,
interop `share-lock-timeout`.

### SG-42. A waiting ShareFetch
With nothing to acquire it waits up to MaxWaitMs for a partition to grow, a record to be made available, a start to move or
a topic to change, and makes one last look at the end.
Test: interop `share-implicit`, `share-one-partition` (members waiting on records that arrive).

## Durability

### SG-50. What is stored, and when
On every change Kafka writes (an acknowledgement, a release, a lock that runs out): the state epoch, the start offset as it
was before the write's own move (Kafka's lag of one write), and every record from it that is not simply waiting for its
first delivery. A record acquired is stored as it was before its acquisition: Kafka writes no acquisition, so a lock does
not outlive the broker and a delivery never acknowledged is not counted after a restart. One write per partition, flushed
before the request is answered.
Test: `share::partition::tests::what_is_stored_is_the_start_and_every_record_not_simply_waiting_with_a_held_one_as_before_its_acquisition`,
`kafka-meta store::shares::tests::*`.

### SG-51. A restart keeps what was acknowledged and archived, and lets go of every lock
After a kill or a power cut: acknowledged and archived records are never delivered again, released ones come back with the
deliveries they had, acquired ones as if never delivered. A stop at any point of an acknowledgement leaves it stored or not,
and stored if it was answered.
Test: `tests/share_crash.rs::an_acknowledgement_survives_a_restart_and_a_lock_does_not`,
`a_stop_at_any_point_of_an_acknowledgement_leaves_it_done_or_not_and_done_if_it_was_answered`.

### SG-52. A failed write changes nothing
The acknowledgement is answered with the store's error (KAFKA_STORAGE_ERROR) and the share-partition is as it was.
Test: `tests/share_crash.rs::a_failed_write_answers_the_acknowledgement_with_an_error_and_changes_nothing`.

### SG-53. A share group's epochs and configs outlive a restart; its states go with its topic
The group record keeps its epochs (version 3 of the record, reading versions 1 and 2), the group configs their own table; a
deleted topic's share states go in the transaction that deletes it, and a topic made again under its name starts afresh.
Test: `tests/share_crash.rs::a_share_groups_epochs_and_configs_outlive_a_restart`,
`kafka-meta life::compatibility::a_record_the_second_version_wrote_keeps_its_consumer_group_and_is_no_share_group`,
`store::shares::tests::a_deleted_topic_takes_its_share_states_with_it`.

## The admin APIs (A31-A43, D17-D25)

### SG-60. DescribeShareGroupOffsets
Per group: DESCRIBE on it (30). Topics null: every initialized partition the caller may DESCRIBE (the rest left out). Named:
each partition's stored start (-1 if none) and, from v1, lag = log end - start - records done with (-1 where either is not
known); a topic missing is all -1; a topic the caller may not DESCRIBE is 29 "Topic authorization failed." on each partition,
after the others.
Test: `tests/share_groups.rs::acknowledgements_ride_on_the_next_fetch_and_the_start_moves_past_what_is_done`,
`handlers::share_group_offsets::tests::what_is_done_with_counts_from_the_start_and_an_unstarted_partition_counts_nothing`.

### SG-61. AlterShareGroupOffsets
READ on the group (30, "Group authorization failed."). Another kind of group: GROUP_ID_NOT_FOUND "Group <g> is not a share
group."; members: NON_EMPTY_GROUP "The group is not empty." with no partitions. Per topic: READ (29), not there (3), a
partition past its count (3); the rest stored with the start given and nothing in flight. A group not there is made, empty.
Test: `tests/share_groups.rs::offsets_are_reset_only_while_the_group_is_empty_and_a_group_not_there_is_made`, interop `share-admin`.

### SG-62. DeleteShareGroupOffsets
DELETE on the group (30). Not there: GROUP_ID_NOT_FOUND "Group <g> not found."; another kind or members as SG-61. Per topic:
READ (29, first in the answer), not there (3), no state "There is no offset information to delete." (3), else its states
deleted; the next heartbeat initializes it again with no start.
Test: `tests/share_groups.rs::deleting_offsets_is_kafkas_per_topic_and_the_next_member_starts_by_the_reset_strategy`.

## Group configs (E7-E11)

### SG-70. The share group configs, on resource type GROUP
`share.session.timeout.ms`, `share.heartbeat.interval.ms`, `share.record.lock.duration.ms`, `share.delivery.count.limit`,
`share.partition.max.record.locks`, `share.auto.offset.reset`, `share.isolation.level`, `share.renew.acknowledge.enable`,
`share.assignment.interval.ms`: set by IncrementalAlterConfigs (ALTER_CONFIGS on the group, 30), described by DescribeConfigs
(DESCRIBE_CONFIGS) with source GROUP_CONFIG (8) or DEFAULT_CONFIG (5) and the broker synonym. The group need not exist.
Refusals are INVALID_CONFIG in Kafka's words: by kind ("Not a number of type INT", "Value must be at least ...", "String must
be one of: read_committed, read_uncommitted"), against the broker's bounds ("<key> must be in the range <min> to <max>
inclusive."), "Unknown group config name: <name>", "share.session.timeout.ms must be greater than
share.heartbeat.interval.ms"; an empty name INVALID_REQUEST "Default group resources are not allowed." / "Group name must not
be empty".
Test: `group_configs::tests::*`, `tests/share_groups.rs::group_configs_are_described_with_their_source_and_refused_in_kafkas_words`.

## Idle cost

### SG-80. Nothing until a share group fetches
No share-partition, no session, no timer until one is used: the map and the cache are empty and the lock task waits on an
alarm nothing has set (SG-41). A share group with nothing held costs no wake-up.
Test: `tests/idle_upkeep.rs::the_share_lock_task_does_not_wake_while_nothing_is_held_and_wakes_once_when_a_lock_runs_out`.

## How the rules are held

Rule by rule: the share-partition, the session cache, the assignor and the group configs are pure code with a clock passed in,
tested with a fake clock in `share/partition/tests.rs`, `share/sessions.rs`, `groups/share/tests.rs`,
`groups/share/assign.rs` and `group_configs.rs`; the handlers end to end in `tests/share_groups.rs`; ACLs in
`tests/authz_share.rs`; every offered version in `kafka-wire/tests/every_offered_version.rs` and `share_versions.rs`.
Crashes: `tests/share_crash.rs` stops the simulated disk after each operation of an acknowledgement in turn, then kills the
process (the disk keeps what was written) or cuts the power (it keeps what was flushed), and checks after each restart that the
acknowledgement is done or not, done if it was answered, and that nothing is held; a failed write answers with an error and
changes nothing; a share group's epochs and configs outlive a restart.

A hand mutation check (2026-11-15): each change made alone, the unit tests of its part run, the file put back. All seven were
caught.

| Mutation | Caught by |
|---|---|
| the delivery limit off by one (`>=` to `>`, SG-35) | `a_record_released_after_its_last_delivery_is_archived` and two more |
| a lock runs out after its deadline, not at it (SG-41) | `a_lock_that_runs_out_makes_the_records_available_again_at_the_same_delivery_count` |
| the throttle limit rounded down (SG-36) | `a_stored_state_comes_back_with_nothing_held_and_its_gaps_fetched_first` |
| RENEW (4) not an acknowledge type (SG-33) | `renewing_restarts_the_lock_and_moves_nothing_else` |
| the session cache takes one more than its limit (SG-11) | `the_cache_is_full_at_its_limit_but_a_member_may_replace_its_own_session` |
| a group config's upper bound left out of its range (SG-70) | `a_session_must_be_longer_than_the_heartbeat` |
| the assignor's sharing factor rounded down (SG-22) | `partitions_are_spread_and_shared_when_there_are_more_members` |

## The oracle

Kafka 4.4.0 (`kafka_2.13-4.4.0.tgz`, SHA-512 checked) on 127.0.0.1:39500, KRaft combined mode, `share.coordinator.state.topic.*`
at one partition and replica, formatted with the defaults. `interop/java/Share.java` (seven scenarios: implicit and explicit
acknowledgement, release and reject, a lock running out and the records going to another member, the delivery limit,
three members sharing one partition, `read_committed`, and the admin calls) passes against it and against this broker.
Kafka's tools against both, a topic of two partitions, ten records, a group set to `earliest`, `kafka-console-share-consumer.sh`
reading them, then `kafka-share-groups.sh --describe` (`--offsets`, `--state --verbose`), `--list`, and `kafka-configs.sh
--describe --entity-type groups`: the same lines but for SG-D1's epochs (Kafka 4/4, here 2/2) and which partition the
records went to (the producer's partitioner).

## Deviations

- **SG-D1.** A share-partition is initialized when the heartbeat that asks for it is answered, not by the coordinator's
  later InitializeShareGroupState (one broker, no round trip): a member's first assignment already has the partitions
  (Kafka's is usually empty until the next heartbeat), and the group epoch moves fewer times (a join and a leave: 2/2, Kafka 4/4).
- **SG-D2.** The simple assignor takes members in the order of their ids; Kafka's hash order decides which member gets the
  larger share when they do not divide evenly.
- **SG-D3.** A waiting ShareFetch answers as soon as anything is acquired; Kafka waits for MinBytes to be available (the share
  consumer sends 1). MaxWaitMs is held to 30 s, as Fetch's.
- **SG-D4.** `by_duration` with no record young enough starts at the log's end; Kafka fails the share-partition with
  OFFSET_NOT_AVAILABLE until a newer record comes.
- **SG-D5.** Only the share group configs are kept. The `consumer.*`, `streams.*` and dead-letter-queue group configs are
  refused by name (INVALID_CONFIG "Group config <name> is not supported by this broker: it keeps the share.* group configs
  only."); the broker's own `group.share.*` configs are Kafka's defaults and not settable.
- **SG-D6.** No dead-letter queue (KIP-1191), though `share.version` 2 is finalized: there is no ARCHIVING state and the
  `errors.deadletterqueue.*` configs are refused (SG-D5).
- **SG-D7.** What is stored is the whole state of the share-partition at each write, not Kafka's changed ranges merged by the
  share coordinator: the same records come back after a restart. Records an aborted transaction wrote, archived in memory, are
  stored with the next write (Kafka stores them never and archives them again when they are read again).
- **SG-D8.** Two ShareFetches on one share-partition at once take turns; Kafka leaves the partition out of the second.
- **SG-D9.** One broker: the leader epoch is always 0, there is no NOT_LEADER_OR_FOLLOWER, CurrentLeader is left at its
  default and NodeEndpoints empty.
