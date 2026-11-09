# Phase 5: KIP-848 consumer groups and classic <-> consumer migration

Status: built, awaiting the independent audit (started 2026-11-09, autonomous run). Catalogue: behaviours/groups848.md.
Goal: a client using `group.protocol=consumer` (ConsumerGroupHeartbeat, ConsumerGroupDescribe) joins, is assigned
partitions by the *broker*, reconciles, rebalances, leaves; a classic and a consumer-protocol member can share one
group (a rolling upgrade, in both directions); and every recorded Apache Kafka 4.3.1 scenario that was waiting on
KIP-848 passes.

Sources: the Go edition (group848.go, group848_assign.go, groupmigrate.go: read for *what* it does), the recordings
(consumer-assignment-interval, consumer-assignment-expiry, consumer-static-leave, mixed-protocol-{groups,rolling,parked,
cooperative,static}, group-requests-refused, group-coordinator-defaults, full-disk-consumer-group-heartbeat, and the
consumer steps of describe-groups-order, offsets-retention*, groups-listed-after-restart, offset-delete-subscribed),
and `conformance/claims/groups.md` (R8a-j, R11*, R21*, R22*, R24*, R25a, G7).

## What differs from the Go edition, on purpose

| Go | Rust | Why |
|---|---|---|
| Two managers (classic, KIP-848) each with a mutex, a `claims` table arbitrating the id, and a `classicBridge` that moves a group between them holding both locks in a fixed order | One actor per group id that owns a `Group` = `Classic(GroupMachine)` or `Consumer(ConsumerGroup)`; a migration is a conversion inside the owner | PLAN design table: no lock order to violate, no claim race to arbitrate; this revises the plan's "claims registry" |
| `time.AfterFunc` closures re-taking the manager lock, guarded by generation counters | The same timer-as-event scheme as classic: `Arm`/`Disarm` effects, generations, the actor's ordered timer map | testable with synthetic time |
| Topic names resolved through the store inside the group code | The machine takes a `Topics` snapshot with each event; it never reaches for a store | sans-IO |
| Assignors are methods on the manager | Pure functions `assign(members, topics, previous) -> targets`, property-tested (covers every partition once, only subscribers, balanced within one, sticky) | |

## Design

