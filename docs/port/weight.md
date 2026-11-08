# Weight

Weight is a feature. Numbers may only rise with a written reason below. They are measured on
darwin/arm64 with `cargo build --release` and compared with the Go build.

| | Go doze-kafka (4081fa6) | Rust (this repo) |
|---|---|---|
| Binary | 28,415,122 B | 339,360 B (empty crates, Phase 0) |
| Idle heap | 1,368,256 B | not measured yet |
| Startup to ready | not recorded | not measured yet |
| Crates / modules | 422 Go modules | 0 dependencies (Phase 0) |

The first real baseline is taken at the end of Phase 3 (a broker that real clients use).
