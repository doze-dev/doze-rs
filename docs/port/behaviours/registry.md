# Schema registry (Phase 10)

**Gaps closed 2026-11-19** (branch `kafka-registry-gaps`, REG-46 to REG-50): data contracts, Protobuf normalization and JSON
Schema numbers past Java's `BigDecimal`, each held to cp-schema-registry 7.7.1 and 8.3.2 by `tools/registry-diff` (`gaps.ts`;
results in `tools/registry-diff/results/2026-11-19/`); REG-D26 and REG-D28 are retired, REG-D29 to REG-D32 say what still
differs and why.

**Audited 2026-11-18** (branch `kafka-registry-audit`) against cp-schema-registry 7.7.1 and 8.3.2 live, with real Kafka, by
`tools/registry-diff` (README there): every route answer compared byte for byte, a generated corpus of schema evolutions
in all three languages at all seven levels, Confluent's own Java serializers and console tools end to end, and restarts and
kills. The rules below that the audit changed say so and name the test that holds them; where a rule was written from the
Go edition's recordings and the live registry answered otherwise, the live registry won.

Slice 10a is built (2026-11-14, branch `p10-registry`): the registry's model, its storage in the broker's metadata database,
every REST route the Go edition serves (Confluent's and doze's own under `/_doze/`), the host guard, basic authentication and TLS
as the Go binary has them, the wire header, and the Avro language whole. Slice 10b (2026-11-14, branch `p10b-protobuf`) adds
Protobuf (REG-28 to REG-32) and slice 10c (2026-11-14, branch `p10c-jsonschema`) JSON Schema (REG-33 to REG-37), each through
`format::Formats` (`phase10.md`, "Extension points"). This file is the catalogue: what Confluent's registry does, as the Go edition
recorded it against cp-schema-registry 7.7.1, and the test that holds each rule. The sources of truth are the recordings copied
from doze-kafka: `crates/kafka-registry/tests/data/conformance/*.json` (30 scripts, 469 steps) and the compatibility corpus
`tests/data/compat/avro.json`, `protobuf.json` and `json.json` (392, 600 and 467 pairs with Confluent's verdict at BACKWARD,
FORWARD and FULL) with `avro-reasons.json`, `protobuf-reasons.json` and `json-reasons.json` (Confluent's reasons, word for word,
for every refusal). Nothing here is written from memory of what Confluent does.

The numbers are checked by `tools/check-registry-catalogue.ts` (`bun tools/check-registry-catalogue.ts`; a CI step): no REG-n or
REG-Dn is defined twice, none is referenced without a definition, every deviation is in the index, and every test name quoted here
is a `fn` in the Rust sources.

## Index

Deviations: where this registry answers otherwise than Confluent's, or than the Go edition, on purpose.

- **REG-D1** retired: IMPORT and READONLY_OVERRIDE are offered since the 2026-11-18 audit (REG-45).
- **REG-D2** a subject of up to 16 KiB is taken (Confluent's HTTP server refuses a path past 8 KiB; as in Go).
- **REG-D3** contexts are not offered: `:.ctx:name` and `:*:` are subjects of those names; `GET /contexts` is `["."]`.
- **REG-D4** bodies past their limit are 413 (as in Go).
- **REG-D5** `/v1/metadata/*` and the other routes listed as not served are 404 (as in Go).
- **REG-D6** where Confluent answers 500, a proper answer (as in Go).
- **REG-D7** a refused PUT or DELETE carries the JSON error (as in Go).
- **REG-D8** no special case for `.doze` names (Rust edition).
- **REG-D9** stored text uses Jackson's escapes, not Go's HTML escapes (Rust edition).
- **REG-D10** a busy default address leaves the broker running without a registry (Rust edition).
- **REG-D11** Avro names may use any letter, as Java's parser allows (Rust edition).
- **REG-D12** the store is the broker's redb database; records the Go edition stored are not read (Rust edition).
- **REG-D13** retired: every language is read since 10b and 10c (it said which one was not yet).
- **REG-D14** the record-value codec came in 10d (REG-38 to REG-41), not 10a.
- **REG-D15** Protobuf: the language rules checked are protoc's main ones; editions are refused (Rust edition).
- **REG-D16** Protobuf: declarations the Go edition's printer dropped are kept in the stored text (Rust edition).
- **REG-D17** Protobuf: a descriptor importing a file no reference gives is refused (Rust edition).
- **REG-D18** a lowered exclusiveMaximum is compared, where Confluent answers 500 (as in Go).
- **REG-D19** a JSON Schema pattern is checked for the syntax errors Java refuses, not compiled (Rust edition).
- **REG-D20** JSON Schema references, repeated keys, multiples and enum-with-type as Confluent, where Go was not (Rust edition).
- **REG-D21** a JSON Schema comparison is bounded: reasons, subschemas, height (Rust edition).
- **REG-D22** JSON Schema: `normalize` changes nothing, `format` is ignored, `$id` is not a reference base (as in Go).
- **REG-D23** the codec shows an Avro logical type as its underlying type (Rust edition).
- **REG-D24** a JSON Schema value is not checked against `pattern` or `format` (Rust edition).
- **REG-D25** Protobuf well-known types read and write as plain messages, not protojson's special forms (Rust edition).
- **REG-D26** retired: data contracts are kept since 2026-11-19 (REG-47 to REG-49).
- **REG-D27** a refusal's details (the parser's own words after `details:`) are doze's, naming the place (audit).
- **REG-D28** retired: Protobuf is normalized as Confluent normalizes it since 2026-11-19 (REG-46).
- **REG-D29** JSON Schema keywords are checked against the draft's meta-schema where Confluent's loaders take more (gaps).
- **REG-D30** rule sets are kept and checked as Confluent Enterprise and Cloud do; the community image drops them (gaps).
- **REG-D31** where 7.7.1 and 8.x differ on data contracts and normalizing, 8.x is followed (gaps).
- **REG-D32** Protobuf normalization: the edges where doze's text is not Confluent's (gaps).

## The API

### REG-1. One table of routes; misses answer in the registry's JSON
Every route is one row of `api/route.rs` (`ROUTES`): Confluent's 31 (`GET /contexts` among them, REG-D3; `GET
/subjects/{s}/metadata` since REG-48) and doze's five under
`/_doze/`. A path that matches a route under another method is 405, any other path 404, both as
`{"error_code":404,"message":"HTTP 404 Not Found"}` with no `Content-Type` (Confluent's framework answers those itself). HEAD is
answered as GET; OPTIONS lists the path's methods in Jersey's order (`HEAD, POST, GET, OPTIONS`, `text/plain`, and in `Allow`). A
wildcard is one path segment, unescaped on its own (`%2F` stays in the subject); empty segments are dropped, as Jetty merges `//`
and ignores a trailing `/`. A malformed escape, bytes that are not UTF-8 or an escaped NUL is Jetty's own 400 page
(`<h1>Bad Message 400</h1><pre>reason: Bad Request</pre>`, `text/html;charset=iso-8859-1`). `Accept` chooses the answer's type
among `application/vnd.schemaregistry.v1+json` (preferred), `application/vnd.schemaregistry+json` and `application/json`; one that
accepts none is 406. A body is Confluent's compact JSON with no line end after it (audit: the Go edition's trailing newline, on
every answer, was the first byte-level difference).
Tests: `routes_match_by_segment_and_method`, `bodies_of_other_types_are_refused_and_the_mux_answers_in_json`,
`segments_are_unescaped_one_by_one`, `a_registration_over_tcp_is_answered_in_the_registrys_type`,
`the_frameworks_own_answers_are_confluents`.

