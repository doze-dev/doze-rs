# S1b: which decode, validate and encode design gets the whole-binary budget?

Status: **prediction written 2026-11-09, before any measurement.** Result and verdict are appended below the line when done.
Code and raw numbers stay in the scratchpad (`spike-s1b-decode`), built on S1's harness (`spike-s1-codegen-full`, `docs/port/aws/spikes/S1.md`).

## Question

S1 showed typed decode and encode for all 18 models costs 6.61 MiB at opt 3 (4.21 at opt z), and that the tables and generic codecs alone are a 2.25 MiB
floor. The whole-binary budget (PLAN.md "Targets") is at most 16 MiB on macOS arm64 with all 17 services and the console, 12 MiB intended. Which design gets there
without weakening validation and without breaking PLAN.md rules 16 to 25 (borrow, don't copy; no serde on wire types; exhaustive matches; no `unsafe`)?

## Designs

| id | design |
|---|---|
| A | S1's typed glue (owned structs, per-type `Decode`/`Encode`), generated **only for the operations doze-aws implements** (tier F or C in `doze-aws/docs/SUPPORT.md`: 589 of 1,372 operations). Every other operation answers 501 by name from the exhaustive `Operation` enum with no decode code. Tables only for the shapes those operations reach. |
| B | One generic schema-driven decoder per protocol that borrows from the request bytes into a zero-copy value tree (an arena of nodes) and validates against the **pointer-based** schema tables as it goes (S1's `Member`/`StructSchema` statics). Per-operation typed accessors are thin generated views that borrow the tree. Tables for all operations. |
| C | B with the tables stored **packed**: one `u32` word array plus one string pool per service, members as fixed-size records (interned names, indexes not pointers), constraints as words. No relocations. Tables for all operations. |

## Predictions (macOS arm64, rustc 1.99.1, fat LTO, `codegen-units = 1`, `panic = abort`, strip; MiB)

Facts used (counted, not measured on the designs): 589 of 1,372 operations are tier F or C; S1 has 16,595 members and about 6,200 shapes; S1's `Member` record is large
(two `&str`, three `Option`s, a binding enum, a `Target` enum with pointers), so I estimate about 100 bytes per member and 1.7 MiB of the 2.25 MiB floor to be tables.

| quantity | A | B | C |
|---|---|---|---|
| all 18, opt 3, stripped | **3.6** (3.0 to 4.2) | **2.8** (2.4 to 3.3) | **1.5** (1.2 to 1.9) |
| all 18, opt z | 2.5 | 2.7 | 1.2 |
| tables part of the above | 0.8 | 1.8 | 0.5 |
| generic codecs (JSON, Query, REST, XML) | 0.45 | 0.45 | 0.45 |
| clean build time, all 18, opt 3, fat LTO | 3.5 min | 4 min (large const initialisers) | **under 2 min** |
| SQS `SendMessage` (1 KiB body, 3 message attributes) allocations per decode | 30 (owned `Value` tree and `String`s) | **3 or fewer** | **3 or fewer** |
| bytes allocated per decode | 3.5 KB | **2.5 KB or less** | **2.5 KB or less** |
| DynamoDB `PutItem` (10 attributes, nested map and list) allocations | 90 | **4 or fewer** | **4 or fewer** |
| decode throughput, SendMessage, one core | 300k/s | **800k/s or more** | **800k/s or more** |
| decode throughput, PutItem | 120k/s | 400k/s or more | 400k/s or more |

Other predictions:

- **Patterns.** Of the 52 patterns `regex-lite` cannot compile, no more than 20 sit on members reachable from implemented operations, and they fall into about 8 kinds of
  hand predicate (14 are the same "not blank"). All 52 compile as hand predicates or are dropped with a recorded reason; none needs a different regex engine.
- **Whole binary** (this is a projection with its basis, not a measurement): runtime stack (tokio current-thread, hyper, redb, bytes, tracing, clap, postcard) 2.3 MiB, from
  kafka's 3.4 MB binary of which about 1.3 MB is kafka-specific code; crypto (sha1, sha2, md5, crc32, hmac, aes-gcm, rsa) 1.0; JSONata (`jlens-core`) 0.6; console (maud views plus
  embedded htmx and css) 1.3; service logic for 17 services 3 to 5, at 2 to 3 times the size of the kafka broker's own logic. Whole-binary projection: **C about 10.5 MiB (9 to 13)**,
  B about 12.0, A about 12.7. Only C reliably meets the 12 MiB intended figure; all three are under the 16 MiB ceiling.
- **Copies.** The designs need a copy in only three places: unescaping a JSON or XML string that contains an escape, decoding a base64 blob, and percent-decoding in Query and REST
  labels. Every other string and blob stays a `&str` or `&[u8]` into the request body.
- **Verdict predicted:** C. B is the reference for validation behaviour, A is rejected because typed owned structs cost both allocations and bytes for what is mostly accept-and-store.

## What is measured

Counting allocator (a global allocator in the test binary, not timing alone) on the two representative requests; release builds for size and throughput; ten-run medians for
throughput; clean builds for time; the 52 patterns classified by reachability from implemented operations.

---
