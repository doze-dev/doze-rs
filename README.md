# doze-rs

> [!WARNING]
> **Work in progress.** This is the Rust rewrite of doze, and it is not ready for use. Expect breaking
> changes, missing pieces and plans that move. The Go repositories remain the working product and the
> reference until each part here reaches parity.

[doze](https://github.com/doze-dev/doze) runs local development infrastructure (Postgres, Kafka, local
AWS, and more) from one config file, and puts what you are not using to sleep. This repository rebuilds
it in Rust: one Cargo workspace, each part its own crate, written for Rust from the ground up rather
than translated from Go.

## What is here

| Part | State | Where |
|---|---|---|
| doze-kafka: a Kafka broker for local development (one binary, single node, disk-backed) | being ported first | `crates/kafka-*`, `crates/doze-kafka`, plan in `docs/port/PLAN.md` |
| doze-aws: local AWS services | planned and measured against recordings from real AWS | `docs/port/aws/`, `conformance/` |
| The doze CLI, daemon, desktop app and module SDK | planned | not in this repository yet |

Targets: macOS (Apple Silicon), Linux (x86-64 and Arm) and Windows (x86-64 and Arm).

## Build

```sh
cargo build
cargo run -p doze-kafka
```

### The doze-kafka web console

`doze-kafka` serves a web console beside the broker, on `127.0.0.1:9093` by default. It has no login and
acts on the broker as a super user, because it is a tool for one developer on their own machine. On a
shared machine that means any local process or user can use the loopback console without the broker's
password or ACLs. Turn it off with `--console=false` (and the Lab, its fault injection, with `--lab=false`).

The console is never allowed to be a wider door than the broker itself: serving it beyond loopback
(`--console-addr 0.0.0.0:9093`) is refused at start when SASL or TLS is on, when ACLs are in force
(`--acl-default deny`, or ACLs stored in the data directory), or when `--listen` keeps Kafka on loopback.
The refusal says what would be exposed. Keep the console on loopback, turn it off, or accept the exposure with
`--console-unprotected`. Served beyond loopback, the Lab starts off unless `--lab` asks for it.

The rule holds while the broker runs, not only at start. A broker with nothing guarding it may serve the console
beyond loopback without the flag; if ACLs are made later (over Kafka or from the console), the console stops
answering other machines while there are any, says why on the page, and answers them again once every ACL is
deleted. It still answers this machine at a loopback address. With `--console-unprotected` it never closes.

## License

Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE). Vendored third-party code under
`vendor/` keeps its own license.