### REG-2. Every recorded script answers as Confluent did
The 30 conformance scripts recorded from Confluent are replayed (ids as offsets from a probed base, as the Go harness does) and
every step must give Confluent's status, challenge, error code and message (an "Invalid schema" message is compared by its start;
a parser's own words after ", details:" are not compared), and anything else whole. The steps that differ on purpose are listed
with their deviation (REG-D1 to REG-D7); nothing waits for a language. Every other step, in all three languages, passes:
Avro, Protobuf (`protobuf-canonical-forms`, `protobuf-descriptor-with-import`), JSON Schema (`nested-references`, the JSON steps of
`canonical-forms`), and `other-schema-types`, which registers all three side by side.
Test: `every_recorded_scenario_answers_as_confluent_did`.

### REG-3. The host is judged first, then the credentials
A request whose `Host` is not an IP literal, `localhost`, or a name the operator listed (`--console-allow-host`, the advertised host,
the name in `--schema-registry-addr`; `*` allows any) is 403 with the registry's error shape, naming the host and the flag that
allows it, before the credentials and the route are looked at: the refusal is the same whatever the credentials, and changes
nothing. (The guard is `kafka-auth`'s, shared with the console.)
Tests: `a_request_for_a_foreign_host_is_refused_and_changes_nothing`, `the_registrys_own_hosts_are_served_and_lookalikes_are_not`,
`the_host_is_judged_before_the_credentials`.

### REG-4. Basic authentication with the broker's SASL users
With SASL on, every route (the `/_doze/` ones too) needs HTTP basic credentials of a user configured at start, checked as a
PLAIN login is (constant time). A refusal is 401 with `WWW-Authenticate: Basic realm="doze-kafka schema registry"`. Five failures
in a row from one address are answered at once; each after waits 100 ms longer, up to 2 s; a success clears the count; 4,096
addresses are remembered. The wait is served by the connection, not a thread.
Tests: `repeated_sign_in_failures_are_slowed_and_a_success_clears_them`, `without_credentials_the_answer_is_a_challenge`,
`the_extension_routes_ask_for_the_same_credentials`, `basic_credentials_are_decoded_strictly`.

### REG-5. Body types and limits
A POST or PUT body is read when it has no type, JSON, the registry's types or `application/octet-stream`; `text/plain`, forms and
multipart are 415 (a web page cannot register schemas). A schema body is read up to 20 MiB, a level or mode body up to 64 KiB; past
that it is 413 and not kept (REG-D4). Every body is read as Jackson binds it to Confluent's request class (`api/body.rs`, audit):
no body or `null` is 422 `register.arg4 must not be null (was null)` (each route its own argument name); text that is not JSON is
400 `Unrecognized token 'not': was expecting ...`; anything after the value is 400 `Trailing token (of type START_OBJECT) found
after value ...`; an array or a string is 400 `Cannot deserialize value of type \`RegisterSchemaRequest\` ...`; a key written twice
keeps its last value; a number or boolean where a string is wanted is its text (`"schema":5` is the schema `5`); `"normalize":"yes"`
is 400. The Go edition's 422 42207 for a setting body that is not JSON, and its reading of only the first value, were wrong.
Tests: `bodies_of_other_types_are_refused_and_the_mux_answers_in_json`, `bodies_are_bounded_by_what_the_route_reads`,
`bodies_are_read_as_jackson_reads_them`, `settings_bodies_are_read_as_jackson_reads_them`.

### REG-6. Query parameters as Confluent's framework reads them
`deleted`, `deletedOnly`, `permanent`, `normalize`, `verbose`, `latestOnly`, `fetchMaxId`, `defaultToGlobal`, `force` are Java
booleans: `true` in any case, anything else (`1` and `t` too) false (audit: the Go edition read Go's booleans); a value is the first
of its name; `+` is a space; `subjectPrefix=:*:` is every subject. `offset` and `limit` cut `GET /schemas` only: missing or negative
is 0 and no limit; a value that is not a 32-bit integer (`x`, `+1`, `1.0`) is 404, as Jersey answers it. `deletedOnly` lists only a
subject's soft-deleted versions (audit: it was ignored and listed the live ones).
Tests: `queries_read_as_javas_framework_reads_them`, `a_page_is_cut_only_by_a_positive_limit`.

### REG-7. The request log
The newest 2,000 requests (method, path with its query cut to 512 bytes, status, time taken, user, address, agent, the subject or id
the path names) are kept in memory for the console, never a body; requests marked `X-Doze-Own` and the `/_doze/` routes are not.
`GET /_doze/requests?subject=&before=&limit=` pages them newest first; a request by id counts for each subject holding that id.
Tests: `the_request_log_keeps_clients_requests_not_the_consoles_and_pages_by_subject`,
`the_ring_holds_exactly_its_bound_and_pages_newest_first`, `a_request_by_id_counts_for_the_subjects_holding_it`.

## The model

### REG-8. Ids are global, stable and never given twice; everything survives a restart
A schema (type, stored text, references) has one id wherever it is registered; a new one takes the next id, and an id freed by a
permanent delete is not given again (the next id is stored). Subjects, versions (soft-deleted ones too), levels, modes and who
registered what survive a restart of the store.
Tests: `everything_survives_a_restart_and_a_spent_id_is_not_given_again`, `the_registry_survives_a_restart_of_the_metadata_database`,
`records_written_together_are_read_back_after_a_reopen_and_a_delete_goes`, `who_registered_a_version_and_when_survive_a_restart`.

### REG-9. A write the store refuses changes nothing
Each operation's records are written in one transaction before memory changes; a refused write is 500 50001 and the registry is as
it was (no id spent, no version, no level).
Test: `a_failed_write_changes_nothing`.

### REG-10. One lock for an operation
One schema registered from many threads at once gets one id and one version: the whole operation, parsing included, holds the lock.
Test: `one_schema_registered_at_once_from_many_threads_gets_one_id_and_one_version`.

### REG-11. Subjects
A subject is refused (422 42208) when it holds a control character or is `__GLOBAL`, or is longer than 16 KiB (REG-D2), and the
refusal does not repeat a long subject; an empty one is 42207. Anything else is a subject: spaces, `/`, `:`, any script.
Test: `subjects_are_bounded_exactly_at_the_limit_and_control_characters_are_refused`.

### REG-12. Nesting is bounded before any parser sees the text
The text of an Avro (or JSON Schema) schema nested deeper than 256 levels is refused as invalid without being parsed or echoed;
exactly 256 is read; brackets inside strings do not count.
Tests: `nesting_is_bounded_and_answered_quickly`, `depth_counts_nesting_outside_strings`.

### REG-13. The level chooses the versions compared
NONE compares nothing; BACKWARD, FORWARD and FULL the latest live version; the transitive levels every live version, newest first,
and Confluent's answer (a 409, a verbose check) names only the first version that does not fit (audit: the Go edition compared
oldest first and listed every version). FORWARD asks the old schema to read the new, BACKWARD the new to read the old, FULL both
(forward reasons first); at FULL only the backward reasons are followed by `{oldSchemaVersion: n}` and `{oldSchema: '...'}`, so a
FULL refusal with forward reasons alone names no version. An old schema longer than 10,240 UTF-16 units is quoted as
`{oldSchema: <truncated> '<its first 10,240>...'}`. A stored version that cannot be read again (its reference gone) refuses the
check as Confluent does, quoting that version as an invalid schema. A verbose check of a schema that is not one is
`{"is_compatible":false,"messages":["Invalid schema ..."]}`; without `verbose` it is the 422. doze's own check lists every finding.
Tests: `the_level_chooses_the_versions_compared`, `a_long_old_schema_is_cut_as_confluent_cuts_it`.

### REG-14. A check and a registration are one engine
For every corpus pair and level, a check says compatible exactly when the registration succeeds, and gives the reasons the 409
gives, word for word.
Test: `a_check_agrees_with_a_registration_on_every_pair`.

### REG-15. References
A reference names a version, a soft-deleted one too, or `-1` for the latest live one, stored as the number it named (audit: both
were refused); one without a subject or version is 42201 `Invalid reference: {name='x', subject='null', version=null}`, one
that names nothing `No schema reference found for subject "s" and version n`. It is followed once per (subject, version) however many paths reach it (a diamond of 26 levels
is resolved in its size), at most 1,000 distinct schemas; a chain of 40 registers. A version another live schema references
cannot be deleted (422 42206), nor its subject.
Tests: `a_diamond_of_references_is_resolved_once_per_schema`,
`a_chain_of_forty_references_registers_and_a_referenced_version_cannot_go`.

### REG-16. Deleting
Soft first, permanent only after (40405, 40407 otherwise; 40404, 40406 for a second soft delete). A subject's own level and mode
go with its soft delete. Registering a schema the subject had soft-deleted makes it a new, latest version, and the deleted one goes
for good. A read-only subject refuses deletes and level changes (42205) and answers a schema it already holds with its id. A
registration may name its `version`: outside IMPORT one that is not the next is 42201 `Version is not one more than previous
version`; an `id` outside IMPORT is 42205 `Subject s is not in import mode` (REG-45).
Tests: `registering_a_soft_deleted_schema_again_makes_it_the_latest_and_drops_the_old_version`,
`every_recorded_scenario_answers_as_confluent_did`.

### REG-17. A record that cannot be read stops the open, and is named
The registry refuses to open over a record it cannot read and says which; the broker then runs without it and changes nothing in
the database.
Test: `a_record_that_cannot_be_read_stops_the_open_and_is_named`.

### REG-18. The stored records
Keys are a tag and the parts (`s` id, `v` subject NUL version, `c` subject, `m` subject, `n`); values are a version byte and
postcard. A key or a value this version does not know is refused, not guessed at.
Tests: `every_key_reads_back_as_itself_and_strangers_are_none`, `a_value_of_another_format_or_none_is_refused`.

## Avro

### REG-19. Every recorded verdict and reason is Confluent's
All 392 pairs of the corpus (92 by hand, 300 generated) give Confluent's verdict at BACKWARD, FORWARD and FULL ("invalid" where it
refused a schema), and every refusal (over a hundred) gives Confluent's reasons word for word: Avro's own rules
(`SchemaCompatibility`), each a typed finding with the JSON pointer into the reader schema, named types compared once per pair (so
recursion ends), a writer union's branches each a question of its own.
Tests: `every_recorded_verdict_and_reason_is_confluents`, `a_recursive_type_compares_with_itself_and_stops`.

