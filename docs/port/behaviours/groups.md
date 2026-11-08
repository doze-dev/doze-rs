# Classic consumer groups (Phase 4)

What the broker does for JoinGroup, SyncGroup, Heartbeat, LeaveGroup, FindCoordinator, OffsetCommit, OffsetFetch,
OffsetDelete, DescribeGroups, ListGroups and DeleteGroups, with the test that covers each. The machine is tested alone
(`groups::machine::tests::*`, in `kafka-broker/src/groups/machine/tests/`), then through the broker
(`kafka-broker/tests/{groups,offsets,group_admin}.rs`), then with real clients (by hand, below).

Where the Rust edition differs from the Go edition on purpose, it follows Kafka: see `phase4.md`.

## The machine

### C1. A client without a member id is given one and comes back with it (JoinGroup v4+)
The answer is MEMBER_ID_REQUIRED (79) with a fresh id and generation -1. The id waits five minutes; a join refused for
another reason does not spend it. Before v4 the client is given an id and joins in one step. An id that was never issued
is UNKNOWN_MEMBER_ID (25).
Test: `membership::a_client_that_joins_without_an_id_*`, `an_older_client_*`, `an_id_that_was_never_issued_*`,
`an_issued_id_is_not_spent_by_a_refused_join_*`

### C2. Check order of a JoinGroup
Session bounds (6000..=1800000 ms, 26) before an id is handed out; no protocols or no protocol type (23); a static
instance held by another id (82), unknown (25); then the group's protocol type and a protocol in common (23).
Test: `membership::the_session_bounds_are_checked_*`, `a_member_with_another_protocol_type_*`,
`static_members::an_instance_held_by_another_member_*`

### C3. Members who join together share a generation; only the leader is told the members
The first rebalance of an empty group waits the initial delay (default 200 ms), and another window for each latecomer,
cut short by the rebalance deadline. The protocol is the one every member supports that most members put first (ties by
name); the lowest id leads a new group and a leader that is still a member keeps leading.
Test: `membership::members_who_join_together_*`, `timers::the_first_member_of_an_empty_group_*`,
`timers::a_member_arriving_during_the_delay_*`, `timers::the_delay_is_cut_short_*`,
`rebalance::a_leader_that_is_still_a_member_keeps_leading_*`

### C4. A running group rebalances when a member comes, goes, or changes what it offers
A follower rejoining unchanged is answered at once; a leader rejoining, a new member, changed protocols, a leave or an
expired session begin a rebalance. Members that do not join again by the longest rebalance timeout are dropped.
Test: `membership::a_follower_rejoining_*`, `a_new_member_or_changed_protocols_*`, `timers::members_that_do_not_join_again_*`,
`rebalance::a_member_who_leaves_makes_the_others_join_again_*`

### C5. SyncGroup: the leader's assignment goes to everyone
Order: member (25/82), generation (22), protocol type and name from v5 (23), then state (27 while members are joining
again). A follower waits for the leader; a second sync from a waiting member replaces the first, which is told 25. A
leader syncing again in a stable group changes nothing.
Test: `rebalance::the_leaders_assignment_is_handed_to_everyone_*`, `sync_answers_by_member_then_generation_*`,
`a_second_sync_from_a_waiting_member_*`, `a_rebalance_tells_the_members_waiting_*`

### C6. Heartbeat renews the session; sessions run out
Order: member, generation, then REBALANCE_IN_PROGRESS (27) only while members are joining again. A member whose
request is waiting on the group is not expired for being silent. A timer that was set again does nothing when the old
one arrives.
Test: `membership::heartbeats_answer_by_member_*`, `timers::a_silent_member_is_dropped_*`,
`a_member_waiting_for_the_group_is_not_expired_*`, `a_timer_that_was_set_again_*`

### C7. LeaveGroup
Each member named is answered (v3+; before that the one member's code is the request's). An unknown member disturbs
nothing. The last member leaving makes the group Empty and the generation goes up, so the next generation is two on.
A leaving member's waiting join is told 25.
Test: `rebalance::leaving_names_each_member_*`, `the_last_member_leaving_empties_*`, `a_member_who_leaves_while_its_join_waits_*`

### C8. Static members
An instance id names a member across restarts. A restart takes the old member's place and assignment without a
rebalance for a follower; a leader restarting, or changed protocols, rebalance. The old incarnation's waiting requests are
told FENCED_INSTANCE_ID (82). Heartbeat, sync, commit and leave check the instance as well as the id.
Test: `static_members::*`

### C9. A JoinGroup sent again replaces the one waiting
The earlier request is never answered; whoever waits for it is told to stop (the connection is closed).
Test: `membership::a_join_sent_again_*`, `groups::a_join_sent_again_ends_the_one_that_was_waiting`

### C10. Invariants over random sequences
The generation never goes down; a group is Empty if and only if it has no members; a running group has a leader that is a
member and a protocol every member offers; every member has a session timer; a rebalance has a deadline; every
request other than a replaced join is answered; a group left alone empties.
Test: `properties::whatever_happens_the_group_keeps_its_invariants_*` (`SIM_SEEDS=5000` found one real defect while
building: a group in its initial-delay window that lost its last member stayed Preparing).

