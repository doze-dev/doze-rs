# Streams groups (KIP-1071, the streams rebalance protocol)

What Apache Kafka 4.4.0 does with streams groups, and the test in this repository that holds each rule. Every rule is read from
the 4.4.0 source (the release's `kafka-4.4.0-src.tgz`; paths below are relative to it: KA = `core/src/main/scala/kafka/server/
KafkaApis.scala`, GCS = `group-coordinator/src/main/java/org/apache/kafka/coordinator/group/GroupCoordinatorService.java`, GMM
= `.../group/GroupMetadataManager.java`, SG = `.../group/streams/StreamsGroup.java`, CAB = `.../streams/CurrentAssignmentBuilder
.java`, ITM = `.../streams/topics/InternalTopicManager.java`, STA = `.../streams/assignor/StickyTaskAssignor.java`, GC = `.../group/
GroupConfig.java`) and held to a real Kafka 4.4.0 (KRaft, one node in combined mode, formatted with its defaults): the Kafka
Streams 4.4.0 scenarios of `interop/java/Streams.java` pass against both brokers unchanged, their heartbeat answers were compared
answer by answer, and `kafka-streams-groups.sh` prints the same against both (see "The oracle").

The parts: the group (`crates/kafka-broker/src/groups/streams.rs` and `streams/`: `heartbeat.rs` the heartbeat's steps, `seat.rs`
who it is from and leaving, `topology.rs` the topology against the topics, `assign.rs` the sticky assignor, `reconcile.rs` a
member's way to its target, `steps.rs` targets and timers, `fence.rs` offsets, `view.rs`, `endpoints.rs`, `stored.rs`); the
handlers (`handlers/streams_group_heartbeat.rs` and its `wire.rs` and `create.rs`, `handlers/streams_group_describe.rs`); the
group configs (`group_configs/streams.rs`, `group_configs/defs.rs`); the stored record (`crates/kafka-meta/src/life.rs`, version
4); the wire (`vendor/kafka-protocol/PATCHES.md` patch 4, `crates/kafka-wire/src/api.rs`).

## Versions and features

### STR-1. Three APIs, at Kafka 4.4.0's versions, in the Kafka 4 profile only
StreamsGroupHeartbeat (88) v0-1, StreamsGroupDescribe (89) v0-1, StreamsGroupTopologyDescriptionUpdate (93) v0, as a 4.4.0
broker advertises them whatever its feature levels (`ApiKeys`, no `latestVersionUnstable`). Version 1 of the heartbeat answer
carries `AcceptableRecoveryLag` (int64) and `TopologyDescriptionRequired`; version 0 the int32 `AcceptableRecoveryLagLegacy`, which
Kafka never sets (0). Version 1 of describe asks for the topology description (answered NOT_STORED, 1) and names the assignor.
Kafka 1 to 3 offer none. The major-4 recording of `crates/kafka-wire/tests/data/api-versions.json` is 4.4.0's.
Test: `kafka-wire profile::tests::only_kafka_4_offers_streams_groups_at_kafka_4_4_versions`, `tests/streams_versions.rs`,
`tests/every_offered_version.rs`, `tests/recorded_api_versions.rs`; `kafka-broker tests/streams_groups.rs::version_zero_answers_carry_no_long_recovery_lag_and_version_one_answers_do`.

### STR-2. `streams.version` 1 is finalized
A 4.4.0 formatted with its defaults finalizes `streams.version` 1 (`StreamsVersion.SV_1`; the default metadata version is past its
bootstrap version 4.2-IV1). ApiVersions lists it as `share.version`: supported 0 to 1 from v4, finalized 1..1 from v3. Streams
groups are on; there is no switch (`group.coordinator.rebalance.protocols` includes `streams` by default).
Test: `kafka-wire api_versions::tests::kafka_4_finalizes_transaction_version_2_and_the_older_majors_list_no_feature`.

### STR-3. The topology description update is refused as by a broker without the plugin
`group.streams.topology.description.plugin.class` is unset by default, so 93 answers UNSUPPORTED_VERSION "The broker has no streams
group topology description plugin configured." (GCS L810-820) after READ on the group, and the heartbeat's
`TopologyDescriptionRequired` is always false.
Test: `kafka-broker tests/streams_groups.rs::the_topology_description_update_is_refused_as_by_a_broker_with_no_plugin`.

## The heartbeat's checks (KA, GCS)

### STR-4. READ on the group, then the topology's names, then DESCRIBE on them
READ on the group (30, no words). On a join, the topics the topology names (sources, repartition sinks and sources, changelogs):
any of Kafka's own is STREAMS_INVALID_TOPOLOGY "Use of Kafka internal topics a,b in a Kafka Streams topology is prohibited."; any
invalid name "Topic names a,b are not valid topic names."; then DESCRIBE on each (29, no words) (KA L2816-2861). SEC-330.
Test: `kafka-broker tests/streams_groups.rs::the_heartbeats_own_checks_are_kafkas_in_its_order_and_words`,
`tests/authz_streams.rs::the_streams_heartbeat_needs_read_on_the_group_and_describe_on_each_topic_of_the_topology`.

### STR-5. The request's own checks, in Kafka's order and words (INVALID_REQUEST)
"Regular expressions for source topics are not supported yet." (a non-empty `SourceTopicRegex`); "MemberId can't be empty.",
"GroupId can't be empty.", "InstanceId can't be empty.", "RackId can't be empty." (blank after trimming); on a join
"RebalanceTimeoutMs must be provided in first request.", "ActiveTasks must be empty when (re-)joining." (and Standby, Warmup: each
a non-null empty list), "Topology must be non-null when (re-)joining.", and a changelog with a partition count is
STREAMS_INVALID_TOPOLOGY "Changelog topic %s must have an undefined partition count, but it is set to %d."; epoch -2 without an
instance "InstanceId can't be null."; below -2 "MemberEpoch is %d, but must be greater than or equal to -2."; "If one task-type is
non-null, all must be non-null."; "Topology can only be provided when (re-)joining." (GCS L553-627).
Test: as STR-4.

### STR-6. The group: made by a join, refused by kind
A join (epoch 0) makes the group, or makes an empty classic group (one known only by its offsets) a streams group. Any other kind
refuses it: GROUP_ID_NOT_FOUND "Group G is not a streams group." A heartbeat that is not a join to a group not there is "Streams
group G not found.", a leave "Group G not found." (GMM L903-982). A streams group refuses the other kinds' requests as a share
group does (a classic join INCONSISTENT_GROUP_PROTOCOL; "Group G is not a consumer group." / "... share group.").
Test: `kafka-broker tests/streams_groups.rs::an_unknown_member_and_a_group_of_another_kind_are_refused_in_kafkas_words`,
`a_streams_group_is_listed_as_one_and_not_described_by_the_classic_api`.

### STR-7. The group is full at `group.streams.max.size`
A join of a member not in the group, when the group has the most members, is GROUP_MAX_SIZE_REACHED "The streams group has reached
its maximum capacity of N members." A static member taking its instance's place is not counted (GMM L1827-1846). The broker's
default is `Integer.MAX_VALUE`.
Test: `kafka-broker groups::streams::tests::members::the_group_is_full_at_its_maximum_size`.

## Epochs and the topology (GMM `streamsGroupHeartbeat`)

### STR-10. A new group is at epoch 1, its targets too
Kafka's `StreamsGroup` starts at group epoch 1 and `TargetAssignmentMetadata.INITIAL` at 1, so the first member's join takes the
group to 2 and a member waiting out the initial delay is at 1 (recorded from the oracle: answers at 1, 1, then 3).
Test: `groups::streams::tests::epochs::the_first_member_waits_out_the_initial_delay_at_epoch_one_and_is_given_its_tasks_at_three`.

### STR-11. What moves the group epoch
One bump per heartbeat for any of: a member joining; a member's metadata changing (a dynamic member: instance, rack, client id
and host, rebalance timeout, topology epoch, process, endpoint, client tags; a static member: topology epoch, rack, tags or
process only); the topics the topology names changing (made, deleted, grown: Kafka's metadata hash); the validated topology epoch
changing (the topology becoming ready, or not); the assignment configs (`streams.num.standby.replicas`, the rack tags) differing
from those last used; a member leaving or fenced (GMM L2166-2267, L5091).
Test: `groups::streams::tests::epochs::changing_the_standby_replicas_moves_the_epoch_at_the_next_heartbeat`,
`a_topic_that_grows_moves_the_epoch_and_the_tasks_follow`, `tests::members::leaving_moves_the_epoch_and_the_last_member_leaves_the_targets_at_it`.

### STR-12. The first topology is the group's; Kafka 4.4.0 updates none
A join to a group with no topology sets it. A join whose topology is at an older epoch than the group's is let in and told
STALE_TOPOLOGY (0) "The member's topology epoch %d is behind the group's topology epoch %d." on every heartbeat; any other that
differs (epoch or content, list order counting) is INVALID_REQUEST "Topology updates are not supported yet." (GMM L2472-2492,
L2441-2455). STREAMS_INVALID_TOPOLOGY_EPOCH and STREAMS_TOPOLOGY_FENCED are never sent: no code of 4.4.0 makes them.
Test: `groups::streams::tests::epochs::a_different_topology_at_the_same_epoch_or_later_is_refused_and_an_older_one_is_told_it_is_stale`.

### STR-13. The initial rebalance delay
When a heartbeat finds the group empty and `streams.initial.rebalance.delay.ms` (3000) is above 0, a timer starts (not restarted
by later joins). Until it runs out every heartbeat is told ASSIGNMENT_DELAYED (5) "Assignment delayed due to the configured
initial rebalance delay." and reconciles to nothing at the assignment epoch. When it runs out the targets are computed, if they
may be (STR-14); members learn them at their next heartbeat (GMM L2269-2281, L4420-4429, L4617-4655). Deadline-driven: one timer
of the group's task, nothing polls.
Test: as STR-10.

### STR-14. Targets are computed at most once per assignment interval
When the group epoch is past the assignment epoch, a heartbeat computes the targets if none were computed yet (or since the group
emptied), or `streams.assignment.interval.ms` (1000; 0: always) has passed since the last computation; else it is told
ASSIGNMENT_DELAYED "Assignment delayed due to the configured assignment interval." and reconciles to the old targets. No timer
retries: the next heartbeat does. A topology that is not ready gives every member nothing (GMM L4187-4207, L4445-4474; TAB
L239-253). The assignor offload (`streams.assignor.offload.enable`) changes nothing seen: 4.4.0 never reads it on this path.
Test: `groups::streams::tests::epochs::without_an_initial_delay_targets_are_computed_at_once_and_then_no_oftener_than_the_interval`.

### STR-15. The group's state
Empty (no members), NotReady (no topology, or its epoch is not the validated one), Assigning (group epoch past the assignment
epoch), Reconciling (a member not stable at the assignment epoch), Stable (SG L1208-1227).
Test: the state asserted throughout `groups::streams::tests::epochs`.

## The topology against the topics (ITM and its helpers)

### STR-20. Missing source topics
Each source topic not there, sorted: MISSING_SOURCE_TOPICS (1) "Source topics a, b, c and N additional topics are missing." (Kafka's
`summarizeTopics`: three, then how many more). Nothing is configured or made (ITM L132-146, L338-348).
Test: `groups::streams::tests::configure::missing_source_topics_are_named_sorted_three_at_most`.

### STR-21. Repartition topics take the partitions of what feeds them
A repartition source with a count keeps it; a sink not known yet takes the most partitions of what its subtopology reads, round
after round; a round that needs a count and makes none is STREAMS_INVALID_TOPOLOGY "Failed to compute number of partitions for all
repartition topics. There may be loops in the topology that cannot be resolved."; a repartition source never written "... because
a repartition source topic is never used as a sink topic." (RepartitionTopics L69-175).
Test: `configure::internal_topics_take_the_partitions_of_what_feeds_them_and_are_to_be_made`,
`a_repartition_topic_nothing_writes_is_an_invalid_topology`, `repartition_topics_that_only_feed_each_other_are_a_loop`,
`a_fixed_repartition_count_is_kept_and_counts_for_tasks`.

### STR-22. Copartitioning
The topics of a copartition group must have the same partitions: INCORRECTLY_PARTITIONED_TOPICS (2) "Following topics do not have
the same number of partitions: [{a=2, b=3}]"; a flexible repartition topic takes the source topics' count; a fixed one that
differs is "Number of partitions [%d] of repartition topic [%s] doesn't match number of partitions [%d] of the source topic."
(CopartitionedTopicsEnforcer L70-177).
Test: `configure::copartitioned_sources_must_have_the_same_partitions`, `a_flexible_repartition_topic_copartitioned_with_a_source_takes_its_count`.

### STR-23. Changelogs and tasks
A changelog has as many partitions as the most its subtopology reads; a subtopology has as many tasks as that too (ChangelogTopics
L70-93, ITM L271-319). A subtopology with a changelog is stateful: only its tasks have standbys.
Test: `configure::internal_topics_take_the_partitions_of_what_feeds_them_and_are_to_be_made`.

### STR-24. Internal topics that are missing are made by the coordinator
Every repartition source and changelog not there is to be made with the partitions worked out, the replication factor sent (0: the
broker's default, -1) and the topic configs the client sent, verbatim: MISSING_INTERNAL_TOPICS (3) "Internal topics are missing: a,
b" in the order of a Java `HashMap` of their names (six orders recorded from the oracle). One that exists with other partitions is
"Existing topic T has different number of partitions: expected E, found F" and nothing is made (ITM L97-116, L204-269). The
handler makes them as the caller, CREATE allowing (SEC-331); Kafka does so without waiting, here before the answer goes, which
answers the same (the answer was worked out before). A creation that fails is remembered for twice the heartbeat interval: not
tried again meanwhile, and later answers add "; Creation failed: t (why) and N more." (KA L2898-2931).
Test: `configure::an_internal_topic_that_exists_with_other_partitions_is_named_and_nothing_is_made`,
`missing_internal_topics_are_named_in_the_order_kafkas_hash_map_has_them`;
`kafka-broker tests/streams_groups.rs::the_coordinator_makes_the_internal_topics_with_the_partitions_and_configs_the_topology_says`.

### STR-25. The tasks a member owns must be the topology's
When the topology is ready, an owned task of a subtopology not in it is INVALID_REQUEST "Subtopology X does not exist in the
topology.", one out of range "Task P for subtopology X is invalid. Number of tasks for this subtopology: N"; task offsets that name
a task twice "Task offsets contain a duplicate entry for subtopology X and partition P." (GMM L1895-1914). Nothing is changed by a
refused heartbeat.

## The sticky assignor (STA)

### STR-30. Active tasks stay, then go where their state is, then to the least loaded
Tasks by (partition, subtopology): kept by their current owner while it is under its quota (the tasks over the members, rounded up,
the quota shrinking as members reach it); then given to the best previous standby or member that reported an offset for it, under
quota (previous standbys first, then the most caught up; a later candidate wins only if both its process is lighter and it has
fewer tasks); the rest, by (subtopology, partition), to the least loaded process (tasks over members) and its least loaded member.
Test: `groups::streams::tests::assignor::a_new_process_gets_the_task_its_owner_has_beyond_its_quota`,
`current_owners_come_before_reported_offsets_and_the_most_caught_up_first`, `on_a_cold_start_tasks_go_where_their_state_is`,
`a_member_keeps_what_it_runs_when_another_joins`.

### STR-31. Load is per member of a process
A process with two members carries twice as much. Test: `assignor::a_process_with_two_members_takes_twice_the_load`.

### STR-32. Standbys
`streams.num.standby.replicas` of each stateful task, highest partition first: to the previous active owner if its process does not
have the task, else to the best previous standby whose process does not, under the total quota; the rest to the least loaded
process without the task. A process never holds a task twice; a replica no process can take is not assigned (STA L410-464).
Test: `assignor::a_standby_goes_back_to_its_previous_owner_below_its_quota`,
`standbys_go_away_from_the_process_that_has_the_task`, `every_task_is_placed_once_and_no_process_holds_a_task_twice`.

### STR-33. No warm-up task, no rack awareness
Kafka 4.4.0's assignor makes no warm-up task and its `AssignmentRefiner` passes the target through; the rack tags only make the
MISSING_CLIENT_TAGS status (STR-58). Kafka Streams itself warns "Warmup replicas are not supported yet with the streams protocol".
Test: `assignor::every_task_is_placed_once_and_no_process_holds_a_task_twice` (no warm-ups).

### STR-34. Ties
Kafka breaks ties between equally loaded processes and members by the order of a hash map; here the process with more members, then
the smaller process id, then the smaller member id. Kafka's own tests hold it to counts and properties where there is a tie
(STR-D2).

## A member's way to its target (CAB)

### STR-40. Revocation before assignment
What the member holds and the target agree on is kept; what it holds and is not to is to be revoked, and while it says it still owns
any of it (or does not say) it is UNREVOKED_TASKS at its epoch, holding only what it keeps; once it reports owning none of it, it
moves to the target's epoch. A task to be given that another member still holds (an active task anywhere; a standby or warm-up on
the same process) is held back: UNRELEASED_TASKS, given at a later heartbeat once released.
Test: `groups::streams::tests::reconciling::a_task_another_member_runs_is_given_only_once_that_member_has_let_go_of_it`,
`a_member_that_does_not_say_what_it_owns_is_not_taken_to_have_let_go`, `a_standby_is_not_given_where_its_process_already_runs_the_task`.

### STR-41. Changes of role
An active task of its own to be a standby is a standby at once (and stays to be revoked as active); a standby or warm-up to be
active is kept until the active task is given, then dropped (CAB L448-700).
Test: `reconciling::a_standby_to_become_active_is_kept_until_the_active_task_is_given`,
`an_active_task_to_become_a_standby_on_the_same_member_is_a_standby_at_once`.

### STR-42. Assignment epochs
Each active task carries the epoch it was given at, kept while it is held or being revoked: a commit at an older epoch is held to it
(STR-71).
Test: `reconciling::an_active_task_to_become_a_standby_on_the_same_member_is_a_standby_at_once`.

### STR-43. The rebalance timeout
A member left UNREVOKED_TASKS has its `RebalanceTimeoutMs` (sent on its join) to revoke; if it is still at that epoch when it runs
out it is fenced (removed, the epoch moves) (GMM L4084-4094, L5439-5472).
Test: `groups::streams::tests::members::a_member_that_does_not_revoke_in_its_rebalance_timeout_is_fenced`.

## Members (GMM)

### STR-50. Member epochs
A heartbeat at a greater epoch than the member's is FENCED_MEMBER_EPOCH "The streams group member has a greater member epoch (R)
than the one known by the group coordinator (K). The member must abandon all its partitions and rejoin."; at a smaller one,
"... smaller ...", unless it is the member's previous epoch and the tasks it says it owns are all its own (a retry) (GMM
L1860-1886). An unknown member is UNKNOWN_MEMBER_ID "Member M is not a member of group G.".
Test: `groups::streams::tests::members::a_member_at_a_greater_epoch_or_a_smaller_one_that_is_no_retry_is_fenced`,
`an_unknown_member_is_told_so_and_a_group_with_no_such_member_is_not_changed`.

### STR-51. Leaving
Epoch -1: the member goes, the group epoch moves; the last one leaves the targets at the new epoch. The answer has the member, epoch
-1, an empty status list and every other field at its default (`AcceptableRecoveryLag` -1) (GMM L4706-4752, L5082-5130).
Test: `members::leaving_moves_the_epoch_and_the_last_member_leaves_the_targets_at_it`.

### STR-52. Static members
A join with an instance id held by a member not at -2 is UNRELEASED_INSTANCE_ID "Static member N with instance id I cannot join the
group because the instance id is owned by O member."; epoch -2 keeps the member's place (no epoch move, its session still running);
a join then takes it under the new id with its tasks and target, moving the epoch only if its topology epoch, rack, tags or process
changed; the old id is then FENCED_INSTANCE_ID "Static member M with instance id I was fenced by member H."; an unknown instance
"Instance id I is unknown." (GMM L1742-1816, L3478-3526, L4799-4826, L5045-5071).
Test: `members::a_static_member_that_leaves_for_now_keeps_its_place_and_its_instance_comes_back_under_a_new_id`.

### STR-53. Sessions
Every heartbeat but a leave starts the member's `streams.session.timeout.ms` (45000) again; when it runs out the member is fenced as
by a leave (GMM L5196-5207, L5311-5332). One timer per member on the group's task.
Test: `members::a_silent_member_is_fenced_when_its_session_runs_out`.

### STR-54. The answer
The member's epoch; `streams.heartbeat.interval.ms` (5000); `streams.task.offset.interval.ms` (60000); `AcceptableRecoveryLag`
(`streams.acceptable.recovery.lag`, 10000) at v1; the three task lists, partitions in order, only when joining or when what it holds
changed (else null); the status list, never null, in Kafka's order: STALE_TOPOLOGY, ASSIGNMENT_DELAYED, the topology's status,
SHUTDOWN_APPLICATION, MISSING_CLIENT_TAGS (GMM L2337-2431).
Test: `kafka-broker tests/streams_groups.rs::version_zero_answers_carry_no_long_recovery_lag_and_version_one_answers_do`; the oracle.

### STR-55. Endpoints for interactive queries
The group's endpoint epoch moves when a member's endpoint changes, or its tasks change and it has one. A heartbeat whose endpoint
epoch is not the group's is told, for each member with an endpoint (the others in member order, itself last), its active tasks and
its standby and warm-up tasks as the partitions of the topics each subtopology reads (task p: partition p, where the topic has
it). The heartbeat that makes a group is answered endpoint epoch 0 and the group keeps 0 (Kafka's new group is made again from its
records) (GMM L2368-2380; SG L1502-1548; EndpointToPartitionsManager).
Test: `members::each_member_with_an_endpoint_is_told_where_every_task_is_itself_last`; the oracle.

### STR-56. Task offsets
A member's task offsets and end offsets are kept in memory (null: unchanged) and shown by describe; the assignor ranks standby
candidates by them (STR-30) (GMM L2216-2225).
Test: `assignor::current_owners_come_before_reported_offsets_and_the_most_caught_up_first`.

### STR-57. Shutdown requests
A heartbeat (or a leave) with `ShutdownApplication` records the first member asking; every member's answer then carries
SHUTDOWN_APPLICATION (4) "Streams group member M encountered a fatal error and requested a shutdown for the entire application."
until the group is empty (GMM L2333-2335, L2393-2400; SG L1210-1212).
Test: `members::a_shutdown_request_is_told_to_every_member_until_the_group_is_empty`.

### STR-58. Missing client tags, to version 1 clients
With `streams.rack.aware.assignment.tags` set, a member whose client tags lack some is told MISSING_CLIENT_TAGS (6) "Missing
required client tags for rack-aware standby assignment: [a, b]. Configure them via 'client.tag.<tagKey>' in your Streams config."
only at version 1 (version 0 clients do not know code 6; KAFKA-20744) (GMM L2402-2421).
Test: `members::missing_client_tags_are_reported_to_version_one_clients_only`.

## The other APIs

### STR-60. StreamsGroupDescribe
Per group: 30 without DESCRIBE (first); INVALID_GROUP_ID for an empty id; GROUP_ID_NOT_FOUND "Group G not found." / "Group G is not a
streams group."; else the state, epochs, the topology (its internal topics' partitions as worked out when it is ready, as sent when
not; lists sorted), the members (process, endpoint, tags, task offsets, assignment and target), and the assignor `sticky`. A group
the broker had before a restart is described from its record, Empty (KA L2968-3066; SG L1411-1440). SEC-332.
Test: `kafka-broker tests/streams_groups.rs::the_coordinator_makes_the_internal_topics_with_the_partitions_and_configs_the_topology_says`,
`tests/streams_crash.rs::a_group_with_a_member_comes_back_empty_with_its_topology_and_its_member_joins_again_later`.

### STR-61. ListGroups, DescribeGroups, DeleteGroups
ListGroups lists a streams group with type `streams`, protocol type `streams` and its state; a state filter is matched as Kafka
spells the state in lower case, so `NotReady` (what the Admin client sends) matches nothing, as on 4.4.0. DescribeGroups calls a
streams group Dead. DeleteGroups refuses one with members (NON_EMPTY_GROUP) and deletes an empty one with its offsets and record;
its internal topics stay (only the tool deletes them) (SG L363-369, L1101-1105; GMM L649-678).
Test: `kafka-broker tests/streams_groups.rs::a_streams_group_is_listed_as_one_and_not_described_by_the_classic_api`,
`a_streams_group_with_members_is_not_deleted_and_an_empty_one_is`.

### STR-70. Who may commit a streams group's offsets
With nobody in the group, a commit naming no epoch goes ahead (the tool's reset); a transaction's that names nobody too; else the
member must be there (UNKNOWN_MEMBER_ID), on OffsetCommit v9 or later (UNSUPPORTED_VERSION), at its epoch, never a greater one
(STALE_MEMBER_EPOCH) (SG L1022-1063).
Test: `groups::streams::tests::fences::an_admin_commit_goes_ahead_only_while_nobody_is_in_the_group`,
`a_member_commits_at_its_epoch_on_offset_commit_nine_or_later`.

### STR-71. A commit at an older epoch
Let through only if every partition named is of a topic the topology reads and of an active task the member holds (or is giving
up) since that epoch or earlier; one that is not fails the whole request with STALE_MEMBER_EPOCH (SG L1574-1617).
Test: `fences::a_commit_at_an_older_epoch_is_let_through_for_tasks_held_since_then`.

### STR-72. Who may fetch them
A request naming no member and no epoch reads them; else the member must be there at exactly its epoch (SG L1070-1088, L1195-1202).
Test: `fences::a_member_fetches_offsets_only_at_exactly_its_epoch`.

### STR-73. OffsetDelete
A partition of a topic the topology reads, while the group has members, is GROUP_SUBSCRIBED_TO_TOPIC; changelogs and sinks never
are (SG L1108-1119).
Test: `kafka-broker tests/streams_groups.rs::offsets_of_a_topic_the_topology_reads_are_not_deleted_while_it_has_members`.

## The group configs (GC)

### STR-90. The streams configs of a group
`streams.session.timeout.ms` (45000, 45000 to 60000), `streams.heartbeat.interval.ms` (5000, 5000 to 15000),
`streams.num.standby.replicas` (0, at most 2), `streams.initial.rebalance.delay.ms` (3000), `streams.assignment.interval.ms` (1000,
0 to 15000), `streams.rack.aware.assignment.tags` (none), `streams.assignor.name` (`sticky`, the only one), and the internal
`streams.assignor.offload.enable`, `streams.task.offset.interval.ms` (60000, at least 15000), `streams.num.warmup.replicas` (2, at
most 20), `streams.acceptable.recovery.lag` (10000). Out of bounds is INVALID_CONFIG in Kafka's words ("<key> must be in the range
<min> to <max> inclusive.", "<key> must be less than or equal to <max>", "<key> must be greater than or equal to <min>",
"streams.rack.aware.assignment.tags must not contain duplicate tag keys.", "streams.assignor.name '<n>' is not a registered task
assignor. Registered assignors are: [sticky].", "streams.session.timeout.ms must be greater than streams.heartbeat.interval.ms").
DescribeConfigs lists the non-internal ones (and an internal one where the group sets it) (GC L269-333, L491-674).
Test: `kafka-broker tests/streams_groups.rs::the_streams_group_configs_are_kept_checked_and_described`.

### STR-91. When a change takes effect
Each heartbeat reads the group's configs: a session, interval or delay changed applies from then; the standby replicas or tags
changed move the epoch at the next heartbeat (STR-11) (GMM L2232-2260).
Test: `epochs::changing_the_standby_replicas_moves_the_epoch_at_the_next_heartbeat`.

## Durability

### STR-80. What is kept
A streams group keeps its epochs and topology in its record (version 4), written when a member is seated (with the topology a join
brings), whenever its epochs move while it has members, and when the last goes. Members, targets, task offsets, the endpoint epoch
and a shutdown request are not kept: after a restart the group is Empty with its topology, at the epoch it had (Kafka keeps its
members in `__consumer_offsets`; here they join again, as they would after the coordinator moved) (STR-D3).
Test: `groups::streams::tests::restart::the_record_keeps_the_epochs_and_the_topology_whole`,
`a_group_seating_its_first_member_keeps_the_topology_the_join_brings`,
`kafka-broker tests/streams_crash.rs::an_empty_group_keeps_its_epochs`,
`a_stop_at_any_point_of_the_first_join_leaves_the_group_whole_or_absent`; `kafka-meta life::before_streams::*`.

### STR-81. A member of the last run joins again
A heartbeat that is not a join, to a streams group the broker had before it started again, brings the group back and is told
UNKNOWN_MEMBER_ID (Kafka Streams then joins again) rather than GROUP_ID_NOT_FOUND, which Kafka Streams takes as fatal. Its join
takes the group past the epoch it had.
Test: `kafka-broker tests/streams_crash.rs::a_group_with_a_member_comes_back_empty_with_its_topology_and_its_member_joins_again_later`,
`groups::streams::tests::restart::a_restored_group_is_empty_with_its_topology_and_its_members_join_again_at_a_later_epoch`;
by hand: a Kafka Streams word count kept running while doze-kafka was stopped and started again was RUNNING again within a session.

### STR-82. Idle cost
No streams group, no cost: nothing is made until a heartbeat. A group is one task of the registry's, woken only by its requests and
its deadlines (a member's session, a rebalance timeout, the initial delay); nothing polls. The internal topics' failures are kept
only when a creation failed, and forgotten as they are read.

## The oracle

Real Apache Kafka 4.4.0 (`kafka_2.13-4.4.0.tgz`, KRaft, one node in combined mode, formatted with its defaults) and doze-kafka side
by side (2026-11-18):
- `interop/java/Streams.java` with Kafka Streams 4.4.0: the five scenarios pass against both. Every heartbeat answer of every
  client was logged (the Java client at debug) and compared after normalising ids: the word count, the restart and the admin
  scenarios answer the same, field by field, in the same order (member epochs 1, 1, 3; the statuses and their words; the task
  lists; the endpoint lists); the scaling scenario assigns the same tasks, standbys and epochs, its answers interleaved differently
  with the three instances' timing and its endpoint epoch counting a few more moves on Kafka.
- `kafka-streams-groups.sh` (`--list`, `--list --state`, `--describe` with `--members`, `--state`, `--offsets` and `--verbose`,
  `--reset-offsets` dry run and refused while running, `--delete` refused while running and done with `--delete-all-internal-topics`
  after, `--delete-offsets`) prints the same against both but for ids and row order.
- A Kafka Streams application with an endpoint (`application.server`) started before its source topic exists gets
  UNKNOWN_SERVER_ERROR from Kafka 4.4.0 on every heartbeat (STR-D1); without one, both say NotReady and count once the topic is made.

## Deviations
- **STR-D1.** A member with an endpoint, in a group whose topology is not configured (source topics missing, or topics wrongly
  partitioned), is answered normally, its endpoint entry with no partitions. Kafka 4.4.0's `EndpointToPartitionsManager` reads the
  configured subtopologies that are not there (`Optional.get()` of an empty value) and answers UNKNOWN_SERVER_ERROR, so such an
  application never joins until its source topics exist.
- **STR-D2.** Ties in the assignor are broken by process size, process id and member id, not by a hash map's order (STR-34).
- **STR-D3.** Members are not kept across a restart (STR-80, STR-81).
- **STR-D4.** The internal topics are made before the heartbeat is answered, not after (STR-24); the answer is the same.