### REG-20. What Confluent's parser refuses is refused, and nothing else
Undefined names, a missing type, name, fields, symbols, items, values or size; an illegal name, field name or symbol (a namespace
is not checked: 7.7.1 takes `a..b` and `a.1b`, audit); a
type named after a primitive; a redefined name; duplicate fields or symbols; an enum default not among its symbols; a nested union
or a type twice in a union; an illegal field order; and, the one default that is read, a string given to a `float` or `double`,
read as Java's `Double.parseDouble` (`empty String`, `For input string: "x"`); a string given to an `int` or `long` is taken
(audit: the Go edition refused `""` for every number). Defaults are otherwise not checked against their types, and an invalid
logical type is ignored, as Confluent does. The refusals are worded as Avro's Java parser words them (`No field type:
{"name":"a"}`, `Record has no fields: {...}`, `Illegal initial character: 1R`, `"Nope" is not a defined name. The type of the "a"
field must be ...`, `Schema not yet supported: 5`, `dangling content after end of schema: trailing`).
Tests: `what_confluents_parser_refuses_is_refused`, `what_confluent_accepts_is_accepted`.

### REG-21. The stored text is Avro's own `toString`
Not the Parsing Canonical Form: docs, defaults, aliases and other attributes are kept, in Avro's order (type, name, namespace, doc,
the type's own, the rest as they came, or sorted with `?normalize=true`); a namespace is written only where it changes; a reference
is written as short as the namespace allows; an unnamed type with only Avro's attributes is its bare name. The same schema written
two ways is one schema with one id.
Tests: `the_stored_text_is_avros_own_order_and_namespaces_are_written_once`, `normalizing_sorts_the_other_attributes`,
`a_referenced_type_is_known_by_its_name_and_written_short`, `order_numbers_and_escapes_survive_a_round_trip`.

### REG-22. The resolved view
`GET /_doze/subjects/{s}/versions/{v}/resolved` gives an Avro schema with each referenced type written out where it is first used,
with its full name, and named after that; it parses on its own. A schema with no references is not "inlined".
Tests: `the_resolved_view_writes_each_referenced_type_out_once`, `a_schema_without_references_is_not_inlined`.

## Protobuf (slice 10b)

Written here, with no Protobuf crate (`src/protobuf.rs` and `src/protobuf/`): a lexer and parser of proto2 and proto3 text, a
linker (imports, Protobuf's scoping, maps and groups made messages, the language's rules), Confluent's printing, Confluent's
compatibility rules, and a decoder of base64 `FileDescriptorProto`s. The oracle is the Go edition's `protobuf.go`, `protoprint.go`,
`protocompat.go` and `protofromdesc.go`, and the recordings: `tests/data/compat/protobuf.json` (600 pairs: 99 by hand, 300
generated, with Confluent's verdict at three levels) and `protobuf-reasons.json`.

### REG-28. Every recorded Protobuf verdict and reason is Confluent's
All 600 pairs give Confluent's verdict at BACKWARD, FORWARD and FULL ("invalid" where it refused a schema, 103 of them), and every
refusal gives Confluent's reasons word for word, in its order. Messages are matched by name (nested ones by their path, `#/O/I`),
fields by number; a change of package, a removed message, a field whose wire type family, message type, map-ness or (for numbers,
where the syntax writes labels) label changed, a required field added or removed, a oneof that lost a member, and fields moved
into a oneof break readers; enums and services are not compared. The Go edition differed on one pair ("random 045"): a field moved
into a oneof that also lost a member. Confluent reports only the lost member, and so does this registry (the rule fits "random
161" too). The order is Confluent's (audit): messages level by level, each compared and then the ones nested in it, the
messages at a level in the order Confluent's Java `HashMap` of names gives them (a changed field of `Order` before a removed
`Line`); a message's fields by number across both schemas; a field that joins or leaves a oneof is not compared with itself
(uint32 to bytes moved into a new oneof is compatible, as Confluent keeps a message's fields and its oneofs' apart).
Tests: `every_recorded_verdict_and_reason_is_confluents`, `a_check_agrees_with_a_registration_on_every_pair`.

