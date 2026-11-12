# Weight

Weight is a feature. Numbers may only rise with a written reason below. They are measured on
darwin/arm64 with `cargo build --release` and compared with the Go build.

| | Go doze-kafka (4081fa6) | Rust (Phase 3, f7fcf18 and after) |
|---|---|---|
| Binary (release, stripped) | 28,415,122 B | 3,386,544 B (3,099,968 B before classic groups) |
| Crates / modules linked | 422 Go modules | 106 unique packages in `cargo tree` (7 ours; many are macro and build-time only; `rlimit` and `getrandom` added since Phase 3) |
| Idle RSS, empty broker | 26.8 MB, 12 threads | 4.4 MB, 3 threads |
| Ready, fresh data directory | 25 ms | 55 ms (the first start makes and flushes the metadata database) |
| Restart with 3,000 partitions | 33 ms, 25 MB | 32 ms, 4.3 MB |
| 300 topics x 10 partitions: create | 4.8 s | 2.7 s (both are flush-bound) |
| ... then ListOffsets for all 3,000 | 174 ms | 58 ms |
| ... resident, descriptors | 33.6 MB, 17 | 7.7 MB, 16 |
| 200k x 1000 B random records, 3 partitions, acks=1, produce / consume | 1.26M / 1.88M records/s | 1.81M / 2.07M records/s |
| same, acks=all (flush per group) | 130k records/s | 176k records/s |
| Resident after that load | 47 MB | 23 MB with the frame pool (200 MB before it; see below) |

Measured 2026-11-08 on an M5 Max, darwin/arm64, release builds, the broker and a franz-go client on loopback (so the
client shares the machine and the rates are a comparison, not a benchmark). Scripts were throwaway; the method is in
the PLAN log. Rust: `--worker-threads 2` (the default), `--max-open-partitions 512` (the default).

**Resident memory after a heavy produce, and the frame pool (2026-11-09).** Before the pool, producing 200 MB in
requests of about 1 MiB left the Rust broker at 200 MB resident on macOS and then it stopped growing. `vmmap` showed
freed memory the allocator keeps ("Malloc Large (empty)"); the live heap was 253 KB. Each request was read into a
buffer of its own and macOS's allocator caches freed large blocks instead of returning them. Linux (glibc) returned them
at once: 43 MB.

What was tried, on the same 200k x 1000 B load:

| | macOS after load | Linux after load | Cost |
|---|---|---|---|
| system allocator, no pool | 200 MB | 43 MB | none |
| jemalloc / mimalloc as `#[global_allocator]` | 43 MB | not needed | a C dependency, a build step |
| **pool of kept frame buffers (kafka-server/src/pool.rs)** | **22 to 24 MB** (acks=1 and acks=all) | 38 MB (peak 48) | 119 lines, no dependency, no unsafe |

