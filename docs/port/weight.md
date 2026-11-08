# Weight

Weight is a feature. Numbers may only rise with a written reason below. They are measured on
darwin/arm64 with `cargo build --release` and compared with the Go build.

| | Go doze-kafka (4081fa6) | Rust (Phase 3, f7fcf18 and after) |
|---|---|---|
| Binary (release, stripped) | 28,415,122 B | 3,049,696 B |
| Crates / modules linked | 422 Go modules | 89 crates (about half are macro and build-time only) |
| Idle RSS, empty broker | 26.8 MB, 12 threads | 4.4 MB, 3 threads |
| Ready, fresh data directory | 25 ms | 55 ms (the first start makes and flushes the metadata database) |
| Restart with 3,000 partitions | 33 ms, 25 MB | 32 ms, 4.3 MB |
| 300 topics x 10 partitions: create | 4.8 s | 2.7 s (both are flush-bound) |
| ... then ListOffsets for all 3,000 | 174 ms | 58 ms |
| ... resident, descriptors | 33.6 MB, 17 | 7.7 MB, 16 |
| 200k x 1000 B random records, 3 partitions, acks=1, produce / consume | 1.26M / 1.88M records/s | 1.81M / 2.07M records/s |
| same, acks=all (flush per group) | 130k records/s | 176k records/s |
| Resident after that load | 47 MB | 200 MB (see below) |

Measured 2026-11-08 on an M5 Max, darwin/arm64, release builds, the broker and a franz-go client on loopback (so the
client shares the machine and the rates are a comparison, not a benchmark). Scripts were throwaway; the method is in
the PLAN log. Rust: `--worker-threads 2` (the default), `--max-open-partitions 512` (the default).

**The one number that goes the wrong way: resident memory after a heavy produce.** Producing 100 MB in requests of about
1 MiB leaves the Rust broker at about 110 MB resident and 200 MB after 200 MB, and then it stops growing (a second and a
third run add nothing). `vmmap` shows it is freed memory the allocator keeps: "Malloc Large (empty)", 67 regions of
about 1.4 MiB, 95 MB resident, while the live heap is 253 KB. Each request is read into a buffer of its own (the
batch is then written from that buffer without a copy), and macOS's allocator caches the freed large blocks instead of
returning them. It is reclaimed under memory pressure and costs nothing at idle, but it is not flat. Not yet measured on
Linux (glibc returns large blocks at once, so it is expected to be much lower). Candidates if it matters: reuse of
request buffers per connection, or a different allocator (none that is pure Rust and production grade).

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
partition that was never written to answers without opening it. Left: sealed segments of an open partition still
hold two descriptors each (lazy opening of sealed segments is not done): a partition with 100 segments holds 200.