### REG-29. What protoc refuses is refused
A syntax error says where, as Confluent words it (`Could not parse Protobuf - Syntax error in :1:28: ...`). The linker refuses an
unknown or unseen type (a type of a file imported by an import is seen only through `import public`), names resolved from the
innermost scope that has their first part (as protoc: no looking outward after a miss), a name defined twice (enum values are
siblings of their enum), field numbers out of range, in 19000 to 19999, reused, reserved or in an extension range, reserved names,
a missing label in proto2, `required`, defaults, groups and extension ranges in proto3, a proto2 enum used in proto3, map keys that
are not integers, bools or strings, labels in a oneof, an empty oneof or enum, a proto3 enum with no value numbered zero (protoc wants it
first; Confluent's normalized text sorts values by number, so a negative one may come first, gaps), aliases without
`allow_alias` (and `allow_alias` without aliases), conflicting JSON names in proto3, `packed` on what cannot be packed, a default
that does not fit its field, built-in options by name and value type, and custom options naming no extension. Nesting past 100
levels is refused, so hostile text cannot exhaust the stack.
Tests: `what_the_language_refuses_is_refused`, `names_resolve_from_the_innermost_scope`, `deep_nesting_is_refused_not_overflowed`.

### REG-30. The stored text is Confluent's printing
Syntax and package; the imports; the file options; every message, then every enum; extends; services. In a message: reserved,
options, fields, oneofs, groups, nested messages then enums, extends, extension ranges, each section after a blank line (padded to
the indent in a nested message); a field's options inline when one, one to a line when several (its own, then `default`, then
`json_name`); comments dropped; tokens as written with white space collapsed. What is stored reads back as itself.
`?normalize=true` is REG-46. A missing import or type is `Could not resolve type: shop.Money` with no
position, and `required` in proto3 is Confluent's syntax error (`'required' label forbidden in proto3 field declarations`); `packed =
false` is taken on any field, as protoc takes it (audit).
Tests: `every_recorded_scenario_answers_as_confluent_did` (protobuf-canonical-forms), `what_the_language_allows_is_read_and_stored`,
`the_stored_text_of_every_corpus_schema_reads_back_as_itself`.

### REG-31. Imports and descriptors
An import is found among the references by their names, or is a well-known type (`google/protobuf/{any, api, descriptor, duration,
empty, field_mask, source_context, struct, timestamp, type, wrappers}.proto`), which need no reference. Text that is not `.proto`
is read as a base64 `FileDescriptorProto` (confluent-kafka's serializers send that), printed as Confluent prints a descriptor and
stored like any other schema; a type it cannot resolve is `Could not resolve type: .dep.Addr`, as Confluent says.
Tests: `imports_are_found_by_reference_name_and_public_imports_pass_through`, `a_descriptor_is_stored_as_confluent_stores_it`,
`garbage_is_not_a_descriptor`, `a_schema_with_an_import_registers_and_is_compared`.

### REG-32. Message indexes name messages
The indexes of a value's header name a message as Confluent's serializers number them: `[0]` the first message of the file,
`[1, 0]` the first message nested in the second (for the codec, 10d).
Test: `message_indexes_name_messages_as_confluents_serializers_number_them`.

## JSON Schema (slice 10c)

### REG-33. Every recorded JSON verdict and reason is Confluent's
465 of the 467 corpus pairs (171 by hand, 300 generated, less the two of REG-D18) give Confluent's verdict at BACKWARD, FORWARD
and FULL, and every refusal gives Confluent's reasons word for word (`{errorType:"…", description:"…'}`, Confluent's own closing
quote). The rules are Confluent's `SchemaDiff`, as the Go edition's `jsoncompat.go` recorded them: a structural walk naming each
difference, a fixed set of them compatible; open, closed and partially open content models for properties and tuple items;
required with and without defaults; number limits, integer to number; enums and const; allOf, anyOf and oneOf branches paired
identical first, then compatible; not; if/then/else; dependencies; properties in the order a Java `HashMap` gives them. A type with
an enum or const beside it is two schemas combined, as Confluent's library reads it (so "enum with type" gives Confluent's
COMBINED_TYPE_SUBSCHEMAS_CHANGED, which the Go edition listed as a difference).
Tests: `every_recorded_verdict_and_reason_is_confluents`, `additional_items_and_dependencies_are_named_as_confluent_names_them`,
`numbers_compare_by_value_and_multiples_are_exact`.

### REG-34. What a schema's draft refuses is refused
Drafts 4, 6, 7, 2019-09 and 2020-12, named by `$schema` (http or https, an empty fragment allowed; draft 7 when none, Confluent's
default, and for a `$schema` that names no draft: 7.7.1 takes `http://example.com/nope`, audit). Each keyword the draft has is checked against the draft's meta-schema (a type that is not
one, a count that is not a non-negative integer, a `multipleOf` of 0, a repeated `required` name, an empty or repeated `enum`
before 2019-09, an empty `allOf`, a boolean schema in draft 4, an array `items` in 2020-12, ...), and every `$ref` the schema uses
must reach something; a reference inside an unused definition is not followed, as Go's library does not compile it.
Numbers, counts and `multipleOf` are read as Confluent's Jackson and loaders read them (REG-50); where its loaders take what the
meta-schema refuses, this registry refuses it (REG-D29). Keywords a
draft does not have are annotations. The refusal is 42201 and names the place (`#/properties/a/type: "nonsense" is not a type`).
Tests: `what_is_not_a_schema_of_its_draft_is_refused`, `what_its_draft_allows_is_accepted`.

### REG-35. The stored text is the schema compacted
Spaces go, keys keep their order, an integer keeps its text and a number with a fraction or exponent is written as Java's
`BigDecimal` writes it (`1e2` is `1E+2`, `1.50` stays; audit; one a `BigDecimal` cannot hold as REG-50 says), strings take Jackson's escapes (REG-D9), and a key written twice is
kept once with its last value where it first was, as Jackson reads it. With `?normalize=true` every object's keys are sorted, as
Confluent normalizes (audit: the Go edition changed nothing). The same schema spaced two ways is one schema with one id
(recorded: canonical-forms).
Tests: `the_stored_text_is_compact_and_a_repeated_key_is_kept_once`, `numbers_are_written_as_javas_big_decimal_writes_them`.

### REG-36. References
A `$ref` names one of the schema's references (all of it, or a fragment of it: `address.json#/definitions/city`) or a fragment of
the document it is written in: a JSON pointer (through objects and arrays, `~0`, `~1` and `%XX` escapes) or an anchor (`$anchor`,
or an `$id` of `#name` before 2019-09). Each referenced schema is its own document with its own names, so `x.json` in a schema
and in a schema it references can be two schemas (recorded: nested-references). A reference back into a schema being read compares
as "anything" at the point of recursion.
Tests: `a_name_means_what_the_schema_using_it_says`, `an_address_changed_to_a_number_is_breaking_through_a_reference`,
`every_recorded_scenario_answers_as_confluent_did`.

### REG-37. A comparison costs the pairs there are, not the paths
Every comparison of two subschemas is asked once (whether it changes anything, whether it breaks anything) and remembered, so a
diamond of 26 levels of references (2^26 paths) and the pairing of combined branches cost their pairs; the reasons are listed by
one walk that goes only where something breaks, at most 1,000 of them (REG-D21). Any schema reads its own data, and its stored
text reads back as itself.
Tests: `a_diamond_of_references_compares_in_its_size_and_its_reasons_are_bounded`,
`a_recursive_schema_compares_with_itself_and_stops`, `any_schema_reads_its_own_data_and_its_stored_text_reads_back_the_same`.

## doze's own routes

