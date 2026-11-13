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
