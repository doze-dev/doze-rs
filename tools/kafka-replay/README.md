# kafka-replay: the doze-kafka replay oracle

Replays scenarios recorded against real Apache Kafka 4.3.1 against a doze-kafka binary, and compares every answer
step by step. It is the oracle of `docs/port/oracle.md`, moved here from the Go doze-kafka repository's
`conformance/` (branch `rust-oracle`, commit `0dfb284`) so it does not depend on that repository.

A local gate. It is not in CI.

## Run it

```sh
export CARGO_TARGET_DIR=...                       # your own
cargo build --release -p doze-kafka
cd tools/kafka-replay
CONFORMANCE_BIN=$CARGO_TARGET_DIR/release/doze-kafka GOWORK=off \
  go test . -run 'TestReplay$' -count=1 -v -timeout 30m
```

The full replay takes about four and a half minutes and passes 61 of 61 scenarios (2026-11-18). Narrow it with:

- `CONFORMANCE_ONLY=<prefix>,<prefix>`: only the scenarios whose names start with one of the prefixes.
- `-args -only <name>`: one scenario by exact name.
- `KAFKA_REPLAY_PORTS=<lo>-<hi>` (say `39900-39999`): every port the replay and the recorder hand out (the binary's listener, the Docker
  broker's published port) is the first free one in that range, for a machine where the free ports are shared out.

To read a run as a list: `grep -E '^    --- (PASS|FAIL|SKIP)'` over the `-v` output.

## What it does

Each scenario (`scenarios*.go`) is a script of raw Kafka requests (franz-go's `kmsg`). Its recording
(`recordings/<name>.json`) is what Apache Kafka answered, normalised (`normalize.go`: ids, timestamps and other
values a broker makes up are replaced by placeholders). The replay starts the binary, runs the same script, normalises
the answers the same way and compares. A difference fails unless the scenario lists it under `Known` (a written-down
difference); a `Known` entry that matches nothing is not reported, since that list is the Go edition's.

Of the 84 recordings, 61 are replayed. The others are skipped by `TestReplay` without a subtest:

- `DiskMiB` scenarios (`full-disk-*`): recorded on a size-limited volume; the broker's side is its own `SimFs` tests.
- `NoReplay` scenarios: evidence only, recorded from Kafka and never replayed.

## The binary

Each scenario gets a fresh data directory and a free loopback port (never 29181, 29192, 29194 or 39092-39699). The
broker is started as:

```
doze-kafka --listen 127.0.0.1:<port> --data-dir <dir>/data --meta-db <dir>/meta.db \
  --group-initial-rebalance-delay <d> --consumer-session-timeout <d> --offsets-retention <d> --acl-default <allow|deny> \
  [--sasl-mechanisms M,...] [--sasl-user user:password ...] [--acl-super-user User:x ...] [--console --console-addr 127.0.0.1:0]
```

with `DOZE_ZONE=off` and the test knobs `DOZE_KAFKA_TEST_ASSIGNMENT_INTERVAL`, `DOZE_KAFKA_TEST_OFFSETS_CHECK_INTERVAL`
and `DOZE_KAFKA_TEST_CONTROL` (a unix socket; `check-offsets` applies the offsets retention now). The Rust binary reads
`DOZE_KAFKA_TEST_ASSIGNMENT_INTERVAL` and `DOZE_KAFKA_TEST_CONTROL` in every build and derives its check interval from
`--offsets-retention`, so it ignores `DOZE_KAFKA_TEST_OFFSETS_CHECK_INTERVAL`. Its output goes to `broker.log` in the
scenario's directory. SIGTERM must stop it within ten seconds.

What a scenario may change is `config.Config`: only the fields above, so a scenario cannot ask for a broker the replay
has no way to start.

## Files

- `replay_test.go`: `TestReplay`, `CONFORMANCE_ONLY`, `TestRecord`, `TestRecordingsAreCurrent`.
- `recorder.go`: Apache Kafka in Docker (the recorder's target).
- `harness.go`: `Scenario`, `Env` (what a script calls), `Run`, `Compare`, recordings.
- `process_target.go`, `targets.go`: start and stop the binary; flags and knobs; port choice.
- `config/`: the configuration a scenario can set, with the Go edition's defaults.
- `wire.go`, `normalize.go`, `produce_batches.go`: raw connection, normalisation, record batches.
- `scenarios*.go`: the scripts. `version-matrix` uses a fixed API table (`matrixAPIs`), copied from the Go edition's
  Kafka 4 profile, because that is what its recording was made from.
- `recordings/`: the 84 recordings. `claims/`: the Go repository's claims, each citing recordings; kept as data.

## Record

The recorder runs every scenario against `apache/kafka:4.3.1` (`KafkaImage`) in Docker and rewrites `recordings/`. It
needs a running Docker daemon (here: `colima start`); without one it skips and records nothing.

```sh
GOWORK=off go test . -run 'TestRecord$' -count=1 -v -args -record              # every scenario not Frozen
GOWORK=off go test . -run 'TestRecord$' -count=1 -v -args -record -only <name> # one
```

A scenario is recorded against `KafkaImage` unless it names another `Image` (`acl-v0` is recorded against `apache/kafka:3.9.1`, the
last release that serves the ACL APIs at v0, and replayed with `KafkaVersion: 3`, that is `--kafka-version 3`); the two `apiversions-*` scenarios are recorded against `apache/kafka:4.4.0`, which serves ApiVersions to v5 (KIP-1242), as the
Kafka 4 profile does since 2026-11-19). Two step kinds go past
the Kafka framing: `Env.Raw` (a raw SASL token after a SaslHandshake v0, `sasl-handshake-v0`) and `Env.DoOrSilence` (a request Kafka may
leave unanswered, recorded as `no answer`).

Against Kafka a step that answers a coordinator-loading code is asked again until it settles (`Env.settles`); against
doze-kafka never. A new scenario is a `Scenario` in a `scenarios*.go` file, recorded, then replayed.
`TestRecordingsAreCurrent` (no broker, no binary) checks that every scenario has a non-empty recording made against
`KafkaImage`.

The `api-versions` and `client-telemetry` recordings were made by two other recorders of the Go repository
(`TestRecordApiVersions`, `TestRecordClientTelemetry`); they are not scenarios of `TestRecord` and did not come over.

## What did not come over

The Go in-process broker target, the two recorders above, and the Go repository's own checks of its claims, comments
and `docs/limitations.md`.