### REG-23. The overview and a version's history
`GET /_doze/overview` is every subject (soft-deleted ones too) with its level and mode and whether they are its own, the ids in
use and the live ones as runs, and the next id; `GET /_doze/subjects/{s}/versions/{v}` is a version with who registered it, from
where, with what agent (cut to 256 bytes) and when (RFC 3339 UTC).
Tests: `the_overview_gives_ids_as_runs_and_a_soft_deleted_id_still_resolves`, `version_metadata_is_answered_over_http`,
`times_are_utc_with_milliseconds_when_there_are_some`, `ids_are_given_as_runs`.

### REG-24. A check at any level
`POST /_doze/subjects/{s}/check?level=&version=` checks at the level asked (the subject's when none), leaves the subject as it is,
and takes each reason apart (type, path, description, info).
Test: `a_check_at_another_level_leaves_the_subjects_and_takes_its_reasons_apart`.

## The wire, the binary, the parsers

### REG-25. The record-value header
Byte 0 is the magic 0, bytes 1 to 4 the id big-endian; a Protobuf value then names its message by a zig-zag varint count and
indexes (a count of 0 is `[0]`, written as a single 0), at most 128 of them.
Tests: `the_header_names_the_id_and_anything_else_is_not_framed`, `message_indexes_round_trip_and_zero_is_the_first_message`.

### REG-26. The flags
`--schema-registry` (on by default, as in Go), `--schema-registry-addr` (127.0.0.1:8081; 127.0.0.1:0 when `--listen` is on port 0;
a name in it is a name the registry answers to), `--schema-registry-tls` (on: with `--tls-cert` and `--tls-key` the registry is
HTTPS with the broker's certificate; `=false` keeps HTTP and, beyond loopback, says what crosses the network unencrypted). An
address given that cannot be bound stops the start with exit 2; the default one, busy, leaves no registry and says so (REG-D10). A
build without the `registry` feature accepts the flags and says it ignores them.
Tests: `the_default_address_is_confluents_port_or_any_port_beside_a_broker_on_port_0`,
`a_bad_address_is_refused_and_plain_http_beside_tls_is_said`,
`a_busy_default_leaves_no_registry_and_a_busy_chosen_address_stops_the_start`,
`the_table_is_exactly_the_list_of_flags_clap_has_less_the_ones_that_are_accepted_and_ignored`.

### REG-27. Every parser is fuzzed
The ordered JSON, Avro schemas (with a reference), Protobuf schemas (text or descriptor, with an imported file), JSON Schemas (with
a reference, compared with a second one), any request to the API against a seeded registry, and the wire header each have a target
in `crates/kafka-registry/fuzz` (`json_node.rs`, `avro_schema.rs`, `protobuf_schema.rs`, `json_schema.rs`, `registry_api.rs`,
`wire.rs`), run 30 s each in CI. For each schema language a stored text reads again as itself and a schema reads its own data.
Tests: `text_that_is_not_json_is_refused`, `hosts_lose_their_port_and_quotes`.

## The record codec (slice 10d)

What the console reads records with and encodes Produce values with, in the same process: `Registry::codec(id)` and
`codec_for(subject, version)` give a `codec::Codec`; each language reads and writes its own payloads (`ParsedSchema::decode`,
`encode`, `sample`). No codec library: the Go edition's used hamba/avro, santhosh-tekuri/jsonschema and protobuf-go.

### REG-38. Avro payloads read and written as Avro's JSON
A payload reads as Avro's JSON encoding (a union's value tagged with its branch, null as `null`; bytes and fixed one character
a byte; NaN and the infinities as words), and a value read writes back to the same bytes. A field left out takes its default; a
union value may be tagged or is tried branch by branch; bytes may be base64. A value that does not fit says where (`id: expected a
number, not "x"`, `field "lines" is missing and has no default`).
Tests: `avro_values_round_trip_with_tagged_unions_defaults_and_bytes_as_characters`, `avro_values_that_do_not_fit_say_where`,
`bytes_are_characters_or_base64`, `floats_are_numbers_and_the_others_are_words`.

### REG-39. Hostile payloads are bounded before anything is allocated
Every length and count in a payload is checked against the bytes left; a value makes at most 1,048,576 items and nests at most 64
deep (a block of 2^62 nulls, a string longer than the payload, a recursive record a thousand deep and a group that never ends are
refused in words: "the payload does not match the schema (...)"). The guard of the Go edition's `avroguard.go`, for every language.
Tests: `hostile_avro_bytes_are_refused_before_anything_is_allocated`, `a_recursive_avro_type_is_bounded_in_depth`,
`hostile_protobuf_bytes_are_refused`, `the_budget_bounds_items_and_depth`; fuzz target `codec.rs`.

### REG-40. Protobuf payloads read by their message index, as protojson
After the header, the message index names the message (`[0]` the first of the file, `[0, 1]` the second nested in it, a map's entry
counted where protoc puts it); the body reads as protojson writes it (JSON names, 64-bit integers as strings, bytes as base64, enums
by name, maps as objects; unknown fields skipped; packed and unpacked alike). Encoding writes the first message with a single 0, as
the Go edition did, from JSON names or field names.
Tests: `protobuf_values_read_as_protojson_by_their_message_index`, `hostile_protobuf_bytes_are_refused`, `bytes_are_base64`.

### REG-41. A JSON Schema payload is JSON text, checked against its schema
A payload reads as JSON; one that does not fit its schema still reads, with why (`#/id: 0 is under its minimum`); encoding refuses
it. Checked: type, enum, const, lengths, bounds and `multipleOf` (exact decimals), items, `additionalItems`, the item counts,
`uniqueItems`, properties, `required`, `additionalProperties`, the property counts, array `dependencies`, `allOf`, `anyOf`, `oneOf`,
`not`, `if`/`then`/`else`, references followed.
Tests: `json_schema_values_are_checked_and_shown_as_they_are`, `an_unknown_id_is_confluents_40403`.

## What the audit added (2026-11-18)

### REG-42. The settings beside a level
`PUT /config` takes Confluent's `ConfigUpdateRequest`: `compatibility`, `alias`, `normalize`, `validateFields`,
`compatibilityGroup`, `defaultMetadata`, `overrideMetadata`, `defaultRuleSet`, `overrideRuleSet`. What a PUT gives is laid over what
was set (the rest stays), echoed in Confluent's order (`alias, normalize, validateFields, compatibility, compatibilityGroup, ...`),
and `GET /config` answers `compatibilityLevel` then the others; a rule set with no rules is not written. A subject's first
configuration without a level takes BACKWARD; `DELETE` answers the configuration as it was. The configuration in force for a
subject is its own, whole, when it has one, else the global one (Confluent's `getConfigInScope`; gaps: a subject's own level
hides the global default metadata): its `normalize` normalizes every registration, lookup and check as `?normalize=true` does,
its group and contract settings act as REG-47 and REG-48 say. Metadata and rule sets are read as Jackson binds them (REG-47) and a
rule set is checked (REG-49) before anything is kept. `alias` and `validateFields` are kept (under their own key, `k`, read
back after a restart) and answered but change nothing.
Tests: `settings_bodies_are_read_as_jackson_reads_them`, `every_key_reads_back_as_itself_and_strangers_are_none`.

### REG-43. A schema in another form
`GET /schemas/ids/{id}?format=serialized` answers a Protobuf schema as the base64 of the `FileDescriptorProto` Confluent builds
(`protobuf/serialize.rs`): what confluent-kafka for Python asks for to deserialize. It is Confluent's, byte for byte, not
protoc's: the file is named `default`, type names stay as written, a message or enum field has a `type_name` and no `type`, a
proto3 field without a label has none, oneof fields come first, a map is a nested `<field>Entry` after the declared nested
messages, a proto3 `optional` has a synthetic oneof, groups are left out, ranges end one past their last number.
`?format=resolved` writes an Avro schema's referenced types out where they are first used, then as Avro writes a schema. The
version routes ignore `format`, as 7.7.1 does. A schema by id is `schemaType, schema, references, metadata, ruleSet, maxId` in that order (REG-47).
Tests: `every_recorded_scenario_answers_as_confluent_did` and tools/registry-diff's `protobuf-serialized` scenario and corpus.

### REG-44. A refused schema is quoted as Confluent quotes it
`Invalid schema {subject=s,version=0,id=-1,schemaType=AVRO,references=[{name='x', subject='y', version=1}],metadata=null,
ruleSet=null,schema=<text>,schemaTags=null} with refs [...] of type AVRO, details: <why>` (a contract written as its Java
`toString`: `metadata=Metadata{tags={}, properties={owner=a}, sensitive=[]}`) for a schema its parser refuses; without
`with refs ... of type` for one refused after reading (a JSON Schema not valid for its draft, a Protobuf type not resolved). A
`schemaType` must be `AVRO`, `PROTOBUF` or `JSON` exactly (`avro` and `""` are `Invalid schema type avro`); an empty schema is
`Empty schema` on every route.
Test: `a_refusal_quotes_the_schema_as_confluent_does`.

### REG-45. IMPORT and READONLY_OVERRIDE
Both modes are offered (REG-D1 retired). IMPORT is refused where schemas exist (the subject's, or any for the global mode) with
42205 `Cannot import since found existing subjects` unless `?force=true`. In IMPORT a registration must give an `id`
(`Subject s is not in read-write mode` otherwise), takes its `version` or the next, checks no compatibility, may not give an id
another schema has (`Overwrite new schema with id 5000 is not permitted.`), and later ids start past it. READONLY_OVERRIDE is
READONLY that, set globally, no subject's own mode lifts. A PUT echoes the mode as written (`readonly`). `DELETE /mode/s` of a
subject with no mode of its own is 40401. The recorded readonly-subject and modes scripts pass whole.
Test: `every_recorded_scenario_answers_as_confluent_did`.

## What closing the gaps added (2026-11-19)

### REG-46. Protobuf is normalized as Confluent normalizes it
`?normalize=true` (or the `normalize` setting, REG-42) stores `protobuf/normalize.rs`'s text, a port of Confluent's
`ProtobufSchemaUtils.toNormalizedString` (8.3.2's source, the same as 7.7.1's but where REG-D31 says): no `syntax` line for proto2;
imports and public imports sorted, each once; every type name qualified with its leading dot (`.shop.v1.Money`), found as
Confluent's `Context.resolveFull` finds it (the main file's package and the enclosing names tried innermost first, among the types
of the file, its references and the well-known files), a map's value type too, and a field typed by a descriptor's map-entry
message written as its `map<,>` (the entry message left out); options sorted by name, a custom one named by its extension's full
name, a dotted one (`(rule).min = 1`) made a message literal, a message literal's keys sorted, an extension key qualified, a
one-item list written as its item, an empty list dropped; numbers in options and defaults as Java's `DecimalFormat` writes what
commons-lang's `createNumber` reads (`0x10` is `16`, `010` is `8`, `2.50` is `2.5`, at most three decimals half to even, `1e20`
as the float it is read as, `inf` as written); a field's `default` and `json_name` sorted among its options; fields, a oneof's
fields, groups and an extend's fields by number; oneofs by their first field (an empty one dropped); enum values by number, then
name; reserved and extension ranges one to a statement by start (names after numbers, by name), a range of one number as the
number, `max` from the largest field number; the extends of one type made one. Messages, enums, services and rpcs keep their
order (a client numbers messages by place). The normalized text is stored as it is, and every language's references are sorted
by subject, then version, each kept once. As Confluent's `validate()` normalizes every schema, a type its resolution does not
find refuses the schema whether or not it is normalized. The descriptor of a normalized proto2 schema has no `syntax`, and an
enum's reserved `max` ends at Wire's largest field number, as Confluent's do.
Tests: `a_schema_normalizes_as_confluent_normalizes_it`, `numbers_are_written_as_javas_decimal_format_writes_what_commons_lang_reads`,
`indenting_drops_trailing_empty_lines_as_java_splits`, `strings_are_read_as_wire_reads_them`, and tools/registry-diff's
`protobuf-normalize/*` (eleven schemas: options of every kind, proto2 with groups and extensions, names in nested scopes, custom
options merged, numbers, enums, a descriptor with maps, references, compatibility).

