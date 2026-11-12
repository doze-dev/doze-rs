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

## License

Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE). Vendored third-party code under
`vendor/` keeps its own license.
