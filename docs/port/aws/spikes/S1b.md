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

# Result (2026-11-09)

Machine: Apple silicon Mac, rustc 1.99.1, `lto = "fat"`, `codegen-units = 1`, `panic = abort`, strip. Sizes are stripped files in MiB (Mach-O pads segments, so a
size is good to about 16 KiB). Throughput is the median of three runs of seven; the machine was shared with other sessions, so treat each rate as plus or minus 25 percent.
Code and raw numbers: scratchpad `spike-s1b-decode` (`results-all.txt`, `results-a.txt`, `crates/pk-rt`, `crates/pk-diff`, `crates/pk-bench`).

## What was built

- `smithy-gen gen-packed` (`emit_packed.rs`, 661 lines): one neutral table model built from the IR, written two ways: pointer statics (design B, as spike A and S1 did, plus the constraints they
  lacked) and packed words plus one string pool (design C: four `u32` per member and per shape, nine per constraint, strings as offset and length in one `u32`, constraints and enum sets
  deduplicated). It also writes typed key constants (`keys.rs`) and the exhaustive `Operation` enum. `--impl implemented.json` keeps only the operations doze-aws implements.
- `pk-rt` (about 1,400 lines, clippy clean under the workspace lints): a `Tables` trait that both forms implement, so **one decoder serves B and C and only the table layout differs**; an
  arena of 24-byte nodes that refer to the request body by offset (no lifetime, no pointer, `clear()` to reuse); a JSON decoder that validates in the same pass (types, required members,
  length, range, enum, pattern through a lazy per-constraint cache, union exactly-one, timestamp shape, nesting cap of 100); a schema-driven JSON encoder; typed borrowing views where
  a `Key<Shape, Kind>` constant is the only per-member item. No serde, no `unsafe`, no `unwrap`.
- Design A is S1's generator (`--glue --no-serde`) restricted to the 589 tier F or C operations (of 1,372): 2,759 of about 6,200 types, with every other operation answering 501 by name.
- `pk-diff`: a differential test. Bodies are generated from the schema (boundary lengths, bad enums, missing required members, wrong types, multi-byte text, unions with zero, one and two members)
  and accepted or rejected by A (constraints in generated newtypes) and C. `pk-bench`: a counting allocator (`stats_alloc`, so the `unsafe` stays in a library) and timing on two requests.

## Sizes, all 18 services in one binary

| design | opt 3 | opt z | clean build (all 18 crates + fat LTO link) |
|---|---|---|---|
| A, typed glue for implemented operations (inputs, plus output decode and encode as in S1; 501 by name adds about 0.04) | **3.43** | **2.27** | 191 s (opt 3) and 82 s (opt z), for four binaries |
| B, one decoder over pointer tables, all 1,372 operations | **2.47** | **2.46** | 7 s and 5 s |
| C, the same decoder over packed tables, all 1,372 operations | **1.11** | **1.08** | 5 s and 5 s |
| reference: S1 typed glue, every operation (server-realistic) | 6.61 | 4.21 | 342 s and 44 s |
| reference: an empty Rust binary | 0.29 | 0.27 | |

The packed tables are **0.71 MiB** for all 18 models (494 KiB of words and 233 KiB of strings; SSM 108 KiB, S3 77, Lambda 61, API Gateway v2 61, IAM 57, SQS 11). The generic decoder and
`regex-lite` add about 0.11 MiB. The Query, REST and XML codecs with `serde_json`, which this spike's JSON-only decoder does not replace, are 0.40 MiB as a binary of their own (0.11 over empty);
a zero-copy rewrite over packed tables should not cost more, but that rewrite was not built. B and C agree exactly: the same 3,153 accepted and 4,152 rejected probe bodies and the same output hash.

Per service (opt 3, standalone, includes the 0.29 base): SQS 0.40 for C and 0.43 for B.

## Allocations and speed (SQS `SendMessage`, 1 KiB body, three message attributes; DynamoDB `PutItem`, 10 attributes, nested list and maps)

| | allocations per decode (fresh arena) | reallocations | bytes | allocations with a reused arena | time, median | rate |
|---|---|---|---|---|---|---|
| SendMessage, A (`serde_json::Value`, then typed) | 39 | 0 | 6,763 | 39 | 1.63 us | 610k/s |
| SendMessage, B | 2 | 1 | 3,504 | **0** | 1.05 us | 950k/s |
| SendMessage, C | 2 | 1 | 3,504 | **0** | 1.29 us | 780k/s |
| SendMessage with escapes, A / B / C | 40 / 3 / 3 | 7 / 1 / 1 | 8,661 / 4,338 / 4,338 | 40 / 0 / 0 | 2.6 / 1.6 / 1.9 us | 385k / 610k / 530k per s |
| PutItem, A | 129 | 0 | 19,344 | 129 | 4.4 us | 225k/s |
| PutItem, B | 2 | 1 | 1,776 | **0** | 1.41 us | 710k/s |
| PutItem, C | 2 | 1 | 1,776 | **0** | 1.84 us | 540k/s |