### REG-47. A schema's contract is part of it
A registration's `metadata` (`tags`, `properties`, `sensitive`) and `ruleSet` (`migrationRules`, `domainRules`, and 8.x's
`encodingRules`) are read as Jackson binds Confluent's classes (unknown keys read past, a number or boolean as its text, kinds and
modes defaulted to `TRANSFORM` and `WRITEREAD`, Jackson's 400 for anything else: `Cannot deserialize value of type \`Metadata\`
from Array value ...`, `... \`RuleKind\` from String "NOPE": not one of the values accepted for Enum class: [CONDITION,
TRANSFORM]`), sorted as its Java collections sort them, and stored with the schema (a schema record of format 2, `store.rs`; one
without a contract keeps the first format). They count in the schema's identity: the same text with other metadata is another
schema, with its own id and version, and the same schema and contract under another subject has the same id. Outside IMPORT the
contract is Confluent's `maybeSetMetadataRuleSet`: the registration's own metadata and rule set, else the subject's latest live
version's, merged over the configuration's `defaultMetadata`/`defaultRuleSet` and under its `overrideMetadata`/`overrideRuleSet`
(a key, a property, a tag or a rule of the same name from the later one wins; `sensitive` is the union), and `confluent:version`
set to the version taken when the registration asks for a version or the metadata already has one. A registration whose schema
then has a contract is answered with it (`{"id","version","metadata","ruleSet","schema"}`, 7.7.1's `RegisterSchemaResponse`); one
without, or one found as sent (Confluent's `registerOrForward` looks up first, unless defaults or overrides are set), with
`{"id"}`. Every read answers the contract: `/subjects/{s}/versions/{v}` and `GET /schemas` beside the references, `/schemas/ids/{id}`
after them. A lookup finds the version holding the schema with the contract sent, or else with the one a registration would give
it (REG-D31). IMPORT stores the contract given; the same id with another contract is `Overwrite new schema with id 7000 is not
permitted.` `metadata: {}` is kept and answered as `{}`.
Tests: `a_contract_is_part_of_the_schema_and_survives_a_restart`, `defaults_lie_under_and_overrides_over_what_a_registration_gives`,
`a_rule_of_the_same_name_replaces_the_older_one`, and tools/registry-diff's `data-contracts/*`.

### REG-48. Compatibility groups and the latest with metadata
With `compatibilityGroup` set, a schema is compared only with the live versions whose metadata property of that name has the same
value as its own (absent matching absent), the level then choosing among those; a registration without metadata inherits the latest
version's, so its group. `GET /subjects/{s}/metadata?key=k&value=v&...` is the newest version (a soft-deleted one too with
`deleted=true`) whose metadata has every pair given, keys and values paired in order as far as both go; a version with no metadata
is never one. None is `40403 Schema not found`, and a subject with no version to look at `40401`.
Tests: `a_contract_is_part_of_the_schema_and_survives_a_restart`, tools/registry-diff's `data-contracts/compatibility-group` and
`data-contracts/latest-with-metadata`.

### REG-49. Rule sets are checked as Confluent checks them
A registration's rule set, and a configuration's default and override, are checked by `RuleSet.validate` before anything else
(422 42210): `Found rule with duplicate name 'a'` (across the lists), `Missing rule name`, `Empty rule name`, `Rule name too long`
(past 64 UTF-16 units), `Illegal initial character in rule name: 1abc`, `Illegal character in rule name: a.b`, `Missing rule type`,
`Multiple actions only valid with WRITEREAD and UPDOWN`, `Migration rules can only be UPGRADE, DOWNGRADE, UPDOWN`, `Domain rules
can only be WRITE, READ, WRITEREAD`, `Encoding rules can only be WRITE, READ, WRITEREAD`. A rule is written as Confluent writes one
(`name, doc, kind, mode, type, tags, params, expr, onSuccess, onFailure, disabled`). REG-D30 says why the reference cannot show it.
Test: `rule_sets_are_checked_as_confluent_checks_them`.

### REG-50. JSON Schema numbers past Java's BigDecimal
Confluent's Jackson reads a number with a fraction or exponent as a `BigDecimal` when Java 17's `BigDecimal(String)` takes it (at
most ten exponent digits after leading zeros, the exponent and the scale within a 32-bit `int`), else as a `double` (`json/java.rs`,
`java_reads`): a zero one is stored as `0.0` or `-0.0` (`0.0` once normalized, as the text is read again) and compares as zero; an
infinite one, anywhere in the schema, refuses it (`Invalid JSON Schema`, 42201, the check stage). So `1e2147483647` is
`1E+2147483647`, `12e2147483647` is `1.2E+2148483648`, `1e2147483648` and `0.5555e+000011999153157185615783` are refused, and
`1e-2147483648` is `0.0`. Before 2019-09 a count is read by everit as a Java `Integer`: written as an integer (`1.0`, `1e1`, `1E0`
are refused) no larger than 2147483647, and a `multipleOf` must be above zero as a double (`1e-400` is refused). A number's bounds
(`minimum`, `maximum`, the exclusive ones) are compared as doubles, as Confluent's `NumberSchemaDiff` compares them. 8.3.2's newer
Jackson differs (it takes `1e2147483648` and refuses `1e-2147483648` with `Exponent overflow.` in another sentence); this registry
follows 7.7.1, the reference of every other rule here.
Tests: `a_number_too_large_to_rewrite_is_kept_as_sent`, `a_number_past_the_clamp_is_one_value_however_it_is_written`,
`what_is_not_a_schema_of_its_draft_is_refused`, and tools/registry-diff's `json-schema-huge-numbers`.

## Deviations

- **REG-D1.** Retired by the audit (2026-11-18): IMPORT and READONLY_OVERRIDE are offered as Confluent offers them (REG-45). It
  said they were refused as invalid modes, as the Go edition refused them; an IMPORT mode is how a local registry takes a copy of
  another with its ids, which a dev tool should do.
- **REG-D2.** A subject of up to 16 KiB is taken: Confluent's HTTP server refuses a path past about 8 KiB (414). As Go.
- **REG-D3.** Contexts (Schema Linking) are not offered: `GET /contexts` is `["."]` (Confluent's answer with no contexts made; it was
  404), and `:.ctx:orders` or `:*:` are subjects of those names in the one registry, with ids from the one id space (Confluent gives a
  context its own). Registering under `:*:` sends cp-schema-registry 7.7.1 into a StackOverflowError on every later listing
  (recorded by the audit), which is one more reason not to copy it.
- **REG-D4.** A schema body past 20 MiB is 413 where Confluent's JSON parser answers 400; a level or mode body is at most 64 KiB
  (413) where Confluent reads 20 MiB of padding and ignores it. As Go.
- **REG-D5.** Not served (404): `/exporters`, `DELETE /mode` (405, as it is not a Confluent route either), the
  `/v1/metadata/id` and `/v1/metadata/version` endpoints, the DEK registry. As Go. (`GET /subjects/{s}/metadata` is served since
  REG-48.)
- **REG-D6.** Where Confluent answers 500 this registry answers properly: `format=serialized` on an id is ignored; a lookup in a
  soft-deleted subject is its 40401; `GET /schemas/ids/{unknown}/schema` is 404 40403; `format=serialized` on a JSON schema is the
  stored text; a mode body without a mode is 42204; a normalized lookup of an Avro schema with properties is answered. As Go.
- **REG-D7.** A PUT or DELETE without credentials is 401 with the registry's JSON error; Confluent's HTTP server sends an empty
  body for those two methods. As Go.
- **REG-D8.** The Go edition answers to every name under `.doze` (its zone); this edition's host guard has no zone and answers to
  the names the operator lists, with the console (`kafka-auth/src/hostguard.rs`, SEC-180 to SEC-189).
- **REG-D9.** The stored text of a schema escapes as Confluent's JSON library (Jackson) does: `<`, `>`, `&`, U+2028 and U+2029 are
  themselves, and a control character is `\b`, `\t`, `\n`, `\f`, `\r` or `\u00XX`. The Go edition wrote Go's HTML escapes
  (`<`) into the stored text, so a schema with `<` in a doc read back differently from Confluent's.
- **REG-D10.** With the default address busy (another broker has 8081) the broker runs without a registry and says how to choose
  another address, as the Rust console does with its own; the Go edition stopped the start. An address the operator gave still
  stops the start (exit 2).
- **REG-D11.** An Avro name, namespace part, field name or symbol is a letter or `_` then letters, digits and `_`, where a letter is
  any Unicode letter, as Avro's Java parser (Confluent's) checks it; the Go edition's library allowed ASCII only.
