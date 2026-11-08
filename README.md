# doze-rs

> [!WARNING]
> **Still in active development.** This project is not cleared for use yet. Expect breaking changes, and do not rely on it for real work.

doze in Rust: one Cargo workspace, each part its own crate. doze-kafka is ported
first; see [docs/kafka-port.md](docs/kafka-port.md). The Go repositories stay
the reference until each port reaches parity.

```sh
cargo build
cargo run -p doze-kafka
```