Throughput is unchanged (acks=1 1.66M to 1.83M records/s against 1.62M to 1.72M; acks=all 172k either way). The pool
is the `Bytes::from_owner` shape: a frame of 64 KiB to 16 MiB is read straight into a `Vec` taken from a shelf for its
size class (powers of two), travels as `Bytes`, and goes back to the shelf when the last copy is dropped. The shelves
hold at most 16 buffers per class and 64 MiB in all, so the broker's worst resident cost is that plus what is in flight.
Frames over 16 MiB are read in chunks as before, so a peer's claimed length is not allocated before it sends the bytes.
No published crate fit: zeropool's buffer borrows its pool and cannot be `'static` (needed for `from_owner`), and
buffer-pool pulls in a heavy dependency (survey in the PLAN log).

What the numbers are not: a durability comparison. The Rust broker flushes metadata once per commit; the Go broker's
metadata store flushes twice. Both flush partitions per group of acks=all producers.

## Metadata store (redb 4.5.0, measured 2026-11-08 on an M5 Max, darwin/arm64, release)

A spike with 10,000 offset keys and the same workload in Go on bbolt 1.5.0: redb adds 606 KB to a stripped
binary (946 KB against 339 KB for an empty main); idle RSS is 3.6 to 3.9 MB against 6.9 to 7.2 MB for bbolt, and
5.1 MB against 13.5 MB after the workload; no background threads (bbolt's Go process has 5 to 10); the data file is
1.06 MB against 2.1 MB. A durable one-key commit is 3.1 ms at the median against 6.0 ms, because redb syncs once
per commit and bbolt twice (both use `F_FULLFSYNC` on macOS). The disk's flush time drifts between 3 and 12 ms from
run to run, so absolute times are good to about 2x; only the ratio is reliable. These are an input to the Phase 3
baseline, not part of it.

## Partition log (kafka-storage, measured by the Phase 2 audit, 2026-11-08)

An open partition holds about 1.7 KB and 12 allocations, plus about 300 B and 4 allocations per extra segment; nothing
grows with the bytes of data (the index is read from the file, not held). Each segment holds two file descriptors: 5,000
partitions of 10 segments each is about 22 MB of heap and **100,000 descriptors**, which is beyond common limits and so
a Phase 3 design item (lazy open, or a descriptor cache). A fetch allocates the bytes it returns plus up to 16 KiB.

**Phase 3 answer to the descriptor item:** at most `--max-open-partitions` (512) partitions are open; one over the limit closes
the least recently used that nothing holds, after flushing it, and it opens again when asked. A ListOffsets for a
partition that was never written to answers without opening it. A partition with many segments
holds two descriptors for each, so open partitions are also bounded by the files they hold between them (a budget taken from
the limit on descriptors; debt D1, 2026-11-09).

## Phase 6 (transactions), measured 2026-11-09, release, darwin/arm64

Binary 3,776,752 B after the debt pass (3.62 MB at Phase 5: +155 KB for idempotence, the transaction index, the coordinator,
the id expiry and the descriptor budget), 107 packages (none added), idle RSS 4.4 MB. The producer tracker keeps at most five
batches per producer and 50,000 producers per partition, built when a partition is first written to, not at start. The
coordinator keeps an entry for each transactional id in use, forgotten after seven days with nothing open. A log that never
held a transaction is not read for them (a note beside its segments, made before the first, says one may be); one that did is
read once, when it is opened. Open partitions are bounded by the files they hold as well as by their number.

## Phase 7 (config APIs, CreatePartitions, retention, compaction), measured 2026-11-09, release, darwin/arm64
Binary 4,012,832 B (3,776,752 B after the Phase 6 debt pass: +236 KB for three config APIs, CreatePartitions, the retention
and cleaner tasks, and compaction), 107 packages (none added), idle RSS 4.6 to 4.7 MB (4,592 to 4,720 KB measured by the
Phase 7 audit with 3 threads; 4.5 MB on a quieter run). The cleaner holds nothing while idle: a
round builds an offset map of at most 16 MiB of keys (`Config.cleaner_map_bytes`) and drops it when the round ends; a round reads
a segment 1 MiB at a time and writes a segment's replacement only from the first change in it. The live defaults are one small
shared value; dynamic broker configs are five numbers.

**At the close of Phase 7 (2026-11-10, after both audits' fixes, release, darwin/arm64):** binary 4,029,504 B (+16,672 B over the
figure above: removing a segment cleaning has emptied, the install in parts, the Kafka-worded config value check, the producer-state
rule for a marker found at open), 107 packages (none added), idle RSS 4,816 KB with 3 threads. All three under the CI limits
(the CI job runs on Linux x86_64: 5.15 MB, which was measured at 4.90 MB there; 115 packages counted with `sort -u`; 8 MB idle). The
4.2 MB figure that stood here is the darwin/arm64 one and is not what CI gates.

## Phase 8, slice 8f (users over the API, the users file), measured 2026-11-12, release, darwin/arm64

Binary 4,250,384 B (4,183,056 B at the close of 8e, built the same way: **+67,328 B, +1.6%**, for the credentials table, the two handlers and their rules, the layered user lookup and the users-file and environment parsers), **123 packages, none added** (the same
`cargo tree -p doze-kafka --prefix none --edges normal | sort -u | wc -l` as the CI; the `arc-swap` that `kafka-auth` now names was already in the build through the broker), idle RSS 4,880 KB without SASL (2 worker threads, 3 in all; 4,752 KB at 8e). Inside every limit of
the CI weight job (the Linux size is the CI's to measure; +67 KB is under its 5% margin: 4,899,328 B measured at 8c, plus 34 KB of 8c and 8e's 384 B, plus this, is about 4.97 MB against the 5.15 MB limit). The broker reads the credentials table once when it opens (an empty one costs a read transaction).

## Phase 8, slice 8e (TLS, the `tls` cargo feature), measured 2026-11-12, release, darwin/arm64

**The default build is unchanged.** Binary 4,183,056 B against 4,166,544 B for the base commit built the same way on the same machine (one more 16 KB page of `__TEXT`: `__text` grew by 384 B, the two new
parameters of `serve` and of the accept loop, and the longer refusal message of `--tls-cert`), **123 packages by the CI method on darwin both before and after** (the 122 of the 8c paragraph is the Linux count;
darwin counts one more), idle RSS 4,752 KB (2 worker threads, 3 in all). No `rustls`, `ring` or `tokio-rustls` is in the default tree (`cargo tree -p doze-kafka --prefix none --edges normal | sort -u`).

**The `tls` build** (`cargo build --release -p doze-kafka --features tls`): binary **5,021,984 B (+838,928 B, +20%)**, **134 packages (+11)**, idle RSS 4,848 KB with the feature compiled in and no certificate, **5,456 KB with a certificate
configured and no connection** (5,280 KB with an RSA one), 5,776 KB with one TLS connection open. The reason: rustls 0.23 (`default-features = false`, `std`, `tls12`, `ring`), tokio-rustls 0.26 (`tls12`), and their
dependencies `ring`, `rustls-webpki`, `rustls-pki-types`, `untrusted`, `zeroize`, `once_cell`, `getrandom 0.2` (ring's); PEM is read by `rustls-pki-types`'s own `PemObject` (`rustls-pemfile` is unmaintained and fails
`cargo deny`), and no certificate is generated (`rcgen` would add 9 packages and 68 KB). The spike (`phase8/prior-art-weight.md`) measured +872,608 B and +12 packages on an echo server; the broker, which already
has tokio's pieces, is +838,928 B and +11 here. ring is the first dependency with C and assembly (its build script needs a C compiler; `aws-lc-rs` would be +2.3 MB). `cargo deny check` (licences, bans, sources, advisories)
passes with the feature on: `deny.toml` already checks `all-features`.

**CI.** A second job, `weight-tls`, builds `-p doze-kafka --features tls` with its own gates: size 6,100,000 B (the Linux x86_64 default build is 4,899,328 B, measured; the darwin step is +0.84 MB, taken as +0.9 MB as in the spike because
ring's x86_64 code is not the arm64 code: 4,899,328 + 900,000 = 5,799,328 B; +5% headroom = 6,089,294 B, rounded up to 6,100,000), packages 137 (the Linux default 122 + 11 measured here + 4), idle RSS 8192 KB, measured with a certificate configured. The default job's gates (5.15 MB, 126 packages, 8 MB) stand
and need no exception. The numbers of the Linux job are the CI's to measure; this machine cannot.

**What the feature needs to build:** a C compiler (and, for ring on some targets, an assembler). Nothing in the default build needs one, so the `windows` CI job (`cargo check --workspace --all-targets`) and a developer
without a toolchain are unaffected; a Windows developer who wants TLS needs the MSVC build tools.

## Phase 8, slice 8c (SCRAM), measured 2026-11-12, release, darwin/arm64

Binary 4,098,432 B (4,064,208 B at the close of 8a: +34,224 B for `scram.rs`, its parsers and keys, the second login round and the two derived verifiers),
**122 packages, none added** (the same `cargo tree -p doze-kafka --prefix none --edges normal | sort -u | wc -l` as the CI; `getrandom`, which the server nonce and the
salts use, was already in the build through the broker and the metadata store, and SCRAM uses the three crates 8a added), idle RSS 4,736 KB without SASL and
4,816 KB with all three mechanisms and two users (3 threads). Inside every limit of the CI weight job (the Linux figure for the size is the CI's to measure; +34 KB
is under its 5% margin).

## Phase 8, slice 8a (SASL/PLAIN, the login session), measured 2026-11-12, release, darwin/arm64

Binary 4,064,208 B (4,029,504 B at the close of Phase 7: +34,704 B for the `kafka-auth` crate, the login session and the SASL
flags, and hmac, sha2 and subtle), **122 packages (107 before: +15 lines of `cargo tree --prefix none --edges normal | sort -u`, which
is 12 distinct packages: `kafka-auth`, `hmac`, `sha2`, `subtle`, `digest`, `crypto-common`, `block-buffer`, `hybrid-array`, `typenum`,
`cmov`, `ctutils`, `cpufeatures`)**, idle RSS 4,816 KB without SASL and 4,848 KB with it (3 threads); the login state is one small enum
and an `Arc` per connection, and the users are a map built at start.

**The reason for the larger package count (owner's decision D1, 2026-11-12):** the cryptography for SASL is RustCrypto (`hmac`,
`sha2`, `subtle`: pure Rust, no C compiler, no primitives of our own); PBKDF2 and base64 are written in-tree on top of them
(`kafka-auth/src/crypto/`), which is why the step is 12 packages and not the 14 the spike measured with `pbkdf2` and `base64` as
crates (`pbkdf2` is a dev-dependency only, for a differential test). The CI limit on packages moves from 115 to **126** (the measured 122 plus 4, so the
next small dependency needs no second exception); the binary limit (5.15 MB on Linux) and the idle limit (8 MB) stand. The one
file to change if the backend is ever swapped is `crypto.rs`. SCRAM (slice 8c) adds no package: it uses the same three crates.