## The wire

### G1. FindCoordinator
This broker is every group's and every transaction's coordinator. v0-3 answer in the message (a success reads "NONE" from
v1 to v3); v4+ answer one entry per key in `coordinators`, and nothing at the top (a field the version lacks stops the
message being written). A key type that is neither 0 nor 1 is INVALID_REQUEST with no broker (v1+).
Test: `groups::this_broker_is_every_groups_*`, `a_key_type_that_has_no_coordinator_*`

### G2. A group id that cannot be kept is INVALID_GROUP_ID (24) in every group API
Empty, a NUL, or over 4096 bytes. A heartbeat, sync or leave to a group that does not exist is answered as an empty group
would (UNKNOWN_MEMBER_ID) and does not make it.
Test: `groups::an_empty_or_unusable_group_id_*`, `requests_to_a_group_that_does_not_exist_*`; `kafka-wire ids::group::tests`

### G3. JoinGroup in the wire's forms
Rebalance timeout from v1 (the session's before); protocol type and name are strings before v7 and null on a refusal
from v7. Member ids are `doze-` and 32 hex digits, behind the instance id for a static member.
Test: `groups::a_member_joins_syncs_heartbeats_and_leaves`, `kafka-wire ids::group::tests::an_issued_member_id_*`

### G4. OffsetCommit
The fence first (C11), then each partition: unknown topic or partition 3, metadata over 4096 bytes 12 (the unknown
check first). Negative offsets and offsets past the end are stored. What passes is stored in one write; a store failure
is NOT_COORDINATOR (16) for the partitions it covered.
Test: `offsets::*`; mutants of the handler and the fence killed

### C11. The commit fence
No generation (negative): allowed only while the group has no members. Otherwise the member (25/82), the generation
(22), and not CompletingRebalance (27). A group that does not exist takes only a commit with no generation.
Members who are about to join again may commit.
Test: `commit::*` (machine), `offsets::a_member_commits_at_its_generation_*`

### G5. OffsetFetch
Both forms (v0-7 single group; v8+ batched). Anything unknown is offset -1, metadata "", error 0, so a new consumer starts
where `auto.offset.reset` says; a null topic list means everything the group has, by topic and partition. A group id that
cannot be kept is a group with nothing.
Test: `offsets::a_simple_consumers_commit_is_read_back_*`, `nothing_known_is_not_an_error_*`

### G6. Offsets are durable and go with their topic
An acknowledged commit survives a power cut; deleting a topic removes its offsets in the same commit.
Test: `offsets::a_commit_that_was_acknowledged_survives_*`, `deleting_a_topic_takes_the_offsets_*`;
`kafka-meta` crash sweep (commits and the cascade in the random workload)

### G7. DescribeGroups
Groups are answered in the order Kafka answers them (not the request's), duplicates kept. A group never heard of, an empty
id and an unusable id are Dead, not an error; one known only by its offsets is Empty. Only a Stable group shows its
protocol and its members' metadata. Member client host is a fixed `/127.0.0.1` (the peer address is not carried to the
group APIs yet).
Test: `group_admin::*`, `groups::order::tests` (13 orders recorded from Apache Kafka 4.3.1, past a resize of its map)

### G8. ListGroups
Groups in memory and groups known only by offsets, by id; the state and type filters ignore case.
Test: `group_admin::groups_are_listed_by_id_*`

### G9. DeleteGroups
In Kafka's order: a group with members NON_EMPTY_GROUP (68); an empty one goes with its offsets; nothing there and no
offsets GROUP_ID_NOT_FOUND (69); an unusable id 24. A group deleted can be made again by a join.
Test: `group_admin::deleting_a_group_needs_it_empty_*`

### G10. OffsetDelete
GROUP_ID_NOT_FOUND (69) for a group with no members and no offsets, 24 for an unusable id; per partition, a topic a member
subscribes to is GROUP_SUBSCRIBED_TO_TOPIC (86), an unknown topic or partition 3, otherwise the offset is forgotten.
Subscriptions are read from the consumer protocol (versions 0-3); an unreadable one subscribes to nothing assumed.
Test: `group_admin::offsets_are_deleted_unless_*`, `offset_delete_of_a_group_with_nothing_*`, `groups::subscription::tests`

## By hand

- `kcat -G` (librdkafka): a group consumed 10 messages and committed; a second run of the same group got only the one
  produced since.
- franz-go: two members shared a 3-partition topic (all 30 records, none twice); one left and the other took over its
  partitions and read the records produced after; a fresh member of the group read nothing it had already consumed.
  (Scripts: a scratch Go module, not in the repository.)
- Found by the client, not by a test, and then covered: FindCoordinator v4 answered at the top of the message as well as in
  `coordinators`, which the encoder refuses; the test rig now encodes every response it receives.

## Not done (recorded)
Offsets retention and the group liveness record (slice 4g); KIP-848 groups and the migration between protocols
(Phase 5); transactional offset commits (Phase 6); ACLs (Phase 8); the Java client (no JDK on the machine).
