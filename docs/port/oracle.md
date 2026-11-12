# The oracle: how the Rust broker is checked

The Go repository's conformance suite replays 81 scenarios recorded from real Apache Kafka 4.3.1.
Branch `rust-oracle` of doze-kafka (commit `259140f`, local) lets it run against **any binary**:

```sh
# in doze-kafka
go build -tags testknobs -o /tmp/doze-kafka-knobs ./cmd/doze-kafka      # the Go edition, for the gate
CONFORMANCE_BIN=/path/to/binary go test ./conformance -count=1 -run 'TestReplay$|TestKnown'
```

Phase 0 gate, met 2026-11-08: the replay passes through the process target against the Go binary
(254 s, 0 failures), as it does in-process.

## What a binary must accept

Every setting a scenario changes already has a flag, and the harness passes them:
`--listen --data-dir --meta-db --group-initial-rebalance-delay --consumer-session-timeout
--offsets-retention --acl-default --acl-super-user --sasl-mechanisms --sasl-user user:password
--console --console-addr`. A scenario whose configuration differs from the defaults in any field the
harness cannot pass **fails loudly** instead of being replayed against a different broker.

The Rust edition does not need `--meta-db` or the console flags until it has those parts; the harness
is told which scenarios to skip by phase (see below).

## The test knobs (Go: build tag `testknobs`; Rust: the control socket below, compiled into every build)

Things no flag covers, passed as environment variables. The Rust edition has only the last: its retention check
interval is derived from `--offsets-retention` (a hundredth of it, between 50 ms and ten minutes), and KIP-848's assignment
interval does not exist yet.

| Variable | Meaning |
|---|---|
| `DOZE_KAFKA_TEST_ASSIGNMENT_INTERVAL` | Go duration: the least time between two assignments |
| `DOZE_KAFKA_TEST_OFFSETS_CHECK_INTERVAL` | Go duration: how often offsets retention runs (`1h` means "only when told") |
| `DOZE_KAFKA_TEST_CONTROL` | path of a unix socket; the line `check-offsets\n` applies the retention now and is answered `ok\n` once it has |

The Rust edition's socket is only opened when the variable is set; it removes a file already at that path only if it is
a socket.

## Process contract

SIGTERM stops the broker cleanly within ten seconds; it listens on the `--listen` address and accepts
connections once ready; stderr/stdout go to `broker.log` in the scenario's directory.

## Per-phase scope

The Go suite has scenarios for every area. A Rust phase is checked against only its area's scenarios:
a `CONFORMANCE_ONLY` selection (scenario-name prefixes) is added to the harness in Phase 3, when there is a
binary to run it against. Until then the wire is checked by its own suites, including the recorded ApiVersions
of real brokers (copied into `crates/kafka-wire/tests/data`).

## Phase 3 result (2026-11-08)

`CONFORMANCE_ONLY` (comma-separated scenario-name prefixes) selects scenarios; a foreign binary's stale
"known difference" entries are not reported (they are the Go edition's own list). Against the Rust binary:

```sh
CONFORMANCE_BIN=target/release/doze-kafka \
CONFORMANCE_ONLY="createtopics-,metadata-autocreate,listoffsets,produce-,fetch-above" \
  go test ./conformance -run 'TestReplay$' -count=1 -v        # in the oracle worktree, GOWORK=off
```

| Scenario | Result |
|---|---|
| createtopics-validation (41 steps) | pass |
| createtopics-collisions | pass |
| listoffsets | pass |
| fetch-above-the-end | pass |
| produce-acks-zero | pass |
| produce-mixed-partition-errors | pass |
| produce-decompressed-size | pass |
| produce-inflated-size | pass |
| produce-validation (about 130 steps) | pass except the idempotent-producer sequence steps (`unknown-pid-seq-*`: OUT_OF_ORDER_SEQUENCE_NUMBER, INVALID_PRODUCER_EPOCH, duplicate answers), which are Phase 6 |
| metadata-autocreate | pass except its one `IncrementalAlterConfigs` step (Phase 7) |

Found by the oracle and not by the broker's own tests (each now has one): gzip with bytes after its last member is
accepted; zstd windows past 128 MiB are unreadable and under it are too large only past this broker's cap; a topic's
`compression.type` makes the broker write the batch again (renumbered, size held as stored); a transactional bit with no
producer id is stored; nonsense after the batch fails it without Kafka's text; the value length is in the timestamp-type text.

## Phase 5 result (2026-11-09)

The consumer-group scenarios pass against the Rust binary: `consumer-assignment-interval`, `consumer-assignment-expiry`,
`consumer-static-leave`, `mixed-protocol-{groups,rolling,parked,cooperative,static}`, `group-requests-refused`,
`describe-groups-order`, `offsets-retention{,-members,-recreate,-restart}`, `groups-listed-after-restart`,
`offset-delete-subscribed`. `--consumer-session-timeout` is a flag; the assignment interval is the test knob
`DOZE_KAFKA_TEST_ASSIGNMENT_INTERVAL`. A full replay passes 38 scenarios; the failing ones are of Phases 6 to 11.
The helper used: `CONFORMANCE_BIN=... CONFORMANCE_ONLY=<prefixes> go test ./conformance -run 'TestReplay$' -v`, output
reduced to the `--- PASS/FAIL` and `differs from` lines.

**Phase 6 (2026-11-09).** `produce-validation` now passes whole (its `unknown-pid-seq-*` steps included), as do
`endtxn-decisions` and `txn-empty-id`. `endtxn-while-preparing` and `full-disk-*` are fault scenarios that `TestReplay` does
not drive (the first is a unit test of the same table, `rules::each_end_is_answered_as_the_recording_says`; the second is
covered with `SimFs`). `idempotent-write-from-topic-write` needs SASL and ACLs (Phase 8). A full replay: 41 scenarios pass and 17 fail,
every failure an API of a later phase: configs, DeleteRecords, CreatePartitions (Phase 7), ACLs and SASL (Phase 8),
telemetry and DescribeCluster (Phase 9).

**Phase 7 (2026-11-09).** `list-config-append` passes; the Phase 7 APIs (DescribeConfigs, AlterConfigs, IncrementalAlterConfigs,
CreatePartitions, DeleteRecords) now match their rows of `version-matrix`, which still differs only on CreateAcls, DeleteAcls
(Phase 8), DescribeCluster and the telemetry APIs (Phase 9); `metadata-autocreate` passes whole. A full replay: 43 scenarios
pass and 15 fail, every failure of Phase 8 (`acl-unknown-codes`, `sasl-plain`, `sasl-scram`, `idempotent-write-from-topic-write`,
`version-matrix` in its ACL rows) or Phase 9 (`telemetry-*`, DescribeCluster). No recording covers compaction itself;
`interop/` scenario `compaction` (franz-go, sarama, librdkafka) and `compaction.md` do.

**Phase 8, slice 8b (2026-11-12).** `sasl-plain` (whole: its fixture's CreateAcls works and the 13 `*-who` probes answer 29),
`acl-unknown-codes` (77 steps) and `idempotent-write-from-topic-write` pass. `version-matrix` has no ACL row left among its differences; it
differs only on DescribeCluster v0 to v2 and GetTelemetrySubscriptions and PushTelemetry (Phase 9). The harness passes `--acl-default deny`
and `--acl-super-user User:admin` for these scenarios and both are honoured.
