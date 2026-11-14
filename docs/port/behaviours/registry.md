# Schema registry (Phase 10)

Slice 10a is built (2026-11-14, branch `p10-registry`): the registry's model, its storage in the broker's metadata database,
every REST route the Go edition serves (Confluent's and doze's own under `/_doze/`), the host guard, basic authentication and TLS
as the Go binary has them, the wire header, and the Avro language whole. Slice 10b (2026-11-14, branch `p10b-protobuf`) adds
Protobuf (REG-28 to REG-32). JSON Schema (10c) plugs in through
`format::Formats` (`phase10.md`, "Extension points"). This file is the catalogue: what Confluent's registry does, as the Go edition
recorded it against cp-schema-registry 7.7.1, and the test that holds each rule. The sources of truth are the recordings copied
from doze-kafka: `crates/kafka-registry/tests/data/conformance/*.json` (30 scripts, 469 steps) and the compatibility corpus
`tests/data/compat/avro.json` (392 pairs with Confluent's verdict at BACKWARD, FORWARD and FULL) with `avro-reasons.json`
(Confluent's reasons, word for word, for every refusal). Nothing here is written from memory of what Confluent does.

The numbers are checked by `tools/check-registry-catalogue.ts` (`bun tools/check-registry-catalogue.ts`; a CI step): no REG-n or
REG-Dn is defined twice, none is referenced without a definition, every deviation is in the index, and every test name quoted here
is a `fn` in the Rust sources.

## Index

Deviations: where this registry answers otherwise than Confluent's, or than the Go edition, on purpose.

- **REG-D1** IMPORT and READONLY_OVERRIDE modes are not offered (as in Go).
- **REG-D2** a subject of up to 16 KiB is taken (Confluent's HTTP server refuses a path past 8 KiB; as in Go).
- **REG-D3** contexts are not offered: `:.ctx:name` and `:*:` are subjects of those names (as in Go).
- **REG-D4** bodies past their limit are 413 (as in Go).
- **REG-D5** the metadata endpoints and the other routes listed as not served are 404 (as in Go).
- **REG-D6** where Confluent answers 500, a proper answer (as in Go).
- **REG-D7** a refused PUT or DELETE carries the JSON error (as in Go).
- **REG-D8** no special case for `.doze` names (Rust edition).
- **REG-D9** stored text uses Jackson's escapes, not Go's HTML escapes (Rust edition).
- **REG-D10** a busy default address leaves the broker running without a registry (Rust edition).
- **REG-D11** Avro names may use any letter, as Java's parser allows (Rust edition).
- **REG-D12** the store is the broker's redb database; records the Go edition stored are not read (Rust edition).
- **REG-D13** JSON Schema is not read yet (slice 10c).
- **REG-D14** the record-value codec (decode a payload to JSON, encode JSON) is not in 10a.
- **REG-D15** Protobuf: the language rules checked are protoc's main ones; editions are refused (Rust edition).
- **REG-D16** Protobuf: declarations the Go edition's printer dropped are kept in the stored text (Rust edition).
- **REG-D17** Protobuf: a descriptor importing a file no reference gives is refused (Rust edition).

## The API

### REG-1. One table of routes; misses answer in the registry's JSON
Every route is one row of `api/route.rs` (`ROUTES`): Confluent's 29 and doze's five under `/_doze/`. A path that matches a route
under another method is 405, any other path 404, both as `{"error_code":404,"message":"HTTP 404 Not Found"}` in the registry's
content type. HEAD is answered as GET. A wildcard is one path segment, unescaped on its own (`%2F` stays in the subject), and never
an empty one; a malformed escape is 400.
Tests: `routes_match_by_segment_and_method`, `bodies_of_other_types_are_refused_and_the_mux_answers_in_json`,
`segments_are_unescaped_one_by_one`.

### REG-2. Every recorded script answers as Confluent did
The 30 conformance scripts recorded from Confluent are replayed (ids as offsets from a probed base, as the Go harness does) and
every step must give Confluent's status, challenge, error code and message (an "Invalid schema" message is compared by its start;
a parser's own words after ", details:" are not compared), and anything else whole. The steps that differ on purpose are listed
with their deviation (REG-D1 to REG-D7); the scripts about JSON Schema wait for 10c (REG-D13). Avro and Protobuf steps
(protobuf-canonical-forms, protobuf-descriptor-with-import): all pass.
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
that it is 413 and not kept (REG-D4). A schema body that is not JSON is 400; a setting body that is not JSON is 422 42207; only a
setting body's first JSON value is read, as Go's decoder reads it.
Tests: `bodies_of_other_types_are_refused_and_the_mux_answers_in_json`, `bodies_are_bounded_by_what_the_route_reads`,
`a_registration_over_tcp_is_answered_in_the_registrys_type`.

### REG-6. Query parameters as Go reads them
`deleted`, `deletedOnly`, `permanent`, `normalize`, `verbose`, `latestOnly`, `fetchMaxId`, `defaultToGlobal` are Go booleans (`1`,
`t`, `true`, ... ; anything else false); a value is the first of its name; `+` is a space; `subjectPrefix=:*:` is every subject.
`offset` and `limit` cut `GET /schemas` only: a missing, negative or unreadable offset is 0, a limit that is not positive is none.
Tests: `queries_read_as_go_reads_them`, `a_page_is_cut_only_by_a_positive_limit`.

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
NONE compares nothing; BACKWARD, FORWARD and FULL the latest live version; the transitive levels every live version. FORWARD asks the
old schema to read the new, BACKWARD the new to read the old, FULL both (forward reasons first). A check names the versions it
compared.
Test: `the_level_chooses_the_versions_compared`.

### REG-14. A check and a registration are one engine
For every corpus pair and level, a check says compatible exactly when the registration succeeds, and gives the reasons the 409
gives, word for word.
Test: `a_check_agrees_with_a_registration_on_every_pair`.

### REG-15. References
A reference names a live version; it is followed once per (subject, version) however many paths reach it (a diamond of 26 levels
is resolved in its size), at most 1,000 distinct schemas; a chain of 40 registers. A version another live schema references
cannot be deleted (422 42206), nor its subject.
Tests: `a_diamond_of_references_is_resolved_once_per_schema`,
`a_chain_of_forty_references_registers_and_a_referenced_version_cannot_go`.

### REG-16. Deleting
Soft first, permanent only after (40405, 40407 otherwise; 40404, 40406 for a second soft delete). A subject's own level and mode
go with its soft delete. Registering a schema the subject had soft-deleted makes it a new, latest version, and the deleted one goes
for good. A read-only subject refuses deletes and level changes (42205) and answers a schema it already holds with its id.
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
Undefined names, a missing type, name, fields, symbols, items, values or size; an illegal name, namespace, field name or symbol; a
type named after a primitive; a redefined name; duplicate fields or symbols; an enum default not among its symbols; a nested union
or a type twice in a union; an illegal field order; and, the one default that is checked, an empty string as the default of a
number. Defaults are otherwise not checked against their types, and an invalid logical type is ignored, as Confluent does.
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
161" too).
Tests: `every_recorded_verdict_and_reason_is_confluents`, `a_check_agrees_with_a_registration_on_every_pair`.

### REG-29. What protoc refuses is refused
A syntax error says where, as Confluent words it (`Could not parse Protobuf - Syntax error in :1:28: ...`). The linker refuses an
unknown or unseen type (a type of a file imported by an import is seen only through `import public`), names resolved from the
innermost scope that has their first part (as protoc: no looking outward after a miss), a name defined twice (enum values are
siblings of their enum), field numbers out of range, in 19000 to 19999, reused, reserved or in an extension range, reserved names,
a missing label in proto2, `required`, defaults, groups and extension ranges in proto3, a proto2 enum used in proto3, map keys that
are not integers, bools or strings, labels in a oneof, an empty oneof or enum, a proto3 enum not starting at zero, aliases without
`allow_alias` (and `allow_alias` without aliases), conflicting JSON names in proto3, `packed` on what cannot be packed, a default
that does not fit its field, built-in options by name and value type, and custom options naming no extension. Nesting past 100
levels is refused, so hostile text cannot exhaust the stack.
Tests: `what_the_language_refuses_is_refused`, `names_resolve_from_the_innermost_scope`, `deep_nesting_is_refused_not_overflowed`.

### REG-30. The stored text is Confluent's printing
Syntax and package; the imports; the file options; every message, then every enum; extends; services. In a message: reserved,
options, fields, oneofs, groups, nested messages then enums, extends, extension ranges, each section after a blank line (padded to
the indent in a nested message); a field's options inline when one, one to a line when several (its own, then `default`, then
`json_name`); comments dropped; tokens as written with white space collapsed. What is stored reads back as itself.
`?normalize=true` changes nothing for Protobuf, as in the Go edition.
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
The ordered JSON, Avro schemas (with a reference), any request to the API against a seeded registry, and the wire header each have
a target in `crates/kafka-registry/fuzz` (`json_node.rs`, `avro_schema.rs`, `protobuf_schema.rs`, `registry_api.rs`, `wire.rs`), run 30 s each
in CI. Protobuf schemas (text or descriptor, with an imported file) too: a stored text reads again as itself and a schema reads its
own data.
Tests: `text_that_is_not_json_is_refused`, `hosts_lose_their_port_and_quotes`.

## Deviations

- **REG-D1.** IMPORT is refused as an invalid mode (42204), forced or not, and READONLY_OVERRIDE likewise; the list of valid modes
  names READWRITE and READONLY. Confluent refuses IMPORT while subjects exist (42205). As the Go edition.
- **REG-D2.** A subject of up to 16 KiB is taken: Confluent's HTTP server refuses a path past about 8 KiB (414). As Go.
- **REG-D3.** Contexts (Schema Linking) are not offered: `GET /contexts` is 404, and `:.ctx:orders` or `:*:` are subjects of those
  names in the one registry. As Go.
- **REG-D4.** A schema body past 20 MiB is 413 where Confluent's JSON parser answers 400; a level or mode body is at most 64 KiB
  (413) where Confluent reads 20 MiB of padding and ignores it. As Go.
- **REG-D5.** Not served (404): `/contexts`, `/exporters`, `DELETE /mode` (405, as it is not a Confluent route either), the
  metadata endpoints (`/v1/metadata/id`, `/v1/metadata/version`), subject metadata and rules, the DEK registry. As Go.
- **REG-D6.** Where Confluent answers 500 this registry answers properly: `format=serialized` on an id is ignored; a lookup in a
  soft-deleted subject is its 40401; `GET /schemas/ids/{unknown}/schema` is 404 40403. As Go.
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
- **REG-D13.** Until slice 10c, a JSON schema is refused as invalid (42201, "not read by this build") and `GET /schemas/types`
  lists PROTOBUF and AVRO. The conformance scripts and corpus file for JSON Schema are kept for it.
- **REG-D14.** The Go edition's codec (an Avro, Protobuf or JSON payload decoded to JSON for the console, and JSON encoded for its
  producer, behind a guard against hostile Avro bytes) is not in 10a: the console has no registry page yet. Slice 10d.
- **REG-D15.** Protobuf is checked by protoc's main rules (REG-29), not by all of protocompile's: a custom option's value is not
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
