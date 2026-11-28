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

## Phase 8 close (security: SASL, ACLs, TLS, the guard), measured 2026-11-12 on branch p8g, release, darwin/arm64

**The default build** (`cargo build --release -p doze-kafka`, `CARGO_BUILD_JOBS=3`, the slices 8a to 8g in): binary **4,267,504 B** (4,029,504 B at the close of Phase 7: **+238,000 B, +5.9%**),
**123 packages** (`cargo tree -p doze-kafka --prefix none --edges normal | sort -u | wc -l`; 107 at Phase 7: +16 lines, the 15 of 8a (12 distinct packages: `kafka-auth`, `hmac`, `sha2`, `subtle`, `digest`, `crypto-common`, `block-buffer`,
`hybrid-array`, `typenum`, `cmov`, `ctutils`, `cpufeatures`) and the one line by which the darwin count stands above 8a's, as the 8e paragraph records; the Linux CI figure is the CI's to measure), idle RSS **4,960 KB** (3 threads, a fresh data
directory, 3 s after start, `ps -o rss`; 4,816 KB at the close of Phase 7). The `tls` build, not repeated at 8g (the guard and flags are in the default build and the feature adds nothing to them): **5,021,984 B and 134 packages at 8e**, idle 5,456 KB with a certificate
configured and no connection (all measured in the 8e paragraph below). Nothing in the default tree is a C or assembly build: `rustls`, `ring` and `tokio-rustls` are only behind the feature.

The per-slice deltas of the default build, so that the sum is visible (the figure for 8b alone was not taken: the 8c paragraph measured from the close of 8a, so 8b and 8d are one step):

| Slice | What | Binary | Delta | Packages (by the CI method) | Where measured |
|---|---|---|---|---|---|
| (Phase 7 close) | | 4,029,504 B | | 107 | the Phase 7 paragraph |
| 8a | `kafka-auth`, PLAIN, the login session, the SASL flags, hmac/sha2/subtle | 4,064,208 B | +34,704 B | 122 (+15 lines, 12 distinct packages; the CI gate rose from 115 to 126) | the 8a paragraph |
| 8c | SCRAM-SHA-256 and -512 (measured from the close of 8a) | 4,098,432 B | +34,224 B | 122 (none) | the 8c paragraph |
| 8b + 8d | the authorizer and three ACL APIs; the matrix on 45 handlers (one atomic load per check with no ACLs) | 4,166,544 B | +68,112 B | none | the base commit of the 8e paragraph |
| 8e (default build) | the `tls` parameters, the longer refusal text | 4,183,056 B | +16,512 B | 123 on darwin (122 + 1 in the counting) | the 8e paragraph |
| 8f | the credential APIs 50 and 51, the credential table, the users sources | 4,250,384 B | +67,328 B | 123 (none) | the 8f paragraph |
| 8g | the guard (`hostguard.rs`), the console flags, the start-up rule and notes, exit code 2 | 4,267,504 B | +17,120 B | 123 (none) | this paragraph |
| **Sum** | | **4,267,504 B** | **+238,000 B** | **107 to 123** | |
| 8e (`tls` build, optional) | rustls 0.23 + tokio-rustls 0.26 on ring | 5,021,984 B | +838,928 B on top of 4,183,056 B | 134 (+11) | the 8e paragraph |

The two largest steps are the matrix (8b + 8d, about 68 KB for the checks and their texts) and the credential APIs (8f, 67 KB); the 12 packages of 8a are the only step in the package count, and the one the owner chose
(decision D1: RustCrypto, pure Rust, no C compiler). The guard is 107 lines and a few KB; the 8g step is mostly the start-up rule's messages and `clap`'s two flags.

**The CI limits now** (`.github/workflows/ci.yml`; Linux x86_64, the CI measures): the `weight` job, default build: binary **5,150,000 B** (4,899,328 B is the Linux figure measured before Phase 8 started, commit affb6ba, and the limit is that plus 5%; the 8a to 8g steps add 238 KB on darwin, so if they carry over one for one the Linux binary is about
**5,137,000 B, 0.25% under the limit: the first small addition will need a written rise**. The 8f paragraph's estimate of 4.97 MB added only 8c, 8e and 8f to that base and, by the date of the figure, left out 8a, 8b and 8d (about 137 KB), so it was too low. The darwin-to-Linux ratio of the steps is unmeasured and the CI's first run after this merge settles it), packages **126** (123 measured here, 3 spare), idle RSS **8,192 KB**. The `weight-tls` job, `--features tls` with a
certificate configured: binary **6,100,000 B**, packages **137**, idle **8,192 KB**. If the `weight` job fails on size after this merge, the figure to move to is the CI's own measurement plus about 5%, with the reason written here.
Neither job has run on this branch (no CI here); the **Linux weight of the `tls` build has never been measured**, only its limit set from the darwin delta (debt I3).

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

**Phase 8, after the audits (2026-11-12, release, darwin/arm64, commit e6d986b):** binary 4,301,200 B (Phase 7 closed at 4,029,504 B: +271,696 B, +6.7%; the 8g
figure above was 4,267,504 B, so the audit fixes cost +33,696 B: the credential APIs' handling of super users, the rights that members carry in the group
machines, the failed-login delay, the lingering close, the private file creation), 123 packages by the CI method (unchanged since 8a), idle RSS 4,928 KB with 3
threads (4,816 KB at the close of Phase 7). On Linux x86_64 the size limit (5.15 MB, measured 4.90 MB at the close of Phase 7) has no real headroom left if the darwin
steps carry over (about 4.90 + 0.27 = 5.17 MB would be over): the first CI run decides, and the limit is to be set from the CI's own measurement plus 5% with the
reason written here (debt.md I4). The default build is still pure Rust (no C compiler); TLS stays the off-by-default `tls` feature.

## Phase 9, slice 9a (the console in the default build), measured 2026-11-12, release

The owner decided (D1, 2026-11-12) that the console is in the default build and that the size gate rises by about 1 MB with headroom. The measured cost is larger
than the estimate in `phase9.md` (0.8 to 0.95 MB on Linux), so the gate rises by more than the owner's 1 MB, and the reason is here.

| Build | darwin arm64 (this Mac) | Linux aarch64 glibc (the `doze-ubuntu` Lima VM, native build) |
|---|---|---|
| lean (`--no-default-features`, what Phase 8 closed on) | 4,301,232 B, 122 packages, idle 5,008 KB | 4,660,632 B, 122 packages, idle 5,236 KB, 3 threads |
| default (the console) | 5,521,456 B, 150 packages, idle 5,936 KB (7,184 KB after two page loads) | 5,971,376 B, 150 packages, idle 6,532 KB, 3 threads |
| the console's step | +1,220,224 B (+28%), +28 packages | +1,310,744 B (+28%), +28 packages |