- **REG-D12.** The records live in a `schemas` table of the broker's `meta.redb` (postcard values), not in bbolt with JSON values;
  a Go data directory is not read. So the Go edition's re-indexing of schemas stored in an earlier text form (`indexCurrentForms`,
  `TestOldCanonicalFormsAreStillFound`) has nothing to do here; if this edition's stored form ever changes, that index comes with it.
- **REG-D13.** Retired. Until both languages were in, it said which one this build did not read (a PROTOBUF or JSON schema
  refused as invalid, 42201, and `GET /schemas/types` listing only what was read). Since 10b and 10c every language is read,
  `GET /schemas/types` is Confluent's `["JSON","PROTOBUF","AVRO"]`, and no conformance step or scenario is pending. The number is
  kept so it is not given to another deviation.
- **REG-D14.** The Go edition's codec (an Avro, Protobuf or JSON payload decoded to JSON for the console, and JSON encoded for its
  producer, behind a guard against hostile Avro bytes) is not in 10a: the console has no registry page yet. Slice 10d.
- **REG-D15.** Protobuf is checked by protoc's main rules (REG-29), which are stricter than Confluent's parser: cp-schema-registry
  7.7.1 takes a field number used twice (recorded by the audit) and other schemas protoc refuses, which no code generator could use;
  this registry refuses them, saying where. It is not checked by all of protocompile's rules: a custom option's value is not
  checked against its extension's type, and `descriptor.proto` is known by its message names (enough to extend the options
  messages), not its fields. Editions (`edition = "2023"`) are refused: Confluent 7.7.1 reads proto2 and proto3, and the Go
  edition's printer dropped the edition line.
