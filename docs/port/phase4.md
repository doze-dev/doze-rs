# Phase 4: classic consumer groups

Status: DONE 2026-11-09 (slices 4a to 4h; an independent audit at 23dcee3 said DONE after four rounds, each of which found
less; the Java client is not run: no JDK, and the owner dropped it in favour of the Go clients sarama, franz-go and kafka-go, `interop/`). Catalogue: behaviours/groups.md. Goal: a Kafka client (franz-go, librdkafka, kcat -G, later the Java client) joins a group,
is assigned partitions, heartbeats, commits and fetches offsets, rebalances when members come and go, and sees the
same answers as real Apache Kafka 4.3.1 in the recorded classic-group scenarios. KIP-848 (ConsumerGroupHeartbeat) is
Phase 5; transactions (TxnOffsetCommit, AddOffsetsToTxn) are Phase 6. Both stay out.

Source of the behaviour: the Go edition (`doze-kafka` at origin/main, group files identical to the checkout) read for
*what* it does, the recordings under `conformance/` for what Kafka does, `conformance/claims/groups.md` for the claim
ids. Where the Go edition is known to differ from Kafka (below), the Rust edition follows Kafka.

## Prior art

Searched 2026-11-09. No Rust crate implements a server-side classic coordinator that could be read or reused: what
exists is client-side (`crabka-client-consumer`, `samsa`, `rustfs-kafka`: JoinGroup, SyncGroup, Heartbeat from the
consumer's side) or says outright that it does not do classic groups (`kafkit-client`). `fluxmq` and `rivven-core`
describe a coordinator but are not Kafka-fidelity. Kafka's own `GroupMetadata`/`GroupCoordinator` and the Go edition
are the references; `kafka-python`'s `BaseCoordinator` was read for the client's side of the same exchange.

## What the Go edition does that the Rust edition does differently, on purpose

| Go | Rust | Why |
|---|---|---|
| One mutex over all groups, parked requests on channels, timers with generation counters checked under the lock | One actor task per group owning a sans-IO `GroupMachine`; timers are events carrying a generation; a parked request is a `oneshot` | PLAN design table: no lock order to violate, timers testable with synthetic time |
| `selectProtocol` takes the lowest-sorted member's list and the first protocol all support | Kafka's vote: each member votes for its most preferred protocol that every member supports; most votes wins, ties by protocol name order | Kafka behaviour; not pinned by a recording but cheap to be right (Go finding, to be fixed in Go first) |
| Leader's SyncGroup overwrites assignments in any state; v5 protocol type/name never compared (GRP-11, GRP-27) | Kafka's rules: a stale or mismatched sync is refused | Go's own open findings |
| Initial delay 200 ms default | the same (Kafka's 3 s makes a local broker feel slow); `--group-initial-rebalance-delay` becomes real | documented in Go's limitations |
| Members memory only; liveness (empty-since, session) persisted so offsets retention survives a restart | the same, in `kafka-meta` | |

## Design

**A pure machine.** `GroupMachine::step(event, now) -> Vec<Effect>`; no clock, no tasks, no channels. Events:
`Join`, `Sync`, `Heartbeat`, `Leave`, `Timer { kind, generation }`. Effects: `Reply(Token, Answer)`,
`Arm { kind, generation, at }`, `Disarm`, `Persist(Liveness)`. A `Token` names a parked request; the machine never
holds the channel, so it can be driven by tests and by `proptest` event sequences.

Invariants to check over random sequences: the generation never goes down; a Stable group has a leader that is a
member; every parked request is answered exactly once (or dropped by a retry, as Kafka does); a stale timer does
nothing; no member in a Stable group has an empty assignment slot it was not given; a rebalance that completes answers
every joined member with the same generation and protocol.

**An actor per group** (`groups/actor.rs`): owns a machine, receives `Command`s over an mpsc, turns `Arm` into an entry of its own ordered timer map
(no extra dependency), `Reply` into a `oneshot::send`. It stops when the group is deleted or every sender
of commands is gone. The handler is `async fn`: send the command, await the oneshot. A connection that drops
leaves its request parked until the rebalance or the session ends, as in Go and Kafka.

**A registry** (`groups/registry.rs`): `GroupId → handle`, opened by a JoinGroup from a client with no id or with one it was
given (a client only asking for an id leaves a bounded entry in the registry's table of issued ids, not a group; one with
an id made up is told so), with a claim slot per group id for
Phase 5 (a group is classic or consumer-protocol, never both). Phase 4 only ever claims classic.

**Offsets** live in `kafka-meta` (redb): `offsets` keyed `(group, topic, partition)` with offset, metadata, commit time;
`groups` keyed by group with the liveness record. A commit checks the topic exists in the same write transaction;
deleting a topic removes its offsets in the same transaction (the store already runs topic deletes through one
transaction). Writes follow the broker's existing rule: a commit error is fatal to the request, never retried silently.

**Offsets expiry** is a sweep (`offsets_retention`, default 168 h, 0 keeps forever; the check interval is retention/100
clamped to 50 ms..10 min): a classic group with no members expires from its empty-since time; a group with members
keeps the topics it subscribes to (consumer-protocol subscription metadata v0..3 decoded for this); nothing expires
during a rebalance, for a non-`consumer` protocol type, or for an unreadable subscription.

**Typed domain.** `GroupId` (non-empty, at most 4096 bytes, no NUL), `MemberId`, `InstanceId`, `Generation` in
`kafka-wire::ids`; the machine takes only parsed values.

## Slices

| | Slice | Gate |
|---|---|---|
| 4a | Domain types; `kafka-meta` offsets and liveness tables, topic delete removes offsets; crash sweep | round-trip, SimFs crash sweep: acknowledged commits survive any cut |
| 4b | `GroupMachine` without timers: join (member ids, MEMBER_ID_REQUIRED, static membership), sync, heartbeat, leave, protocol vote, leader election | unit tests per transition; proptest invariants |
| 4c | Timers: session, rebalance timeout, initial delay with extension windows, stale-timer generations | tests with synthetic time (no sleeps), proptest with timers |
| 4d | Actor, registry, handlers and served versions for FindCoordinator, JoinGroup, SyncGroup, Heartbeat, LeaveGroup | `classic-join-flow`, `classic-rebalance-generations`, `classic-rebalance-timeout`, `classic-initial-rebalance-delay`, `joingroup-session-timeout`, `findcoordinator` |
| 4e | OffsetCommit, OffsetFetch, OffsetDelete with the commit fence | `offsetcommit-generations`, part of `group-requests-answers` |
| 4f | DescribeGroups, ListGroups, DeleteGroups (Kafka's hash order for answers) | `group-requests-answers`, `describe-groups-order` (classic steps) |
| 4g | Offsets expiry, liveness persistence, restart; flags `--group-initial-rebalance-delay`, `--offsets-retention` become real | `offsets-retention*` (classic steps), `groups-listed-after-restart` |
| 4h | franz-go consumer group, kcat -G / librdkafka, a rebalance by hand; weight; independent audit | the checkpoint |

Each slice ends green on the usual gates and is committed. Tests follow PLAN rule 10.

## Served versions (Kafka 4 profile)

FindCoordinator 0-4, JoinGroup 0-9, SyncGroup 0-5, Heartbeat 0-4, LeaveGroup 0-5, OffsetCommit 2-9, OffsetFetch 1-9,
OffsetDelete 0, DeleteGroups 0-2, DescribeGroups 0-5, ListGroups 0-5. The other profiles are narrower (see
`kafka-wire` profile tables). Advertised == enforced, as in Phase 3 (`SERVED`).

## Check order (from the recordings; ACLs arrive in Phase 8 and go first when they do)

JoinGroup: 24 (empty/NUL/oversize group id) → 42 (over-long name) → 26 (session bounds 6000..1800000, before the member
id is issued) → 23 (no protocols / empty protocol type) → 82 (held instance, different member id) → 79 (v4+, empty
member id, not static) → 25 (unknown, never issued) → 23 (protocol type mismatch / none in common). An errored join
answers generation -1.
SyncGroup: 24 → 25 (no group) → static check → 25 (no member) → 22 (generation) → 27 (Preparing).
Heartbeat: 24, 25, static check, 25, 22, then 27 only while PreparingRebalance.
LeaveGroup: v3+ top-level 0 with a code per member (25 or 82); v0-2 the member's code at top level; an unknown member
does not rebalance.
OffsetCommit: group fence first (24, then missing-group rules: generation < 0 accepted else 22; static; member; 22; 27 in
CompletingRebalance), then per partition (3 unknown, 12 metadata over 4096 bytes); negative or past-the-end offsets are
accepted. OffsetFetch: unknown anything is offset -1, metadata "", error 0; a null topic list means all stored offsets.
OffsetDelete: 24, 69, then per partition (86 subscribed, 3, delete). DeleteGroups: 68 with members, 69 none.

## Risks

- The commit fence and the missing-group rules are the densest part of the check order; they have the most recorded
  steps. Build them from the recordings, not from the summary above.
- Timers racing completion: handled by generations, tested with synthetic time and by proptest.
- DescribeGroups/DeleteGroups answer order is Kafka's string-hash order (`grouporder.go` in Go); copy the order function
  and test it against the recorded steps.
- Real clients (franz-go sticky/cooperative, librdkafka) exercise paths the recordings do not: found only by running
  them. Budget for it in 4h.
- Phase 3 left a note owed about librdkafka and FindCoordinator (it could not be exercised without groups): re-check it
  with kcat and librdkafka in 4h and write down what is found.