Where the megabyte goes is only partly measured: the embedded assets (CSS, two fonts, the script) are about 140 KB (the agent's count); the HTTP stack was measured by the research at +309 KB and 25 packages (darwin); the rest, about 0.7 MB, is the pages' markup built in code, the backend, the probe, the stream and `ConsoleView`, and was not split further (a `cargo bloat` pass is a task for the first slice that adds a lot of pages). The estimate had the pages and recorder as 100 to 200 KB and 80 KB; the pages are larger than that, and nine more slices of pages follow.

There is no x86_64 machine here and no CI has run, so the ubuntu x86_64 figure is an **estimate from two measurements that agree**: the darwin figure times the
ratio measured at the close of Phase 7 (4,899,328 B on ubuntu x86_64 over 4,029,504 B on darwin, 1.216) gives 6.71 MB for the default build and 5.23 MB for the lean
one; the Linux VM figure times about 1.12 (x86_64 code against aarch64: the lean build gives 4.66 x 1.12 = 5.22 MB, which agrees with the first) gives 6.69 MB. So the
Phase 8 close, which was thought to be at the edge of the old 5.15 MB gate, was already over it by these numbers (5.23 MB); that gate was never exercised.

**Limits set in `ci.yml`** (the measured estimate plus about 5%; the first CI run replaces the estimate with a measurement and this paragraph is then rewritten):

| Job | Binary | Packages | Idle RSS |
|---|---|---|---|
| `weight` (default, with the console) | **9 MB** (was 5.15; see below) | **154** (was 126) | 8 MB (unchanged; 6.5 MB measured in the VM) |
| `weight-lean` (new; `--no-default-features`, and the console must not appear in its tree) | 5.5 MB | 126 | 8 MB |
| `weight-tls` (default plus `tls`) | 10 MB (was 6.1; see below) | 165 (was 137) | 8 MB |

So the rise is about **+1.55 MB on the default gate** (5.15 to 7.1 MB, of which about 0.08 MB is the old gate having been under the Phase 8 close and 1.3 to 1.5 MB is
the console), not the 1 MB the owner allowed; the owner is told. If that is too much, the levers are: drop the two fonts for a system stack (-100 KB, D12: the pixel
check then differs everywhere), compile the page code smaller (`opt-level = "s"` for `kafka-console` only: not measured), or split a slice. The idle memory limit
holds with room (6.5 MB with the console on Linux, 7.2 MB on macOS after two page loads).

**The owner's decision on the size (2026-11-12, after the numbers above):** 1.55 MB is fine, "even 2 MB is fine", and "anything less than 10 MB is fine". So the limits above were raised before any CI run: the default build to **9 MB** (the estimate of 6.7 MB plus 2 MB, 2.3 MB of room for the eight slices of pages still to come) and the TLS build to **10 MB**, the owner's ceiling. The lean build (5.5 MB), the package limits (154, 126, 165) and the idle memory limit (8 MB) are unchanged: the owner spoke about size, not about packages or memory. The rule stands: a slice that moves a number writes the reason here, and the 9 MB is for the pages, not for a dependency that could be avoided. The first CI run still replaces the estimate with a measurement.

**After A1, the shared kit extracted (2026-11-12, darwin/arm64, release, branch `aws`):** binary 4,030,448 B (+944 B over the Phase 7
close above), 112 lines in the CI count (107 plus `doze-fs`, `doze-store` and `doze-runtime`, and two `(*)` repeat lines for `redb` and
`postcard`, which the `sort -u` count includes; the real new packages are our three), idle RSS 4,816 to 4,880 KB over three runs (4,816 at
Phase 7). Under the CI limits (115, 5.15 MB, 8 MB). Kafka now links `doze-store` and the lock from `doze-runtime`; it does not link
`Lazy` or `Expiring`. `cargo xtask weight` now measures this.

**The model layer (2026-11-12, A2, darwin/arm64, release, thin LTO):** a binary that links all 18 generated models and the JSON decoder and uses each (`cargo build --release -p aws-modelcheck --example everything`) is 1,186,144 B (1.13 MiB); an empty Rust binary of the same settings is 0.33 MiB. The ceiling for the model layer is 1.25 MiB; the whole-binary budget (PLAN.md "Targets") is re-based at A5.

**doze-aws, first measurement (2026-11-12, darwin/arm64, release, thin LTO):** the binary with the gateway, the wire layer, SigV4, the JSON protocol and SQS is 1,713,536 B (1.63 MiB), 104 lines in the CI package count, idle 3,056 KB resident on one thread. The Go edition: about 22 to 24 MB stripped and 31.4 MB idle. S6 predicted idle under 6 MiB. The Linux limits (2.6 MB, 110, 6 MB) are macOS times the 1.45 the Kafka binary shows between the two, plus 5%, and are re-based from the first Linux CI run.

**doze-aws with all of SQS (2026-11-12, darwin/arm64, release, thin LTO):** 1,914,240 B (1.83 MiB), 109 lines in the CI package count, idle 3,072 KB. Over the tracer bullet: +200 KB, mostly `serde_json` for the policy documents, which the IAM evaluator needs too.

**doze-aws with SQS on disk (2026-11-12, darwin/arm64, release, thin LTO): 2,687,088 B (2.56 MiB), 118 packages, idle 3,120 KB.** Before SQS kept its state on disk the binary was 1,914,240 B and 109 packages, so the database costs 0.74 MiB and 9 packages (`redb` is 507 KiB of the text; `serde_derive` and its macro crates are most of the packages and are not in the binary). Every service after SQS shares this cost, which is why it is paid once here and not per service. The Linux limits became 3,700,000 B and 120 packages (macOS size times 1.3 plus 5%, re-based from the first Linux run); the idle limit stays 6 MiB, and the idle figure did not move (3.0 to 3.1 MB). The idle reap (`--idle-secs`, 300 by default) closes the database after five quiet minutes; resident memory did not fall in a short test because the allocator keeps its pages, and 4.7 MB busy is the whole process.

**doze-aws with SQS and SSM Parameter Store (2026-11-12, darwin/arm64, release, thin LTO): 3,086,160 B (2.94 MiB), 139 packages, idle 3,232 KB.** Over SQS alone (2,687,088 B, 118 packages): +399 KB and +21 packages. The packages are AES-GCM and the RustCrypto trait crates under it (`SecureString` is sealed with AES-256-GCM; KMS needs the same set, so it is paid once), `regex-lite` (`AllowedPattern`; the model validation already used it, but only in the generated crates that were linked) and the SSM model's packed tables (SSM has 146 operations; the refused fleet half is in the tables because the gateway needs them to route and to name the operation it refuses). The 399 KB was not split between those three. The Linux limits became 4,250,000 B and 142 packages (macOS size times 1.3 plus 5%); idle stays 6 MiB (3,232 KB measured). The idle limit is unaffected: the parameter store opens nothing until its first request.

**doze-aws with Secrets Manager too (2026-11-12, darwin/arm64, release, thin LTO): 3,320,864 B (3.17 MiB), 143 packages, idle 3,152 KB.** Over SSM: +234 KB and +4 packages (`aws-secrets`, `aws-seal`, the model crate, and the lines `sort -u` counts twice). It adds no dependency of its own: sealing is the AES-GCM already paid for in SSM. The Linux limits became 4,550,000 B and 147 packages (macOS size times 1.3 plus 5%).

**doze-aws with KMS too (2026-11-12, darwin/arm64, release, thin LTO): 3,858,704 B (3.68 MiB), 183 packages, idle about 3.2 MB.** Over Secrets Manager: +538 KB and +40 packages. The packages are the RustCrypto elliptic-curve stack for ECDSA on four curves (`p256`, `p384`, `p521`, `k256`, with `elliptic-curve`, `ecdsa`, `primeorder`, `primefield`, `crypto-bigint`-class crates under them), `ed25519-dalek`, `hmac`, and the KMS model's tables. The 538 KB was not split between those. **Not yet in it: RSA** (`rsa` is a release candidate and big, and RSA key specs are refused by name until it is added and measured). The Linux limits became 5,300,000 B and 188 packages (macOS size times 1.3 plus 5%). The whole of A6 so far (SSM, Secrets Manager, KMS) takes the binary from 2.69 MB (SQS alone) to 3.86 MB.

**KMS grants and data key pairs (2026-11-12): 3,910,144 B (+51 KB), 183 packages (none added).** The Linux byte limit became 5,400,000 B (macOS size times 1.3 plus 5%).

**After merging `aws` into `main` (2026-11-12, darwin/arm64, release):** doze-kafka default build 5,522,560 B, 155 lines in the package count, idle 6,192 KB. The main branch's own limit was 154 (150 for the console build plus 4); the shared kit from A1 (`doze-fs`, `doze-store`, `doze-runtime`, and the lines `sort -u` counts twice for `redb` and `postcard`) is the one line over. The package limit became 159 (the measured 155 plus 4); the byte limit stays the owner's 9 MB ceiling. The lean and TLS CI jobs keep their inline limits and were not re-measured here.

**KMS RSA keys and key import (2026-11-12, darwin/arm64, release, thin LTO): 4,378,880 B (4.18 MiB), 192 packages, idle 3.3 MB.** Over grants and key pairs (3,910,144 B, 183): +469 KB and +9 packages (`rsa` 0.10.0-rc.22 and the `crypto-bigint` stack it uses, `sha1`, `aes-kw`, `getrandom`'s system RNG adapter). Linux limits 6,000,000 B and 197 packages (macOS size times 1.3 plus 5%). All of A6 together: SQS alone was 2.69 MB; SQS, SSM, Secrets Manager and KMS with RSA and import are 4.38 MB.

**doze-aws with the peer bus and KMS-backed sealing (2026-11-12, darwin/arm64, release, thin LTO): 4,447,712 B (4.24 MiB), 194 packages, idle 3.3 MB.** Over RSA and import (4,378,880 B, 192): +69 KB and +2 packages (`aws-peers`, and base64 in `aws-core`); SSM and Secrets Manager lost their own sealing key and took `aws-peers` instead. Linux limits 6,100,000 B and 199 packages. `ring` is a dev-dependency of `aws-kms` only (the cross-check tests) and is not in the binary.
**Phase 9, slice 9i (DescribeCluster and client telemetry, 2026-11-12, darwin/arm64, release, thin LTO):** the default build is 6,127,920 B against 6,111,040 B for the same tree without the slice (measured side by side, `git checkout` of the commit before it): **+16,880 B (+0.28%)**, 155 lines in the package count both ways (`crc32c` was already in the tree through `kafka-wire`; `kafka-broker` now names it), idle RSS 6,112 to 6,144 KB over three runs against 6,112 to 6,176 KB (3 threads both): no change within noise, as the store allocates nothing until a client first asks. The 9b figures quoted in the slice briefs (5,773,712 B and 5,841,520 B) are of the two branches before they were merged; the merged tree at the start of 9i was already 6,111,040 B. The step is the three handlers, the gate, and the bounded-inflation reader that already existed for Produce (`inflates_within` adds a loop over it).
**Phase 9, slice 9h (Settings and Access, 2026-11-12, darwin/arm64, release, thin LTO):** the default build is 6,965,712 B against 6,681,184 B for the tree it was cut from (main at f991059d, built side by side in a second worktree): **+284,528 B (+4.3%)**, 156 lines in the package count both ways (no dependency added), idle RSS 6,256 to 6,448 KB over three runs against 6,080 to 6,192 KB (3 threads both): about +0.2 MB, the pages' own text and tables being touched at start. The step is eight boards (two pages of many sections and three dialogs), their view-models, `ConsoleView::{access, intervals, data_size}`, the flags table made from clap's own list in the binary (38 flags with their groups), the ACL words and a second set of fixtures behind the `examples` feature (not in the release build). Weight on the Settings page itself is read where the platform lets the process read it (Linux for resident memory); the limits it names (9 MB, 8 MB) are the ones above, held by a test.

**Phase 9, integration of 9g and 9h with main (p9x, 2026-11-13, darwin/arm64, release, thin LTO):** the default build is 7,755,392 B (`cargo xtask weight`: 157 packages, idle 6,736 KB), against 6,983,504 B for main at 0b10acdb (before 9f, the UX pages and these two): **+771,888 B (+11%)** for 15 boards (Log, Producers and its transaction pages, Settings, Access), the survey and producers view in `kafka-broker`, the flags table in the binary, the wire dock and the recorder's names under the Log and Producers pages, and the three Settings sentences (9f and the UX pass are in the number too, so the step of this slice alone is smaller; it was not measured apart). No dependency was added (157 lines in the package count, as 9h alone). The owner's rule of 2026-11-13: weight is not a constraint for a couple of MB.

**UX foundation: the client stack (2026-11-12, darwin/arm64, release, thin LTO):** the default build is 7,198,672 B against 6,983,504 B before the pass: **+215,168 B**, 157 lines in the package count (no package added: htmx, idiomorph and Alpine are files of `assets/`, embedded with `include_bytes!`). The step is the asset bytes: the vendored script is 151,125 B (htmx 52,182, idiomorph 11,037, Alpine CSP 72,242, anchor 15,664), our two scripts are about 40 KB (`console.js` 511 lines, `components.js` 355), the stylesheet grew by the components and the drawer geometry; the console's whole served asset set is about 330 KB with the fonts (it was 140 KB), and the rest of the step is the `ui/components`, the region layer and the gallery-free page code. The gate is 9 MB and the owner's ceiling 10 MB; the owner lifted the 150 KB script budget the same day ("let's not cheap out for a couple of MBs"), so nothing here was trimmed for bytes except what had a quality reason (client-stack.md). Idle RSS was not re-measured (the assets are served from the binary's read-only data and not touched until requested).

## Phase 9, weight pass (2026-11-13), release, darwin/arm64 and the Linux aarch64 VM

**Why.** The default build had grown to 8,055,312 B on darwin (4.30 MB at the Phase 8 close, 5.5 then 7.2 MB with the console). By the two ratios used before
that is 9.8 MB (darwin times 1.216) or 9.6 MB (the VM times 1.12) on ubuntu x86_64, over the 9 MB gate and near the owner's ceiling of 10 MB. The pass found the waste
and took the free wins; no feature, font, board or library was dropped, and the security headers are as they were.

**Before and after** (same machine, `cargo build --release -p doze-kafka`, `-j 3`; the VM is `doze-ubuntu`, native aarch64 glibc build from a `git archive` of each tree):

| Build | darwin before | darwin after | VM before | VM after | Packages (VM) | Idle RSS (VM) |
|---|---|---|---|---|---|---|
| default (console) | 8,055,312 B | **5,797,328 B** (-2,257,984, -28%) | 8,592,928 B | **6,626,840 B** (-1,966,088, -23%) | 157 to 158 (the `flate2` line, see below) | 7,712 to 7,104 KB |
| lean (`--no-default-features`) | not taken | 3,917,008 B | 4,726,200 B | 4,398,512 B (-327,688) | 127 both | 5,440 to 5,236 KB |
| `tls` (default plus) | not taken | 6,592,016 B (+794,688 over default; 838,928 at 8e) | not taken | not taken | 169 on darwin (+11) | not taken |

**Estimate for ubuntu x86_64** (no machine here; both ratios as before): default 5,797,328 x 1.216 = **7.05 MB**, and 6,626,840 x 1.12 = **7.42 MB**; before the pass 9.80 and 9.62 MB.
Lean: 3,917,008 x 1.216 = 4.76 MB and 4,398,512 x 1.12 = 4.93 MB. `tls`: the default plus 0.9 MB (ring's x86_64 code, as in 8e) = 8.3 MB.

**Where the bytes were** (`cargo bloat --release --crates`, the build before the pass, `.text` 6.3 MiB of a 9.3 MiB unstripped file): `kafka_console` 1.5 MiB (23%), `std` 1.2 MiB (20%), `kafka_broker` 850 KiB,
`redb` 566 KiB, `axum` 287 KiB, `tokio` 217 KiB, `clap_builder` 192 KiB, `kafka_wire` 189 KiB, `regex_syntax` 122 KiB and `regex_automata` 90 KiB (the log filter), `doze_kafka` 112 KiB, `kafka_meta` 111 KiB,
`kafka_protocol` 103 KiB, `ruzstd` 82 KiB, `tracing_subscriber` 79 KiB, `hyper` 66 KiB. Outside `.text`: `__const` 826 KB, of which the embedded assets were 403 KB (the stylesheet 75, our scripts 86, vendored scripts 151, three fonts 91, the favicon).
After the pass: `kafka_console` 892 KiB, `std` 790 KiB, `kafka_broker` 687 KiB, `redb` 465 KiB, `axum` 189 KiB. The console's functions are about 940 of them, none above 30 KB (`settings_page` is the largest): the cost is spread, not one waste.
Checked and clean: no fixture or larder symbol is in the release binary (`nm`: none; everything under `cfg(any(test, feature = "examples"))` is out); `cargo tree -d` shows only build-time duplicates (`syn` 2 and 3 for the proc macros);
no dependency feature is on without a user (`serde_json` and `tracing-serde` are the `--log-format json` flag, `regex` is `EnvFilter`). `cargo llvm-lines` found no monomorphization hot spot (nothing above 0.3% of the lines).
`--gc-sections` is rustc's default on Linux and `-dead_strip` on macOS; unreferenced functions, derived `Debug` impls included, are removed by LTO, so a `Debug` gate would save nothing measurable and the workspace lint `missing_debug_implementations` asks for them anyway.

**The steps, darwin, each measured on its own** (the VM was built for the base and the final tree only):

| Step | Binary | Delta | Kept |
|---|---|---|---|
| base (main at f14d65ac: thin LTO, opt-level 3 everywhere) | 8,055,312 B | | |
| `lto = "fat"` alone | 7,521,232 B | -534,080 | yes |
| thin LTO plus `kafka-console` at opt-level "z" | 7,831,696 B | -223,616 | (shown for comparison) |
| fat LTO plus `kafka-console` at "z" | 6,241,072 B | -1,280,160 more | yes |
| ... plus the cold crates at "z": axum, axum-core, hyper, hyper-util, http, httparse, tower, matchit, clap, clap_builder, clap_lex, tracing-subscriber, regex-automata, regex-syntax | 6,011,920 B | -229,152 | yes |
| text assets embedded as gzip (`tools/assets-compress.ts`; 311 KB of text is 96 KB) | **5,797,328 B** | -214,592 | yes |

Settings tried and not kept: `kafka-console` at "s" (7,109,904 B under fat LTO, 8,575,344 B under thin: bigger than "z" by 0.9 MB and, under thin, bigger than 3), at 1 (8,998,032 B) and 2 (8,038,912 B); the `doze-kafka` crate itself at "z"
(+1.5 MB: the binary crate is where the generics of everything get instantiated; do not set it); `kafka-registry`, `serde_json`, `regex-lite`, `nu-ansi-term`, `tracing-serde` and `anyhow` at "z" (-16 KB together: not worth the lines).
The hot path stays at opt-level 3: `kafka-broker`, `kafka-wire`, `kafka-storage`, `kafka-meta`, `kafka-server`, `redb`, `tokio`, the codecs. `produce_bench` (one connection, 20,000 one-record Produce requests, in-memory broker, 7 rounds, best of the rounds) before and after:
no observer 28.9 us to 28.4 us per request, recorder installed 33.1 us to 29.1 us, recorder and a page open 29.5 us to 29.1 us (the machine was loaded both times, load 6 to 9; the figures say "no slowdown", not "faster").
`interop/run.sh` (sarama, franz-go, kafka-go, librdkafka, transactions, compaction, churn) passes on the final build.

**The assets.** The stylesheet and the nine scripts are `include_bytes!` of their `.gz` files (`assets/<name>.gz`, made by `bun tools/assets-compress.ts`, which also takes `--check`). gzip, not brotli: a browser offers `br` only over https and the
console is http on loopback (brotli at level 11 would be 11 to 13% smaller again, about 11 KB). The stylesheet is compressed with its `@asset:<font>` placeholders already replaced by the fonts' hashed names (the tool has the same FNV-1a
`fingerprint` as `assets.rs`), so nothing is rewritten or copied to the heap at start (the old code made a `Vec` copy of every asset: about 400 KB of heap, the reason idle fell too). A served file's hashed name is the fingerprint of the embedded bytes.
`static_file` answers `Content-Encoding: gzip` with those bytes to a client whose `Accept-Encoding` lists gzip (or `*`) with a quality above zero, and inflates them (flate2, which the broker links already) for any other, with its own ETag
(`"name.identity"`) and `Vary: Accept-Encoding`. Fonts (woff2) and the favicon are untouched. Tests: every `.gz` must inflate to exactly its source (so a stale one fails `cargo test -p kafka-console`, with the command to run in the message),
the manifest test maps `x.gz` to `x`, and `tests/headers.rs` covers accepted, refused (`gzip;q=0`), absent and `*`. The `shell.html` golden holds the hashed names and was regenerated (`DOZE_UPDATE_DESIGN=1`). After a change to a script or to the stylesheet:
`DOZE_UPDATE_DESIGN=1 cargo test -p kafka-console` (the stylesheet join), then `bun tools/assets-compress.ts`. The vendored scripts are the official minified builds (`dist/htmx.min.js`, `dist/idiomorph-ext.min.js`, `@alpinejs/csp` and `anchor` `dist/cdn.min.js`).
The CSS has 14 class selectors that nothing in `src/` or the scripts names (`tgl chk spark toast toast-wrap dbar d-compute d-observe d-infra svc-card tlmap lane recv address-row`): under gzip that is under 1 KB, and the files are the kit's, shared with boards the pixel gate reads, so they were left.

**The package count.** 158 lines on both darwin and the VM against 157 before: `kafka-console` now names `flate2` itself (it was a dev-dependency), which makes one more `(*)` repeat line in `cargo tree`; no package was added.
The `weight-lean` job's 126 was already one under the real count (127 in the VM, before and after) and is now 130; `weight-tls` was 165 for a default of 150 and is now 173 (158 + 11 + 4).

**The limits chosen** (the larger estimate plus about 5%, in `xtask/weight-limits.txt` and `ci.yml`; the first CI run replaces the estimates with measurements):

| Job | Was | Now | Estimate (x86_64) | Reason |
|---|---|---|---|---|
| `weight` default | 9,000,000 B, 159 packages, 8,192 KB | **7,800,000 B**, 161, 8,192 KB | 7.05 to 7.42 MB | 7.42 x 1.05 = 7.79; the old limit was the 6.7 MB estimate plus the owner's 2 MB, which the build had since spent |
| `weight-lean` | 5,500,000 B, 126 | **5,200,000 B**, 130 | 4.76 to 4.93 MB | 4.93 x 1.05; the package line was one short already |
| `weight-tls` | 10,000,000 B, 165 | **8,750,000 B**, 173 | about 8.3 MB | default estimate plus 0.9 MB, plus 5% |

The owner's ceiling of 10 MB stands: the default estimate is 2.6 MB under it, the tls one 1.7 MB.

**Levers not pulled, and what each costs.** (1) `EnvFilter` to `tracing_subscriber::filter::Targets` (no `env-filter` feature, so no regex): -233,008 B on darwin (5,797,328 to 5,564,320 B) and several packages
(`matchers`, `regex-automata`, `regex-syntax`), at the price of the span and field directives of `RUST_LOG` (`target[span{field=value}]=level`, regexes on values); plain `target=level,...` lists and `--log-level` keep working. Not applied: it narrows a documented input.
**Applied 2026-11-15** (owner's approval): -216,448 B on darwin (8,001,440 to 7,784,992 B on the one build) and 184 to 180 packages; span and field directives are refused at start with exit 2. See "The log filter without regex" below.
(2) `kafka-broker` at "z": -180,288 B (5,617,040 B), but it is the request handlers of the hot path. Not applied. (3) Brotli for the assets: about -11 KB, and only over https. (4) Dropping the two JetBrains Mono weights (-43 KB) or all three fonts for a system stack (-91 KB): a visible change (D12). None is needed to meet 9.0 MB.

**A side effect in the same profile.** `[profile.release]` is the workspace's, so `doze-aws` is also built with fat LTO: 3,897,792 B (4,447,712 B when last recorded), 194 packages, 3,232 KB idle. Its limit (6,100,000 B) is not touched here; it can be lowered by whoever owns that job.

## The React console, slice R0 (2026-11-13, branch `react-r0`)

The console's new page layer (React + shadcn/ui, `web/`) is built into `crates/kafka-console/web-dist/` and embedded in the binary with the
weight pass's own mechanism: text files as their `.gz` (`pack` of `tools/assets-compress.ts`), served by `handlers::plain::packed`, fonts as
they are. The classic console stays embedded (under `/classic/`) until slice R5 removes it with its maud page layer, htmx, Alpine and
idiomorph.

| | Raw | Embedded (gzip) |
|---|---|---|
| main chunk (React, Radix, TanStack Router and Query, the shell, Home, Connect) | 595,914 B | 183,580 B |
| gallery chunk (loaded only on `/gallery`: TanStack Table, react-hook-form, zod) | 271,601 B | 77,483 B |
| stylesheet (Tailwind v4, the kit's tokens) | 78,503 B | 13,289 B |
| fonts (Inter, JetBrains Mono 400 and 500; a second copy of the classic console's until R5) | 91,256 B | 91,256 B |
| index.html, favicon | | 476 B |
| **total** | 1,037,274 B | **366,084 B** |

Binary on darwin (arm64, fat LTO): **6,161,072 B** against the weight pass's **5,797,328 B**: +363,744 B, which is the embedded bundle
(366,084 B) and the API (serde derives on the view-models, the JSON handlers; the merged Lab engine is in the same build). Packages: 160
lines (limit 161): serde, serde_json and tower were already in the tree (tracing-subscriber, axum), so the console naming them adds no
package; `ts-rs` is a dev-dependency only. Under the 7,800,000 B limit; no limit moved.

What R5 takes back: the classic assets (about 105 KB gzipped plus the 91 KB of fonts a second time) and the maud page layer (an estimated
1 MB of code). Levers not pulled: zod in the main chunk was avoided (search params are parsed by hand, `src/lib/search.ts`); brotli
would save about 15% of the JS but browsers ask for it only over https.

## The React console, slice R5: the classic console removed (2026-11-13, branch `react-r5`)

The server-rendered console (maud pages, the route table, htmx, idiomorph, Alpine, the kit's stylesheet, its own copy of the fonts) is gone
from the crate; the React bundle is the only page layer. `maud` and `maud_macros` (and `proc-macro2-diagnostics` under them) left the tree,
and `tower` is no longer a direct dependency of the console (it stays in the tree through axum).

| darwin arm64, fat LTO | main before R5 | R5 |
|---|---|---|
| doze-kafka binary | 6,593,728 B | **5,945,008 B** (-648,720 B) |
| packages (`cargo xtask weight`) | 160 (R4's measurement) | **155** |
| idle | | 6,800 KB |

No limit moved (7,800,000 B, 161 packages): a person lowers them, with the first CI measurement.

**doze-aws with IAM, STS, the Query protocol and policy enforcement (2026-11-12, darwin/arm64, release, thin LTO): 5,097,200 B (4.86 MiB), 198 packages, idle 3,312 KB.** Over the peer bus (4,447,712 B, 194): +649 KB and +4 packages, in two steps. IAM and STS with the Query codec (4,963,664 B) cost +516 KB: the IAM model (180 operations) and the STS model, the form and XML codec, session signing (`hmac` was already in). Then the policy evaluator, the simulator, the authorizer and the per-service request descriptions cost +133 KB and no package (`aws-iam-policy`, `aws-peers` authz, and a small `authz.rs` in each service). The Linux byte limit became 7,000,000 B (macOS size times 1.3 plus 5% is 6,957,678); the package limit stays 203 and the idle limit 6 MiB. Idle did not move because nothing opens the IAM database until a request carries a key IAM could have issued: a caller with `test` or any other key IAM did not issue is the account root and is never looked up.

**Regions kept apart (2026-11-12, darwin/arm64, release): 5,114,720 B, 198 packages, idle unchanged.** +17 KB and no package over policy enforcement (`doze_runtime::Keyed`, the region check in `aws-auth`). The limits do not move.

**doze-aws with SNS (A8) (2026-11-12, darwin/arm64, release, thin LTO): 5,550,688 B (5.29 MiB), 201 packages, idle 3,328 KB.** +436 KB and +2 packages over regions kept apart (5,114,720 B, 198; the package count by `cargo tree` was 199 before SNS, so SNS adds the two, `aws-sns` and `aws-model-sns`). `sha2`, `tracing` and `aws-iam-policy` were already linked. Idle memory did not move: the SNS actor and its tables are made on the first request in a region. The limits stand (7,000,000 B, 203 packages): two packages of room.

**doze-aws with SNS and DynamoDB (2026-11-12, darwin/arm64, release, thin LTO): 6,253,904 B (5.96 MiB), 205 packages, idle 3,488 KB.** Measured alone over regions kept apart (5,114,720 B, 198): SNS +436 KB and +2 packages; DynamoDB +737 KB and +3 packages (`aws-dynamodb`, `aws-expr`, `aws-model-dynamodb`; no new third-party crate). The Linux limits became 8,600,000 B (macOS size times 1.3 plus 5% is 8,536,000) and 209 packages (205 plus 4). This is the first stage that moves the byte limit by more than a few hundred KB: at this rate the owner's 10 MB ceiling (9 MB CI default) is reached within two or three more services, so every later stage is measured against it before it is given room.

**doze-aws with SNS, DynamoDB and S3 (2026-11-12, darwin/arm64, release, thin LTO): 6,974,576 B (6.65 MiB), 209 packages, idle 3,632 KB.** S3 alone, measured by its stage: +722 KB and +4 packages (the REST-XML codec, the streaming path in the gateway, the S3 model tables; `bytes` is the only new dependency). The Linux limits became 8,800,000 B (macOS size times 1.2 plus 5%, the ratio doze-kafka's Linux build showed; the earlier times 1.3 would give 9.5 MB) and 213 packages. **This is 88% of the owner's 10 MB ceiling on the Linux scale, with Logs, Kinesis and CloudWatch (+1.2 MB measured alone), EventBridge, Lambda, Step Functions, API Gateway and CloudFormation still to come.** A size plan is needed before the next merge, not after.

**doze-aws on its own size-optimised profile (2026-11-13, darwin/arm64): 4,660,320 B (4.44 MiB), 209 packages, idle 3,520 KB.** The same code as the SNS, DynamoDB and S3 build (6,974,576 B under `release`) built with a new `[profile.release-aws]` (`opt-level = "s"`, fat LTO), which `cargo xtask weight` now uses for doze-aws; doze-kafka keeps `release`. That is 33% smaller (`opt-level = "z"` was 3,600,768 B, 48% smaller; not chosen, because only S3 transfer and one RSA key generation were timed). Measured speed: the 1 GiB S3 upload and download took 3.3 s and 0.4 s on all three builds, and the server's memory was the same or lower (peak 7.6 MiB against 8.6 MiB); RSA_4096 key generation is dominated by random prime search (0.17 to 0.67 s across runs of the same binary). The Linux limits became 5,900,000 B (macOS size times 1.2 plus 5%) and stay 213 packages and 6,144 KB. Chosen by the owner on 2026-11-13 so that the remaining stages (Logs, Kinesis and CloudWatch measured +1.2 MB under `release`, then EventBridge, Lambda, Step Functions, API Gateway and CloudFormation) fit under the 10 MB ceiling.

**doze-aws with Logs, Kinesis and CloudWatch (2026-11-13, darwin/arm64, release-aws profile): 5,371,872 B (5.12 MiB), 220 packages, idle 3,600 KB.** Over the 4,660,320 B / 209 packages of SNS, DynamoDB and S3 on the same profile: +711 KB and +11 packages (the three models’ tables, the three services, the CBOR codec and `flate2` with its pure-Rust backend: `flate2`, `miniz_oxide`, `adler2`, `crc32fast`, `simd-adler32`). The stage measured +1.2 MB alone under the `release` profile. Nothing runs until a request; the alarm timer is one task per region with a pending deadline. The Linux limits became 6,800,000 B (macOS size times 1.2 plus 5%) and 224 packages. The package count rose from 198 to 220 in one round of four stages: it is now the number that moves fastest, and each new third-party package must be named in its stage note.

**doze-aws after the Go-parity round (2026-11-13, darwin/arm64, release-aws): 5,438,064 B (5.19 MiB), 220 packages, idle 3,632 KB.** Over the 5,371,872 B of Logs, Kinesis and CloudWatch: +66 KB and no package. IAM `GetAccountAuthorizationDetails` and `AssumeRoot`, Kinesis resharding and account settings, and S3 Object Lock and policy status together cost 66 KB. The limits stand (6,800,000 B, 224 packages).

**doze-aws with EventBridge (2026-11-13, darwin/arm64, release-aws): 5,785,936 B (5.52 MiB), 224 packages, idle 3,728 KB.** Over the 5,438,064 B / 220 packages of the Go-parity round: +348 KB and +4 packages, all first party (`aws-eventbridge`, `aws-cron`, `aws-model-eventbridge`; the tool counts one more). Nothing runs until a request; the schedule timer is one task per region with a pending deadline. The Linux limits became 7,300,000 B (macOS size times 1.2 plus 5%) and 232 packages (the old limit of 224 was met exactly). Next come Lambda (with the REST-JSON codec) and Step Functions; every new third-party package must be named in its stage note.

**doze-aws with Lambda (2026-11-13, release-aws): 6,500,256 B (6.20 MiB) on darwin/arm64, 233 packages, idle 3,920 KB; 8,206,736 B on x86_64 Linux (musl).** Over EventBridge (5,785,936 B / 224): +714 KB and +9 packages, all first party (`aws-lambda`, and the REST-JSON codec in `aws-wire`). **The first real Linux measurement:** the same commit cross-built with `cargo build --profile release-aws -p doze-aws --target x86_64-unknown-linux-musl` is 1.26 times the macOS size; the 1.2 used since the first size limits was doze-kafka’s ratio, and a stage agent’s 1.45 was an estimate. The CI job builds glibc (dynamic), which is a little smaller than musl, so the musl figure is the safe side. The Linux limits became 8,600,000 B (the musl size plus 5%) and 240 packages. That is 86% of the owner’s 10 MB ceiling and well past his 9 MB CI default, with Step Functions, API Gateway and CloudFormation still to come. The next lever is `opt-level = "z"` on the `release-aws` profile, which measured 77% of the `"s"` size on 2026-11-13 (3.60 MB against 4.66 MB at that time); it is to be taken before the limit moves again. Candidates to trim after that: the embedded Node and Ruby runtime clients and the model tables of the refused operations.

**doze-aws at `opt-level = "z"` (2026-11-13, release-aws): 4,867,344 B (4.64 MiB) on darwin/arm64, 233 packages, idle 4,400 KB; 7,481,552 B on x86_64 Linux (musl).** The same code at `"s"` was 6,500,256 B, 233 packages and 3,920 KB idle on macOS and 8,206,736 B on Linux. `"z"` took 25% off macOS and only 9% off Linux: of the Linux binary, `.text` is 5.1 MB, `.rodata` 1.0 MB, `.eh_frame` 0.8 MB (plus 0.1 MB of index), `.rela.dyn` 0.17 MB. The Linux limits are 7,900,000 B (the musl size plus 5%) and 240 packages. Cost of `"z"`: server CPU 30 to 60% higher per request (0.2 to 0.7 ms), idle memory +0.4 MB, wall time unchanged; see the PLAN entry for the benchmark. Linux is now 1.54 times the macOS size, which is the ratio to plan with.

**doze-aws with every Go service (2026-11-13, release-aws): 7,444,224 B (7.10 MiB) on darwin/arm64, 258 packages, idle 5,344 KB; 11,764,560 B on x86_64 Linux (musl), 10,561,264 B with JSONata off.** Over the "z" base (4,867,344 B / 233 on macOS, 7,481,552 B on Linux): Step Functions, API Gateway and CloudFormation together add 2.58 MB on macOS and 4.28 MB on Linux (1.66 times). The owner raised doze-aws's ceiling to 20 MB the same day; the CI line is 12,400,000 B (musl plus 5%) and 264 packages. Cross-building now needs a C compiler and an ELF archiver for the target (JSONata's `psm`): a `clang --target=x86_64-unknown-linux-musl` wrapper as `CC_x86_64_unknown_linux_musl` and `llvm-ar` as `AR_x86_64_unknown_linux_musl`.


## Phase 11, the module face in the default build (2026-11-14, branch `p11-module`), release, darwin/arm64

`doze-kafka` gained the doze module face (feature `module`, on by default like the console; docs/port/phase11.md): the module
SDK (`doze-module`, `doze-module-proto`, `doze-os`, `minicbor`, `sha2`, `hex`, `rustix`) and the client half of the Kafka
messages the module speaks to its own broker with.

| darwin arm64, fat LTO | without `module` | default build |
|---|---|---|
| doze-kafka binary | 6,011,568 B | **6,574,304 B** (+562,736 B, +9.4%) |
| packages (`cargo xtask weight`) | 158 | **171** (+13: the SDK's five crates, `minicbor`, `minicbor-derive`, `hex`, `sha2`, `const-oid`, `rustix`, `errno`, `bitflags`; `kafka-protocol` and `syn` counted again on new edges) |
| idle | | 7,152 KB (the module code does not run in a broker) |

`cargo bloat` puts about 250 KiB of text in the SDK (`doze_module` 120 KiB, `doze_module_proto` 82 KiB, `minicbor` 46 KiB); the rest
is the module code and the request encoders and response decoders it uses. The Linux limits became 8,400,000 B (the macOS size times
1.216, the Phase 7 ratio, plus 5%: 8,394,071) and 175 packages (171 plus 4); idle unchanged. Under the owner's 10 MB ceiling, but a
cost every standalone user pays for a face only doze uses: **owner's decision** (phase11.md, cutover readiness) whether the module
stays in the default build or moves behind a non-default feature that the release of `kafka-plugin` turns on (the lean broker would
then be 6.0 MB again, and the gate's test run would need that feature turned on to keep testing the module).

**Corrected the same day, merging with main:** CI had by then measured the default build before the module on ubuntu x86_64 at 8,174,048 B, so the
macOS-to-Linux ratio is 1.36 (8,174,048 / 6,011,568), not 1.216. With the module that is about 8.94 MB on ubuntu x86_64, about 0.6 MB under
the 10 MB ceiling; the limit is now 9,386,171 B (that plus 5%) and 174 packages until the next CI run measures it (`weight-limits.txt`).
The schema registry (Phase 10) must fit in what is left, or the module face moves behind a feature: the owner decides.
## Limits raised to the measured values (2026-11-14, owner decision)

Main's CI had been red on the weight jobs since the React console and the doze-aws A14 to A16 merges. The owner decided on 2026-11-14 to raise
the budgets to what CI measures on the ubuntu x86_64 runners. Byte and package limits are the measurement exactly (they are the same in every
run of one commit). Idle memory is not: one sample two or three seconds after start moved by up to 870 KB between runs of the same commit
(doze-aws: 8,612 and 9,484 KB). So the method changed with this entry:

- **Idle memory is the median of three fresh starts**, each with an empty data directory, read two (`cargo xtask weight`) or three (the
  `weight-lean` and `weight-tls` jobs) seconds after the server listens. All three samples are printed.
- **An idle limit is that median plus 5%**, rounded up to the KB, for the noise that is left. Byte and package limits get no headroom.

Measured in run 43572452563 (main plus PR #7's build fixes):

| job | measured | old limit | new limit |
|---|---|---|---|
| `weight`, doze-kafka binary | 8,174,048 B | 7,800,000 B | **8,174,048 B** |
| `weight`, doze-kafka idle | 9,428 / 9,296 / 9,280 KB, median 9,296 | 8,192 KB | **9,761 KB** |
| `weight`, doze-aws idle | 8,964 / 8,952 / 9,088 KB, median 8,964 | 6,144 KB | **9,413 KB** |
| `weight-lean` binary | 5,263,296 B | 5,200,000 B | **5,263,296 B** |
| `weight-lean` idle | 6,972 / 6,884 / 6,876 KB, median 6,884 | 8,192 KB | **7,229 KB** (lowered: same rule) |
| `weight-tls` binary | 9,279,000 B | 8,750,000 B | **9,279,000 B** |
| `weight-tls` idle | 10,080 / 10,104 / 10,280 KB, median 10,104 | 8,192 KB | **10,610 KB** |

Packages did not move (doze-kafka 155 of 161, doze-aws 259 of 264, lean 127 of 130, tls 166 of 173), nor did doze-aws bytes (11,641,952 of
12,400,000). The two byte limits the weight pass of 2026-11-13 had lowered (doze-kafka 7.8 MB, tls 8.75 MB) were crossed by later merges and
raised under the same decision; both stay under the owner's hard ceilings (kafka 10 MB, doze-aws 20 MB).

**Why idle grew** (issue #36, from the `weight` job of every main push since 2026-11-12): doze-aws went from about 5.2 MB to about 9 MB in two
steps. 128eab40 (doze-aws at `opt-level = "z"`) cost about 1.4 MB: a deliberate trade for a smaller binary, accepted. The A14 to A16 merges
(`04c500b4..e1ab82d4`: Step Functions, API Gateway, CloudFormation) cost about 1.5 MB more with no request made, which points at something
built at start; it stays open in #36 as a regression to fix. doze-kafka's default and TLS builds rose about 0.3 to 0.5 MB with the React
console merge (bc4db1ac); the lean build stayed flat.

## Phase 10, slice 10a: the schema registry (2026-11-14, branch `p10-registry`), release, darwin/arm64

The registry (Avro, every REST route, storage in `meta.redb`) on by default behind the `registry` feature. The libraries weighed for
it and not taken (apache-avro, protox with prost-reflect, jsonschema, boon) are in `phase10.md`, "Weight first".

| | main (2f1bb66) | 10a |
|---|---|---|
| doze-kafka binary | 6,011,568 B | **6,244,864 B** (+233,296 B; +413,984 B before the crate was built at `opt-level = "z"`) |
| packages (`cargo xtask weight`) | 155 | **156** (a repeated `cargo tree` line; no third-party package added) |
| idle | 6,960 KB | 7,184 KB (the registry listens on 127.0.0.1:8081) |
| with `--features tls` | | 7,039,920 B, 168 `cargo tree` lines |

No limit moved (7,800,000 B, 161 packages, 8,192 KB; TLS job 8,750,000 B and 173 lines). On the Linux estimate (darwin times 1.216)
the default build is about 7.59 MB (97% of its limit) and the TLS build about 8.56 MB (98%): 10b and 10c are to be measured against
that before they merge.

**10a on top of the module face (merged with main at Phase 11, 2026-11-14):** the default build 6,791,104 B on darwin arm64 (+216,800 B over
the module's 6,574,304 B), 171 packages, 7,248 KB idle. With `--features tls`: 7,586,144 B and 183 `cargo tree` lines.

**The ceiling, with CI's measured ratio (2026-11-14).** CI measured ubuntu x86_64 at 1.36 times darwin (8,174,048 B against
6,011,568 B), not the 1.216 used above. On that scale: the default build with the module face and 10a is about 9.24 MB, under main's
line of 9,386,171 B (which does not move) and 0.76 MB under the owner's 10 MB ceiling; 10a itself is about 0.29 MB of it. **The TLS
build does not fit:** 7,586,144 B times 1.36 is about 10.32 MB, over the 10 MB ceiling, and the module face alone already puts it at
about 10.0 MB (main's TLS line, 9,279,000 B and 173, was measured before the module; the module adds 13 packages). No limit was raised;
the CI TLS job will fail on main with or without the registry until the owner decides (leave the module or the registry out of the
`tls` release, or take a size lever such as `opt-level = "z"` on more of the tree).

## The TLS build over 10 MB: the owner raised its ceiling (2026-11-14)

With the module face (Phase 11) and the schema registry's foundation (Phase 10a), `--features tls` is 7,586,144 B on darwin arm64, about
10.32 MB on ubuntu x86_64 at CI's measured ratio of 1.36. Asked whether to make room (module face opt-in, no module or no registry in the
TLS build, or a higher ceiling), the owner chose to raise the ceiling. The `weight-tls` job's limit is now 10,833,014 B (that estimate
plus 5%) and 186 packages, until CI's next run on main measures it; Protobuf (10b) and JSON Schema (10c) will move it again, each with
its numbers. The default build's limit (9,386,171 B, 174 packages) is unchanged.

## One build: TLS in the default (2026-11-14, owner decision)

Asked whether to keep a separate TLS build "for a few MB", the owner chose one build, and raised the ceiling to 15 MB the same day. `tls` is
now a default feature of `doze-kafka`; `--no-default-features` (no console, registry, module or TLS) stays only as an internal check that the
broker core needs none of them. CI: the `weight-tls` size job is gone; its size and packages are the `weight` job's single line
(`xtask/weight-limits.txt`), and a `weight-tls-idle` job keeps the one thing it measured that the default job does not: idle memory with a
certificate loaded (10,610 KB). The single build, with the module face and the registry's Avro and JSON Schema, is 7,669,200 B on darwin
arm64, 183 packages, 7,472 KB idle; about 10.43 MB on ubuntu x86_64 at CI's 1.36. Limit 10,951,617 B, 186 packages, 9,996 KB, estimates
until CI measures them. Building from source needs a C compiler and an assembler (ring); release binaries are unaffected.

## One build: CI's measurements (2026-11-14)

Main's first CI run with the one build (TLS in the default, the registry with Avro, JSON Schema and Protobuf), run 43588988185 on PR #45,
replaces the estimates above, by the rule of "Limits raised to the measured values": bytes and packages exact, idle the median of three
plus 5%. Small, justified rises under the owner's delegation of 2026-11-14.

| job | measured | old limit | new limit |
|---|---|---|---|
| `weight`, doze-kafka binary | 10,634,592 B, 183 packages | 10,951,617 B, 186 (estimates) | **10,634,592 B, 183** |
| `weight`, doze-kafka idle | 10,488 / 10,048 / 10,576 KB, median 10,488 | 9,996 KB (estimate) | **11,013 KB** |
| `weight-lean` binary | 5,274,120 B | 5,263,296 B | **5,274,120 B** (+10,824 B: the module face's and registry's shared code reached the lean build) |
| `weight-tls-idle` | 10,932 / 10,916 / 10,936 KB, median 10,932 | 10,610 KB | **11,479 KB** |

doze-aws held (median 8,728 KB of 9,413) and the lean build's idle held (6,920 KB of 7,229). doze-kafka's idle has risen about 1.2 MB since
2026-11-12 (the console, then the registry on by default); that is in issue #36.

doze-aws, from the same PR's next run (43592223803, main with the aws-tls, aws-gaps and aws-correctness merges): 13,128,520 B and 274
packages, inside the 13,900,000 B / 280 lines its stage set; idle 9,964 / 10,036 / 10,024 KB, median 10,024 KB, so its idle line is
**10,526 KB** (was 9,413). That is +1.06 MB over the median of 8,964 KB measured before those merges; added to issue #36 beside the A14 to
A16 step. In the same run doze-kafka's idle median was 10,648 KB, inside its new 11,013 KB.


## The desktop app (`apps/doze-app`), measured 2026-11-13, darwin/arm64

The app has a weight line of its own, apart from the engines' (docs/port/app/PLAN.md §6): GPUI is a few hundred crates
and a 15 MB binary, and none of it reaches doze-kafka or doze-aws (the app is not a default workspace member, and the
`weight` jobs build only the engines). CI's `app` job (macOS) enforces the line.

| | Budget (PLAN.md §6) | Measured |
|---|---|---|
| Binary, release, stripped, fonts included, no capture | ≤ 16 MB | **12,484,800 B with M2 (verbs, dialogs, toasts, preflight, empty folder), `release-app` profile**; 15,575,872 B with the instance, logs and console pages (M1 steps 1.5–1.7; 14,871,584 B before them) |
| Packages (`cargo tree -p doze-app --edges normal`, unique lines) | limit 560 | 535 with M2 (534 before) |
| Menu bar ready after launch | ≤ 100 ms | 71–84 ms |
| Menu bar only, footprint | ≤ 18 MB | 18 MB on an empty doze home; 22 MB with the fixture daemon running in the process |
| First frame of the stack page after a tray click | ≤ 100 ms | 38–47 ms with M2 (`release-app`, shop and larder); 35–66 ms |
| Window open and idle, footprint | ≤ 85 MB | 55 MB at 14 s and 26 s with M2 (larder, `release-app`); 82 MB at 10 s, 74 MB at 25 s (larder) |
| After the window closes, footprint | ≤ 35 MB | 35 MB at 8 s (larder), 28 MB on an empty home |
| CPU, window open and static; menu bar only | ≤ 0.3 %; ≤ 0.05 % | 0.0 % (ps, sampled) |
| Frames over 16 ms while scrolling 100k log lines | 0 | 0 of 300: a full offscreen draw per worst-case scroll step (about a thousand rows a jump, the whole list) is median 4.4 ms, p99 8.8 ms, max 13.7 ms, including the GPU readback a screen frame does not do |
| 100k log lines held | ≤ 25 MB | 6.5 MB (the ring's heap: 100,000 lines of 40–70 bytes) |
| Window while logs follow at 200 lines/s | ≤ 160 MB sustained; under 100 MB within 10 s of stopping | **not met**: 235–250 MB while following; 88–103 MB 10 s after the lines stop |

Measured with `doze-app bench` (it opens the window two seconds after launch as a tray click would, prints its
timings, idles 30 s, closes the window) and macOS `footprint` (Activity Monitor's Memory figure), on a quiet machine
(HID idle time over an hour). The memory budgets hold only with two choices made here:

- **The component library loads after the menu-bar item is up.** `gpui_component::init` takes about 140 ms (most of
  it resolving the system's fonts); done first, menu-bar ready was 233–263 ms. It now runs on the next turn of the
  loop, before any click can open the window.
- **No MSAA for paths** (vendor/gpui-pre-apple/PATCHES.md): upstream's 4 samples measured 87 MB / 79 MB at 10 s / 25 s,
  over the 85 MB budget.

The binary is 3 MB over the spike's 12 MB: doze-desk's hub and the control client, doze-display's words, doze-map's
layout, the ten themes, the tray icons, and the release profile's `opt-level = 3` (the spike used `"s"`). A
size-optimised profile for the app is the first lever if the line is approached.

### After M1 steps 1.1, 1.2 and 1.4 (the Library, a stack's page, the Grid), measured 2026-11-13

Same machine and method; the window now opens on a stack's page in its remembered view (Tiles when none is), or on the
Library when no stack is named.

| | Budget | Measured |
|---|---|---|
| Binary, release, no capture | ≤ 16 MB | 15,707,840 B (+836 KB; 15,992,144 B with `fixtures`): **292 KB under the line** |
| Packages | limit 560 | 535 |
| Menu bar ready after launch | ≤ 100 ms | 70–78 ms |
| First frame after the tray click | ≤ 100 ms | 59–71 ms (larder's page; the empty Library) |
| Window open and idle | ≤ 85 MB | larder's page (Tiles): 59 MB at 10 s, 55 MB at 25 s; the empty Library: 76 MB, 72 MB |
| After the window closes | ≤ 35 MB | 35 MB 3 s after (larder) |
| CPU, window open and static | ≤ 0.3 % | 0.0 % (ps, sampled at 10 s and 25 s) |

The page watches its stack's meters while it shows (one frame a second from a real daemon) and redraws only when what
it shows changed; the fixture daemon sends no meters after its script, so the static readings above are the floor.
The binary is close to its line: the size-optimised profile above is the next step if M1's remaining pages need room.
### After M1 combined (the Library, a stack's page, the Grid, the instance page, logs and consoles), measured 2026-11-13

Same machine and method (`doze-app bench --fixture larder --idle 30`, footprint sampled at 10 s and 25 s after the
first frame, two runs each). With the instance page, the logs and the consoles in, the release build passed the line,
so the app now has its own profile, `release-app` (`opt-level = "s"`, fat LTO, on top of the release profile's
codegen-units 1, strip and panic = abort), and CI's weight line builds that.

| | Budget | release (opt-level 3, thin LTO) | release-app ("s", fat LTO) |
|---|---|---|---|
| Binary, no capture | ≤ 16 MB | 16,378,592 B: **379 KB over** | 12,136,112 B: 3.9 MB under |
| Binary with `fixtures` (what bench runs) | | 16,728,992 B | 12,385,072 B |
| Packages | limit 560 | 535 | 535 |
| Menu bar ready | ≤ 100 ms | 62–80 ms | 58–75 ms |
| First frame after the tray click | ≤ 100 ms | 66–70 ms | 67–73 ms |
| Window open and idle (larder's page) | ≤ 85 MB | 59 MB at 10 s, 55 MB at 25 s | 59 MB, 55 MB |

The size-optimised build costs nothing measurable in the first frame or in idle memory here; what it costs while
drawing hard (the Map at 2,000 cards, the logs following) is not measured yet.

**The logs viewer** (`doze-app bench --fixture shop --logs`, release, `capture` for the draw timing): All logs on a fixture
stack, 100,000 lines printed through the fixture daemon (the same socket, hub and fold a daemon's lines take; filled in
0.4 s), then five seconds of scrolling and thirty of following at 200 lines a second. The page draws only the rows in
view, read from the ring as they are drawn; a new filter is one pass over the ring on a background thread (18.5 ms for
100k lines and a pattern).

The follow budget is not met, and the measurements say why: a window that redraws at all sits at about 220–250 MB here,
whatever is drawn. The stack page idles at 56 MB; four seconds of redrawing the *Map* (no logs open) took it to 220 MB;
following at four redraws a second (the logs take streaming lines in every 250 ms, as Go paced its pushes) peaked at
250 MB, and at one redraw a second still 236 MB. The plan's own spike found the same floor (216 MB at ten redraws a
second, 168 MB for a bare Metal view). Ten seconds after the lines stop the window is back at 88–103 MB. Meeting 160 MB
while following needs the redraw floor itself lowered (the drawable's size and count, gpui's surfaces), not the logs
view; that is open.

On this machine the window's frames were paced at 33 ms even with nothing to draw, so frame intervals cannot show a
16 ms overrun; the draw timing above measures a frame's work directly instead.

### After M2 steps 2.3, 2.4, 2.7 (⌘K, Needs you, keys and menus), measured 2026-11-13

Same machine and method (`release-app`; `doze-app bench --fixture larder --idle 30`, footprint at 12 s and 27 s, two
runs).

| | Budget | Measured |
|---|---|---|
| Binary, `release-app`, no capture | ≤ 16 MB | 12,451,376 B (+315 KB for the palette and attention models, the bell, the alerts, the help sheet and the menus) |
| Binary with `fixtures` | | 12,700,304 B |
| Menu bar ready | ≤ 100 ms | 60–65 ms |
| First frame after the tray click | ≤ 100 ms | 47–48 ms |
| Window open and idle (larder's page) | ≤ 85 MB | 61 / 57 MB and 67 / 63 MB |

Needs you is built off the UI thread on each hub batch (a crash's port is looked up with `lsof`, cached three seconds)
and draws nothing while nothing changes; an item waiting out its grace or its "fixed" asks for one more build three
seconds on, then stops.

**Why a redrawing window sits at 220–260 MB (the follow budget).** Investigated with `footprint -v` while `doze-app bench
--fixture shop --logs --follow` ran, and with the vendored renderer temporarily instrumented (every Metal texture and
buffer gpui allocates logged; knobs for the drawable count, the frame source and the command queue). Findings:

- **What gpui allocates is small.** For the whole run: one path intermediate texture (2560 × 1602, 16 MB, private), two
  1024² atlas textures (shared), one 2 MB instance buffer. The instance buffer never grew; the atlas never passed two
  textures. The drawables are three IOSurfaces of 16 MB (48 MB, mapped).
- **The jump is the GPU driver's.** While frames are being drawn, `footprint` shows about 147 MB of *Owned physical
  footprint (unmapped) (graphics)*: eighteen regions of exactly 8 MB plus a few small ones. None of them is a texture or
  buffer gpui made. They appear within a frame or two of drawing (the window's first frames show 220 MB, then 74 MB once
  idle) and go about ten seconds after the last frame (88–113 MB), which is the "back under 100 MB" figure.
- **Nothing in the app moves it.** Measured follow peaks (MB): window 800 × 500 / 1280 × 800 / 1600 × 1000 → 258 / 259 /
  258 (it does not scale with pixels, so it is not render targets); 3 vs 2 drawables → 259 / 260; command queue capped at
  2 or 4 command buffers → 260 / 258; `CAMetalDisplayLink` vs the CVDisplayLink fallback → 259 / 241 (one run each; the
  fallback's 18 MB is within run-to-run spread and not adopted). The Map redrawing with no logs reaches the same 220 MB, and
  one redraw a second still holds it, because the driver keeps its working set while frames keep coming at all.
- **So it is a fixed per-process cost of an active Metal context on this Mac** (the plan's spike: 168 MB for a bare Metal
  view), not text layout, the atlas or the logs view. Getting under 160 MB while following would need not drawing with
  the GPU at all for a while: frames spaced further apart than the driver's ~10 s idle window, which is not "following".

Recommendation for the owner (§6 budget row): measure the follow line as *growth over the redraw floor* (the logs view
adds 20–40 MB over a window that redraws anything: 221 MB with All logs open, 241–260 MB following 200 lines a second)
and keep "back under 100–115 MB within 10 s of stopping", or accept ~260 MB while a window streams. The knobs used for
the measurements were not kept.

### M2 combined (act + ⌘K + Needs you, wired), measured 2026-11-13

Same machine and method (`release-app`; `doze-app bench --fixture larder --idle 30`, footprint at 12 s and 27 s, two
runs).

| | Budget | Measured |
|---|---|---|
| Binary, `release-app`, no capture | ≤ 16 MB | 12,833,104 B (+382 KB over the ⌘K/Needs you build: the verbs, dialogs, toasts, the checks before Up, the empty folder) |
| Binary with `fixtures` | | 13,065,600 B |
| Menu bar ready | ≤ 100 ms | 71 / 82 ms |
| First frame after the tray click | ≤ 100 ms | 72 / 42 ms (the first run cold) |
| Window open and idle (larder's page) | ≤ 85 MB | 59 / 55 MB and 59 / 55 MB |

Acting adds nothing at rest: Needs you is built again from acting only when a stack begins or ends an action or a
failure comes or goes (a toast alone does not), and the toasts' expiry timer runs only while a toast shows.

### The Map for 20–150 services (branch `app-map-150`), measured 2026-11-13

Same machine and method (`release-app`; `doze-app bench --fixture larder --idle 30`, footprint at 12 s and 27 s, two
runs; and the same on `medium-150`, a 142-card stack drawn by region). Frame times: `doze-app bench --fixture
medium-150x500 --map` (a `release` build with `capture`): each frame drawn in full offscreen (layout, paint, the GPU's
render and the readback a screen frame does not do), 120 frames a phase.

| | Budget | Measured |
|---|---|---|
| Binary, `release-app`, no capture | ≤ 16 MB | 12,949,072 B (+116 KB over M2: regions, the tile and its list, the focus views, the fades, the camera's flights) |
| Binary with `fixtures` | | 13,214,624 B |
| Menu bar ready | ≤ 100 ms | 90 / 76 ms (larder), 90 / 83 ms (medium-150) |
| First frame after the tray click | ≤ 100 ms | 64 / 41 ms (larder), 56 / 58 ms (medium-150) |
| Window open and idle | ≤ 85 MB | larder 65 / 61 and 77 / 72 MB; medium-150 74 / 70 and 74 / 70 MB |
| Layout, 150 services and 500 calls | ≤ 20 ms | 0.63 ms (best of five in the app; `cargo bench -p doze-map`: 0.57 ms median) |
| Frames over 16 ms at 150 / 500 | 0 | 0 of 720: pan max 5.4 ms, zoom 5.7, hover 6.0, every line 6.4, a selection mid-fade 6.2, a camera flight 6.8 (medians 4.8–5.3 ms) |
| Frames over 16 ms at 2,000 (synthetic, by region) | — | 0 of 480 before the fades (max 5.9 ms) |

Before the fades and eased zoom (the same bench, same build otherwise): pan max 5.7 ms, zoom 6.3, hover 5.2, every line
5.3. The fades cost about half a millisecond a frame while they run and nothing at rest.

**After the region links and the states (same branch, 2026-11-13).** One link per pair of regions under opaque frames
(the grid routing removed), and the state marks, words and module versions every screen draws. `release-app`:
12,965,680 B (13,231,248 B with `fixtures`); menu bar ready 71–90 ms; first frame 47 / 55 ms (larder), 73 / 74 ms
(medium-150); idle 59 / 55 MB (larder), 73 / 69 MB (medium-150). Frames at 150 / 500 (same bench): 0 of 720 over 16 ms;
max pan 9.0 ms, zoom 4.8, hover 5.3, every line 5.3, a selection mid-fade 5.2, a flight 4.5 (medians 3.8–4.0 ms).
Layout 1.4 ms; the overview with its links under a millisecond.
### M3 steps 3.1, 3.2, 3.5 (the menu bar complete, notifications, Doze.app), measured 2026-11-13

Same machine and method (`release-app`; `doze-app bench --fixture larder`, footprint sampled with `footprint`; menu
bar only is `doze-app --menu-bar` under a throwaway `DOZE_HOME`). The bench now starts the app's round-the-clock part
as a launch does (`crate::around`: the menu, Needs you while no window is open, the notification rules).

| | Budget | Measured |
|---|---|---|
| Binary, `release-app`, no capture | ≤ 16 MB | 12,899,472 B (+66 KB over M2 combined: the tray model, the notification rules, `open`) |
| Packages | limit 560 | 535 (no new crate: notifications are gpui's own `UNUserNotificationCenter` path) |
| Menu bar ready | ≤ 100 ms | 75 ms; 135 ms in a second run while another worktree was building (machine busy) |
| First frame after the tray click | ≤ 100 ms | 47 / 54 ms |
| Window open and idle (larder's page) | ≤ 85 MB | 60 MB at 12 s, 56 MB at 27 s; 57 MB |
| Menu bar only, footprint | ≤ 18 MB (PLAN §6); ≤ 35 MB (M3 brief) | 18 MB on an empty home; 20 MB with one live stack (fixture in its own process; M2: 21 MB); 22 MB with the fixture in the process |
| After the window closes, footprint | ≤ 35 MB | 37 MB with the fixture daemon in the process; **35–36 MB with it in its own process** (below) |
| CPU, menu bar only | ≤ 0.05 % | 0.0 % (ps, sampled at 25 s) |

The menu is rebuilt only when its signature changes: Needs you's detail (its "next in 4s") is left out of the
signature, and the stack line no longer carries the countdown (Go's line did, so Go rebuilt its menu every second
through a crash loop).

**After the window closes, the fixture out of the process.** `doze-app fixture-serve larder ROOT` (feature `fixtures`)
runs the fixture daemon alone, on its stack's own socket; the app (`release-app`, no fixtures) is benched against
`DOZE_HOME=ROOT/home` like a real home, so the reading is the app alone. Three runs each, interleaved, M2's binary
(`origin/app-m2`) against M3's, `--idle 15`, footprint 3 s and 8 s after the close:

| | Window open | 3 s after close | 8 s after close |
|---|---|---|---|
| M2 | 59 / 63 / 59 MB | 36 / 36 / 35 MB | 35 / 35 / 35 MB |
| M3 | 60 / 59 / 60 MB | 36 / 36 / 36 MB | 36 / 35 / 36 MB |

M3 costs at most 1 MB over M2 after the close (the menu bar's own Needs you list and notification rules). The 37 MB
was the in-process fixture plus noise. A 60 s reading after the close stays at 36 MB; `footprint -v` puts 20 MB of
it in Malloc Small: what the window's libraries (gpui-component, the fonts and their caches) leave in the heap after
the window is dropped. That is M2's floor too, sitting on the 35 MB line; bringing it under is a 3.6 (`app-bench`)
item, not this step's.

**Menu bar only, the fixture out of the process** (larder served by `doze-app fixture-serve`, the app `--menu-bar` on
that home, `release-app`): M3 20 MB at 10 s and 25 s (two runs); M2's binary 21 MB the same way. So with one live stack
the menu-bar-only app sits 2 MB over PLAN §6's 18 MB (18 MB on an empty home), as M2 did; under the M3 brief's 35 MB.
The menu bar's own Needs you list, notification rules and service submenus cost nothing measurable here.

### M3 Activity and Settings (steps 3.3, 3.4), measured 2026-11-13

Same machine and method (`release-app`; `doze-app bench --fixture larder --idle 30`, footprint of that pid at 12 s and
27 s, two runs).

| | Budget | Measured |
|---|---|---|
| Binary, `release-app`, no capture | ≤ 16 MB | 13,131,648 B (+298 KB over M2 combined: Activity, Settings, the leftovers look, the admin prompt, the login item) |
| Binary with `fixtures` | | 13,363,952 B |
| Packages (`cargo tree -p doze-app --edges normal`, unique lines) | limit 560 | 540 (no new outside crate: `doze-journal` and `doze-os` were in the tree) |
| Menu bar ready | ≤ 100 ms | 71 / 90 ms |
| First frame after the tray click | ≤ 100 ms | 44 / 48 ms |
| Window open and idle (larder's page) | ≤ 85 MB | 60 / 56 MB and 60 / 56 MB |

**The update check's HTTPS client (PLAN.md Q6).** Measured on a small program that fetches GitHub's latest release and
reads its JSON (opt "s", fat LTO, stripped, darwin/arm64): 337,296 B with the system's `curl` as a subprocess;
933,856 B with `minreq` 2 and rustls (+0.58 MB, +13 crates); 1,284,080 B with `ureq` 3, rustls and the platform
verifier (+0.93 MB, +32 crates). rustls is in the workspace's lock for other crates but not in the app's tree. The app
uses `curl` (`doze_desk::settings::update::fetch`): no bytes, no crates, and the check runs only when About opens or is
asked (never in the background), so a subprocess per check costs nothing at rest. With no `curl`, About says it could not
reach GitHub. When the Rust CLI has `doze version --check`, the app asks it instead.

Activity at rest: it reads the journal again every ten seconds while shown (as Go's page did), off the UI thread, and
redraws only when a figure changed; the strips are painted, so a 2,000-service stack builds only the rows in view
(`doze-desk` lays out 2,000 services over 10,000 events in a few milliseconds, `tests/activity.rs`).

**The app carries the CLI (core C1, 2026-11-14).** Owner decision: Doze.app holds the GUI and the full CLI in one
binary, `doze-app <command>` being the doze CLI (`doze` alone stays CLI-only, under 10 MB). Linking `doze-cli` into
`doze-app` (darwin/arm64, `release-app`):

| | Line | app-try (a112aa29) | With the CLI |
|---|---|---|---|
| Binary | ≤ 16 MB | not re-measured here (13,131,648 B at M3 combined) | 15,137,792 B |
| Packages (`cargo tree -p doze-app --edges normal --target aarch64-apple-darwin`, unique lines) | was 560, now 640 | 537 | 631 |

The 94 packages are the CLI's own: clap, the registry client (ureq, rustls, ring, the platform roots, ed25519), the
YAML reader and jiff for the wall clock. The package line moves to 640 (the measured 631 plus 9) because the owner's
decision puts them there; the byte line stays 16 MB, now with 0.86 MB of room. If that room is needed back, the
lever is one HTTP and TLS stack shared by the app and the CLI (core plan §6.3).

## Core P1 (dns-setup, trust, doctor --fix), measured 2026-11-14, release

The privileged commands bring three things into the CLI: the zone's setup and trust scripts and the doze certificate
authority (doze-names `setup`: x509 and p256 were in the tree, now linked), the network daemon the setup installs, which
is this same program run as `__doze-netd` (doze-names `netd`: tun-rs and the crates under it), and `doze-doctor`. A
separate netd binary would save the CLI those bytes but would be a second program to ship, install and keep in step;
Go's model, which the plan keeps, is one binary.

| | Line | Before (C1 + app, 10e18e80) | P1 |
|---|---|---|---|
| `doze`, darwin/arm64, `release` | 9 MB (ceiling 10) | about 3.8 MB | 4,592,176 B (+0.79 MB) |
| `doze`, Linux aarch64 (Lima VM), `release` | 9 MB | not measured | 4,986,168 B, 185 packages |
| `doze-app`, darwin/arm64, `release-app` | 16 MB | 15,137,792 B | 15,654,752 B (+517 KB) |
| `doze-app` packages (CI's count) | was 640, now 650 | 631 | 640 |

New packages: `doze-doctor`, `tun-rs`, `nix`, `route_manager`, `getifaddrs`, `ipnet`, `memoffset` (the network
daemon), `privesc` (UAC on Windows; on macOS and Linux the system's prompt is `doze-os`'s). The app's package line
moves to 650: the measured 640 plus the room the C1 line had. **The app's byte room is now 345 KB under 16 MB**: the
next core crates that join the CLI (the daemon, the engine, config) will not fit in the app without the lever §6.3 names
(one HTTP and TLS stack for the registry client and the front door; ureq + rustls-native-certs is the obvious
duplicate) or a size profile change; this has to be decided before D1 joins the CLI. CI now holds `doze` to 9 MB and
200 packages (the `weight` job).
**doze-aws with every Go service (2026-11-13, release-aws): 7,444,224 B (7.10 MiB) on darwin/arm64, 258 packages, idle 5,344 KB; 11,764,560 B on x86_64 Linux (musl), 10,561,264 B with JSONata off.** Over the "z" base (4,867,344 B / 233 on macOS, 7,481,552 B on Linux): Step Functions, API Gateway and CloudFormation together add 2.58 MB on macOS and 4.28 MB on Linux (1.66 times). The owner raised doze-aws's ceiling to 20 MB the same day; the CI line is 12,400,000 B (musl plus 5%) and 264 packages. Cross-building now needs a C compiler and an ELF archiver for the target (JSONata's `psm`): a `clang --target=x86_64-unknown-linux-musl` wrapper as `CC_x86_64_unknown_linux_musl` and `llvm-ar` as `AR_x86_64_unknown_linux_musl`.


## Core F1 (`doze` with doze-fetch), measured 2026-11-14, release, darwin/arm64

The `doze` CLI (package `doze-cli`) gets its first line in `xtask/weight-limits.txt`. F1 links the registry client: ureq,
rustls with ring, rustls-native-certs, tar and flate2, ed25519-dalek and yaml-rust2. With the F1 audit fixes (cross-process
locks from doze-os, which `doze` already linked): **3,148,064 B, 169 packages**. The F1 audit measured 3.05 MB before
them; the fixes cost about 100 KB (the capped unpack, the lock merge, the platform list), which is the judgement call
the owner leaves to the builder.

- Limit: 4,300,000 B (the macOS size times 1.3 for an x86_64 Linux build, plus 5%, the rule of the doze-aws line) and
  175 packages (six of room). It is re-based from the first Linux CI run.
- Idle memory is not weighed: the CLI serves nothing by itself (the line's `0`; the daemon gets its own line when it
  exists). Downloads run in `doze __fetch` children, so none of this is resident in a daemon.
- The 10 MB ceiling (owner, 2026-11-12) stands far above.

## Core C2b (`doze`: init, add, modules upgrade, bug, open, update), measured 2026-11-14, release, darwin/arm64

| | Line | C2a (#51) | C2a + F1 merged | C2b |
|---|---|---|---|---|
| `doze` bytes | 9,000,000 | 6,207,024 | 6,505,152 (+298,128) | 6,802,928 (+297,776) |
| `doze` packages (CI's count) | 230 | 210 | not counted | 221 (210 without `tls`) |

- **F1 merged in (+298 KB).** doze-fetch replaces doze-cli's registry code (the same bytes) and brings what `doze` did not
  have: the `__fetch` child (module resolve, the engine mirror, archive unpacking with tar and flate2, tokio's process
  support) and the `doze.lock` writer. These are F1's design (downloads run in `doze __fetch`, never in a daemon), and
  `modules upgrade` needs them in `doze` itself.
- **C2b's commands (+298 KB).** doze-detect (Go's detector without its engine table; regex and yaml-rust2 were already
  linked), `init`, `add`, `modules upgrade` (resolving and verifying a module build), `bug`, `open`, `update`, and
  the examples in their help.
- **One HTTP stack.** `doze_cli::http` is doze-fetch's client: ureq over rustls in `doze` (feature `tls`), the system's
  curl without it. The app can build doze-cli with `default-features = false` to drop ureq and rustls; that is the app
  owner's change (its size profile is being worked on separately).
- Over C2a in all: +595,904 B, under the 9 MB line by 2.2 MB. xtask's `doze-cli` line, which F1 had set at 4.3 MB for the
  registry client alone, is now the CLI's: 9,000,000 B and 230 packages, as the `weight-doze` job.

## Idle (2026-11-15, branch `fix-idle`), release, darwin/arm64 and the Linux aarch64 VM

What an idle broker does, before (main at 33ed4871) and after this pass, by the method of the idle audit of 2026-11-14 (its
`idle.sh`): the release binary on a fresh data directory, the console and the schema registry on, sampled at the start, half way
and the end. "Filled": a franz-go client made a compacted topic and a plain one (3 partitions each, `segment.bytes` 64 KiB) and wrote
30,000 records of 200 B to each, the broker was stopped and started again (so every partition is closed, as after a restart), and
it runs with `--retention-ms 86400000` (every topic has retention, as when `doze.hcl` sets `retention`) and
`--log-retention-check-interval 30s` (so the retention sweep runs inside the window; the default is 5 minutes). Context switches are
`top`'s CSW on macOS and the sum of the threads' `voluntary_ctxt_switches` on Linux (live threads only, so the "before" figures
with threads that came and went are low).

| | macOS before | macOS after | Linux before | Linux after |
|---|---|---|---|---|
| Empty, 120 s: context switches | 537 (4.5/s) | 28 (0.23/s) | 510 (4.3/s) | 26 (0.22/s) |
| Empty: CPU time over 120 s | 0 | 0 | 30 ms | 0 |
| Empty: threads, descriptors, RSS | 3, 18, 7.6 MB | 3, 18, 7.7 MB | 3, 14, 8.1 MB | 3, 14, 8.6 MB |
| Filled, 180 s: threads at 90 s and 180 s | 6, 6 | 3, 3 | 10, 6 | 3, 3 |
| Filled: descriptors at the end | 38 | 18 | 36 | 14 |
| Filled: RSS at 90 s, at the end | 10.9, 10.9 MB | 11.4, 7.3 MB | 10.0, 10.3 MB | 10.2, 10.2 MB |
| Filled: CPU time, first half and second half | 30 ms, 20 ms | 30 ms, 0 | 50 ms, 40 ms | 20 ms, 0 |
| Filled: context switches in the second half | 722 (8/s) | 23 (0.26/s) | not comparable (threads died) | 23 (0.26/s) |

What made the difference, item by item:
- **The transaction coordinator** looked every 500 ms whatever there was: those were nearly all of an idle broker's wake-ups. It now
  sleeps until the first timeout, idle-id expiry or retry, is woken when a change makes that sooner, and parks with no
  transactional id (`txn/alarm.rs`). The remaining 0.22 wake-ups a second are the cleaner's pass every 15 s (it reads the topic
  list; with no compacted topic it touches nothing) and the runtime's own timer wheel.
- **The cleaner** reopened every non-empty compacted partition every 15 s (replaying up to 16 MiB of producer history, planning,
  closing), which was the 3 to 5 to 3 thread churn and about 2 MB a topic. It now keeps each partition's verdict (not due, and the
  first time it could be: `compact::plan::next_due`) with the count of changes the partition's writer had made, and does not open
  or list a partition whose verdict still holds. After the first pass of a run nothing is opened until a write or that time.
- **The retention sweep** opened closed partitions and left them open until the 512 limit (the "before" descriptors and RSS). It
  now closes what it opened and keeps the same kind of verdict: an age bound takes the oldest segment first, so nothing can go
  before that segment outlives `retention.ms` (`PartitionLog::retention_due`); a size bound moves only with writes.
- **Sweeps and the blocking pool.** A sweep's directory listings and store reads went through `block_in_place`, which hands the
  worker's core to a thread started for it; the thread left behind lingers for the pool's 10 s keep-alive. They now go to
  `spawn_blocking` (`blocking::in_background`), which uses one pool thread for the whole pass and never swaps a worker. With the
  verdicts, an idle pass does no blocking work at all, so the pool stays empty: 3 threads throughout the filled run.
- **Partitions opened by traffic** are closed after five quiet minutes (`Config::partition_idle_close`; a fetch or request holding
  one keeps it), by a task that sleeps until the next is due and parks with none open; **the frame shelf** (up to 64 MiB of request
  buffers) is freed after a quiet minute by a server task parked on a bell while the shelf is empty.
- The RSS that stays after the first pass on Linux (10.2 MB against 8.6 MB empty) is glibc keeping the arena the first pass's history
  replay used; macOS gave it back within the window (7.3 MB). Nothing holds it: a `malloc_trim` after a pass would return it, and was
  not added (a glibc-only call, for 1.6 MB after a restart with compacted data; the CI idle limit is measured on an empty broker and
  is unchanged).

**Not changed, measured: the produce path's `block_in_place`.** Under a franz-go client writing 60,000 records to six partitions
(acks=all) the macOS broker ran 14 threads; twelve seconds after the load it was back to 4 (the keep-alive). That is the partition
writers' flushes handing their worker over; whether a dedicated flush thread or `spawn_blocking` does better under load is a
benchmark for the hot path, not this pass.

**Not changed: the committed-offset expiry sweep** (every 600 s by default: a hundredth of the 7-day retention, capped). Sleeping
to the earliest expiry would mean knowing, for every group, when its offsets begin to expire, which depends on whether the group has
members, when it emptied and its members' sessions, all of which change outside the store; one wake every ten minutes is 0.0017 a
second. It now reads the store on the blocking pool like the other sweeps.

**Not changed: the schema registry loads every schema at start.** Measured (release, in memory): 5,000 Avro schemas of 21 fields,
4.1 MB of records, open in 6.4 ms. Loading on first use would save that at start and the memory of the schemas for a registry never
used, at the cost of a corrupt record failing the first request instead of the start; at a local developer's scale (tens of
schemas) the start cost is under a millisecond, so it stays eager.

**The console recorder's ring** (2,000 rows, 1 MiB at most) keeps its rows however long the broker is idle: they are the history
the Live page shows. The comment that said "an idle ring holds nothing" now says it holds nothing only until the first request
(CON-307).

### A quiet consumer (2026-11-26), release, darwin/arm64

The idle figures above are a broker with no clients. The usual local state is different: an app is connected, its consumer group
waits for records, and now and then it writes some. An audit of that state, measured with kcat (librdkafka) and franz-go on
loopback:

| | Before | After |
|---|---|---|
| No clients, 120 s | 3 threads, 0.01 s CPU, no idle wake-ups | unchanged |
| One quiet consumer group, 60 s | 4 threads for good | 3 |
| An app at 5 records/s (acks=all) and its group, 40 s | 5 to 6 threads, 4,000 to 5,800 switches | 5 to 6 threads, 4,150 to 4,480 switches |
| Three apps at 200 records/s each, 40 s | 10 to 13 threads, 212,000 to 219,000 switches | 10 to 12 threads, 186,000 to 191,000 switches |
| 12 s after that load | 6 threads | 3 threads |

What changed:
- **A fetch with nothing to read runs where it is.** Each fetch round went through `block_in_place`, even when every partition was at
  its end and the round touched no file. A quiet consumer fetches twice a second (Java's `fetch.max.wait.ms` is 500 ms), so a
  blocking thread was woken for nothing every half second, and its 10 s keep-alive never ran out. A round whose partitions are all
  at their end, or refused, now runs on the worker (`handlers/fetch.rs`, `at_the_end`).
- **A small produce batch is checked where it is.** The check is CPU work only (a CRC, a walk of the records, a re-encoding only
  where the codec changes). Batches of 16 KiB or less, the usual size from an app, no longer hand the worker over
  (`handlers/produce.rs`, `CHECKED_HERE`). The partition writer's own hand-off stays: a write into the page cache can stall while
  the kernel throttles dirty pages, and a flush always blocks.
- **An offset committed again unchanged is not written again** for a minute, or a tenth of `--offsets-retention` when that is
  shorter (`MetaStore::commit_offsets_unless_held`). Java's consumer commits every position it holds every five seconds whether it
  moved or not, and each commit was a flush of the metadata database (`F_FULLFSYNC` on macOS). So a quiet Java app cost one full
  flush every five seconds; now it is one a minute. The stored commit time lags by up to that minute, which is how much sooner such
  an offset can expire and how stale the console's "committed" time can be. librdkafka already skips unchanged commits (a quiet kcat
group wrote nothing to the database in 60 s).
- **Console tails stop in a hidden tab.** The Live tail and a topic's Messages tail kept their stream open in a background tab,
  and the broker kept reading for them once a pace; the Messages tail's reads also kept its partitions open past their 5-minute
  idle close. Both now close the stream while the tab is hidden and go on from where they were when it is seen again (the Tail page
  already did).

**Not changed, measured: the cleaner's look every 15 s** with no compacted topic: it reads the topic list in memory and touches
nothing (0.07 wake-ups a second). Parking it would also change the console's "last looked" time.

**Not changed: blocking-thread limits.** The runtime keeps tokio's defaults (at most 512 blocking threads, 10 s keep-alive). The
most measured under load is 13 threads; a cap would only matter in a stall, and the keep-alive is what lets a burst reuse its
threads.

### A flush per produce on macOS (2026-11-26, the owner's call)

std's `sync_data` and `sync_all` are `fcntl(F_FULLFSYNC)` on Apple platforms: the drive empties its write cache before the call
returns. That takes milliseconds and slows every other program's writes meanwhile. doze-kafka flushes once per group of acks=all
produces (Java producers send acks=all by default), once per partition every `--fsync-interval` for acks=1, and once per
metadata commit, so a busy app asked for a full flush many times a second. PLAN.md had chosen `F_FULLFSYNC` to match Go.

doze-kafka now calls `doze_fs::fs::full_flush(false)` at start, and its flushes on macOS are `fsync(2)` (through rustix: no
`unsafe`). What is flushed survives a crash of the broker and of macOS; only a power cut can lose what the drive had not yet
written from its cache. `kill -9`, the usual crash in development, never lost anything either way. Linux is unchanged (its `fsync`
is already the whole flush), and so is every other user of `doze-fs` (doze-aws, doze's own store): the default stays `true`.

Measured (release, darwin/arm64, loopback, franz-go, two runs each):

| | `F_FULLFSYNC` | `fsync(2)` |
|---|---|---|
| 200k × 1000 B, acks=all, one producer | 161k to 173k records/s, broker CPU 0.14 to 0.16 s | 1.68M to 2.14M records/s, 0.08 to 0.09 s |
| An app at 100 records/s (acks=all) and its group, 20 s | broker CPU 0.42 to 0.43 s | 0.38 to 0.39 s |

acks=all is now about as fast as acks=1 (1.25M to 1.68M records/s on the same machine and load), since the flush no longer waits
on the drive's cache.

## One build: CI's measurements (2026-11-14)

Main's first CI run with the one build (TLS in the default, the registry with Avro, JSON Schema and Protobuf), run 43588988185 on PR #45,
replaces the estimates above, by the rule of "Limits raised to the measured values": bytes and packages exact, idle the median of three
plus 5%. Small, justified rises under the owner's delegation of 2026-11-14.

| job | measured | old limit | new limit |
|---|---|---|---|
| `doze`, darwin/arm64, `release` | 9 MB (ceiling 10) | about 3.8 MB | 4,592,176 B (+0.79 MB) |
| `doze`, Linux aarch64 (Lima VM), `release` | 9 MB | not measured | 4,986,168 B, 185 packages |
| `doze-app`, darwin/arm64, `release-app` | 16 MB | 15,137,792 B | 15,654,752 B (+517 KB) |
| `doze-app` packages (CI's count) | was 640, now 650 | 631 | 640 |

New packages: `doze-doctor`, `tun-rs`, `nix`, `route_manager`, `getifaddrs`, `ipnet`, `memoffset` (the network
daemon), `privesc` (UAC on Windows; on macOS and Linux the system's prompt is `doze-os`'s). The app's package line
moves to 650: the measured 640 plus the room the C1 line had. **The app's byte room is now 345 KB under 16 MB**: the
next core crates that join the CLI (the daemon, the engine, config) will not fit in the app without the lever §6.3 names
(one HTTP and TLS stack for the registry client and the front door; ureq + rustls-native-certs is the obvious
duplicate) or a size profile change; this has to be decided before D1 joins the CLI. CI now holds `doze` to 9 MB and
200 packages (the `weight` job).

**Data control (2026-11-15, `docs/port/data-control.md`).** Measured on darwin arm64 against main at 3636c3d2 built the same day: the
default build 8,001,424 to 8,167,424 B (+166,000 B), times 1.3747 is 11,227,858 B on ubuntu x86_64, under its 11,549,536 B line; the lean
build 4,099,056 to 4,198,448 B (+99,392 B), so its CI line rises by that times 1.3747, 5,274,120 to **5,410,754 B**, an estimate until CI
measures it. No new package in either build (143 unique names before and after in the default, 125 in the lean on Linux): the default
tree's package line goes 184 to 186 for two repeat entries (`doze-fs (*)`, `rustix (*)`), as in the registry's `kafka-registry (*)`
before. statvfs (rustix) is behind doze-fs's `space` feature, which only kafka-console turns on, so the lean build and doze-aws do not
carry it. Idle 7,952 KB on darwin (nothing new runs: the usage is walked when a page asks).

doze-aws held (median 8,728 KB of 9,413) and the lean build's idle held (6,920 KB of 7,229). doze-kafka's idle has risen about 1.2 MB since
2026-11-12 (the console, then the registry on by default); that is in issue #36.

**`weight-tls-idle` after the efficiency pass (2026-11-15).** Line not moved: main's idle with TLS rose from about 10,820 KB to about
11,300 KB at 3636c3d2, and the 11,479 KB line was measured before that. Bisected on aarch64 Linux: +268 KB, mostly 086e4b28 (`fix-correct`,
+132) and 0ee5cf74 (`fix-idle`, +68). Numbers and runs are in ci-health.md ("ci-flakes-2"); for the kafka session to explain or take back.

doze-aws, from the same PR's next run (43592223803, main with the aws-tls, aws-gaps and aws-correctness merges): 13,128,520 B and 274
packages, inside the 13,900,000 B / 280 lines its stage set; idle 9,964 / 10,036 / 10,024 KB, median 10,024 KB, so its idle line is
**10,526 KB** (was 9,413). That is +1.06 MB over the median of 8,964 KB measured before those merges; added to issue #36 beside the A14 to
A16 step. In the same run doze-kafka's idle median was 10,648 KB, inside its new 11,013 KB.

## doze's profile: room for the daemon (2026-11-15, core C2b), `doze` on darwin/arm64 and x86_64 Linux

At C2b `doze` was 8,948,248 B on CI's x86_64 Linux, 52 KB under its 9 MB line, with the daemon, the engine and the module host
still to come. Each lever was measured on the C2b head (284d2ff7). Linux numbers are a cross build (`x86_64-unknown-linux-gnu`,
zig as the linker), within 0.3% of CI's (8,924,224 B here against CI's 8,948,248 B for the same code). `doze status` is warm, with
the daemon down, on examples/shop's doze.hcl, median of 80 after 5 warm-ups, on the reference Mac.

| Option | x86_64 Linux | darwin/arm64 | `status` warm (darwin) |
|---|---|---|---|
| C2b as it was (`release`, the broker's profile) | 8,924,224 B | 6,802,944 B | 7.65 / 8.06 ms (two runs) |
| doze's own crates at "s" (doze-*, yaml-rust2, ureq, x509-cert, der, tar, jiff, clap_complete, num-bigint, flate2, winnow) | 7,522,720 B (−1.40 MB) | not measured | |
| the same at "z" | 7,257,776 B (−1.67 MB) | | |
| the same without the crates the broker or doze-aws link (flate2, miniz_oxide, der, doze-os, doze-module-proto), "z" | 7,263,136 B (−1.66 MB) | 7,109,168 B (**+306 KB**) | |
| the same at "s" | | 7,226,160 B (**+423 KB**) | |
| **the whole build at "s": profile `release-doze`** | **7,123,432 B (−1.80 MB)** | **5,531,712 B (−1.27 MB)** | 8.29 / 8.20 ms |
| the whole build at "z" | 6,901,448 B (−2.02 MB) | | |

- **Chosen: `release-doze`**, `inherits = "release"`, `opt-level = "s"`, the same move as the app's `release-app`. It is the only
  option that shrinks both platforms: per-package levels on doze's crates shrank Linux but grew darwin (fat LTO with mixed levels;
  measured twice, the baseline rebuilt to the byte). The broker's profile and its hot path are untouched, since `release` is unchanged.
  The workspace's per-package overrides (clap and regex at "z") are inherited: `release-doze` measures the same as `release`
  with `opt-level` overridden.
- **Headroom**: 1.88 MB under the 9 MB line on x86_64 Linux (target 1.5 MB), 3.47 MB on darwin. CI confirmed it on bc68fd9b: `doze` 7,086,496 B on ubuntu x86_64 (from 8,948,248 B, so 1.91 MB under the line), 219 packages, and a warm `doze status` with a median of 11.15 ms on the shared runner (no earlier CI figure to compare with; the darwin before and after is above).
- **Cost**: about 0.3 ms on a warm `doze status` (7.65–8.06 to 8.20–8.29 ms), inside run-to-run noise and half the 15 ms budget.
  CI's weight-doze job now also reports the x86_64 Linux number (not gated). "z" saves 0.22 MB more on Linux but turns off loop
  vectorisation for the daemon to come (log store, proxy); not worth it with this headroom.
- **Not levers**:
  - Duplicate dependencies (`cargo tree -d`): only getrandom 0.2/0.3/0.4 (ring, doze-names, crypto-bigint), a few KB, and syn 2/3
    (build time only).
  - tokio: 96 KB of `.text`; the daemon needs `net`, `time`, `process`, `signal` and `io-util` anyway.
  - jiff: 104 KB with jiff-core; `tzdb-bundle-platform` bundles only on Windows. chrono with a zone database would not be smaller.
  - Linux carries `.eh_frame` (489 KB) and `.rela.dyn` (352 KB) that Mach-O does not; dropping them needs RUSTFLAGS
    (`-C force-unwind-tables=no`, a static relocation model), which CI's own `RUSTFLAGS` would override, so they are left.
  - What is heavy and linked once (cargo-bloat, Linux `.text`): yaml-rust2 236 KB (doze-fetch's meta.yaml), the regex stack
    about 520 KB (doze-hcl's `regex`), rustls and ring about 600 KB (ureq, the front door). Each is needed; yaml-rust2 is the
    one candidate for a small owned parser later.
- **Builds**: CI's weight-doze job and `cargo xtask weight` build `--profile release-doze` (binary at `target/release-doze/doze`).
  Release tooling for `doze` must use the same profile; `--release` still builds a working, bigger `doze`.
- **The app** (`release-app`, already "s" throughout) is not changed by this: its 16 MB line has been red since C2a
  (16,733,040 B at #51, 17,064,576 B at #53); that is the app's size profile, worked on separately.

## C2c: the daemon inside `doze` (2026-11-15)

With D1's `doze __daemon` merged into C2c (the daemon, the engine, doze-config's full load and the control server are now in the
CLI's binary, as Go's were), `doze` in `release-doze` on darwin/arm64 is 6,211,408 B (C2b: 5,531,712 B, +680 KB) and 234 packages
(C2b: 221). Bytes stay 2.8 MB under the 9 MB line (x86_64 Linux, about 1.29 times darwin, should be near 8.0 MB; CI's weight-doze
job reports it). The package line moves from 230 to 240: the daemon's own crates, a small rise with its reason, under the owner's
delegation of 2026-11-14.

Session 10 (D1's session 9 merged: converge on start, meters, the data ops and the console topic, the plan op; and `connect`):
`doze` in `release-doze` on darwin/arm64 is 6,725,008 B (+514 KB over the line above), about 8.7 MB on x86_64 Linux at CI's
1.29; CI's weight-doze job gives the measured Linux number on this push. Under the 9 MB line, with less room than before:
most of the rise is the daemon's (the console topic and meters), not the CLI's.

Session 11 (D1's session 10 merged: add and remove at run time, `migrate` and `lint` as daemon calls, per-major builds;
and C2c's migrate, lint's module pass, the verbs that start the daemon, sync's block drift, the fetch child's progress line).
Measured 2026-11-15 on the C2c head, `release-doze`; x86_64 Linux is a cross build with zig as the linker (within 0.3% of
CI's, above):

| | x86_64 Linux | darwin/arm64 | packages |
|---|---|---|---|
| session 10 as CI measured it (5328d9e9) | 8,505,920 B | 6,725,008 B | 233 |
| D1 session 10 merged, and this session's work | **8,994,120 B** (5,880 B under the line) | 6,940,192 B | 235 |
| `regex` without its `perf` engines (doze-hcl, doze-detect) | **8,607,968 B** (392 KB under) | **6,723,920 B** | 233 |

- **The lever: `regex` with `default-features = false, features = ["std", "unicode"]`** in doze-hcl and doze-detect, the only
  two users in `doze`. The `perf` features (the lazy DFA, the one-pass and bounded backtracker engines, aho-corasick and
  memchr literal search) make matching faster, never different: the same matches come out of the PikeVM. −386 KB on Linux,
  −216 KB on darwin, two packages fewer (aho-corasick and memchr). Cost: `doze init --dry-run` over a 14,725-file Go
  project (doze-detect's scans) is 12 ms before and 14 ms after (median of four, warm); HCL's `regex` functions run on
  config values of a few bytes. A side effect is closer parity: without the DFA the compiled-size limit no longer refuses
  three `\pL{0,1000}` that Go accepts (`fn2.regex3.07` matches Go now; it was a known difference, hcl-values.md F22).
- Where the rest of the rise went (cargo-bloat, darwin `.text`): doze-daemon 373 KB, doze-engine 398 KB, doze-cli 564 KB,
  doze-config 251 KB. D1 session 10 (add/remove, migrate, lint, per-major builds) is most of +215 KB on darwin; the CLI's
  share this session is migrate, lint's pass and the block drift, a few tens of KB.
- Not taken: `opt-level = "z"` for the whole build (−0.22 MB on Linux at C2b, but no loop vectorisation for the log store
  and the front); `regex-lite` in doze-hcl (Go's `\w` is ASCII in both, but `regex-lite` has no Unicode classes, which
  HCL's `regex` must take). doze-engine's `regex-lite` (31 KB) stays: its log patterns are Go's RE2 and need nothing more.
- The 9 MB line is unchanged; the package line stays 240.

## D2: names, the front door, TLS and downloads in the daemon; `release-doze` at "z" (2026-11-19)

D2 put the zone into the daemon: `doze-bind`, doze-names' resolver and front door (hyper's server and client, rustls'
server side, the CA's leaves), the hand-off to modules, the download fetch. x86_64 Linux is a cross build with zig as
the linker (the base measured 8,652,824 B here against CI's 8,607,968 B, +0.5%), `release-doze`:

| | x86_64 Linux | darwin/arm64 | packages |
|---|---|---|---|
| core-land (4ae41117), "s" | 8,652,824 B | | 233 |
| D2, "s" | **9,192,984 B** (193 KB over the 9 MB line) | 7,171,888 B | 234 (doze-bind) |
| D2, "s" with "z" on doze's own crates, rustls, yaml-rust2, hcl-edit, ureq | 8,729,016 B | **9,009,552 B** (+1.84 MB) | |
| **D2, the whole build at "z"** | **8,754,344 B** | **6,031,248 B** | 234 |

- Where the +540 KB went (cargo-bloat, Linux `.text`, D2 against the base): doze-names +199 KB (the front door, the
  resolver with its upstream relay, the certificate resolver), doze-daemon +87 KB, rustls +65 KB (the server side),
  hyper +28 KB (the server), doze-bind +18 KB, std +19 KB.
- **Chosen: `release-doze` at "z"** (Cargo.toml, dated comment). Per-package levels grew darwin by 1.8 MB again (fat LTO
  with mixed levels, as C2b found), so only the whole build moves. Cost measured on darwin: a warm `doze status` on the
  acme fixture, median of 60, 31.55/30.70 ms at "s" and 31.80/31.24 ms at "z" (both runs include the timer's own process
  start), so about 0.4 ms; the wake bench below is on the "z" build. C2b's reason not to take it (no loop vectorisation
  for the log store and the front) is the price for staying under the line; the splice is tokio's copy either way.
- Headroom: about 250 KB under the 9 MB line on Linux, before CI confirms it.
- Wake, on the "z" build (darwin/arm64, a scratch bench after Go's `test/bench`: boot then down five times over the
  control socket, the journal's `ready` times; postgres 16 and valkey 8 from a local signed registry): names off, db
  median 35.0 ms (32.0–38.8), cache 18.8 ms (17.2–22.6), a valkey PING through the front after a sleep 19.3 ms to its
  PONG; names on (Shared, a private zone): 36.8 ms, 19.5 ms, 20.7 ms. Go's budget (`budget.json`) is 250 ms.

## Headroom for doze (2026-11-26, branch `doze-trim`)

Main had reached 8,993,656 B on CI's x86_64 Linux (232 packages), 6 KB under the 9 MB line, and the next pull requests
were over it. x86_64 Linux here is the zig cross build (main measured 9,022,944 B against CI's 8,993,656 B, +0.3%),
`release-doze`; darwin/arm64 is native. Each row is measured on the row above:

| | x86_64 Linux | darwin/arm64 | packages |
|---|---|---|---|
| main (4ff67532) | 9,022,944 B | 6,181,536 B | 234 (232 on CI) |
| doze-hcl, doze-config and hcl-edit at "z" | 8,822,832 B (−200,112) | 5,853,712 B (−327,824) | |
| serde_json reads bytes only (`from_slice`, never `from_str`) | 8,723,152 B (−99,680) | 5,787,200 B (−66,512) | |
| the wall clock without jiff's `strftime` | 8,599,744 B (−123,408) | 5,687,568 B (−99,632) | |
| the public suffix list decoded per top-level label | **8,607,376 B** (+7,632) | **5,704,112 B** (+16,544) | 234 |

- **The profile.** A custom profile inherits its parent's package overrides: `release` puts doze-hcl, doze-config and
  hcl-edit at "s" (core C2a), so D2's "the whole build at z" left those three at "s". Overriding them in `release-doze`
  makes it whole. D2's warning (mixed levels grew darwin by 1.8 MB) was about "z" on a few crates over an "s" build;
  this is the other way round, and both platforms shrink.
- **serde_json.** Each reader type is a separate copy of every `Deserialize` it reaches: `from_str` and `from_slice`
  of `InstanceView` and the rest of the control types were both in the binary (StrRead about 99 KB of `.text`). Nine
  call sites (doze-cli lint, doze-fetch's child, doze-daemon ctl and topics, doze-journal and doze-projects gojson,
  doze-names routes and ca, doze-module-proto snapshot) now pass `s.as_bytes()`; the input is already UTF-8, so the
  values and the errors are the same.
- **The wall clock.** `clock::wall` (log lines, `why`) formatted `%H:%M:%S` through jiff's `strftime`, which brought
  jiff's whole strtime formatter and its errors. It now adds the zone's offset to the floored second and writes the
  three fields; a unit test holds it to `strftime` over 1900–2100, negative fractions, quarter-hour offsets and both
  DST edges. The zone itself (TZ, IANA, POSIX rules, the system's) is still jiff's.
- **The public suffix list.** doze-config at "z" made a warm `doze status` on the acme fixture (one app domain) about
  2 ms slower: decoding the whole front-coded list into a 10,000-node `BTreeMap` was most of the command (about 5 ms
  of 9 at "s"; 3.8 ms without the domain). The list is now indexed by top-level label in one pass and only the
  label a name ends in is decoded, on first use, which is the same table for that name (only rules that start with
  the label touch its nodes; a unit test compares every node against the whole decode, and tests/psl.rs still
  matches `psl` on every rule). Warm `doze status`, darwin, interleaved, median of 100 on a loaded machine: acme
  10.50/11.14 ms on main and 8.25/8.78 ms on this branch; without the domain 6.25/6.31 and 6.29/6.50 ms.
- **Headroom**: CI's figure should be about 8,579,000 B, 420 KB under the 9 MB line. The line is unchanged.
- **Considered, not taken:**
  - `.eh_frame` and `.eh_frame_hdr` are 1.06 MB of the Linux file and `.rela.dyn` 404 KB. Dropping them needs
    `-C force-unwind-tables=no` or a static relocation model in RUSTFLAGS, which CI's and the release workflow's own
    `RUSTFLAGS` replace; linking with `-z pack-relative-relocs` needs glibc 2.36 and the release builds on ubuntu-22.04
    (2.35); `-no-pie` gives up ASLR for a program that runs a TLS front door. Left.
  - ed25519-dalek to ring's Ed25519 (ring is already linked by rustls): curve25519-dalek and SHA-512 are about 50 KB of
    `.text`, but `verify_strict`'s weak-key and small-order checks would have to be carried by hand, and
    `VerifyingKey` is in doze-fetch's API. Not worth the risk for this size.
  - The RustCrypto stack under the CA (p256, ecdsa, x509-cert, der, spki: about 280 KB of `.text` by symbol) next to
    ring: replacing it means owning the certificate's DER and the key files' formats. The largest remaining lever,
    for a separate change with its own audit.
  - jiff for the zone (about 90 KB with jiff-core before this change, less now): the POSIX rules and TZif reading are
    Go's `time.Local` behaviour and fuzzed; not replaced.

## Core D1 with the module catalog (`doze` carries the daemon), measured 2026-11-15, release, darwin/arm64

| | bytes | packages (normal, unique) |
|---|---|---|
| D1 `__daemon` and `ctl` (b51674d3) | 6,219,760 | 168 |
| + E2's CI fixes, the module catalog, the front, pins in doze.lock, the reaper (96c9137b) | 6,832,160 | 168 |

+612 KB, no new package: doze-module-host and doze-module-proto were already linked through the engine; the cost is the
catalog's and the front's code, F1's module path now reached from the daemon, and E2's merge. Under the 8.5 MB line
(the owner's ceiling for `doze` is 9 MB). Measured with `--release`: the `release-doze` profile is on #53/#56, not on
this branch; Linux with it is measured once D1 and C2c meet (CI's weight job measures x86_64 on each push).
## The app's ceiling raised to 20 MB (2026-11-15, branches `core-c2b`, `core-c2c`)

The owner set the doze app's hard ceiling to **20 MB** on 2026-11-15: `doze-app` carries the GUI, the full CLI and the
daemon, so the old 16 MB line (set when the app was the GUI alone) no longer fits what it is. CI measured doze-app at
17,064,576 B and 662 packages at #53, which failed the 16 MB / 650 line. The CI lines are now:

| | Ceiling | CI line | Measured (#53) |
|---|---|---|---|
| `doze-app`, darwin/arm64, `release-app` | 20 MB | 17,918,000 B (measured + 5%) | 17,064,576 B |
| `doze-app` packages (CI's count) | | 700 | 662 |

Branch `app-size-profile` (#55, merged into `app-finish`) separately brings the app down to about 14.5 MB with a size
profile; when that meets these branches the byte line tightens again to its measured + 5%.
## The lean line after the efficiency pass (2026-11-15)

Main's runs on 3636c3d2 (43665814939) and 1f182947 (43696911937) measured the lean build (`weight-lean`) at **5,450,072 B**, over the exact
5,274,120 B line: +175,952 B (+3.3%), packages unchanged (125), idle 6,904 and 7,008 KB (inside 7,229). The kafka session's 6985735d moved
the default build's line for the same work and left the lean line where it was.

Bisected by cross-building the lean binary for x86_64 Linux on darwin (zig as the linker; 5,300,368 B where CI has 5,274,120, so the
steps, not the absolute number, are what count) over the first-parent merges of the kafka session:

| commit | what | x86_64 lean | step |
|---|---|---|---|
| 33ed4871 | before the pass (same as 987233f8, the aws side CI measured at 5,274,120) | 5,300,368 B | |
| 086e4b28 | `fix-correct` merged | 5,326,856 B | +26,488 |
| 0ee5cf74 | `fix-idle` merged | 5,407,360 B | +80,504 |
| 07c07a88 | `fix-idiom` merged | 5,476,976 B | +69,616 |
| 3636c3d2 | the rest (interop docs, Metadata v12 by id) | 5,477,440 B | +464 |

Darwin arm64 shows the same rise (3,950,064 to 4,082,544 B, +3.4%). By symbol (unstripped x86_64 builds, `llvm-nm --size-sort`), +156,842 B
of code: `Response::compute_size` and the per-message `compute_size` methods (+37 KB and about +10 KB more; 445920b2 sizes a response's
buffer up front instead of growing it by doubling), the idle chores' own tasks (the partition closer, the log sweeper, the transaction alarm,
the offsets sweep on the blocking pool, the frame shelf's trim: about +35 KB of task and future code, fb8791ad, fea62b9f, 8fbc537c) and
the group registry as a plain table (0d432949: `Groups::put` +12 KB against `Groups::ask` -15 KB). Every item is a deliberate change
with its reason in the kafka docs; no new package and nothing unexpected, so this is a small, explained rise and the line follows it:
**5,450,072 B**, exact, as before.

## Data control reaches main (2026-11-15)

The aws session's data-control merge (78ba6190, pushed to main) moved two lines in run 43703449990:

- **doze-aws idle: 10,548 KB** (samples 10,492, 10,692, 10,548) over the 10,526 KB line; the six runs before had medians of 9,820 to 10,140 KB.
  Bytes rose 511,112 B to 13,669,488 B (inside 13,900,000), packages 274. An A/B on aarch64 Linux (binaries cross-built on darwin with zig,
  run in the Lima VM, 7 interleaved samples each): 8,184 KB before (727d8c3f), 8,596 KB after (dae5512c), 8,384 KB after with the start-time
  size check left out. So about 210 KB is `mention_size` (run.rs, 9346ebbc), which walks the data directory on a one-shot OS thread: glibc keeps
  the thread's arena and cached stack after it ends. The other 200 KB comes with the new code (the binary is 346 KB larger on aarch64). The line
  is the median plus 5%, **11,075 KB**. The thread's share is avoidable (the walk could run before the runtime's threads start, or on a thread
  that already exists) and is listed for the aws session in ci-health.md.
- **weight-lean: 5,450,504 B** (+432 B; +368 B on the x86_64 cross-build). The only lean-tree crates the merge touched are doze-store and
  doze-runtime; `Store` now keeps its file's path (d39dd0cd), and kafka-meta opens a `Store`. Kafka never calls `Db::close`, so the closing
  compaction (58834909) does not run in the broker. The line follows: **5,450,504 B**.

## The log filter without regex, and Fetch records written without a copy (2026-11-15), release, darwin/arm64

Two efficiency changes the owner approved. Each measured on its own tree, `cargo build --release -p doze-kafka`, packages counted as `cargo xtask weight` counts them:

| tree | binary | packages |
|---|---|---|
| main (3636c3d2) | 8,001,408 B | 184 |
| Fetch records written without a copy | 8,001,440 B (+32) | 184 |
| and the log filter without regex | **7,784,992 B** (-216,448) | **180** |

`cargo xtask weight doze-kafka` on the last: 7,784,992 B, 180 packages, idle 7,584 KB (samples 7,584, 7,552, 7,664). The limit moves down
(`xtask/weight-limits.txt`): times CI's 1.3747 is 10,702,029 B, plus 5% = 11,237,130 B; packages 180.

**The log filter** (`crates/doze-kafka/src/logging.rs`). `tracing_subscriber::filter::Targets` instead of `EnvFilter`, so the `env-filter`
feature is off and `regex`, `regex-automata`, `regex-syntax` and `matchers` leave doze-kafka's tree (`cargo tree -p doze-kafka -e normal -i regex`
finds no such package). `RUST_LOG` and `--log-level` take what they took before in practice: a level, or a comma-separated list of levels and
`target=level` pairs (a bare target is every level of it). Span and field directives (`target[span{field=value}]=level`) are refused at start,
exit 2, with one line naming the source and what is supported; a misspelt level is refused too (`EnvFilter` dropped it silently), and an empty
`RUST_LOG` counts as unset. `regex-lite` stays (kafka-broker's own use). doze-aws keeps `EnvFilter` in its own `run.rs`: its setup is not shared.

**Fetch records** (`crates/kafka-wire/src/response/fetch.rs`, `frame.rs`). A Fetch reply was encoded into one buffer, so every record byte read
from the log was copied into it before the write. Now the envelope is written into one buffer and each partition's records (4 KiB and up) stay
in the `Bytes` the log read them into; the server writes the pieces with one vectored write (TCP and rustls both gather) and flushes. Size:
+32 B. Memory: `examples/fetch_frame` (Fetch v12, median of 9, loopback socket drained by a thread):

| response | allocated by the encode, before | after | encode + write, before | after |
|---|---|---|---|---|
| 1 MiB, 1 partition | 1,048,642 B | 234 B | 0.292 ms | 0.269 ms |
| 50 MiB, 1 partition | 52,428,867 B | 235 B | 5.849 ms | 4.932 ms |
| 50 MiB, 16 partitions | 52,429,467 B | 2,627 B | 5.705 ms | 4.906 ms |

End to end (the release binaries of main and of the Fetch change, `/usr/bin/time -l`, kcat producing one or fifty 1 MB messages to one
partition and consuming them with the fetch limits at 64 MiB, three runs each): one message, peak RSS 10.6 MB to 9.5 MB; fifty, peak RSS
108.6 MB to 58.6 MB (peak footprint 104.2 to 54.1 MB) and the consume 184.8 to 188.3 ms to 169.5 to 175.6 ms (kcat's start included).

## Share groups (KIP-932, 2026-11-15), release, darwin/arm64

Measured back to back with `cargo xtask weight doze-kafka`: the tree before share groups (kafka-next with KIP-890, c64f4b76e) and the tree
with them (docs/port/behaviours/share-groups.md).

| | bytes | packages | idle (median of three) |
|---|---|---|---|
| before | 7,984,176 B | 182 | 7,840 KB (7,856, 7,824, 7,840) |
| with share groups | 8,431,392 B (+447,216 B, +5.6%) | 182 | 7,968 KB (7,968, 7,968, 7,984), +128 KB |

No package is added. The bytes are code: the seven APIs' messages, whose generated codecs are linked now that something uses them; their
handlers; the share-partition (acquisition, acknowledgement, locks, the start), the share sessions, the lock task and the group configs; the
recorder's summaries and the console's share views. The embedded console assets grew 10 KB. At rest a broker holds an empty map, an empty
session cache and one task waiting on an alarm no one has set: nothing wakes until a lock is held (`tests/idle_upkeep.rs`); the idle rise is
presumably the larger binary's pages (not split into file-backed and anonymous in the VM, as the idle line above was). The line (xtask/weight-limits.txt) moves to 8,431,392 B times CI's 1.3747 plus 5%, 12,170,166 B, under the 15 MB
ceiling, and its idle to CI's 11,132 KB times the measured rise plus 5%, 11,880 KB, until CI measures the tree.

## The broker's debt cleared (2026-11-18, branch kafka-debt-broker)
darwin arm64, release, default features: **9,110,048 B** (+16,560 B, +0.18% over streams groups' 9,093,488 B), **182 packages** (none added).
The bytes are SaslHandshake v0 and the raw-token path (SEC-401), the v0 codecs vendor patch 5 restores (ACLs, DescribeLogDirs,
OffsetForLeaderEpoch), the PREFIXED index (SEC-400), the authorizer's three broker configs (SEC-402) and the long CLUSTER_AUTHORIZATION_FAILED
text (SEC-404). The size line moves to 9,110,048 x 1.3747 + 5% = 13,149,762 B until CI measures it (ceiling 15 MB); idle is unchanged (nothing new
runs while idle; three starts read 4,848, 6,912 and 4,768 KB with a 6 s settle). `cargo xtask weight` now looks for the binary under
`CARGO_TARGET_DIR` when it is set.

## Core M1: the mariadb module, measured 2026-11-14, release, darwin/arm64

`doze-module-mariadb` 905,968 B, beside `doze-module-valkey` 888,128 B and `doze-module-temporal` 903,824 B built the
same way: no new dependency (the module runs the engine's own `mariadb` client, D6). The line for a module of this
kind is 1 MB until the modules plan sets per-module lines.

## One HTTP stack: the app makes its requests through the system's curl (core step 0, measured 2026-11-14)

Same machine and method (darwin/arm64; `release-app` for the app, `release` for `doze`). Before, the app carried two HTTP
paths: the CLI's registry client in the binary (ureq over rustls with ring and the system's trust store, linked when the
CLI joined the app) and the update check's system `curl` (Settings › About, Q6). Now there is one, `doze_cli::http::get`,
built one of two ways by the `doze-cli` feature `tls`:

| Binary | Client | Before (core-m1, b2c73a09) | After |
| `doze-app` | the system's `curl` (no `tls`): the registry and the update check | 15,654,736 B, 640 packages | **14,792,432 B (−862,304 B), 634 packages** |
| `doze` | ureq + rustls in the process (`tls`, the default) | 4,608,720 B | 4,608,720 B (unchanged) |

The 0.84 MB is what the HTTPS client cost the app: ureq, its HTTP parser, rustls's client side, ring, webpki and the
platform roots (a curl build of the same code measured 14,792,432 B too). `doze` keeps the in-process client because it
must work where nothing is installed beside it (a container, a CI image without curl); the app runs on a desktop whose
system ships curl (macOS /usr/bin/curl, Windows 10 and later in System32). Both clients say the same words for the same
failure (`http.rs`; both are tested against a local server in every build). **The app's room under its 16,000,000 B
line is now 1,207,568 B** (was 345,264 B).

**What else `cargo tree -d` shows for the app**, and why it stays:
- `objc2` 0.5 and 0.6, `png` 0.17 and 0.18, `itertools` ×3, `getrandom` ×3, `base64` 0.22 and 0.23, `dirs` 5 and 6,
  `hashbrown` ×2, `miniz_oxide` ×2: all inside GPUI and gpui-component's own trees; none can go without patching them.
- `image` with every format (exr, tiff, webp, jpeg, gif, …: about 0.55 MB with `image` itself): gpui-component turns on
  gpui-pre's default features, which turn on the formats; the app's own `default-features = false` cannot undo it. The
  lever is a patched gpui-component (vendor it, as gpui-pre-apple is), for the app's owner to weigh.
- `syn` 2 and 3, `toml_edit`, `winnow` 0.7 and 1.0: build-time only, no bytes in the binary.
- `jiff` (the CLI's wall clock, about 120 KB with `jiff-core`) beside `chrono` (GPUI's): the CLI could use chrono and
  save that in the app; not done here, the CLI's clock being the CLI's choice.

**The profile lever, measured, not taken:** the same app at `opt-level = "z"` (fat LTO, as `release-aws`) is
**10,915,200 B, 30% smaller** than at `"s"`. doze-aws measured 30 to 60% more CPU per request at `"z"`; for the app that
means frame and layout time, which this step did not measure. It is the lever that makes room for the engine, the daemon
and the config when they join the CLI (the CLI alone is 4.6 MB at `"s"`), and it is the app owner's call, with the
frame-time numbers of `doze-app bench` beside it.

## Core M1: the ferret module, and the OP_MSG client (D6), measured 2026-11-14, release, darwin/arm64

**D6, the client converge talks to FerretDB with.** Three small programs, each sending one command over a unix socket
and reading the reply (opt "s", fat LTO, stripped, current-thread tokio and serde_json in all three, as a module has
them): a hand-written BSON document and OP_MSG frame, 353,840 B and 12 packages; the same frame with the `bson` crate
(2.x) for the documents and the JSON seed conversion, 801,120 B (+447 KB) and 54 packages; the official `mongodb` driver
(3.x, rustls, its smallest feature set), 2,790,000 B (+2.4 MB) and 144 packages. The module writes its own: BSON is
seven element types to write and a length rule to step over the rest, about 400 lines with tests, fuzzed
(`fuzz_bson`, `fuzz_reply`). The plan named the `bson` crate; the measurement made it a module's half again for no
behaviour the module uses.

`doze-module-ferret` **1,006,976 B**, 49 packages (mariadb 922,288 B and 44 on the same build): +85 KB over mariadb for
the BSON, the OP_MSG client and serde_json, which reads the seed files. Under the 1 MiB (1,048,576 B) line a module of
this kind has until the modules plan sets per-module lines.

### app-finish (full parity, privileged actions), measured 2026-11-14

Same machine and method (`release-app`; `doze-app bench --fixture larder`, `footprint` of that pid; menu bar only is
`doze-app --menu-bar` on an empty throwaway `DOZE_HOME`). The base is `app-try` at a112aa29, built the same way.

| | Budget | app-try (a112aa29) | app-finish |
|---|---|---|---|
| Binary, `release-app`, no capture | ≤ 16 MB | — | 13,447,312 B at the end of the branch (13,430,736 B before the audit's fixes) |
| Binary with `fixtures` | | 13,530,368 B | 13,712,704 B (+182 KB: the run sheet, the command source and plan, the streaming hands, the parity rows) |
| Packages (`cargo tree -p doze-app --edges normal --prefix none`, unique) | limit 560 | 407 | 407 (none added: `objc2-user-notifications` and `block2`, now named by `doze-os` for the notification question at start, were in the tree through gpui) |
| Menu bar ready | ≤ 100 ms | 81–145 ms | 77–180 ms (interleaved runs, same spread; the machine was building in another worktree) |
| First frame after the tray click | ≤ 100 ms | 43–68 ms | 40–96 ms |
| Window open and idle (larder's page) | ≤ 85 MB | **224–226 MB** | **225–226 MB** (0.0 % CPU) |
| After the window closes | ≤ 35 MB | — | 38 MB (two runs, 10 s after the close) |
| Menu bar only, footprint | ≤ 18 MB | — | 19 MB, 0.0 % CPU |

**The open window is over its budget, and was before this branch.** Of the 225 MB, 147 MB is one category,
"Owned physical footprint (unmapped) (graphics)" in 30 regions (GPU memory), against 4 MB in the M3 and Map 150
builds; the rest is in line with those (IOSurface 32 MB, Malloc Small 26 MB). The window is static (0.0 % CPU), so it
is a held allocation, not a redraw loop. Bisected on release builds of the merges into `app-preview`:

| build | open and idle | graphics |
|---|---|---|
| db499759 (M2, before the merges) | 57 MB | 4 MB |
| 8e36b742 (+ app-type) | 56 MB | 4 MB |
| 902ddd0d (+ app-states) | 226 MB | 147 MB |
| 59e22ed2 (+ app-map-150) | 56 MB | 4 MB |
| 4792f2ae (+ app-m3-tray), ebe7d11b (Geist), a112aa29 (app-try) | 224–226 MB | 147 MB |

It is not monotonic in the merge order, so it is an interaction (a texture held for something app-states draws, which
the Map-150 merge happened to avoid and the tray merge brought back), not one commit's cost. Not fixed here: it predates
this branch and needs its own look (Metal's allocation trace in Instruments on the larder page). After the close it
drops to 38 MB, 3 MB over that line (35–36 MB at M3).

What this branch adds at rest: nothing that wakes. The run sheet exists only while open; the config look stamps the
known stacks' files every two seconds (a `stat` each) and redraws only on a change; doctor and the leftovers look keep
their old cadence. Notifications ask for leave once at start, from a bundle only.

### The idle window (app-finish, follow-up), measured 2026-11-14

**Root cause of the 225 MB.** The idle stack page drew one frame a second, and while frames keep coming, even one a
second, the GPU driver keeps about 147 MB of "Owned physical footprint (unmapped) (graphics)" for the process (the
redraw floor described above). The frames came from `shell/clock.rs` (added by `app-states`): while a page shows text
that changes with time alone, it is read again every second, and larder has a dozing instance whose "sleeps in 0:48"
changes every second, so every read was a frame. Evidence, same binary (the renderer's frame counter, vendor
PATCHES.md §3, read by `doze-app bench`): on the external display 27 frames in 27 s of idling, 223–225 MB, 147 MB
graphics; with a log line per read, "sleeps in 0:48" → "0:47" → … once a second.

**Why the bisect was not monotonic.** It measured where the window landed, not the code. `bench` centred the window on
the main display; on this Mac the built-in display's copy of it was hidden (the window reported `Hidden` and the page
still changed every second, but no frames were delivered), so builds measured while it landed there read 56–57 MB
(4 MB graphics) and the rest 224–226 MB. Re-run here: the pre-fix build reads 57 MB on display 0 and 225 MB on display 1.
`bench --display N` now chooses the display, so a reading is never taken from a window the system hides.

**The fix.** Time-driven reads need the person (`shell/attention.rs`): the window counts as in use while it is key and
was touched (a key, a click, the pointer, the wheel; becoming key counts) in the last 30 s. The clock, the instance
page's tick and Activity's ten-second read run only then, and the hub streams the meters only while the window is on
screen and in use (`Root::sync_hub`: a real daemon sends meters every second, so a page showing changing figures would
otherwise draw a frame a second too). Left alone, the page still hears every change of state, at most once a second;
a touch reads everything again at once. Also fixed: the instance page's tick started a second loop on each of its own
reads, so loops doubled every second while a countdown showed (4,092 reads in 10 s in the test).

**Measured** (`release-app`, `doze-app bench --fixture larder --display 1 --check --budget 85`, the window on screen;
footprint at 12 s and 27 s after launch, and 42 s, which is about 10 s after the close; two runs):

| | Budget | before (app-finish b55487cf) | after |
|---|---|---|---|
| Window open and idle (larder, on screen) | ≤ 85 MB | 223–225 MB (147 MB graphics), 27 frames in 27 s | **61 / 57 MB** and 61 / 57 MB (4 MB graphics), **0 frames** |
| The same on the main display (where it was hidden) | | 57 MB | 61 / 57 MB, 0 frames |
| Window in use (`--in-use`: counted as touched at open), then left alone | | — | 224 MB while the countdown ticks (28 frames, the last at 31 s), 78 MB 9 s after the last frame, 73 MB from then on |
| After the window closes | ≤ 35 MB | 38 MB | 37–38 MB |
| Menu bar only (empty home, `--menu-bar`) | ≤ 18 MB | 19 MB | 19 MB, 0.0 % CPU |
| CPU, window open and idle | ≤ 0.3 % | 0.0 % | 0.0 % |
| Binary, `release-app`, no capture | ≤ 16 MB | 13,447,312 B | 13,464,000 B (+16,688 B: attention, hands-off, the bench's check) |
| Binary with `fixtures` | | 13,712,704 B | 13,745,968 B |
| Packages (CI's count, `--target aarch64-apple-darwin`) | limit 560 | 538 | 539 (`gpui-pre-apple`, already in the tree, now named directly for its frame count) |

What a window in use costs is the redraw floor, as before: about 225 MB while something on it moves, back to about
75 MB within ten seconds of the last frame. The open window over the 85 MB line is now the window someone is using,
not one left open.

**The guard.** `shell/idle_tests.rs` (gpui test windows over the fixture daemon, the app's time and the hub's clock run
on together): an untouched window draws 0 frames in 30 s while its countdown runs; a window in use draws, then stops
once left alone, and a touch draws again; a pointer over a window that is not key draws nothing; the meters stream
only while in use; the instance page reads once a second, not more. Without the gate the first two fail, and the
instance test reports 4,092 reads. `doze-app bench --check [--budget MB]` exits 1 when a window left alone drew a frame
or passed the budget (checked: an `--in-use` run with `--budget 85` exits 1 at 223 MB); CI's app job runs it on larder.

Not covered: a window left alone while All logs follows still draws (that is following), and an untouched window's
countdown and "last update Ns ago" show the figure of the last batch until a touch.

### A truthful idle window (app-finish, follow-up), measured 2026-11-14

**What was wrong with the fix above.** Stopping every time-driven read on an untouched window kept it at ~57 MB, but a
window someone was *looking at* without touching froze: "sleeps in 0:48" stayed 0:48, the meters stopped, and a
daemon that froze never got its "last update Ns ago" banner (the page asked for no meters, so it was not even
metered). That is the "untrue dashboard" the banner exists to prevent.

**The rule now** (`shell/attention.rs` `Mode`, `shell/clock.rs`):

| The window is | Words that move with time | Changes of state | Meters | Frames of its own |
|---|---|---|---|---|
| hidden (minimised, covered, another Space, display asleep: the platform's occlusion state) | not read | kept, taken in when shown | not streamed | none |
| on screen, not in use ("glanced at") | to the minute: "sleeps in 4m", "<1m", "up <1m", "last update <1m ago", "next in <1m" | at most once a second | streamed, taken in at most every 10 s | one when a word to the minute changes |
| in use (key, touched in the last 30 s) | to the second, as before | as they come | as they come | as before |

- The words come from doze-display (`doze_display::coarse`, `grain.rs`): fine is Go's and the default everywhere, so
  the CLI and the oracle are unchanged; coarse uses Go's `Ago` units (`4m`, `3h`) and `<1m` for the last minute.
  Every coarse word also notes when it will next read differently, and `coarse` returns the soonest; the window
  schedules one wake for that moment (`arm_next`) instead of ticking. A wake whose words read the same draws nothing
  (the views are `PartialEq`). Needs you and Activity build their words off the UI thread, so they carry the grain
  into the job; the instance page's countdown ring moves when its words do.
- The stale banner: one wake at the shown stack's `last_frame + STALE_AFTER`, put off by every frame; a frozen
  daemon's banner shows in one frame. Entrances (the banner's 240 ms settle) are skipped on a window glanced at, so
  each change is one frame.
- doze-desk: a meters frame now reports `Parts::STATS` only (it reported `INSTANCES` too for each row's ram/cpu/
  connections), so a window can tell the meters moving from a change of state.

**Tests** (`shell/idle_tests.rs`, gpui test windows over the fixture daemon; the daemon's meters arrive every second of
the test's time, `Source::beat`):

- glanced at for 180 s on larder: 4 frames, each one a change of what the page says (at 48.0 s the dozing countdown
  ran out; at 57.7, 117.8 and 177.5 s the rows' "up 18m" turned 19m, 20m, 21m); no frame with nothing changed;
- hidden for 150 s: 0 frames and the page not read; shown again, it catches up at once;
- larder-stale glanced at: the banner "last update <1m ago" in exactly 1 frame, about 3 s after the last frame, then
  "1m ago" at the minute;
- in use: the countdown to the second (a frame a second); left alone, it goes to the minute in one frame, then
  nothing while the words hold;
- `grain.rs`: the coarse words and their next change; fine words are Go's (`last update 5s ago`, `0:48`, `12s`).

`bench --check` now allows a window glanced at `--per-minute N` frames a minute (default 2), and a `--hidden` one
(the app hides once the window is open, as ⌘H) none; CI runs both on larder. `--at 12,30,60` prints the footprint at
those seconds after the window opens.

**Measured** (`release-app` with `fixtures`, `doze-app bench --fixture larder --display 1 --idle 63 --at 12,30,60`).
This Mac's displays were asleep and the session locked while it ran (`pmset -g log`: display off since 18:38), and the
platform reported the window `Hidden` on both displays, so every reading below is a hidden window's:

| | before (1e486a31) | after |
|---|---|---|
| Hidden, footprint at 12 / 30 / 60 s | 60 / 57 / 57 MB | 57 / 57 / 57 MB |
| Hidden, frames in 60 s | 0 | 0 (`--check --hidden`: ok) |

**Not measured yet, for the owner, at the screen:** the visible readings. Untouched for 60 s, expect at most about two
frames in 60 s and ~57–60 MB most of the time, with a spike towards ~200 MB for ~10 s after each frame; in use, the
redraw floor (~225 MB) as before:

```
DOZE_HOME=$(mktemp -d) doze-app bench --fixture larder --display 1 --idle 63 --at 12,30,60 --check --budget 85
DOZE_HOME=$(mktemp -d) doze-app bench --fixture larder --display 1 --idle 63 --at 12,30,60 --in-use
```

**Open: the meters' 10 s.** The fixture sends the same meters frame every second, so the page never changes for
them here. A real daemon's meters do change: a glanced-at stack page then takes them in every 10 s, and if a figure
it shows moves (a row's MB, the vitals), that is a frame every 10 s, the same span as the driver's ~10 s hold, so the
~147 MB would stay. The `--per-minute 2` check would fail for such a page. Options: a longer cadence (30–60 s) or
meters only in use. It needs a decision and a measurement against a live stack.

## The app's size profile: small core, fast GUI (measured 2026-11-15)

The app carries the whole CLI (owner decision) and, with core C2a, doze-config and the HCL crates. Measured on a branch
that merges PR #49 (`app-finish-http`), PR #51 (`core-c2a`) and core step 0 (47276b53, "One HTTP stack", from
core-m1: #49 is only its app half), the `release-app` build with everything at `opt-level = "s"` was **16,102,720 B and
650 packages**, over its 16,000,000 B line. The engine and the daemon have still to join.

**Method.** darwin/arm64 (M5 Max), each option a cargo profile inheriting `release-app` with per-package
`opt-level` overrides (fat LTO, one codegen unit). Size: the plain build, as CI builds it. Timings: the same profile with
`capture`, `doze-app bench` on the external display (`--display 1`; the built-in display hides the window and sends it no
frames): `--fixture larder` (menu bar ready, first frame), `--fixture medium-150 --map` (Map frames drawn in full
offscreen, layout), `--fixture shop --logs` (100k lines: fill, scroll on screen, each frame drawn offscreen). The CLI:
`doze-app status` against `doze-app fixture-serve larder` (a real round trip on the control socket) and `doze-app
version`, 40 runs after 3 warm-ups, median. Options interleaved, rounds in both orders; medians of 3 to 8 rounds. The
machine was shared with other builds (load average 14 to 21), so differences under about 5% are noise. The package
count (650) does not depend on the profile.

| Option | What is at which level | Binary | Map layout | Map pan, median / p99 | Logs: fill 100k; draw, median | First frame | `status` / `version` |
|---|---|---|---|---|---|---|---|
| A (before) | everything "s" | 16,102,720 B | 0.87 ms | 10.5 / 11.4 ms | 389 ms; 7.3 ms | 82 ms | 9.62 / 8.96 ms |
| B | everything "z" | 12,224,112 B | 1.31 ms | 10.6 / 11.3 ms | **990 ms**; 6.8 ms | 74 ms | 9.79 / 9.00 ms |
| C | GUI 3, the core "z" | 17,963,600 B | 1.13 ms | 9.5 / 10.5 ms | 357 ms; 6.8 ms | 73 ms | 9.19 / 8.33 ms |
| D | GUI "s", the core "z" | 15,074,752 B | 1.13 ms | 9.8 / 10.6 ms | 398 ms; 6.8 ms | 69 ms | 9.00 / 8.40 ms |
| E | base "z", the GUI's frame path "s" | 20,966,320 B | | | | | |
| F | base "z", the GUI's frame path 3 | 22,955,248 B | | | | | |
| G | D, and the GUI tree's cold parsers "z" | 14,880,272 B | 1.12 ms | 9.8 / 10.5 ms | 398 ms; 7.1 ms | 74 ms | 9.76 / 8.96 ms |
| H | D, and the GUI's frame path 3 | 22,865,632 B | | | | | |
| **I (taken)** | G, with doze-desk and doze-map back at "s" | **15,240,704 B** | **0.82 ms** | 10.0 / 10.7 ms | 394 ms; 7.2 ms | 76 ms | 9.86 / 9.18 ms |

"The core" is the 124 packages the app links only for the CLI and what it runs (the doze-* core crates, clap, the HCL
and YAML readers, jiff and time, tokio, hyper, rustls, ring, the CA's and netd's crates), serde_json and the regex
crates. "The GUI's frame path" is gpui and its platform crates, gpui-component, the app, taffy, font-kit, core-text,
lyon, etagere, ropey and the text-segmentation crates. "The GUI tree's cold parsers" are what GPUI links but reads once:
the image decoders, markdown and HTML, TOML, the i18n tables, file watching. Logs scroll on screen at the display's
pace in every option (33.3 ms a frame, the external display at 30 Hz), so the offscreen draw time is the figure that
compares.

What the numbers say:
- **"z" everywhere (B) costs the GUI**: the 100k-line fill takes 2.5 times as long and the Map's layout 50% longer.
- **GPUI at opt-level 3 does not fit**: C is 1.9 MB over the line. H and F, which raise only the frame path to 3, and E, which
  lowers everything else to "z", are bigger still (20.9 to 23.0 MB). Mixing opt levels across GPUI's own crates
  inflates the binary under fat LTO. The mechanism was not chased; the measurement rules those options out.
- **The core at "z" costs the GUI nothing**, except that doze-desk and doze-map are the app's own crates on the
  page's path: at "z" (D, G) the Map's layout is 30% slower (0.87 to 1.13 ms). Kept at "s" (I), it is back (0.82 ms).
- **The CLI through the app is within noise**: `status` +2.5% and `version` +2.5% for I against A. A command's time is
  mostly the process starting (dyld and the GUI frameworks), not the CLI's code.

**Taken: I**, 15,240,704 B (−862,016 B, −5.4%), 650 packages; `Cargo.toml`'s `release-app` lists the packages, grouped.
No measurable GUI regression; the CLI under 10%.

**The engine and the daemon.** A probe built from core-d1 as it stands (doze-engine and doze-daemon, serving the control
socket, over a base of tokio and serde_json; fat LTO) adds 619,600 B at "z", 748,912 B at "s" and 1,062,336 B at 3.
At "z", as the core is here, that projects the app to about **15.86 MB**, under 16 MB with about 140 KB of room. Both
crates are still in progress (D1 and E2 are open PRs), so this is a floor. If they grow by more than about a fifth, the
app no longer fits 16 MB. The levers left are a patched gpui-component without the image formats (about 0.55 MB,
above) and the CLI's clock on chrono instead of jiff (about 120 KB). After that, the 16 MB ceiling itself is an owner
question.

**The CI line** first moved to 15,500,000 B, this build plus about 260 KB. It now follows the 20 MB ceiling below.

**The image formats (vendor/gpui-pre/PATCHES.md).** gpui-pre, gpui-pre-macos and gpui-pre-apple each hard-code
`image` with every format it decodes. The app decodes none of them: its tray PNGs go through AppKit, and everything
else is SVG through resvg. All three are now vendored with `features = ["png"]`, and GIF and WebP decoding return an
error. Result: **14,526,560 B (−730,768 B), 634 packages** (was 650). The shots of library, tiles, map, palette, about
and instance (the bat mark and the icons) were compared with the build before the patch and draw the same.

**The ceiling is now 20 MB (owner decision, 2026-11-15).** The app is one binary with the GUI, the whole CLI and,
soon, the daemon, so the 16 MB budget, set for a GUI alone, no longer describes it. The CI line follows the rule the
other binaries use: **the measured size plus 5%, 15,252,888 B**, and the measured package count, 634. With the engine
and the daemon as they stand (+0.62 MB at "z") the app projects to about 15.15 MB, 4.85 MB under the ceiling.

**opt-level 3, checked again with the room** (same method, the machine quieter; medians of 5 interleaved rounds; P is
the profile above with the image patch):

| | Binary | Map layout | Map pan / zoom / hover / fly, median | First frame | `status` / `version` |
|---|---|---|---|---|---|
| P (taken) | 14,526,560 B | 0.56 ms | 5.1 / 5.4 / 5.5 / 6.1 ms | 45 ms | 7.56 / 7.32 ms |
| J: P with doze-desk and doze-map at 3 | 14,806,928 B | 0.79 ms | 6.8 / 6.7 / 8.0 / 8.1 ms | 59 ms | 10.07 / 8.18 ms |
| K: P with the base at 3 (all of GPUI and the app) | 17,348,784 B | 0.70 ms | 5.2 / 4.9 / 5.5 / 5.2 ms | 50 ms | 9.02 / 8.83 ms |

Neither is a real gain. J is no faster anywhere, and reads slower in every figure on this shared machine (the cause
was not chased; it is not a gain either way). K costs 2.8 MB and moves the Map's frames by under a millisecond either way. So P stays. The
logs bench could not run in this round: the window got no frames from the display, as before `--display` existed, so
the logs figures are the ones above.

`doze-app bench --logs` now takes `--display N` as the open bench does. Without it, on a Mac whose built-in display
hides the window, the scroll step waits for frames that never come.


## The app lands on main (2026-11-19, branch `app-land`), release-app, darwin/arm64

app-finish, its one-HTTP-path follow-up and the size profile above, merged over the Rust core (core-land, PR #84). The
core now carries doze-daemon, doze-detect, doze-engine, doze-fetch and doze-module-host into the CLI the app links; by
the profile's own rule (built for size what the app carries but does not draw) they join the "z" list.

| | Line | core-land alone (weight.md, "The app's ceiling raised to 20 MB") | app-land, profile as merged | app-land, new core crates at "z" |
|---|---|---|---|---|
| `doze-app` bytes | 17,918,000 → 16,563,000 | 17,064,576 | 15,922,064 | 15,774,480 |
| `doze-app` packages (CI's count) | 700 → 660 | | 656 | 656 |

The CI line is the measured size plus 5% (16,563,000 B) and the package count plus four, under the owner's 20 MB ceiling.

## The app runs its stacks' daemons (2026-11-19, branch `app-next`), release-app, darwin/arm64

The CLI the app carries starts a stack's daemon as the running program (`doze-app __daemon`), but the app's `main` did
not answer to `__daemon`, so the app's Up started a daemon that exited at once ("unknown flag --config"). The app now
runs the daemon's entry as the `doze` binary does, which links the daemon's serving half (its control server, the
engine's run loop) that the CLI alone did not reach. The packages were already in the tree through the CLI (one more
edge: doze-app → doze-daemon).

| | Line | app-next before (Sleep all, in-place busy state) | with the daemon's entry |
|---|---|---|---|
| `doze-app` bytes | 16,563,000 → 17,491,000 | 15,824,208 | 16,657,840 (+833,632, +5.3%) |
| `doze-app` packages (CI's count) | 660 | 656 | 657 |

A rise with its reason: without it the app cannot start a stack at all. The line is the new measure plus 5%; the
owner's ceiling stays 20 MB.

## A live stack's idle window (2026-11-21, branch `app-design`), release-app, darwin/arm64

**What the owner saw.** The window open and untouched on a real running stack (shop, 13 services, a live daemon):
255 MB footprint, peaks of 401 MB (111 MB IOSurface, ~57 MB driver graphics, ~60 MB heap). The budget above is
85 MB open and idle, 35 MB closed; CI's `bench --fixture larder` passed at 55–77 MB because a still fixture never
moves, and because the bench's window never took the hub's batches at all (`bench` pumped them to the menu bar only),
so no bench had ever measured a window following a live daemon.

**Method.** A real daemon, as the owner's rule asks: a throwaway `DOZE_HOME` under /tmp, the built `doze-app` as CLI
and daemon (as `tests/real_daemon.rs`), and a 14-service stack (nine bun HTTP servers, two `nc` listeners, two bun
workers logging every 3 s, a bun driver hitting four of the servers every 0.7 s; seven dozing ones fell asleep after
their two minutes, so the stack had both kinds). `doze-app bench` now opens the window on a page (`--screen
home|tiles|list|service|logs|activity`, `--select NAME`), takes the hub's batches as a launch does (`app::take`),
floats the window over the others (`--on-top`, `doze_os::ui::float_over`: a covered window is sent no frames, and
the first readings here came from a window the system hid), counts the frames the renderer committed over 120 s after
a 3 s settle, samples `footprint` every 5 s, reads the footprint again once 15 s have passed with no frame
(`--settled`), and after the window closes (`--closed`). The causes were found by tracing every batch, read and
redraw against the frames (a temporary `eprintln` trace, not kept).

**What drew the frames (each with its fix):**

1. *Every instance frame was a change of state.* A real daemon sends its instances again every ~3 s with nothing a
   person would call a change, and a connection flips a service between held (`active`) and dozing (`idle`) every
   second. Glanced at, `shell/clock/glance.rs` took any `INSTANCES`/`CONN`/`LOGS`/`JOURNAL` part in within a second,
   and the read picked up the newest meters with it, so the page redrew every 1–3 s. Now a change of state is decided
   on the rendered meaning, not the raw frame: `glance::signature` hashes each stack's running, link and each
   service's state (held and dozing both count as up), error, restarts and switches; only a different signature is
   taken in at once.
2. *The meters every 10 s* (`METERS_EVERY`), which on a live stack always differ: the weight.md "Open: the meters'
   10 s" above, now measured. Everything that is not a change of state (meters, connections, holders, logs, the
   journal) waits for a grid of 30 s (`glance::SLOT`), and so do the words that move with time:
3. *Each row's "up Nm" turned at its own second* (services start seconds apart), and each turn was a wake and a frame:
   up to a dozen frames in a few seconds once a minute. `arm_next` now wakes at the grid step on or after the first
   word's change, so every word, and the meters, change in one frame.
4. *The root redrew the whole window on every batch on a service page, All logs, Activity and Settings*
   (`Root::apply_now`, `refresh_shown`: an unconditional `cx.notify()`). The root draws the sidebar and its foot
   from the hub itself; it now keeps what it drew (`inside::chrome`) and redraws only when that changed.
5. *Doctor's report every 30 s woke every page* (`facts.rs`: notified even when the report was the same), and the
   Library and the stack page heard the facts themselves with fine words (to the second) on a glanced window. A
   report the same as the last now wakes nothing (unless a repair waits for a fresh one), and the root alone hears the
   facts and reads the page shown in the window's words (`clock::hear_facts`).
6. *Activity's clock ticked every 10 s for as long as the page lived, hidden or not.* It now waits 10 s in use, 60 s
   glanced at, and stops while hidden; the window winds it again when it shows (`ActivityView::wind`).
7. *One change, many frames.* A colour or a knob that changed ran its 120 ms transition (a dozen frames), and
   gpui-component's `AreaChart` (a service page's meters) re-derives its hover every frame: after a relayout under a
   resting pointer it asked for frame after frame, 32 frames in 1.6 s at each grid step on a fresh stack's service
   page (found with `bench --real` and a backtrace on `request_animation_frame`). Glanced at, transitions go
   straight to their end (`motion::still`) and charts neither draw in nor answer the pointer, so a change is one frame.

**What held the memory once the frames stopped.** The service page settled at 98 MB: 16.5 MB of it was the renderer's
window-sized path texture (the meters' curves and the countdown ring are paths), resident for good once any path was
drawn. vendor/gpui-pre-apple PATCHES.md §6: its pages go back (`MTLPurgeableState::Empty`) when the last frame
completes with no newer one encoded, and come back with the next frame. And the closed window's heap is handed back
to the system (`doze_os::ui::give_back_memory`, `malloc_zone_pressure_relief`, 2 s after the close).

**Measured** (release-app; `bench --screen S --idle 123 --closed 15 --on-top --settled 85 --at 5,10,…,120`; the live
stack above; before = app-design 723f5907e with only the bench's harness added; after = this branch):

| Page | Frames in 120 s, before → after | Footprint while idle, before | after (min / median / max of 24 samples) | Settled (15 s after a frame) | Closed after 15 s, before → after |
|---|---|---|---|---|---|
| Home (Library) | 2 → 0 | 76–81 MB | 75 / 75 / 80 | 75 | 36 → 36 |
| Stack page, Tiles | 65 → 0–5 | 79 MB, 225 at a minute's turn | 74 / 74 / 79 | 74 | 38 → 38 |
| Stack page, List | 68 → 0–4 | **224 MB** throughout | 74 / 74 / 78 | 74 | 38 → 38 |
| A service's page (api, its logs following) | 121 → 0–3 | **255–257 MB** throughout | 82 / 83 / 92 | 83 (98 before PATCHES §6) | 40 → 40 |
| All logs | 119 → 4 | **231 MB** throughout | 79 / 79 / 83 | 79 | 41 → 39 |
| Activity | 2 → 2 | 76–77 MB | 75 / 75 / 80 | 75 | 41 → 39 |

The same through CI's check, `bench --real 13` (a fresh stack of sh and `nc`, its minute words turning; release-app, the
window on top; before = the same harness over 723f5907e):

| `--real 13`, idle | Frames, before → after | Footprint idle, before → after | Settled | Closed |
|---|---|---|---|---|
| Tiles, 120 s | 102 → 3 (one per 30 s step) | 227 → 75 MB | 228 → 71 MB | 41 → 36 MB |
| A service page (driver), 60 s | 60 → 1 | 256 → 88–90 MB (the frame's 10 s) | 258 → under 85 | 44 → 39–40 MB |
| Home, 60 s | 3 → 1 | 80 → 80 MB | 76 → 76 MB | 36 → 37 MB |
| Tiles, hidden, 30 s | → 0 | → 58 MB | | → 38 MB |

The ranges in "after" are runs while the stack's minute words were turning (a fresh stack: one frame per 30 s step
at most) and runs once it had settled (no visible change in two minutes, so no frame at all). A frame on this Mac
brings the driver's ~150 MB back for about ten seconds (the 224 MB at 15, 45, 75 and 105 s of one Tiles run landed
on grid steps); with at most two frames a minute the window spends most of each minute under 85 MB, and 15 s after a
frame it is always under it.

Also: menu bar only (the window never opened) 21 MB, 0.0 % CPU, 9 threads, no idle wake-ups (`top`); the glanced
window 0.0–0.2 % CPU, 11 threads, 0 idle wake-ups, and a 5 s `sample` shows every thread blocked (the main thread and
AppKit's event thread in `mach_msg`, the hub's threads on semaphores, its control client in `recvfrom`, the
discovery thread on a timed wait): nothing busy-polls. The owner's running preview read 344 MB (174 MB driver
graphics), 0.0 % CPU, 15 threads, 2 idle wake-ups at the start of this work.

**The closed window (owner decision, 2026-11-21: "does not use more resources than needed").** Closed, the app read
36–40 MB depending on the page last shown, over the 35 MB line (menu bar only, never opened: 21 MB). Two things held
what nobody could see:

- *The hub's log ring* (up to 100,000 lines, ~24 MB, a stack), kept for the life of the app once a page had shown
  logs. Now, when the window has closed (the hub hidden and no page watching logs), the stack's thread lets the ring
  go (`LogRing::release`: the lines and their text back to the allocator; line numbers and added-counts go on, so
  nothing that remembered them misreads). A page that shows logs again subscribes with the daemon's 400-line backlog,
  as a reconnect does; while the window shows, a page that stops watching keeps the ring (it may be gone back to).
  Tests: `rings.rs` (a released ring holds under 4 KB and takes the backlog again, numbered on), `watch.rs` (kept
  while the window shows, let go when it closes, the stack still followed for the menu bar, the backlog once each on
  reopening), and the app's `shell/reopen_tests.rs` (All logs shows the same lines after a close and a reopen).
- *The doze mark, decoded at 512 px* (`img("brand/doze.svg")` in the Library's head and the sidebar): gpui decodes an
  image at the file's own size and keeps the pixels for the life of the app, a 4 MB allocation for a 32 px mark (found
  with `malloc_history` on a closed window). The app now serves the mark at 64 px (`assets::brand`; the file stays
  512 px for the app icon). It also took 3–5 MB off every open window.

Closed footprint, 15 s after the close (`bench --real 13`, release-app), before → after: Tiles 38 → 33 MB, Home
36–37 → 32 MB, a service page 39–40 → 33 MB, All logs 39 → 35 MB. The open window: Tiles 74 → 69 MB, Home 80 → 72
MB, a service page 78 → 73 MB. CI holds the closed line at 35 MB.

**The guard.** CI's `app` job, beside the still-fixture checks it keeps: `bench --real 13` brings up 13 services of
sh and `nc` (listeners that would sleep after an hour, two workers logging every 2 s, a driver connecting every
second) under the release-app binary as CLI and daemon on a /tmp home, and checks, on Tiles for 120 s, a service page
and Home for 60 s, at most two frames a minute, 85 MB settled and 35 MB closed (Tiles), and a hidden window draws
nothing. Locally, `shell/live_tests.rs` (gpui test windows over the fixture daemon sending moving meters every second
and its instances every third): a glanced stack page draws 3–7 frames in 180 s and its vitals still move; a service
page at most 7; a hidden window none. `idle_tests`' frozen-daemon test now allows the banner's "1m ago" the grid
step after the minute.

## Core M3: doze-aws's module face, measured 2026-11-15, release-aws, darwin/arm64

The module face (`crates/doze-aws/src/module`, feature `module`, on by default as kafka's is) adds the module SDK to doze-aws:
`doze-module`, its derive, `doze-module-proto`, `doze-os`, `minicbor` and its derive, `hex`, `rustix`, `errno`, `bitflags` (13
`cargo tree` lines). The template, the checks and the CloudFormation calls are doze-aws's own code (its `cfn` client).

| darwin arm64, `release-aws` | `--no-default-features` | default (module) |
|---|---|---|
| doze-aws binary | 8,375,808 B | **8,592,624 B** (+216,816 B, +2.6%) |
| packages (`cargo tree`, unique lines) | 271 | **284** |
| idle | | unchanged: the module code runs only as `aws-plugin`, never in the server |

On ubuntu x86_64 (CI's measured ratio for doze-aws, 1.54) that is about +334 KB on CI's 13,128,520 B, about 13.46 MB, inside the
13,900,000 B line and the owner's 20 MB ceiling. The package line goes from 280 to **287** (CI's 274 plus the 13), a small
justified rise recorded here; CI's next run on this branch measures it exactly. doze-kafka gains only its `client` declaration
(kcat), a few strings.

### Core M3: CI's measurement (2026-11-15, PR #57, run 43641162106, ubuntu x86_64)

CI measured `doze-aws` with the module face: **13,507,440 B** (+378,920 B over main's 13,128,520 B; the estimate was about
+334 KB), **286** packages (+12), and an idle median of **10,716 KB** (samples 10,716, 10,632 and 11,016). That is over the
10,526 KB line. In the same week main measured 10,104 KB (samples 9,940, 10,176 and 10,104).

The earlier "idle unchanged" was wrong. An A/B on darwin, with five starts each and the same tree with and without the
feature, measured medians of 6,544 KB and 6,656 KB: the module face costs about **+112 KB** idle. Those are the pages of its
code, because the server never calls it. The rest of CI's +612 KB is within the run-to-run spread that issue #36 records:
several hundred KB between runs of one commit.

Those lines were for the branch as it stood on 2026-11-15. Since then main moved doze-aws's lines to the owner's ceilings
(20,000,000 B and 16,384 KB idle, 2026-11-19), so the face fits without a new byte or idle line.

### Core M3 landed on main (2026-11-21)

Re-applied on main, with the take-over of a Go module's data (`module/godata`, both modules). That adds one package to each
binary: `doze-bolt`, a read-only bbolt reader of about 300 lines with no dependencies. The package lines move to **293** for
doze-aws (280 on main, +12 for the module SDK as CI counted on 2026-11-15, +1) and **183** for doze-kafka (+1). The kafka
face's other new code (the importer, `serde` and `serde_json`, already in its tree) runs only in `kafka-plugin`'s provision.

### The finished CLI inside the app (cli-finish, 2026-11-21)

Measured on darwin arm64 (`cargo build --profile release-app -p doze-app`, CI's method) on cli-finish with main at
#112 merged: **16,958,368 B**, **657** packages. Main's line was 16,563,000 B (the app as landed, 2026-11-19); the
rise, about +395 KB, is the CLI's finished commands, which the app carries for its own `doze` (one binary, no Go).
The line moves to the measure plus 5%, **17,806,000 B**, under the owner's 20 MB ceiling; packages stay at most 660.

## The lean idle line and runner noise (2026-11-22)

`weight-lean` failed #110's run 44946494610 at 7,832 KB idle (samples 7,744, 7,900, 7,832) against the 7,749 KB line, with a lean
binary byte for byte the same as green runs that idled at 7,412 KB. Over the 35 CI runs of that binary (7,104,784 B, 121 packages)
the idle medians run from 7,104 to 7,832 KB (median 7,404 KB) and single samples from 7,040 to 7,900 KB: nothing in the code moves,
the runner does (issue #36 saw the same). A line of a measurement plus 5% sits inside that noise and knocks PRs out of the queue.
The line is now the median plus 5% plus the run-to-run spread of the medians: 7,404 x 1.05 + 728 = **8,502 KB**. A real rise still
shows: the next lean idle line moves only with a measured median over several runs, and the spread is added again.

## Offset commits share a flush (2026-11-27)

Each OffsetCommit that changed something used to be its own durable redb commit: one transaction and one flush, one at a
time behind the metadata store's write lock. Now commits that arrive together, from any groups and connections, share one
transaction and one flush (`kafka-meta/src/store/together.rs`): the first caller to find nobody writing writes everything
queued by then, and each caller answers only once that flush is done. A flush that fails fails every commit it held. No
thread is started for it, and a commit that arrives alone is written at once, as before. The OffsetCommit v0 path,
EndTxn's offsets and share-partition states go the same way, and a commit whose offsets are all unchanged still writes
nothing. On the simulated disk, eight groups committing at once make the flushes of one commit
(`together::tests::commits_from_different_groups_that_arrive_together_share_one_flush_*`). Against a release broker on
loopback (darwin/arm64), eight franz-go clients, each in its own group, committed a moving offset (OffsetCommit v8) as fast
as they could for 15 s. Before: 5,758 to 6,115 commits/s over three runs (a first run, cold, made 3,154), and the broker
used about 150 µs of CPU per commit. After: 11,019 to 13,367 commits/s over four runs, about 105 µs of CPU per commit. More
CPU in all (about 20 s against 12 to 14 s in 15 s), because it served twice the commits.

## The window's drawables stop climbing (2026-11-23, branch `eff-app`), release + fixtures, darwin/arm64

A resource audit of the owner's app (open for hours, maximised at 3024 × 1898) found 208 MB, of which 111 MB was five
`CAMetalLayer Display Drawable` IOSurfaces of 22.2 MB each, against the three the layer allows. The cause: each time the
window's `CAMetalDisplayLink` is replaced (covered and shown, minimised, a synchronous draw), the old link was
invalidated while it held the drawable for its next update, and now and then kept it. Fixed in the vendored renderer
(vendor/gpui-pre-apple/PATCHES.md §7). `doze-app bench --churn` now measures it: the window maximised and restored,
dragged through a dozen sizes, minimised and brought back, hidden and shown, then ten quick rounds of all of it, with
the footprint and the drawables `vmmap` lists read after each step (15 s after the last frame).

| `bench --fixture larder --on-top --churn` | before: footprint, drawables (MB held) | after |
|---|---|---|
| fresh window, after drawing | 130 MB, 5 (80); 89–93 MB, 3–4 in three earlier runs | 72 MB, 3 (32) |
| maximised | 136 MB, 3 (67) | 88 MB, 3 (44) |
| restored | 102 MB, 3 (32) | 76 MB, 3 (32) |
| minimised, then brought back | 117 MB, 3 (48) | 92 MB, 3 (48) |
| hidden, then shown | 117 MB, 3 (48) | 92 MB, 3 (48) |
| after ten more rounds | 102 MB, 3 (32) | 76 MB, 3 (32) |
| closed (12 s) | 46 MB, 0 | 40 MB, 0 |

Frames drawn back to back after each step came at the same pace before and after (120 of 120, median 33.3 ms, none
late). `setMaximumDrawableCount(2)` was tried as well and dropped: the count still read three or four, and the
footprint was no lower (fresh 88 MB, shown 129 MB).

The logs bench now leaves the page and closes the window after its follow (`after_logs_left_footprint_mb`,
`after_close_footprint_mb`). Measured: 257 MB peak, 103 MB ten seconds after the follow, 104 MB after leaving the page,
44 MB after closing. A `malloc_zone_pressure_relief` when the logs page goes was tried and not kept: on macOS 27 it
gives back 0 bytes, as does the call `window_gone` already makes (the allocator returns empty pages itself: "Malloc
Small (empty)" held 864 KB dirty of 27.5 MB).

## Workstream 2: runtime, store, timers, gateway (2026-11-27, branch `perf-runtime`), darwin/arm64

The performance review found waste at doze-aws's edges. What changed and what it measured. The machine was building other branches
throughout (load average 25 to 47), so timings are noisy; thread counts are not. Release profile unless said; loopback; 16 clients
from a bun script (`fetch`, a signature-shaped header under `--auth trust`) or 40 `curl --parallel` downloads.

**A flush on macOS is `fsync(2)`.** doze-aws never called `full_flush(false)`, so every group commit, every S3 body and every directory
flush was `F_FULLFSYNC` (3 to 12 ms, spike S6), which also empties the drive's cache for every other program. It now calls it at start,
as doze-kafka does (the reasoning is the owner's call of 2026-11-26 above: a crash of doze-aws or of macOS keeps what was flushed; only a
power cut can lose what the drive had not written; Linux is unchanged).

| 16 clients | `F_FULLFSYNC` | `fsync(2)` |
|---|---|---|
| DynamoDB PutItem (2,000) | 270 to 286 a second | 1,396 to 2,273 a second |
| S3 PutObject, 1 KiB (1,000) | 86 to 88 a second | 440 to 876 a second |

**The blocking pool is 8 threads, kept 2 s** (tokio's default: 512, kept 10 s). A burst of 40 concurrent 8 MiB GetObjects (S3 reads each
piece of a body on a blocking thread) made 34 to 45 threads, which then sat for 10 s. Peak threads by pool size, 4 bursts from bun, two
runs each; and the median of 12 bursts from curl, the sizes taking turns so the machine's load fell on each alike:

| `max_blocking_threads` | peak threads | burst time, median (min to max) |
|---|---|---|
| 512 (before) | 34, 45 | 353 ms (152 to 546) |
| 4 | 7, 7 | 323 ms (203 to 598) |
| 8 (chosen) | 11, 11 | 374 ms (235 to 826) |
| 16 | 19, 19 | 388 ms (182 to 570) |

The transfer time does not depend on the size here (the files are in the page cache; the copy is the work), so the pool is small. 8, not
4: the same pool now opens and closes the services' databases (below), and a restart resumes eight services at once. After a minute the
threads are back to 3 in every case; macOS's footprint after the burst was 6.3 to 6.6 MB in every case too (one earlier run of the old
binary held 45 MB after a burst whose 43 threads all ran; not reproduced). The saving the review counted on is Linux's: glibc keeps an
arena for each thread that ever ran at once (about 210 KB each, the 2026-11-15 measurement above), which 43 threads multiplied.

**One store writer per data directory, not per database.** Every open database had its own `doze-store-writer` thread. With S3, SQS and
DynamoDB in use, doze-aws ran 5 threads; it now runs 3 (one writer). The thread ends when the last database closes, so after the
services are reaped none is left (2 threads before, 1 after, in the after-use run below). Keyed by data directory, not by process:
`aws-cloudformation`'s and `aws-sfn`'s test binaries run dozens of servers at once, and with one thread for all of them each test's
flushes queued behind every other test's (six CloudFormation tests and one Step Functions test missed their timings, every run). DynamoDB
PutItem and S3 PutObject at once (16 clients each, three rounds) ran 1,099 to 1,413 and 379 to 830 a second with one writer against 390
to 1,413 and 216 to 795 with a writer each: within the noise.

**A service opens and closes off the serving thread.** `Service::open` (redb's open and repair, `Records::load` of every SQS message or
Step Functions execution) and `close` (the closing compaction) ran on the one thread that serves every request, at the first call and
after each idle reap. They run on the blocking pool now (`Handle::block_on`, one type-erased hand-off so the 18 services do not each
carry a copy of `block_on`); the actor's calls wait in its inbox. Measured by the test `a_slow_open_does_not_hold_up_another_service`:
another service answered after 2.0 s (the slow open) before, at once after. `Resumable::resume` runs on a task, the eight services at once,
so the ready line no longer waits for their databases. `Db::space` reads the tables' page counts in a read transaction instead of
starting a write, which waited out a commit under way.

**Timers.** `Deadlines` kept the answer of a wake-up that had awaited its save over a deadline a call armed meanwhile (lost or late); every
arm now moves a generation on and a wake-up that finds it moved keeps the earlier deadline. CloudWatch's alarm timer and Step Functions'
clock use the shared `Due` now. No nap is longer than 10 minutes (`LONGEST_NAP`; it was 24 h): the sleep is monotonic and stops while the
laptop sleeps. A task exists only while something is armed, so an idle server still has no timer.

**The gateway's accept loop** backs off 50 ms after a failed accept (out of descriptors was a busy loop and a log line per turn), caps
connections at 1024, closes a connection that sends no head for 60 s (which is also the keep-alive idle limit), and sets `TCP_NODELAY`.
The soft limit on open files is raised toward the hard one at start (macOS starts a terminal's programs at 256). **New dependency:**
`rlimit` 0.11 (doze-kafka's; it links only libc, already in the tree): +1 package (307 to 308 of the 310 line), +208 B (release profile).

**Not changed: a reused `Sleep` in `Lazy::run`.** `timeout` puts its `Sleep` inside the actor's future (no allocation); a reset one
re-registers in the timer wheel just as a new one does. Measured over a million messages through a channel on one thread: 118 to 175 ns
a message with a new timeout, 85 to 149 ns with a reset `Sleep`, 49 to 90 ns with no timer: tens of nanoseconds against a request's tens
of microseconds.

**Weight after use** (`cargo xtask weight doze-aws`, new): the server with `--idle-secs 2`, 20 PutObjects of 64 KiB, 20 SendMessages and
a ReceiveMessage, 20 PutItems, then the reap. First figures, release-aws: 8,928 and 9,136 KB resident, 1 thread. Reported, not yet a
limit.

**The whole workstream weighs** 12,305,040 B against main's 12,237,920 B (release-aws, +67,120 B: the shared timer, the writer, the
gateway's limits and timer, `rlimit`), 308 packages against 307, idle 4,912 KB (median of 4,608, 8,064 and
4,912: macOS idle readings swing that much). The blocking hand-off was first generic per service, 16,720 B more.

## Workstream 1: wakeups and polling (2026-11-27, branch `perf-wakeups`), release, darwin/arm64

What woke an idle doze-aws, and what a drained queue cost. Measured with `cargo build --release -p doze-aws`, a throwaway
server per run (`--no-console`, fresh `--data-dir`), the AWS CLI, and `top -l 2 -s 10 -pid <pid> -stats pid,cpu,idlew,csw,th`
(CPU of the second sample; context switches as the difference of the two samples, 10 s apart).

| Case | Before | After |
|---|---|---|
| No mapping, idle | 0.0% CPU | 0.0% CPU |
| One enabled SQS mapping on an empty queue | 35.6-36.3% CPU, ~250,000 switches / 10 s | 0.0% CPU, 3 switches / 10 s |
| Ten enabled SQS mappings on empty queues | 66-77% CPU, ~130,000 switches / 10 s | 0.0% CPU, 0 switches / 10 s |
| Five idle `receive-message --wait-time-seconds 20` | 0.0% CPU, 229 switches / 10 s (a re-ask every 250 ms each) | 0.0% CPU, 0 switches / 10 s |
| Fill 50,000 messages, drain with receive(10) + delete-message-batch: server CPU time | 20.9 s (5.45 s at 20,000: quadratic) | 8.0 s |

The drain's wall time (64-119 s before, 71-87 s after, same machine, runs back to back) is the disk's: every answer waits
for its commit, and the spread between runs is larger than the change. The server's own CPU time is the number that moved.

What changed:
- SQS woke every waiter of every queue on every call that was not a receive, `GetQueueAttributes` included, and the Lambda
  poller of an empty queue read that queue's attributes after each empty receive: it woke itself, forever. Wake-ups are now
  per queue (`aws-sqs/src/arrivals.rs`), fired only when the actor reports that a message was added or shows sooner (send,
  a shorter visibility, a move or redrive into the queue, a forced release, the queue's deletion), and an entry exists only
  while someone waits. The same for Kinesis (per stream, `aws-kinesis/src/signals.rs`, also `SubscribeToShard` sessions)
  and DynamoDB streams (`aws-dynamodb/src/signals.rs`).
- A receive that finds nothing is told when the queue's first hidden message shows; a long poll and the Lambda poller sleep
  until then or an arrival, not every 250 ms / every second. Only a receive copies its call.
- A queue's messages are kept by `ord` with an index of the visible ones and one of the hidden ones by when they show; a
  receipt handle carries its message's `ord`, so a delete or a change of visibility is a lookup and a receive does not go
  through the queue. A message keeps its latest 8 handles, not every one it was given.
- The Lambda pollers wait on an enabled `Notified` instead of a task spawned per loop, and parse their filters once.

Heap: a full queue costs a little more per message (two index entries); RSS after the 50,000 drain was 13.1 MB against
9.4 MB before (allocator high water while the queue was full). The idle line is unchanged: an empty SQS holds no index
and no watch.

## Workstream 3: heavy work off the event loop (2026-11-27, branch `perf-offloop`), release-aws, darwin/arm64

doze-aws answers every service on one current-thread event loop; whatever blocks it delays every other call. Measured with
`tools/loop-probe.ts` (an SQS ListQueues every 10 ms; "stall" is the probe's worst round trip while the operation runs). Before and
after ran back to back on the same machine under the same heavy load from other builds (load average 18 to 44), so wall times are
noisy; the stalls are the comparison. A quiet-machine baseline earlier the same day is in brackets.

| Operation | Stall before | Stall after |
|---|---|---|
| KMS first Sign of an RSA_4096 key (key made) | 151-951 ms [300-1030 ms] | 11-73 ms |
| KMS GenerateDataKeyPair RSA_4096 | 526-1323 ms [84-514 ms] | 5-22 ms |
| KMS GetParametersForImport RSA_4096 | 438-1563 ms [684-2032 ms] | 5-35 ms |
| Lambda UpdateFunctionCode, 30,000-file package | 5.2-6.9 s [0.94-1.13 s] | 98-246 ms |
| S3 1 GiB PUT (one request) | 64-168 ms, p50 2.6 ms | 37-60 ms, p50 0.4-0.9 ms |
| S3 1 GiB multipart upload (aws s3 cp) | 244-471 ms, p99 228 ms [138-148 ms] | 41-65 ms, p99 31-38 ms |
| S3 1 GiB CopyObject | 115-131 ms (and the region's actor held for the whole copy) | 24-30 ms, actor free |
| Logs FilterLogEvents, 100,000 events, no match | 132-155 ms [35-40 ms] | 8-15 ms |
| DynamoDB CreateBackup, 100,000 items | 230 ms [85-150 ms] | 15-30 ms |
| DynamoDB RestoreTableFromBackup, 100,000 items | 270-814 ms [185-345 ms] | 11-16 ms |

Also: ListObjects with a delimiter over 100,000 keys under one prefix read 100,005 rows in 158 ms; it now seeks past the fold (a
handful of rows, 74 us; `list_tests`). The server's resident memory after the whole run was 132 MB before (a restore held every item)
and 12 MB after.

What moved: KMS key generation, RSA decryption, every Sign and Verify and an import's unwrap run on blocking threads (`offload.rs`);
Lambda drops code trees by renaming them into `code-trash/` and empties it in one blocking job, and decodes, hashes and unpacks a
package in one; S3 sums and writes an upload in 512 KiB batches (one blocking job each, at most two batches held), flushes and names
a blob off the loop, and copies outside the actor in one blocking job, 1 MiB at a time; the CRCs take eight bytes a step; Logs reads
at most 10,000 rows a FilterLogEvents call and answers a nextToken, and parses a filter's pattern once; DynamoDB copies a backup's
and a restore's items inside the writer's transaction.

Size: 12,237,920 B before, 12,254,816 B after (+16.9 KB), with `md-5` at opt-level 3 (+48 B of that; MD5 2.2 times faster, 256 MiB
in 1.02 s then 0.45 s). `aws-s3` at opt-level 3 for the CRCs would have been +412 KB and was not taken. No new crate.

## Workstream 4: Lambda runtime, outbound calls, caps on fan-out (2026-11-27, branch `perf-lambda-outbound`), darwin/arm64

Measured with `cargo build --release -p doze-aws` (the `release` profile, not `release-aws`), a throwaway server
(`--no-console --lambda-quiet`), Python 3.9 (the system one), and bun scripts that call the API directly. Before is
origin/main db6cbc2c8.

| what | before | after |
|---|---|---|
| 1000 sequential warm no-op Python invokes, p50 / p99 / mean | 9.84 / 42.6 / 11.98 ms | 1.0–1.8 / 2.2–4.3 / 1.1–1.7 ms (six runs) |
| the same with `LogType: Tail`, p50 / p99 | 10.15 / 42.4 ms | 1.4–1.9 / 2.2–2.5 ms |
| TIME_WAIT sockets left by 1000 invokes (Python client) | 1,035 | 0 |
| 2000-message SNS fan-out to `http://localhost`: server threads at most / time until all delivered | 516 / 847 ms | 6 / 311 ms |
| idle: RSS, threads, wakeups (top, 5 s) | 7,888 KB, 4, 0 | 7,616 KB, 4, 0 |
| binary (`release`) | 22,831,312 B | 22,864,304 B (+33 KB) |

- **Where the 10 ms went:** every invocation slept at least one 2 ms step (up to ten) waiting for output to stop arriving,
  then waited for `PutLogEvents` (with the Logs durability ack) and the metrics call before answering. Now the lines are taken
  when the pipes hold nothing unread (`FIONREAD` on a second handle of each pipe, the reader let run between looks; no
  sleep; Windows keeps the old settle), the log is queued to one writer task per Lambda (started when there is something to
  write, gone when the queue is empty; one `PutLogEvents` per stream for all that is queued; at most 20,000 events wait,
  then an invocation waits for room), and the metrics are counted at once and sent by a task when an alarm watches.
  `Duration` now counts from when the process fetched the invocation (it was from when the waiter noticed).
- **Keep-alive in the Python and Ruby clients** is latency-neutral within the run-to-run spread above (the release runs
  read 0.3–0.7 ms slower at p50 than the build with only the first change, the debug runs read the same); it is kept for the
  sockets: one connection per process instead of two per invocation.
- **Fan-out:** `localhost` was resolved with `getaddrinfo` on the blocking pool for every request, and nothing capped the
  requests under way, so the pool grew to its 512 threads and they lingered. Now an IP address and `localhost` skip the
  resolver and at most 64 requests are under way in the whole process (`aws_webhook::MAX_UNDER_WAY`).
- **The pool:** at most `lambda-max-processes` (32) function processes in all; a full pool ends the one idle longest.
  **`lambda-idle-secs` default 600 → 300**: a warm process holds 6–16 MB (Python) or about 33 MB (Ruby) of RSS here, and a
  cold start costs 0.1–0.22 s (Python, Node, Ruby measured); after five quiet minutes that start is not noticed, and the
  memory is let go twice as soon after a burst.
- **Dependency:** `rustix` (features `std`, `process`) is now a direct dependency of `aws-lambda` on Unix, for
  `kill_process_group` and `ioctl_fionread`; it was already in the binary through doze-os, so it adds no package (the
  binary delta above is the whole change). `aws-sfn` now depends on `aws-webhook`, which was already linked.

## Workstream 6: the leftovers of workstreams 1-4 (2026-11-27, branch `perf-round`), release, darwin/arm64

Before is `perf-round` at 66b1d86ef (workstreams 1-4 merged), after is the branch with this workstream; both `cargo build --release -p
doze-aws`, run back to back on the same seeded data directory (100,000 S3 objects under `deep/` in a versioned bucket with a lifecycle
rule; a DynamoDB table of 100,000 small items), load average 8 to 14. "Stall" is `tools/loop-probe.ts`'s worst round trip (an SQS
ListQueues every 10 ms) while the operation runs; the idle probe's worst was 3 to 12 ms.

| Operation | Before | After |
|---|---|---|
| S3 lifecycle sweep, 100,000 keys: stall | 119, 172, 123 ms | 0.5, 2.2, 0.5 ms |
| S3 ListObjectVersions `--delimiter /`, 100,000 versions under one prefix | 10 to 17 ms (100,005 rows read) | 0.4 to 0.8 ms (a few dozen rows; `list_tests`) |
| DynamoDB DescribeTable, 100,000 items, five in a row: each / stall | 4.4 to 6.5 ms / 4.4 ms | 0.26 to 0.49 ms / 0.7 ms (counted once off the loop, kept until a write) |
| DynamoDB `doze-aws data` empty of that table (deletes and compaction): stall / total | 20.8 ms / 21 ms | 0.5 ms / 11 ms |
| Kinesis mapping, 32 shards, 2,000 PutRecords 3 ms apart (about 1,980 invocations): server CPU | 2.02, 2.08, 2.26 s | 0.88, 1.42, 0.94 s |

What changed: `doze-store` reads (`Db::read_off_loop`, `doze_store::scan`) and compacts (`Db::compact_off_loop`,
`GroupCommit::paused_off_loop`) on the blocking pool, one job each; the S3 lifecycle walk, DescribeTable's counts (and the data view's
usage counts), and DynamoDB's and Kinesis's data-view compaction use them; the S3 janitor naps at most `LONGEST_NAP`; ListObjectVersions
seeks past a folded prefix; `Lazy` numbers its answers and `Due::arm_ordered` drops an older answer's arm (CloudWatch, Step Functions,
DynamoDB); one `doze_runtime::Signals` replaces three copies and counts appends per shard, so a Kinesis mapping reads only the shards
that took records and lists them only when they changed. Not moved: the other thirteen services' data-view compaction (`Db::compact`
on the loop) and Step Functions' (its `core.rs` is workstream 5's; patch kept for the lead).

Size: `release` 23,162,720 B before, 23,311,600 B after (+148,880 B, not yet bisected: `Lazy`'s numbered answers are monomorphised per
service). `release-aws` after: 12,388,544 B, 308 packages, idle 8,000 KB (samples 8,000, 7,920, 8,032), after use 8,880 KB, 1 thread
(`cargo xtask weight doze-aws`); the `release-aws` before build was stopped when work paused. No new crate (`doze-store` turns on
tokio's `rt` feature, already linked).

## Workstream 5: the request hot path and memory (2026-11-27, branch `perf-hotpath`), release-aws, darwin/arm64

Measured on a throwaway server (`--no-console` for the loads, the console on for idle), with a bun driver that signs under
`--auth trust` (scratch `load.ts`), server CPU from `ps -o time` around the load, peak from `vmmap -summary` ("Physical
footprint (peak)"), idle heap from `heap(1)` 2.5 s after start. Base is `db6cbc2c8`; profiles were taken with `sample` on a
`release-aws` build with line tables. Every row is two or more runs; the spread is in the text.

| | base | after | what did it |
|---|---|---|---|
| idle heap | 462,640 B | **159,424 B** | one set of pattern slots a model, allocated at its first match (E) |
| idle footprint (dirty) | 3.3 MB | 2.9 MB | the same |
| binary | 12,237,920 B | 12,254,576 B | +16.7 KB |
| Lambda `Invoke`, 6 MB echo, server CPU | 520-580 ms | **44 ms** | base64 by table; payload into the tree once (D) |
| the same, peak footprint | 141-150 MB | **70-76 MB** | |
| DynamoDB `Query`, 1 MB page, server CPU | 28-31 ms | **11-14 ms** | strings copied in runs (encoder, decoder); `reply_json` without text (A3) |
| the same, one at a time, peak | 24.4 MB | 19.2 MB | |
| Kinesis `PutRecords` 500 × 10 KB, server CPU | 200-214 ms | 131 ms | base64 by table (the rest is Kinesis' own work) |
| SQS `SendMessage` × 10,000, `--iam off` / `soft`, server CPU | 2.1-2.8 s / 2.3-2.7 s | 2.2-2.3 s / 2.1 s | soft costs no more than off within noise (B, C, A2) |
| Step Functions, 5,000 executions with 10 KB input, peak | 86-88 MB | **67-69 MB** | a succeeded execution drops the interpreter's input copy (F) |
| access log, 50,000 new rows into a full log | 113 ms | 14 ms | a recency list instead of a scan of 2,000 rows a row (B; `cargo test -p aws-iam --release accesslog_bench -- --ignored`) |

What the profiles showed and what changed:

- **base64** (`aws-core`) searched the 64-character alphabet for every character: 1,850 of 2,679 samples of a 6 MB `Invoke`, most
  of a 5 MB `PutRecords`. A 256-entry table.
- **JSON strings**: the encoder pushed a string a character at a time (half of a 1 MB `Query` page's encode); it now copies the runs
  that need no escape. The decoder finds a string's end with one slice scan.
- **`reply_json`** wrote the handler's `Value` to text and decoded it back leniently, then copied every string again (`detach`):
  1,530 of 3,542 actor samples of a `Query`. `handler::tree` reads the value into the arena by the shape, with the same checks; a
  test holds it to the text path byte for byte.
- **Lambda `Invoke`**: the payload went to base64 in a `Value`, to text, through the decoder, then Lambda parsed the 8 MB body
  again, decoded the base64 again and built a `Value` of the payload to validate it. Now the body is added to the decoded tree as
  the blob (`Service::payload_aside`, Lambda's two invoke operations), read from there, and checked without building a value.
- **Patterns** were built three times a model (gateway, bus, service), 80 B a slot, 1,476 slots over the 18 models, at start.
- **authz**: every service but SQS and Kinesis reads its needs from the decoded tree (`aws_peers::input::In`) instead of parsing the
  body again; SQS and Kinesis are workstream 1's to switch.

Found and left to their owners: `doze_store::Records` keeps each record's encoded bytes beside the record (`Row::written`) to
tell a change, which doubles every table's memory (a finished Step Functions execution is held twice); an SNS `Publish` to ten SQS
subscriptions takes about 70 ms, nearly all of it ten durable SQS commits one after another (the bus's serialization is about 1%
of it, so no typed fast path was added for it); idle RSS moves between 4.5 and 7.9 MB from start to start with the same 2.9 MB
footprint, which is file-backed text paged in or not (the noise #36 records), not heap.

## Workstream 7a: the store, read-your-writes, compaction everywhere, and the size of the round (2026-11-28, branch `perf-w7a`), release-aws, darwin/arm64

Measured on throwaway servers (`--no-console`, fresh `--data-dir`) of `release-aws` builds, with bun drivers that sign under
`--auth trust` and `tools/loop-probe.ts` (an SQS ListQueues every 10 ms; "stall" is its worst round trip). Base is `perf-round` at
0a57d5545 (workstreams 1-6). The machine was building other branches throughout (load average 23 to 38): rates are noisy, memory
and stalls much less so. Every row is two or more runs.

| | base | after |
|---|---|---|
| Step Functions, 5,000 executions with 10 KB input, footprint after | 67.6 MB (RSS 47.3 MB) | **16.2 MB** (RSS 19.7 MB); 43.0 MB with the hash alone |
| SQS, 100,000 messages sent, footprint after | 84.6 MB (RSS 21.8 MB) | 84.3 MB (RSS 20.8 MB): unchanged, see below |
| DynamoDB PutItem, 16 clients, 2,000 items | 730 to 2,428 a second, server CPU 0.79 to 1.15 s | **5,457 to 15,901 a second**, CPU 0.21 to 0.34 s |
| `doze-aws data compact` after deleting one of two 100,000-event log groups (53 MB back): stall | 568, 1,100, 1,287 ms | 3.8, 151, 196 ms |
| idle (`cargo xtask weight doze-aws`) | | 5,184 KB (samples 7,744, 5,184, 4,800), after use 6,784 KB, 1 thread, 308 packages |

**`Records` keeps a length and a hash of what it wrote, not the bytes.** `Row::written` held each record's encoding to tell a change,
so every table was in memory twice. It is a 16-byte `Seen` now: the length and std's SipHash-1-3 (`DefaultHasher`, fixed keys,
never stored) of the encoding. Two different encodings of one length that hash alike would skip one write: one in 2^64 a change, and
no record can be made to collide on purpose without the keys. No new crate. SQS is unchanged by it: its messages were never
`Records` (they say what changed themselves); what an SQS queue of 100,000 messages holds is the messages and their two indexes.

**Read-your-writes (`doze_store::pending`).** A database's queued, not yet committed writes are kept by the store (an `Arc` of the
batch the writer applies, so no copy) and laid over the file by `Db::get`, `Db::scan`, the row tables' reads and DynamoDB's
`Disk::get`/`Disk::scan`, in the order the writer applies them. An entry leaves after its commit returned; a failed commit drops
them all and the overlay takes no more while the writer refuses the database. A read that cannot see the overlay (`read_off_loop`,
DynamoDB's counts) waits for the queued writes first. What each answer waits for:

| service | before | after |
|---|---|---|
| DynamoDB | a write's answer waited for its commit, inside the actor, so the next call waited too | the answer still waits for its commit (outside the actor: `Answer::durable`); the next call runs at once and reads the queued write; stream consumers are woken after the commit; a backup's or restore's copy (work inside the transaction, which the overlay cannot show) still waits inside the actor; a time-to-live tick's commit is waited for by its timer task |
| Step Functions | the answer waited for its commit outside the actor (`Answer::durable`) | the same; a finished execution is read back through the overlay, so a describe right after the finish sees it |
| the others | the answer waits for its commit | unchanged (their writes are laid over too, which only matters to a read before the ack) |

**Step Functions keeps finished executions on disk.** Once a finished execution's record is staged it leaves memory
(`Records::evict`) and a `Summary` stays in the index (what ListExecutions shows and filters on, what `trim` and the data view
count); describe, stop, redrive and DescribeStateMachineForExecution read the record back; a restart reads only the running ones
(`Records::load_where`). The definition each execution carried as its own `String` is a shared `Arc<str>` (`finished::Def`), written
as the same text: the record format and its version are unchanged, so old files read as before.

**The data view's compaction is off the loop in thirteen more services** (apigateway, cloudformation, cloudwatch, eventbridge, iam,
kms, logs, s3, scheduler, secrets, sns, sqs, ssm: `Db::compact_off_loop`; SQS's `Persist` as DynamoDB's `Disk`). Lambda's is the
lambda workstream's. The residual 150 to 200 ms in the table is SQS's own compaction, which the probe (an SQS call) waits for.

**The size of the round.** On darwin/arm64 a Mach-O segment is a whole number of 16 KiB pages, so the file grows in 16 KiB steps
and hides or exaggerates a change; the sum of the sections (`size -m`) is the honest number. `release-aws`, each workstream's merge on
`perf-round` (`cargo build --profile release-aws -p doze-aws`):

| commit | file | sections | sections, change |
|---|---|---|---|
| main db6cbc2c8 | 12,237,920 | 11,987,667 | |
| workstream 2 (a0219644c) | 12,305,040 | 12,059,227 | +71,560 |
| workstream 1 (420201da9) | 12,321,696 | 12,073,499 | +14,272 |
| workstream 3 (a2301ff44) | 12,338,576 | 12,094,223 | +20,724 |
| workstream 4 (66b1d86ef) | 12,355,216 | 12,116,297 | +22,074 |
| workstream 6, reads and compaction off the loop (290d7ef62) | 12,371,808 | 12,119,189 | +2,892 |
| workstream 6, DescribeTable counts (222ca254d) | 12,371,968 | 12,134,117 | +14,928 |
| workstream 6, numbered answers (77240eddb) | 12,388,512 | 12,141,513 | +7,396 |
| workstream 6, the rest (to fc47014d5) | 12,388,544 | 12,148,185 | +6,672 |
| workstream 5 and Step Functions' compaction (0a57d5545) | 12,405,264 | 12,169,021 | +20,836 |
| this workstream, the hash (1f1379235) | 12,421,760 | 12,185,393 | +16,372 |
| this workstream, all of it (4a9db5fb5) | 12,471,808 | 12,231,473 | +46,080 |

The round is +233,888 B of file and +243,806 B of sections over main; the target was +100 KB and it is not met. What `cargo bloat`
(the same profile with symbols) shows: no one generic is to blame. Workstream 6's guess, `Lazy::call_numbered` per service, is the
7.4 KB of 77240eddb. Workstream 2's +71.6 KB is mostly the services' `open` and `close` moved out of their actors into
`off_the_loop::blocking::<S>` (34 instances, 62 KB, with the actors' polls 66 KB smaller: a move plus the new timer, writer and
gateway code). Under fat LTO a small change moves whole functions in or out of their callers (Step Functions', DynamoDB's and
CloudWatch's `handle` came out of `serve` here: 68 KB out, 66 KB in), so a per-commit attribution finer than the table is noise of
some KB. Two tries to shrink this workstream's part (the hash out of line, one compaction future for every service) measured
+1,408 B and were dropped. The one large structural waste found is old: the same generic instantiated in several crates (release
builds do not share generics), 234,804 B of copies over 1,135 names on main and 246,552 B now; the largest are std's sorts of
`String` (16 copies each) and hashbrown's rehash (25). Collapsing those (sorting through one non-generic helper, fewer distinct
map types) is the trim pass's, not this workstream's.

## Workstream 7b: the last hot-path and Lambda items (2026-11-28, branch `perf-w7b`), release-aws, darwin/arm64

Base is `perf-round` at 0a57d5545, after is the branch with items 1-4 (`cargo build --profile release-aws -p doze-aws`, both); run
back to back, alternating, on fresh data directories, with workstream 5's bun driver (`load.ts`: a fake SigV4 header under `--auth
trust`), server CPU from `ps -o time` around the load, peak from `vmmap -summary`. The machine was loaded (load average 27 to 65): the
spread is wide, and every row is two or more runs of each.

| | base | after | what did it |
|---|---|---|---|
| SQS `SendMessage` × 10,000, `--iam soft`, server CPU | 2.25-2.46 s | 2.25-2.39 s | authz reads the decoded input (item 1); inside the noise, as workstream 5 found elsewhere: an SQS body is small |
| the same, `--iam off` | 1.96-2.42 s | 1.91-1.96 s | (no authz: the noise) |
| Lambda `Invoke`, 6 MB echo, server CPU (6 invokes) | 0.32-0.33 s (about 54 ms each) | **0.08 s (about 13 ms each)** | the answer carried whole beside the JSON, not base64 and back (item 2) |
| the same, peak footprint | 62.8-66.4 MB | **47.8-53.7 MB** | |
| SNS `Publish` to 10 SQS subscriptions, one at a time, each | 7.8-13.5 ms | **3.2-3.8 ms** | the deliveries side by side, their commits grouped (item 3) |
| the same, server CPU for 101 publishes | 0.53-0.58 s | 0.19-0.22 s | fewer commits |
| `Claim::parse` saved a verified request (`--auth verify`) | | 0.65-0.74 µs | `aws_auth::begin_claimed` (item 4; `cargo test -p aws-auth --release claim_read_bench -- --ignored --nocapture`) |
| binary | 12,405,264 B | 12,405,296 B | +32 B |

Every queue got every message after the change (21 of 21 in each of 10 queues for 20 publishes and the warm one). The
6 MB invoke was measured before workstream 5's own number (44 ms) because the machine was busier; the ratio is what to read.

What changed: SQS and Kinesis authz use `aws_peers::input::In` (the last two services); Lambda's synchronous `Invoke` answers with
`RestReply::body` = `aws_wire::rest::whole(payload)` and the gateway writes it through `jsonout::render_with`, while a caller on the bus
gets `Payload` back in the JSON (`RestReply::payload_in_json`, in `Peers::call_rest*` and Lambda's `Service::call`); SNS's `fan_out`
runs its deliveries through `together::all` (no task spawned, at most 16 in flight, polled in the actor's task); the gateway's
`head::authenticate` and the web signature check call `begin_claimed` with the claim the pipeline already read. No new crate.

Also in this workstream, not measured for speed: an asynchronous `Invoke` dropped by a full queue is marked in the console's Live
(DROPPED beside its 202, and a Needs a look chip; nothing more is kept: the record reads it from the call's 8-byte summary), seen
on a server after 10,030 async invokes to a sleeping function (22 dropped); and two Lambda tests that failed under load were two real
faults (the pool's idle-longest tie within a millisecond; the Python shim flushing after its answer) and one test reading order
from the log: before 26 and 27 of 30 runs of the crate passed them, after 30 of 30, and 30 of 30 with 18 more CPU burners.
