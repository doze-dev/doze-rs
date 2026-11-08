# Patches to kafka-protocol 0.18.0

This is `kafka-protocol` 0.18.0 from crates.io, vendored, with the changes below. Each is marked
`PATCH` in the source. Upstream: https://github.com/tychedelia/kafka-protocol-rs

## 1. Array decoding does not trust the wire count (`src/protocol/types.rs`)

`Array` and `CompactArray` decoding did `Vec::with_capacity(n)` with `n` read straight from the
request. A 5-byte Metadata request therefore asked the allocator for 144 GiB, and a tiny JoinGroup for
180 GiB (measured with a counting allocator, 2026-11-08). On Linux that allocation fails and Rust aborts
the process: one malformed packet from any client kills the broker, before authentication.

The capacity is now `min(n, 1024)` (`PREALLOC_LIMIT`). It is only a hint, so correctness is unchanged:
the vector grows (amortised) as elements really decode, and a short buffer fails with the usual error.

A first version capped it at the bytes left in the buffer. An independent audit measured that as too
weak: elements are 60 to 120 bytes and a count equal to the bytes left is still believable, so a 1 MiB
Metadata body reserved 76 MB and a 100 MiB Produce body about 9.7 GiB (96 times). With the cap at 1024
elements a lying count reserves at most about 100 KiB per array.

Regression test: `crates/kafka-wire/tests/hostile_input.rs`.

## Not patched, and not used

`src/records.rs` (record decoding) and `src/compression/*` have the same kind of unbounded allocation
(`records.reserve(record_count)`, `IndexMap::with_capacity(num_headers)`, snappy `resize`). doze-kafka does
not use them: its own `kafka-wire` parses record batches in place and decompresses with explicit size caps.

## 2. Lints silenced in `Cargo.toml`

`[lints.rust] deprecated = "allow"` and `dead_code = "allow"`: 560 warnings from upstream's generated code
(`std::u32::MAX` and unused structs), which would otherwise fail a `-D warnings` build of this workspace.
