# Consumer groups of the new protocol (KIP-848) and mixed groups

Phase 5. What Apache Kafka 4.3.1 does, from the recordings (`conformance/recordings`, claim ids in
`conformance/claims/groups.md`), and the test in this repository that holds each. Machine tests are in
`crates/kafka-broker/src/groups/consumer/tests/` (`protocol`, `ownership`, `simulation`), `.../group/tests.rs` (a group
changing kind), `.../assign/tests.rs` (the assignors); wire tests in `crates/kafka-broker/tests/consumer_groups.rs` and
`versions.rs`; the oracle scenario in brackets; real clients in `interop/`.

## G848-1. Epochs and when targets are computed (R22a, R22b)
A new group is at epoch 1; its first member's join takes it to 2. The group epoch moves with membership, subscriptions
and topics. The members' targets are computed for an epoch only by a member's heartbeat (or a classic JoinGroup) at least
the assignment interval (1 s) after the last computation; a leave, an expiry and the passing of time compute nothing.
Until then the group is `Assigning` with the old targets, and a member joining is answered at the epoch of the targets,
with nothing yet. Test: `protocol::a_join_a_moment_after_the_last_targets_waits_for_a_heartbeat_past_the_interval`,
`a_leave_and_an_expiry_move_the_epoch_but_compute_nothing`. [consumer-assignment-interval, consumer-assignment-expiry]

## G848-2. Ownership (R22c)
A partition is in a member's `assigned` or `revoking` set, or free. A member is told to give up a partition by an
assignment without it; the partition stays the member's (and ConsumerGroupDescribe shows it held) until the member
reports owned partitions that include none of the revoked; only then can another member take it, on its next heartbeat. A
member that does not say so within its rebalance timeout (the session if none) is removed. A rejoin at epoch 0 drops what
the member held. Test: `ownership::*`, `simulation::whatever_happens_no_partition_is_held_by_two_members_and_epochs_only_rise`
(mutation-checked: a `free()` that always says yes fails five tests).

## G848-3. Static members (R24a, R24b, R21j)
Epoch -2 with an instance id keeps the seat and the partitions until the session runs out (described at epoch -2, group
`Reconciling`); the instance joining again under a new member id takes the seat and what the old one had; the old id is
FENCED_INSTANCE_ID (82) with "Static member <old> with instance id <i> was fenced by member <new>." A live holder makes the
join UNRELEASED_INSTANCE_ID (111); a stranger's instance is "Instance id <i> is unknown." (25). Test: `protocol::a_static_member_*`.
[consumer-static-leave, mixed-protocol-static, group-requests-refused]

## G848-4. The request's own checks (R21c, R21d)
In Kafka's order and words: empty group "GroupId can't be empty.", empty instance "InstanceId can't be empty.", a join with
no rebalance timeout, owned partitions null or not empty ("TopicPartitions must be empty when (re-)joining."), no
subscription, -2 without an instance ("InstanceId can't be null."), epoch below -2. A later epoch or a leave in a group
that is not there is GROUP_ID_NOT_FOUND ("Consumer group <g> not found." / "Group <g> not found."). UNSUPPORTED_ASSIGNOR for
a name other than uniform and range; INVALID_REGULAR_EXPRESSION for a pattern that does not compile. A version 0 client
without an id is given one; from version 1 it is INVALID_REQUEST. Test: `consumer_groups.rs::what_a_client_gets_wrong_*`.

## G848-5. Other group APIs see a consumer group (R8b, R21e, R25a, G7, R11)
DescribeGroups: Dead. ListGroups: type `consumer`, state Empty/Assigning/Reconciling/Stable (the type filter matches without
regard to case). ConsumerGroupDescribe of a classic group: "Group <g> is not a consumer group." (69). DeleteGroups with
members: NON_EMPTY_GROUP. OffsetCommit v9 and later carries the member epoch (v8 and earlier naming a consumer-protocol
member: UNSUPPORTED_VERSION; a stale epoch: STALE_MEMBER_EPOCH; a member not in the group: UNKNOWN_MEMBER_ID). OffsetDelete
refuses the topics a member subscribes to (86). Answer order is that of DescribeGroups. Test: `consumer_groups.rs`.
[mixed-protocol-groups, group-requests-refused, describe-groups-order, offset-delete-subscribed]