The two allocations of a fresh arena are the node vector and, when a blob is present, the decoded-bytes vector; the reallocation is the first escape or blob growing its buffer. With the arena reused
(the connection or worker keeps it), a decode and every typed read allocate nothing. Packed tables read about 20 percent slower than pointer tables (decoding words and string references on each access);
against a durable send of about 6 ms (spike S6) that is noise.

## Validation: the typed design against the table-driven design

**1,364,000 generated bodies over all 1,372 operations (1,000 per operation): 353 disagreements, all one case.** `PutResourcePolicy` with `"Policy": ""`: the model's pattern is the look-ahead "not blank",
`regex-lite` cannot compile it, so A silently accepts; C carries a hand predicate for it and rejects, which is what the model says. Nothing else differs.

The differential found six faults in C before it got there, all fixed: members with a model default were treated as required; streaming members (payload blobs, event streams) were required or rejected
in a JSON body; timestamp strings were accepted unchecked (A rejects `"?"`); a string where a number belongs was a syntax error instead of a type error (different AWS error codes); end of input
where a value belongs was a type error; and duplicate union keys. The last of those, and duplicate map keys, are generator artifacts: A (through `serde_json::Value`) keeps the last duplicate, C validates every
occurrence and so can reject a body whose earlier duplicate is invalid. What AWS does with a duplicate key is not known and is a recording to make.

Limits of this check: the bodies come from the same tables C reads, so a mistake in the shared IR is invisible to it; random strings seldom satisfy a pattern, so accepting paths of patterns are lightly
exercised; it is JSON only; and A is S1's glue, which enforces a pattern only where `regex-lite` compiles it. A lean test (`pk-bench/tests/decode.rs`) pins the contract on the SQS and DynamoDB tables.

## The 52 uncompilable patterns

52 types (23 distinct patterns) cannot be compiled by `regex-lite`: 27 use Unicode properties (`\p{L}\p{Z}\p{N}...`), 21 look-ahead or look-behind, 3 something else (`\w-_`, a surrogate range, `\p{ASCII}`).
**22 types (14 distinct patterns) are reachable from the implemented operations**, in 8 kinds: the `\p{L}\p{Z}\p{N}...` family (13 types), not-blank (1), a bucket-name look-ahead, "not only digits" (2),
a `system.` prefix, no trailing dot (look-behind), a Lambda function ARN, and `\p{ASCII}`. C implements the not-blank predicate; the others are overlay work. The `\p{...}` family needs Unicode general
categories (`\p{Z}`, `\p{P}`, `\p{M}`), which `std` does not offer: a small generated range table (not measured) or a crate.

## Whole-binary projection (macOS arm64, stripped; a projection with its basis, not a measurement)

| component | opt 3 | opt z | basis |
|---|---|---|---|
| runtime: std, tokio, hyper, redb, postcard, tracing, clap, regex-lite | 1.52 | 1.17 | spike C cumulative sizes through clap and regex-lite (1.59 at thin LTO), scaled by its fat-LTO ratios |
| codegen and decoder, design C, including the non-JSON codecs | 0.89 | 0.87 | this spike: 1.11 less the 0.29 base and 0.04 shared `regex-lite`, plus 0.11 for the other codecs |
| RustCrypto (aes, hmac, rsa, p256, p384, p521, k256, ed25519) | 0.75 | 0.60 | spike D, 0.60 at opt z; opt 3 assumed 25 percent larger |
| JSONata (`jlens-core`) | 1.10 | 0.86 | spike D, 0.86 at opt z; opt 3 assumed 25 percent larger |
| console: maud views and embedded assets | 1.40 | 1.20 | an assumption: 380 screens at about 2.5 KB plus about 0.5 MiB of compressed assets |
| service logic (17 services, IAM evaluator, ASL, expressions, cron, CloudFormation) | 2.30 | 1.80 | kafka's own crates are about 864 KiB of code for 32.6k lines (`cargo bloat`), 27 bytes per line; 75k lines assumed (Go's engines at 0.7), plus 20 percent for data |
| **total, design C, without a TLS client** | **7.96** | **6.50** | range about 6.5 to 10.5 at opt 3 |
| TLS client (rustls and ring), if HTTPS targets are served | +0.78 | +0.60 | spike C |
| **total, C with TLS** | **8.7** | **7.1** | |
| same with A instead of C | 11.0 | 8.2 | A replaces 0.89 with 3.14 |
| same with B instead of C | 10.1 | 8.5 | B replaces 0.89 with 2.25 |

The service-logic line and the console line carry most of the uncertainty (roughly plus or minus a quarter of the total). On Linux x86_64 the kafka build is 1.45 times the macOS one (4.90 against 3.39 MB);
that factor probably applies to code and not to the table data, so C would be near 12 MiB there and A near 16, which is the ceiling.

## Predictions against measurements

