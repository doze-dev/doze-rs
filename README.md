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

`just` lists the repo's common tasks (the gates, CI's jobs run locally, fuzzing, release builds, the desktop app and
the consoles). Each recipe is a name for a command the repo already runs, so the justfile and CI cannot drift.

Before a push: `just push`. It runs `just prepush` (everything this Mac can run that `.github/workflows/ci.yml` runs, with
Linux in the `doze-ubuntu` Lima VM and a Windows type-check through cargo-xwin) and pushes only if that passed on the same
HEAD. `just prepush-plan` shows what it would run for the current diff and why; each run leaves a proof log in
`.git/prepush/<sha>.log`; what it cannot cover (Windows tests, x86_64 Linux, idle memory on CI's runners) it names in its summary.

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

### The doze-aws web console

`doze-aws` serves a web console beside its endpoint, on loopback at the endpoint's port + 1 (`127.0.0.1:4567`
beside the default `127.0.0.1:4566`), and prints its address when it starts. It shows the calls your app makes
(Live), every resource the services hold, IAM as your app meets it (Access), a Lab to send any built operation,
and the settings. It has no login: it is served on this machine only, even when `--listen` serves the endpoint
wider, and an address beyond loopback (`--console 0.0.0.0:4567`) is refused at start. Its own calls use the key
`console`, which is not one IAM issued, so they act as the account root and are allowed in every IAM mode.

Name another address with `--console 127.0.0.1:<port>`; turn it off with `--no-console`, `--console off`,
`DOZE_AWS_CONSOLE=off` or `console = "off"` in `doze-aws.toml`. When the default port is taken, doze-aws says so
and runs without the console; an address you named that is taken stops the start.

## License

Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE). Vendored third-party code under
`vendor/` keeps its own license.