**`Group` enum** (groups/group.rs). The actor's `Machine` is `Group`; `step(event, now, &Topics)`. Events gain
`ConsumerHeartbeat`, `ConsumerDescribe`, `TopicsChanged`; answers gain `Consumer(..)`, `ConsumerDescribe(..)`;
`TimerKind` gains `Revoke(MemberId)`. A classic request to a `Consumer` group, and a ConsumerGroupHeartbeat to a classic
one, are handled by the owner converting (upgrade needs protocol type `consumer` and readable subscriptions: else
GROUP_ID_NOT_FOUND "Cannot upgrade classic group <id> to consumer group because the group does not use the consumer
embedded protocol."). An *empty* group of either kind is replaced whole.

**The consumer machine** (groups/consumer/): a group `epoch`, a `target_epoch` (the epoch the targets were computed for)
and `assigned_at`; each member has `epoch`, `prev_epoch`, `target`, `assigned`, `revoking`, `sent`. A partition is
free for another member only when it is in neither `assigned` nor `revoking` of any other member (ownership is the
coordinator's record, never inferred from what a client reports). Targets are computed only by a member's heartbeat (or
a classic JoinGroup) at least `assignment interval` (1 s) after the last computation, never by a leave, an expiry or
time (R22a/b). A static member that leaves with epoch -2 keeps its seat and partitions until its session ends (R24).

**Assignors:** `uniform` (sticky, balanced within one, eligibility by subscription) and `range`; a group uses the one
most members ask for, `uniform` on a tie or none.

**Mixed groups:** a classic member of a consumer group is a `ClassicSeat` (its protocols, the one its subscription was
read from, that subscription's version, its session). Its JoinGroup is answered with no leader and no member list; its
SyncGroup carries the partitions it may have now (consumer-protocol assignment bytes at its subscription's version);
its Heartbeat is REBALANCE_IN_PROGRESS when only a rejoin can move its partitions. When the last consumer-protocol
member goes, the group is classic again and every member must rejoin (heartbeat: ILLEGAL_GENERATION).

**Persistence:** `GroupLife` gains the kind of group and, for a consumer group, its epochs and assignor, so an emptied
consumer group is listed and described after a restart as the consumer group it was (R11q). Members are not kept.

## Slices

| | Slice | Gate |
|---|---|---|
| 5a | Wire handlers' skeleton and served versions (ConsumerGroupHeartbeat 0-1, ConsumerGroupDescribe 0-1), `Topics` snapshot, consumer-protocol bytes (subscription v0-3 with owned partitions, assignment) | wire round trips, decoder tests |
| 5b | The assignors as pure functions | properties: complete, eligible, balanced, sticky |
| 5c | `ConsumerGroup` machine: join, poll, leave, static, reconcile, epochs, timers (session, revoke), describe | unit tests with synthetic time; proptest invariants (no partition owned twice, epochs never fall) |
| 5d | `Group` enum, actor/registry/handlers for the two APIs; DescribeGroups/ListGroups/DeleteGroups/OffsetCommit/OffsetFetch/OffsetDelete for consumer groups; `--consumer-session-timeout`; the assignment-interval knob | `consumer-assignment-interval`, `consumer-assignment-expiry`, `consumer-static-leave`, `group-coordinator-defaults`, `group-requests-refused` |
| 5e | Migration: classic member in a consumer group, upgrade, downgrade, parked requests, static takeover | `mixed-protocol-*` (5 scenarios) |
| 5f | Persistence, retention and the rest of the oracle | `offsets-retention*`, `groups-listed-after-restart`, `describe-groups-order`, `offset-delete-subscribed`, `full-disk-consumer-group-heartbeat` |
| 5g | Real clients: librdkafka `group.protocol=consumer` (kcat), mixed with classic members, in `interop/`; franz-go if it speaks KIP-848; weight; independent audit | the checkpoint |

Gates as in Phase 4 (fmt, clippy -D warnings, tests, lint-size). Commit per slice.

## Risks

- The reconcile rules are the densest part; the recordings pin them, and the Go edition passes them. Port the rules,
  not the shape.
- Regular-expression subscriptions (ConsumerGroupHeartbeat v1) need a regex engine; a small dependency (`regex-lite`)
  is weighed against the weight gate when it is added.
- Real clients exercise paths the recordings do not (librdkafka's KIP-848 consumer is new): budget for 5g.

## Result (slices 5a to 5g built)

Oracle, against recorded Apache Kafka 4.3.1 (full replay: 38 scenarios pass; the ones that do not are all of later phases
- transactions and idempotence, ACLs, SASL, telemetry, config APIs): `consumer-assignment-interval`, `-expiry`,
`consumer-static-leave`, the five `mixed-protocol-*`, `group-requests-refused`, `describe-groups-order`,
`offsets-retention`, `-members`, `-recreate`, `-restart`, `groups-listed-after-restart`, `offset-delete-subscribed`, and all
the classic ones of Phase 4. Real clients: franz-go and librdkafka on the new protocol, alone and with classic members
(a rolling upgrade, and the reverse), and under churn (`interop/`, 17 scenarios pass). Weight: binary 3.62 MB (gate 4.2),
107 packages (gate 115). Tests: 495 in the workspace.

Found by the oracle and the clients and not by the unit tests (each now has one): a commit naming a member that is not in
a group with members was let through; a client asking for a member id must be vetted by a consumer group before it is
issued one (the recorded order); franz-go's "848" switch is `ServerSideBalancer`, not `GroupProtocol` (the first
"pass" was a classic run: read the client's log before believing it); kafka-go cannot join a group that reads
subscriptions; a graceful shutdown of a mixed group has to stop the classic members first.