| quantity | predicted | measured | |
|---|---|---|---|
| A, opt 3 / opt z | 3.6 (3.0 to 4.2) / 2.5 | 3.43 (+0.04) / 2.27 | hit |
| B, opt 3 / opt z | 2.8 (2.4 to 3.3) / 2.7 | 2.47 / 2.46 | hit |
| C, opt 3 / opt z | 1.5 (1.2 to 1.9) / 1.2 | 1.11 / 1.08 | better than the range |
| tables part: A / B / C | 0.8 / 1.8 / 0.5 | about 0.6 / about 2.0 / 0.71 | C missed (more than predicted) |
| clean build: A / B / C | 3.5 min / 4 min / under 2 min | 3.2 min / 7 s / 5 s | B and C far faster than predicted |
| SendMessage allocations: A / B, C | 30 / 3 or fewer | 39 / 2 plus 1 | A worse than predicted, B and C hit |
| SendMessage bytes: B, C | 2.5 KB or less | 3.5 KB fresh (the node vector is reserved up front), 0 reused | missed on the fresh case |
| PutItem allocations: A / B, C | 90 / 4 or fewer | 129 / 2 plus 1 | |
| throughput SendMessage: A / B, C | 300k / 800k or more | 610k / 950k and 780k | A faster than predicted; C a hair under |
| throughput PutItem: A / B, C | 120k / 400k or more | 225k / 710k and 540k | same |
| patterns reachable from implemented operations | at most 20 types, about 8 kinds | 22 types, 8 kinds | count missed by 2 |
| whole binary: A / B / C | 12.7 / 12.0 / 10.5 (9 to 13) | 11.0 / 10.1 / 8.7 (6.5 to 10.5) | all lower |

## Verdict: design C

**Generated packed tables, one generic zero-copy decoder per protocol, typed key constants as the only per-member code, and an exhaustive `Operation` enum that answers 501 by name.**

- It is the smallest by 1.4 MiB against B and 2.3 MiB against A at opt 3, and the only design whose projected total stays under the intended 12 MiB across its whole range and keeps Linux x86_64 under 16.
- It allocates nothing per request once the arena exists, against 39 and 129 allocations for A, and validates at least as strictly as A (stricter in the one case where `regex-lite` cannot express the model).
- It builds in 5 seconds where the typed design needs 3 minutes, which keeps `xtask codegen --check` cheap in CI.
- B is the fallback if table-read speed ever matters (about 20 percent faster decode, 1.36 MiB larger); A is rejected: owned typed structs for operations that are mostly accept-and-store cost the most bytes and allocations.

## What the overlay mechanism must contain

1. A per-service **predicate registry** keyed by the exact model pattern, for the 14 patterns on implemented operations (the not-blank predicate exists; the Unicode-category family needs a range table), each entry carrying
   its evidence tag and the model pattern it replaces. Patterns with no predicate are listed as unenforced, never silent.
2. **Cross-field rules as data** (member A requires or forbids B; a name suffix tied to an attribute, as SQS FIFO), checked by the service, not the codec.
3. **Limits the model never states** (SQS has none): `length`, `range`, `pattern`, enum entries and `required` overrides, replacing shapes or members as `overlays/sqs.json` does today; the limit prober writes them with the
   tag `recorded`. Required-ness already follows the model correctly: `@required` and no default, streaming members excluded.
4. **A violation-to-error map per service**: the decoder reports a kind (syntax, type, missing, length, range, pattern, enum, union, depth) and the member; each service maps these to its AWS error code and message once
   (the same input can be `SerializationException`, `ValidationException` or `InvalidParameterValue`). Message text comes from recordings.
5. **Policy decisions to record from AWS**: duplicate keys in a JSON object, unknown members (ignored here, as the typed design does), nulls (treated as absent), timestamp formats per protocol.

## Every place the design needs a copy, and why

1. An **escaped string** (`\n`, `é`) is written once into the arena's text buffer: the decoded form exists nowhere in the body. (A string with no escape is a range of the body.)
2. A **base64 blob** is decoded once into the arena's byte buffer, for the same reason.
3. **Percent-decoding and XML entities** (Query, REST labels, XML bodies) need the same single write into the arena; not built here, same argument.
4. The **node vector** is an allocation (reserved once from the body length; zero on reuse), not a copy of data.
5. The **request body must be one contiguous `&str`**, so the gateway collects it before decode (streaming members bypass the tree). One UTF-8 validation scan, no copy, if the body is already `Bytes`.
6. **Compiling a pattern** allocates (about 50 KB of heap for a Unicode-class pattern, once, lazily): not a data copy.
7. The **response** is written into one output buffer per response (reusable by the connection).

Nothing else copies: keys are compared in place, numbers are parsed from the body slice, views hand out `&str` and `&[u8]` into the body or the arena.

## What this spike did not settle

- Query, REST and XML decoding over packed tables, and the response-building API services use to fill an output tree, are not built; their size is inferred from the codec-only binary.
- Windows and Linux x86_64 sizes are not measured; table data is the same, code differs.
- `json.rs` is 542 lines, over the 450-line hard limit; a real crate splits strings, numbers and values into modules.
- The differential cannot see errors shared by both designs (the IR) and does not compare with real AWS; the limit prober and the cassettes do.
- One stray `python3 --version` was typed during the session (a discarded version check, no effect); no Python was used.