## G848-6. Migration (R8a to R8j)
A ConsumerGroupHeartbeat joining a classic group of protocol type `consumer` with members upgrades it: the members stay,
as members of the consumer group, at the generation they had, with the partitions they held; what was parked is answered
REBALANCE_IN_PROGRESS with the member's own id. A group of another protocol type, or with a subscription that cannot be
read, is GROUP_ID_NOT_FOUND "Cannot upgrade classic group <g> to consumer group because the group does not use the consumer
embedded protocol." A classic member joining a consumer group is answered with no leader and no member list; its SyncGroup
carries the partitions it may have now (at the version its subscription was written at); its Heartbeat is
REBALANCE_IN_PROGRESS when only a rejoin can move its partitions; its commits are by generation = epoch. A cooperative
classic member keeps what its JoinGroup says it still owns. When the last consumer-protocol member leaves, expires or
is replaced, the group is classic again: generation +1, every member must join again (heartbeat: ILLEGAL_GENERATION); a
classic LeaveGroup naming the last consumer-protocol member does not turn it. A classic client asking for a member id is
refused first if the consumer group would refuse its protocols. Test: `group/tests.rs`, `consumer_groups.rs`.
[mixed-protocol-groups, -rolling, -parked, -cooperative, -static]

## G848-7. Across a restart and in retention (R11, R11g, R11q)
An emptied consumer group is kept in its record (kind, epochs, assignor): listed with type `consumer` and described
`Empty` with its epochs after a restart, until the retention forgets it. A consumer group's offsets are counted from each
commit, not from when it lost its last member; one recorded as having members is waited for until its session has
run out after the start. Members themselves are not kept (the same limitation as the classic ones). Test:
`groups/retention/tests.rs`, `kafka-meta` crash sweep. [offsets-retention*, groups-listed-after-restart]

## Real clients (`interop/`)
franz-go (`ServerSideBalancer`) and librdkafka (`group.protocol=consumer`, through kcat) on the new protocol; groups of both
protocols (`upgrade-848`, `classic-joins-848`); churn under the new protocol (a member leaves, one is killed and its
session ends, one joins). kafka-go sends a version 1 subscription without the owned partitions the version requires, so
a group that reads subscriptions refuses it (as the recorded unreadable-subscription step does) and it cannot be mixed
with the new protocol. sarama has no KIP-848.

## Not done
The Java client. `full-disk-consumer-group-heartbeat` (a full-disk fault on the group heartbeat) is a fault-injection
scenario of the Go harness. Rack-aware assignment: rack ids are kept and shown and place nothing, as in Kafka for a broker with no `broker.rack` (one
broker has no racks to place by). The heartbeat and assignment intervals stay at Kafka's defaults, the assignment one a test
knob.

## G848-9. Words from the source (not in a recording)
Read from Apache Kafka 4.3.1: a ConsumerGroupHeartbeat is checked in this order, each INVALID_REQUEST with Kafka's words:
MemberId blank ("MemberId can't be empty."; asked for from version 1, and before that after the first heartbeat), GroupId,
InstanceId, RackId ("RackId can't be empty."), then the joining checks. To a classic group, a member that is not joining
(epoch above 0, or leaving) is GROUP_ID_NOT_FOUND "Group g is not a consumer group."; a joining one upgrades it, or is refused
with "Cannot upgrade classic group g to consumer group because ..." (not the consumer protocol; the group size exceeds the
consumer group maximum size), and a refusal changes nothing. OffsetFetch v9 with a member checks it is in a consumer group
at its epoch (UNKNOWN_MEMBER_ID; STALE_MEMBER_EPOCH, ILLEGAL_GENERATION for a classic member) and checks nothing of a
classic group or one that is not there. Test: `consumer_group_words`, `offset_fetch_checks`, `consumer/tests/fetch.rs`,
`group/tests/timers.rs`.

## G848-10. A commit by a consumer-protocol member at an older epoch (a decided difference)
Kafka accepts an OffsetCommit or TxnOffsetCommit from a member of the new protocol at an epoch older than the member's current one,
partition by partition, for each partition that was assigned (or is pending revocation) at or before that epoch; one assigned
later is STALE_MEMBER_EPOCH for that partition (source: `ConsumerGroup.createAssignmentEpochValidator`). Here the whole request
is STALE_MEMBER_EPOCH (ILLEGAL_GENERATION for a transaction's offsets and for a classic member) when the epoch is not the
member's. The cost is a commit that crosses a change of the member's epoch being refused once and retried, which clients do
after their next heartbeat (the error is retriable); nothing is lost or committed wrongly. Doing it as Kafka does needs the
epoch each partition was assigned at, kept for every member through every reconcile, and a per-partition verdict from the
group, and the reconcile is the part of Phase 5 that had the most defects; it is left until a client is seen to need it.
A commit that names no generation to an empty group is accepted whoever it names (`consumer_group_words`).
