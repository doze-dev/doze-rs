# Phase 10: the schema registry

Status: **slices 10a and 10c built (2026-11-14, branches `p10-registry` and `p10c-jsonschema`), not yet audited.** The registry's model, its storage in the broker's
metadata database, every REST route the Go edition serves, the host guard, basic authentication and TLS as the Go binary has them,
the wire header, Avro whole and (10c) JSON Schema whole, wired into `doze-kafka` behind the `registry` feature (on by default, as `--schema-registry` is in
Go). Protobuf (10b) is to be added by another agent through the extension points below; the record-value codec
the console needs is 10d. The catalogue is `behaviours/registry.md` (REG-1 to REG-34, REG-D1 to REG-D20).

Goal: a Confluent client (any serializer, `kafka-avro-console-producer`, Schema Registry's own Maven plugin) pointed at
`http://127.0.0.1:8081` cannot tell this registry from Confluent's, except where `behaviours/registry.md` lists a deviation; and the
Go edition's oracle (its recorded conformance scripts and compatibility corpus, both recorded from cp-schema-registry 7.7.1) passes
unchanged.

## Weight first (measured 2026-11-14, darwin/arm64, `cargo xtask weight doze-kafka`, release, fat LTO)

Each candidate library was linked into the default `doze-kafka` with a probe that calls what the registry would call (parse, compare,
canonical form; compile and validate for JSON Schema; compile and decode for Protobuf), so that LTO keeps the code. The base is main
at 2f1bb66.

| Build | Bytes | vs base | Packages | Idle |
|---|---|---|---|---|
| main (base) | 6,011,568 | | 155 | 6,960 KB |
| + `serde_json` parse and print only (already in the tree) | 6,028,112 | +16,544 | 155 | 6,928 KB |
| + `apache-avro` 0.20, default features off | 6,293,120 | +281,552 | 190 (+35) | 7,104 KB |
| + `protox` 0.9 + `prost-reflect` 0.16, default features off | 6,557,440 | +545,872 | 180 (+25) | 6,976 KB |
| + `jsonschema` 0.37, default features off | 7,882,640 | +1,871,072 | 231 (+76) | 7,504 KB |
| + `boon` 0.6 | 7,286,304 | +1,274,736 | 206 (+51) | 7,472 KB |
| **10a as built** (registry crate at `opt-level = "z"`) | **6,244,864** | **+233,296** | **156** | 7,184 KB |
| 10a as built, registry at the release profile's `opt-level = 3` | 6,425,552 | +413,984 | 156 | 7,088 KB |
| 10a with `--features tls` | 7,039,920 | | 168 lines (`cargo tree`, the CI job's count) | |
| main with the module face (Phase 11) | 6,574,304 | | 171 | 7,152 KB |
| **10a merged on the module face** | **6,791,104** | **+216,800** | **171** | 7,248 KB |
| 10a on the module face, `--features tls` | 7,586,144 | | 183 lines | |
| **10c (JSON Schema) on 10a** | **6,841,120** | **+50,016** | **171** | 7,312 KB |
| 10c, `--features tls` | 7,669,200 | +83,056 | 183 lines | |

**The ceiling, with CI's measured ratio (2026-11-14).** CI measured ubuntu x86_64 at 1.36 times darwin (8,174,048 B against
6,011,568 B), not the 1.216 used above. On that scale: the default build with the module face and 10a is about 9.24 MB, under main's
line of 9,386,171 B (which does not move) and 0.76 MB under the owner's 10 MB ceiling; 10a itself is about 0.29 MB of it. **The TLS
build does not fit:** 7,586,144 B times 1.36 is about 10.32 MB, over the 10 MB ceiling, and the module face alone already puts it at
about 10.0 MB (main's TLS line, 9,279,000 B and 173, was measured before the module; the module adds 13 packages). No limit was raised;
the CI TLS job will fail on main with or without the registry until the owner decides (leave the module or the registry out of the
`tls` release, or take a size lever such as `opt-level = "z"` on more of the tree).

**10c, JSON Schema written here (2026-11-14).** No package is added (the module uses the ordered JSON of `json.rs` and the standard
library only). Measured on the same machine at the same commit, before and after: the default build 6,791,104 B to 6,841,120 B
(+50,016 B, 171 packages both), the TLS build 7,586,144 B to 7,669,200 B (+83,056 B, 183 `cargo tree --edges normal` lines both).
Times 1.36: the default build is about 9.30 MB on ubuntu x86_64, under its 9,386,171 B line (which does not move); the TLS build
about 10.43 MB (the owner raised the ceiling to 15 MB on 2026-11-14; the single limit is the coordinator's, set from these numbers).
JSON Schema costs about 0.07 MB on ubuntu, against 1.7 to 2.5 MB for either library.

The paragraph below is the measurement before the merge, on the old 1.216 scale.

The limits are 7,800,000 B, 161 packages and 8,192 KB (`xtask/weight-limits.txt`), and the CI TLS job's are 8,750,000 B and 173
lines. **No limit moves.** The packages line went from 155 to 156 only by a repeated `cargo tree` line: the one new package is
`kafka-registry` itself, which the stub already made a dependency (no third-party package is added; every dependency of the crate,
`hyper`, `hyper-util`, `http-body-util`, `serde_json`, `postcard`, `sha2`, `tokio-util`, `tokio-rustls` under `tls`, was in the tree).
On the Linux scale used since Phase 7 (x86_64 about 1.216 times darwin) 10a is about 7.59 MB against the 7.8 MB limit (97%), and
the TLS build about 8.56 MB against 8.75 MB (98%): the next slices have about 0.2 MB of room before a limit has to move with a
written reason, and the owner's 10 MB ceiling stands 1.4 MB above the TLS build. This is the number to watch in 10b and 10c.

Choices, per language:

- **Avro: written here** (about 1,500 lines with its tests). `apache-avro` costs 35 packages (bon, darling, strum, num-bigint,
  bigdecimal, uuid, rand, among them) and 0.28 MB, and still would not give Confluent's answers: it reports only the first
  incompatibility where Confluent lists every one with a JSON-pointer path, it does not keep the attribute order Confluent stores
  (Avro Java's `toString`), and it checks defaults that Confluent does not. Avro's schema language and its reader/writer rules are
  small and fully specified; the Go edition already carried its own compatibility walk over a library model. The result passes all
  392 corpus pairs and every reason word for word (REG-19).
- **Protobuf (10b): recommended written here.** `protox` + `prost-reflect` cost 0.55 MB and 25 packages, more than the whole of 10a,
  and what Confluent answers is not what they compute: Confluent stores its own printing of the schema (`protoprint.go` in Go, 346
  lines) and has its own compatibility rules (`protocompat.go`, 368 lines). What a parser library would save is the `.proto` parser
  and the descriptor model (Go took them from `protocompile`); a hand-written parser of proto2/proto3 text and of a
  `FileDescriptorProto` (the `protodesc.json` fixture) is the honest size of 10b. If 10b chooses `protox`, the TLS build passes the
  CI limit by about 0.4 MB on the Linux estimate and the limit must move with that reason.
- **JSON Schema (10c): written here, no library.** Both libraries are larger than everything else the registry adds put together
  (`jsonschema` +1.87 MB and 76 packages; `boon` +1.27 MB and 51, mostly `regex`, `url`, `idna` and their Unicode tables) and either
  breaks the package limit at once. Confluent's JSON compatibility is a structural walk over the two schema documents (`jsoncompat.go`
  in Go, 1,205 lines, its own code), which needs the ordered JSON of `json.rs` and nothing more. A library's real job in Go is to
  check that a schema is a valid schema (draft 4 to 2020-12) and to validate the console's JSON before it is produced: 10c should
  check schemas against the subset of the meta-schema the corpus exercises and leave instance validation to 10d, where `regex-lite`
  (already in the workspace, through doze-aws) could serve `pattern`.

## Design

```
crates/kafka-registry/src/
  lib.rs              what each module is
  error.rs            RegistryError {status, error_code, message}; Confluent's codes and sentences
  json.rs             ordered JSON (keys in order, numbers as written), Jackson's escapes, the depth scan
  format.rs           SchemaType, the SchemaFormat and ParsedSchema traits, ResolvedRef, CompatReason, Formats   <- the seam
  avro.rs             the Avro language: parse, stored text, can_read
    avro/schema.rs    the parser: an arena of named types, Confluent's checks
    avro/text.rs      the stored text (Avro Java's toString order), defined names
    avro/compat.rs    Avro's reader/writer rules, findings with paths, Confluent's sentences
    avro/inline.rs    the resolved view (referenced types written out)
  registry.rs         Registry: state behind one Mutex, open from Storage, fingerprint, subjects
    registry/prepare.rs   resolve references (each node once), prepare a submitted schema, parse a stored one
    registry/write.rs     register, lookup
    registry/read.rs      versions, subjects, schemas by id, referencedby; VersionRef
    registry/delete.rs    soft and permanent deletes, orphaned schemas
    registry/settings.rs  Level, Mode; get, set and delete for a subject and globally
    registry/check.rs     the one compatibility engine (register's 409, the API's check, the console's check)
    registry/doze.rs      overview, version metadata, resolved view (the /_doze/ answers)
  store.rs            Storage trait, MemoryStorage, MetaStorage (kafka-meta), typed keys, versioned postcard values
  api.rs              Api::serve(Request) -> Response: host guard, sign-in and its back-off, body type, route, limits, log
    api/message.rs    Request, Body, Response, Options
    api/route.rs      the route table (the ledger) and the matcher
    api/confluent.rs  Confluent's routes
    api/extension.rs  /_doze/ routes
    api/log.rs        the request ring
    api/text.rs       Go's reading of paths, queries, booleans, basic credentials
  server.rs           hyper HTTP/1 over TCP or TLS; a request read whole up to its limit; answered on a blocking thread
  wire.rs             the magic byte and id; Protobuf message indexes
crates/kafka-meta/src/store/schemas.rs   the `schemas` table: opaque records, one transaction per operation
crates/doze-kafka/src/cli/registry.rs    the three flags and their rule
crates/doze-kafka/src/registry.rs        bind before anything is created; serve over the broker's MetaStore; stop first
```

- **The API without the network.** `Api::serve` takes a `Request` (method, raw path, query, the four headers it reads, the peer, the
  body or "too large") and gives a `Response`; the server only reads and writes bytes. The tests answer requests as the server does,
  with no socket, as the Go tests did with `httptest`; the conformance replay plays 469 steps in under a second.
- **State.** One `Mutex<State>` held for a whole operation, parsing and the store write included (Go's rule: two registrations of one
  schema must not both take an id). It is a std mutex, never held across an await: the server runs each answer on a blocking thread.
  Memory changes only after the store has committed.
- **Storage in `kafka-meta`.** Go keeps the registry in the broker's bbolt database through an opaque `Storage` interface; this does
  the same with a `schemas` table in `meta.redb` (`registry_records`, `registry_apply`), so `kafka-meta` knows nothing of schemas and
  the registry is crash-tested on the same simulated disk. The keys are typed (`store::Key`); the values carry a version byte, so a
  later format is refused by name rather than misread.
- **The seam.** The registry calls a language only through `SchemaFormat::parse` and `ParsedSchema::{canonical, can_read}`; the type
  checks (SCHEMA_TYPE_CHANGED), the level logic, the 409's and the verbose check's wording are the registry's.

## Extension points (for 10b and 10c, which can run in parallel)

A language is one module and one line:

1. `src/protobuf.rs` (+ `src/protobuf/*.rs`) or `src/jsonschema.rs` implementing `format::SchemaFormat` (its `kind()`, `parse(text,
   refs)`; `bounded_depth()` false for Protobuf, whose comparison is linear in depth) and `format::ParsedSchema` (`canonical(normalize)`,
   `can_read(writer, reader_is, writer_is)` returning `CompatReason`s whose `text` is Confluent's sentence, `as_any`).
2. Add it to `Formats::all()` in `format.rs`. `GET /schemas/types` then lists it, in Confluent's order.
3. References: `ResolvedRef` carries `name`, `subject`, `version`, `kind`, `text`, `direct`, and `node` / `names` (which schema each
   name reaches, for JSON Schema's `$ref` resolution by node); dependencies come before dependents.
4. Tests: copy `testdata/compat/{json,protobuf}.json` and `*-reasons.json` from doze-kafka into `tests/data/compat` and add the type to
   `KINDS` in `tests/corpus.rs`; remove the language's entries from `PENDING` and `PENDING_SCENARIOS` in `tests/conformance.rs` (the
   recordings are all there already; `tests/data/protobuf/protodesc.json` is 10b's descriptor fixture). Go's `corpusDifferences` and
   `corpusReasonDifferences` for the language become deviations in `behaviours/registry.md`.
5. Fuzz: a `// fuzz:parser <target>` marker on the parser and a target in `crates/kafka-registry/fuzz` (one more `[[bin]]`, and the CI
   loop's list).
6. Weight: measure with `cargo xtask weight doze-kafka` and the TLS build before and after; write the numbers here.

What 10b and 10c should not need to touch: `registry/*`, `api/*`, `store.rs`, `server.rs`, the binary. The only shared files are
`format.rs` (one line each) and the two test lists, so the two slices merge with at most trivial conflicts.

## Slices

| Slice | What | State |
|---|---|---|
| 10a | registry model, storage, REST API (Confluent's routes and `/_doze/`), host guard, basic auth, TLS, the wire header, Avro, the flags; ports of the Go tests; fuzz targets; the catalogue | **built** |
| 10b | Protobuf: `.proto` text and `FileDescriptorProto` input, Confluent's printing, compatibility, message indexes; the protobuf conformance scripts and corpus | to do (another agent) |
| 10c | JSON Schema: parsing and validity of the schema, Confluent's structural compatibility, `$ref` across references; the JSON scripts and corpus | **built** (branch `p10c-jsonschema`), not yet audited |
| 10d | the record-value codec for the console (Avro, Protobuf, JSON payload to JSON and back, with the guard against hostile Avro bytes of Go's `avroguard.go`) and the console's registry pages | to do |

## Tests (10a)

- `tests/conformance.rs`: the 30 recorded scripts, 469 steps; every Avro step passes; 24 steps skipped as Go's own deviations
  (REG-D1 to D7); two JSON steps of `canonical-forms` and four whole scripts (`nested-references`, `other-schema-types`,
  `protobuf-canonical-forms`, `protobuf-descriptor-with-import`) wait for 10b and 10c. (The forty-link Avro chain of
  `nested-references` sits behind its JSON steps, whose ids it depends on; it has a test of its own, REG-15.)
- `tests/corpus.rs`: the 392 Avro pairs at three levels, verdicts and reasons; check equals register at four levels.
- Unit tests ported from `registry_test.go`, `bounds_test.go`, `limits_exact_test.go`, `hostguard_test.go`, `api_log_test.go`,
  `doze_ext_test.go`, `coverage_stage5_test.go`, plus the server over TCP, the registry over `MetaStore` on the simulated disk, and the
  flags. Not ported: the codec and guard tests (10d), the JSON and Protobuf tests (10b, 10c), `TestOldCanonicalFormsAreStillFound`
  (REG-D12), the `.doze` host cases (REG-D8).

## Slice 10c: JSON Schema

`src/jsonschema.rs` and `src/jsonschema/{doc,check,node,compat,number}.rs`, about 1,900 lines with tests, behind the one line in
`Formats::all()`. What it does is REG-30 to REG-34; where it departs from Confluent or Go, REG-D16 to REG-D20.

- **doc.rs**: the schema and each schema its references reach as documents with their own names (`ResolvedRef.node` and `names`),
  each with its draft (`$schema`, draft 7 by default); `$ref` resolution by name, JSON pointer and anchor.
- **check.rs**: validity without a validator library: each draft's meta-schema as a table of keyword shapes, checked through every
  subschema; references followed where the schema uses them; a syntax scan for patterns.
- **node.rs**: the form Confluent compares (its library's classes: string, number, object, combined, not, conditional, enum, const),
  in an arena shared by references, bounded in depth, height and size.
- **compat.rs**: `jsoncompat.go` (Confluent's `SchemaDiff`) rewritten over the arena. Each pair of subschemas is asked once
  (changed? broken?) and remembered, so pairing combined branches and diamonds of references are polynomial; the reasons come from
  one walk down the broken pairs. Go's comparison was exponential in both.
- **number.rs**: exact decimals for limits and `multipleOf`.

Tests: the corpus (467 pairs; 465 compared at three levels with every reason word for word, the two `exclusiveMaximum` pairs are
REG-D16), the conformance scripts with every JSON step (`nested-references` whole, the JSON steps of `canonical-forms` and
`other-schema-types`, which waits for 10b only for its Protobuf steps), ten unit and property tests in `src/jsonschema/tests.rs`,
and the `json_schema` fuzz target (about a million runs locally, none failing).

Not done, with reason: instance validation (a payload against a schema) is the codec's, 10d; `$id` as a base URI for references and
`format` are not used by Confluent's compatibility and the corpus has none (REG-D20); patterns are scanned, not compiled (REG-D17).

## Risks

- **Protobuf fidelity** stays the largest (PLAN.md): Confluent's printing and compatibility have many corners, and the corpus is the
  only oracle.
- **Size headroom.** 0.2 MB of the CI byte limits are left after 10a on the Linux estimate; 10b and 10c must be measured before
  merging.
- **The Linux size is an estimate** (the darwin size times 1.216); the first CI run measures it.
- **TLS on the registry has no automated test**: it was checked by hand on 2026-11-14 (a release `--features tls` broker with a
  self-signed certificate: a registration over HTTPS answered `{"id":1}`, plain HTTP to the same port got no answer).
- **No independent audit yet.** The registry's own judgement of its fidelity rests on the oracle files; an audit should read the
  recordings against the catalogue.
