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
a `CONFORMANCE_ONLY` selection (scenario-name prefixes) is added to the harness at the start of Phase 1.
