# doze-kafka (standalone)

A Kafka broker for local development: one binary, a single node, data on disk. Real Kafka clients (Java, librdkafka,
franz-go, sarama, kafka-go and the rest) talk to it as they would to Kafka. It starts in under a tenth of a second and
sits at about 7 MB of memory when idle.

This is the standalone build. It has the whole broker: topics, consumer groups (classic and the new protocol),
transactions, compaction, retention, SASL (PLAIN, SCRAM) and ACLs. It leaves out what the build that doze runs adds
around the broker: the web console, the schema registry, TLS and the doze module face. To get those, run Kafka through
doze (https://github.com/doze-dev/doze). Their flags are accepted here and ignored with a warning, so one command line
works with either build.

## Run it

    ./doze-kafka

It listens on `127.0.0.1:9092` and keeps its data in `./data`. Point a client at it:

    bootstrap.servers=127.0.0.1:9092

Some flags:

| Flag | Default | |
|---|---|---|
| `--listen HOST:PORT` | `127.0.0.1:9092` | where it listens |
| `--data-dir DIR` | `./data` | the metadata database and the partition logs |
| `--advertised-host`, `--advertised-port` | the listen address | what clients are told to reconnect to: set them when the broker is reached through another address (a container, a proxy) |
| `--kafka-version 1-4` | `4` | the Kafka release to stand for: 1.1, 2.8, 3.9 or 4.4 |
| `--auto-create-topics` | `true` | make unknown topics when a client asks about them |
| `--default-partitions N` | `1` | partitions for an auto-created topic |
| `--sasl-mechanisms`, `--sasl-user USER:PASSWORD` | none | ask clients to log in |

`./doze-kafka --help` lists every flag. `./doze-kafka --version` says which release this is.

## macOS

The binary is not notarised. If you downloaded the archive with a browser, macOS will refuse to open it; clear the
quarantine flag once:

    xattr -d com.apple.quarantine ./doze-kafka

An archive fetched with `curl` carries no such flag.

## Checking the download

`checksums.txt`, beside the archives on the release, holds each archive's SHA-256:

    shasum -a 256 -c checksums.txt --ignore-missing

## Licence

Apache License 2.0: see `LICENSE` and `NOTICE`. Source: https://github.com/doze-dev/doze-rs (`crates/doze-kafka`).