- **REG-D16.** The Go edition's printer dropped a group in a oneof, an `extend` inside a message and an rpc's options, so their
  stored text lost declarations. Here they are kept: the group in its oneof, the extend in a section after the nested types, the
  rpc's options in a block (`rpc R (A) returns (B) {\n    option deprecated = true;\n  };`, as Square's wire, Confluent's printer,
  writes it). None of these is recorded from Confluent.
- **REG-D17.** A descriptor that imports a file no reference gives is refused (`Could not resolve import: dep.proto`) even when no
  type of it is used: its stored text would name an import that cannot be read again (found by fuzzing). An unresolved type is
  still worded as Confluent words it.
- **REG-D18.** Confluent answers 500 when a JSON Schema's `exclusiveMaximum` is lowered (its checker has no name for that change;
  recorded: "exclusiveMaximum increased" FORWARD, "exclusiveMaximum decreased" BACKWARD); this registry answers the comparison
  (EXCLUSIVE_MAXIMUM_DECREASED, incompatible). The two corpus pairs are skipped (`DIFFERENCES` in `tests/corpus.rs`). As Go.
- **REG-D19.** A JSON Schema `pattern` or `patternProperties` name is refused for the errors Java's `Pattern` (Confluent's) refuses
  that a syntax scan can see: an unclosed group or class, an unmatched `)`, a trailing backslash, a quantifier with nothing to
  repeat. It is not compiled (no regular-expression engine is linked), so a rarer error Java would refuse is taken. The Go edition
  compiled patterns with Go's RE2, which refuses lookaheads and backreferences that Confluent accepts; they are accepted here.
- **REG-D20.** Where the Go edition's JSON Schema reading differed from Confluent's, this one follows Confluent: a `#...` reference
  inside a referenced schema is resolved in that schema (Go resolved it in the schema being registered); a pointer may pass through
  arrays (`#/allOf/0`) and use `%XX` escapes, and an anchor is found (Go refused both); a key written twice is stored once (Go stored
  both); `multipleOf` is compared in exact decimals (0.3 is a multiple of 0.1; Go used a binary float); a type with an enum beside it
  is two schemas combined, so "enum with type" gives Confluent's reason (Go's `corpusReasonDifferences` listed it).
- **REG-D21.** A JSON Schema comparison lists at most 1,000 reasons; a schema that makes more than 200,000 subschemas, nests more than
  64 deep, or whose references chain more than 160 nodes high is refused as too deep or large to compare (42201). The Go edition had
  the 64 but neither of the others, and a deep chain or a diamond of references could make it compare for hours or recurse without
  bound.
- **REG-D22.** For JSON Schema, `format` is ignored by compatibility (Confluent's rule), and
  an `$id` is not a base for references: a `$ref` to an absolute URI an embedded `$id` declares is unresolved. As Go; the
  recordings have no step on any of them. (It said `?normalize=true` stored the same text: since the audit it sorts keys, REG-35.)
- **REG-D23.** The codec shows an Avro logical type as its underlying type (a `timestamp-millis` is the long, a `decimal` the
  bytes); the Go edition's hamba/avro turned them into RFC 3339 times and decimal text. Encoding takes the underlying type only.
- **REG-D24.** A JSON Schema value is not checked against `pattern`, `patternProperties` or `format` (no regular-expression engine
  is linked: `regex-lite` would add a package to doze-kafka for one keyword); Go's validator checked them.
- **REG-D25.** Protobuf's well-known types (`Timestamp`, `Duration`, the wrappers, `Any`, `Struct`) read and write as the messages
  they are, not in protojson's special forms (`"2026-11-14T00:00:00Z"`, `"3s"`).
- **REG-D26.** Retired 2026-11-19: data contracts are kept (REG-47 to REG-49). It said a registration's metadata and rule set
  were read past and the settings beside a level other than `normalize` changed nothing, a gap the audit found.
- **REG-D27.** A refusal's details, the parser's own words after `details:`, are doze's where Confluent's are a Java library's
  exception text (`com.fasterxml.jackson.core.io.JsonEOFException: Unexpected end-of-input ... [Source: REDACTED ...]`, Wire's
  `expected a word`) or say nothing (`Invalid JSON Schema`, `Invalid JSON {`): doze names the place (`#/type: "nonsense" is not a
  type`, `expected a key at byte 1`). Everything before `details:`, the status and the code are Confluent's (REG-44), and the
  fixed sentences of Avro's parser are Confluent's word for word (REG-20). The conformance test compares messages up to
  `, details:`.
- **REG-D28.** Retired 2026-11-19: `?normalize=true` normalizes a Protobuf schema as Confluent does (REG-46). It said two
  orderings of one Protobuf schema were two ids here and one there.
- **REG-D29.** JSON Schema: this registry checks each keyword against its draft's meta-schema (REG-34) where Confluent's loaders
  (everit before 2019-09, jsonsKema after) take more, found by `json-schema-huge-numbers`: a negative count (`minItems: -1`), a
  repeated `enum` value (`[1, 1]`, and `[0.0, 0]` from a number past `BigDecimal`), a count that is not an integer in 2019-09 and
  2020-12 (`1.5`), a `multipleOf` of zero in them, and a number past a `double` in them (Confluent stores the string `"Infinity"`
  in its place, which changes what the schema says). Such a schema is no schema of its draft and no validator would read it as
  written; refusing it says so at registration, where Confluent would store it. The checks Confluent does make are made as it
  makes them (REG-50).
- **REG-D30.** Rule sets (REG-49) are kept, checked and answered as Confluent Enterprise and Confluent Cloud keep them. The
  community `cp-schema-registry` image, the reference `tools/registry-diff` runs, drops every rule set, a registration's and a
  configuration's, with `RuleSets are only supported by Confluent Enterprise and Confluent Cloud` (its `RuleSetHandler`), so it
  checks none either. A local registry for data-contract development has to keep them (a CEL rule a client runs comes from the
  registry); so `data-contracts/rule-sets` and the rule set steps of `metadata-and-rules-in-a-registration` differ from the
  reference by design. What is kept follows Confluent's classes and its source; it is not recorded from a live Enterprise.
- **REG-D31.** Where 7.7.1 and 8.x differ on what this round built, 8.x is followed: a lookup that finds nothing with the contract
  sent tries again with the contract a registration would give (the latest version's, the configuration's), as 8.x's
  `lookUpSchemaUnderSubject` does (7.7.1 answers 40403); a non-repeated custom Protobuf option given twice is merged as 8.x merges it
  (a singular message field deep, a singular scalar the last value; 7.7.1 makes every repeated key a list); encoding rules are kept
  (7.7.1 reads past them); a rule set's `toString` in a refusal is 8.x's. 8.x's other answers (a registration always answered
  with the schema, `guid`, `ts`, `deleted`, `in context .` in sentences) are not taken: the rest of this catalogue is 7.7.1's.
- **REG-D32.** Protobuf normalization (REG-46) differs from Confluent's at its edges: a `bytes` default stays quoted (Confluent writes
  it bare, `[default = abc]`, which no parser reads back); `google/type/*.proto` and `confluent/meta.proto` are not in the
  resolution's scope unless referenced (Confluent always has them; it matters only for a name that resolves to one of them without
  importing it, which protoc refuses); whether commons-lang reads a number as a `float` is decided by comparing the shortest
  digits of the float and the double (Java 17's `toString` sometimes prints more); a weak import is sorted with the plain ones; an
  extension range's options are written (8.x does, 7.7.1 drops them).
