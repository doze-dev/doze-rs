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

## The test knobs (Rust: cargo feature `test-knobs`; Go: build tag `testknobs`)

Three things no flag covers, passed as environment variables:

| Variable | Meaning |
|---|---|
| `DOZE_KAFKA_TEST_ASSIGNMENT_INTERVAL` | Go duration: the least time between two assignments |
| `DOZE_KAFKA_TEST_OFFSETS_CHECK_INTERVAL` | Go duration: how often offsets retention runs (`1h` means "only when told") |
| `DOZE_KAFKA_TEST_CONTROL` | path of a unix socket; the line `check-offsets\n` applies the retention now and is answered `ok\n` once it has |

A default build compiles none of this.

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
