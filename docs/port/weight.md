# Weight

Weight is a feature. Numbers may only rise with a written reason below. They are measured on
darwin/arm64 with `cargo build --release` and compared with the Go build.

| | Go doze-kafka (4081fa6) | Rust (this repo) |
|---|---|---|
| Binary | 28,415,122 B | 339,360 B (empty crates, Phase 0) |
| Idle heap | 1,368,256 B | not measured yet |
| Startup to ready | not recorded | not measured yet |
| Crates / modules | 422 Go modules | 0 dependencies (Phase 0) |

The wire crate (Phase 1) adds 17 crates to the build, almost all of them `kafka-protocol` (vendored) and
`thiserror`'s macro stack (compile time only); none of it is linked into `doze-kafka` until Phase 3. The
dependency-free release binary stays 339,360 B.

The first real baseline is taken at the end of Phase 3 (a broker that real clients use).

## Metadata store (redb 4.5.0, measured 2026-11-08 on an M5 Max, darwin/arm64, release)

A spike with 10,000 offset keys and the same workload in Go on bbolt 1.5.0: redb adds 606 KB to a stripped
binary (946 KB against 339 KB for an empty main); idle RSS is 3.6 to 3.9 MB against 6.9 to 7.2 MB for bbolt, and
5.1 MB against 13.5 MB after the workload; no background threads (bbolt's Go process has 5 to 10); the data file is
1.06 MB against 2.1 MB. A durable one-key commit is 3.1 ms at the median against 6.0 ms, because redb syncs once
per commit and bbolt twice (both use `F_FULLFSYNC` on macOS). The disk's flush time drifts between 3 and 12 ms from
run to run, so absolute times are good to about 2x; only the ratio is reliable. These are an input to the Phase 3
baseline, not part of it.
