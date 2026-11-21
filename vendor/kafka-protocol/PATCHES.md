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

## 3. Share group messages at Apache Kafka 4.4.0's versions

0.18.0 is generated from Kafka 4.1.0's schemas. Kafka 4.2 to 4.4 added a version to three share-group APIs, and
real 4.3.1 and 4.4.0 brokers offer them (`crates/kafka-wire/tests/data/api-versions.json`), so the share consumer of
those clients sends them. Patched by hand from the 4.4.0 schemas, each change marked `PATCH`:

- `ShareFetchRequest` and `ShareFetchResponse` 1-2: the request's `ShareAcquireMode` (int8, ignorable, KIP-1206) and
  `IsRenewAck` (bool, KIP-1222) at version 2, after `BatchSize`. The response is unchanged at 2.
- `ShareAcknowledgeRequest` and `ShareAcknowledgeResponse` 1-2: the request's `IsRenewAck` after `ShareSessionEpoch`;
  the response's `AcquisitionLockTimeoutMs` (int32, ignorable) after `ErrorMessage`.
- `DescribeShareGroupOffsetsRequest` and `DescribeShareGroupOffsetsResponse` 0-1: the partition's `Lag` (int64,
  ignorable, default -1, KIP-1226) after `LeaderEpoch`.
- `ApiKey::valid_versions` for the three keys.

Regression test: `crates/kafka-wire/tests/share_versions.rs` (each new field written and read back at its versions,
and left out below them).

## 4. Streams group messages (KIP-1071) at Apache Kafka 4.4.0's versions

0.18.0 has no streams group messages: Kafka 4.1.0 marked their versions unstable, and the generator skips unstable
versions. Kafka 4.4.0 serves StreamsGroupHeartbeat (88) 0-1, StreamsGroupDescribe (89) 0-1 and
StreamsGroupTopologyDescriptionUpdate (93) 0 to every client, so the Kafka Streams client of 4.4.0 sends them.

These are not hand edits. The six `src/messages/streams_group_*.rs` files and `src/messages.rs` were regenerated with
upstream's own generator (`protocol_codegen` at the head of `tychedelia/kafka-protocol-rs`, which writes exactly
0.18.0's files from Kafka 4.1.0's schemas) from Kafka 4.1.0's schemas with the six `StreamsGroup*.json` of the 4.4.0
source release put in their place, then `rustfmt`. Against the vendored `messages.rs` the regenerated one differs only by
the streams lines (the three keys, their requests and responses, `iter` up to 93) and by patch 3's three version ranges,
which were applied again on top. Every other message file is byte for byte what 0.18.0 has.

Regression test: `crates/kafka-wire/tests/streams_versions.rs` (each version-dependent field written and read back where
it is, and refused where it is not), and `tests/every_offered_version.rs`.

## 5. The versions KIP-896 removed that the older profiles need (2026-11-18)

0.18.0 is generated from Kafka 4.1.0's schemas, which start DescribeAcls, CreateAcls and DeleteAcls at v1, DescribeLogDirs at
v1 and OffsetForLeaderEpoch at v2 (KIP-896). Kafka 1.1 has only v0 of all five, and 2.8 and 3.9 start them at 0
(`crates/kafka-wire/tests/data/api-versions.json`), so without them `--kafka-version 1` could not offer the ACL APIs at all
(debt I2). Patched by hand from the 3.9.1 schemas, each change marked `PATCH 5`:

- The version checks of the ten messages and their structs, their `VERSIONS`, and `ApiKey::valid_versions` start at 0.
- The ACL pattern type (`ResourcePatternType` of a creation, `PatternTypeFilter` of the two filters, `PatternType` of a
  described resource and of a deleted ACL) is `"versions": "1+", "default": "3"`: read as 3 (LITERAL) at v0, not written
  there, and a value other than 3 cannot be written at v0 (the field is not ignorable, so Kafka throws as well).
- OffsetForLeaderEpoch: the request's `CurrentLeaderEpoch` is `2+` (default -1); the response's `LeaderEpoch` is `1+`
  (default -1) and `ThrottleTimeMs` `2+`.
- DescribeLogDirs v0 is v1 (the schema bumped the version for the throttle behaviour only).

Regression test: `crates/kafka-wire/tests/old_versions.rs` (each version against bytes written by hand from the schema),
`tests/every_offered_version.rs`, and the recorded `acl-v0` scenario of `tools/kafka-replay` (Apache Kafka 3.9.1).

## 6. Wire tail: the versions Kafka 4.4.0 added to the APIs served (2026-11-18 and -19)

0.18.0 is generated from Kafka 4.1.0's schemas. Kafka 4.4.0 serves more versions of eight APIs the broker serves, and of two
4.1.0 marked `latestVersionUnstable`, which the generator skips (debt K1). Two kinds of change, each tested:

**Regenerated** as patch 4 was, by upstream's generator (`protocol_codegen` at the head of `tychedelia/kafka-protocol-rs`), from
patch 4's input (Kafka 4.1.0's schemas with 4.4.0's six `StreamsGroup*.json`) with these 4.4.0 schemas put in their place,
then `rustfmt`; the files taken whole, unmarked:

- `TxnOffsetCommitRequest.json`, `TxnOffsetCommitResponse.json`: v6 (topics named by id, KIP-516: `TopicId` from v6, `Name`
  to v5 only and ignorable; the request's `GenerationId` is renamed `GenerationIdOrMemberEpoch`, the one source change it asks
  of the workspace).
- `InitProducerIdRequest.json`: v6 (KIP-939: `Enable2Pc` and `KeepPreparedTxn`), unstable in 4.1.0 and in 4.4.0 alike. One
  change was made to the input, `"latestVersionUnstable": true` set to `false`, so that v6 is generated; whether it is offered
  is the broker's decision (only with `--unstable-api-versions`, as 4.4.0 only with `unstable.api.versions.enable`). The
  response was already generated to v6.
- `OffsetCommitRequest.json`, `OffsetFetchRequest.json`: v10 (topics by id, KIP-848), unstable in 4.1.0 and stable in 4.4.0.
  0.18.0 had the `TopicId` fields but stopped the requests at v9; the responses were already generated to v10.

Against the vendored files the output differs in exactly those five files; the generated `TxnOffsetCommitResponse` and the
two v10 responses are byte for byte what was vendored, which checks the pipeline. In `messages.rs` only
`ApiKey::TxnOffsetCommit`'s range (0-6) is applied, marked `PATCH 6`.

**By hand** from the 4.4.0 schemas, each change marked `PATCH 6` (the version checks, `VERSIONS`, `ApiKey::valid_versions`):

- `ListOffsetsRequest` and `ListOffsetsResponse` 1-11 (KIP-1023: a new special time, no new field).
- `ApiVersionsRequest` and `ApiVersionsResponse` 0-5: the request's `ClusterId` (nullable compact string, default null) and
  `NodeId` (int32, default -1), both ignorable, after `ClientSoftwareVersion` (KIP-1242).
- `DeleteGroupsRequest` and `DeleteGroupsResponse` 0-3: each result's `ErrorMessage` (nullable, ignorable) after `ErrorCode`.
- `DescribeLogDirsRequest` and `DescribeLogDirsResponse` to 5 (on top of patch 5's v0): each directory's `IsCordoned` (bool,
  ignorable, default false) after `UsableBytes` (KIP-1066).

Regression test: `crates/kafka-wire/tests/kip516_kip939_versions.rs` (TxnOffsetCommit v6, InitProducerId v6),
`tests/every_offered_version.rs` (every offered version of every served API written and read back), and the requests
and answers of `tools/wire-tail-oracle` against a real Kafka 4.4.0.

## 7. Admin APIs: v0 of AlterReplicaLogDirs and of the four delegation-token APIs (2026-11-19)

(Patch 6 is the wire tail's, branch kafka-wire-tail.) Kafka 4.1.0's schemas start AlterReplicaLogDirs and the four
delegation-token APIs (CreateDelegationToken, RenewDelegationToken, ExpireDelegationToken, DescribeDelegationToken) at v1
(KIP-896). Kafka 1.1 has only v0 of all five, and 2.8 and 3.9 start them at 0
(`crates/kafka-wire/tests/data/api-versions.json`), so `--kafka-version 1` to `3` could not offer them as recorded (debt
K3). In the 3.9.1 schemas every one of the ten messages says "Version 1 is the same as version 0" (the responses bumped
the version for the throttle behaviour only), so v0 is v1 on the wire. Patched by hand, each change marked `PATCH 7`: the
version checks of the ten messages and their structs, their `VERSIONS`, and `ApiKey::valid_versions` start at 0.

Regression test: `crates/kafka-wire/tests/old_versions.rs` (v0 against bytes written by hand from the schema) and
`tests/every_offered_version.rs`.

## 8. The oldest versions of eight APIs, for the older profiles (2026-11-19)

Kafka 4.1.0's schemas start Produce at v3, Fetch at v4, ListOffsets, OffsetFetch and DeleteTopics and DescribeConfigs at v1,
OffsetCommit and CreateTopics at v2 (KIP-896). Kafka 1.1, 2.8 and 3.9 list all of them from v0
(`crates/kafka-wire/tests/data/api-versions.json`), and the profiles that stand for them serve every version their recorded
broker lists (debt K2), so the crate reads and writes them again.

**Regenerated** as patches 4 and 6 were, by upstream's generator (`protocol_codegen` at the head of `tychedelia/kafka-protocol-rs`,
37fed76, whose generator is unchanged since 0.18.0), then `rustfmt`; the sixteen files taken whole, unmarked:
`produce_request.rs`, `produce_response.rs`, `fetch_request.rs`, `fetch_response.rs`, `list_offsets_request.rs`,
`list_offsets_response.rs`, `offset_commit_request.rs`, `offset_commit_response.rs`, `offset_fetch_request.rs`,
`offset_fetch_response.rs`, `create_topics_request.rs`, `create_topics_response.rs`, `delete_topics_request.rs`,
`delete_topics_response.rs`, `describe_configs_request.rs`, `describe_configs_response.rs`. The input of each is the schema the
vendored file was generated from (Kafka 4.1.0's; OffsetCommitRequest and OffsetFetchRequest less `latestVersionUnstable`, as
patch 6 took them from 4.4.0), with its `validVersions` starting at 0 (ListOffsets: 0-11, for patch 6's v11) and the fields
only the old versions have put back, each exactly as Kafka 3.9.1's schema has it:

- ListOffsetsRequest `MaxNumOffsets` (int32, v0, default 1) after `Timestamp`; ListOffsetsResponse `OldStyleOffsets` (`[]int64`,
  v0) before `Timestamp`.
- OffsetCommitRequest `CommitTimestamp` (int64, v1 only, default -1) before `CommittedMetadata`.
- DescribeConfigsResponse `IsDefault` (bool, v0) after `ReadOnly`.

Every other field of the eight messages was already in the 4.1.0 schemas with its version range (`TransactionalId` 3+,
`ThrottleTimeMs` 1+, `LogAppendTimeMs` 2+ and the like); with the floor lowered the generator writes the checks for them that it
left out while every version had them. The pipeline was checked first: the unmodified inputs generate fourteen of the sixteen
files byte for byte as vendored, and ListOffsets differs only by patch 6's v11 (its version checks). In `messages.rs` the eight
ranges of `ApiKey::valid_versions` start at 0, marked `PATCH 8`.

Regression test: `crates/kafka-wire/tests/old_versions.rs` (the restored versions against bytes written by hand from the 3.9.1
schemas), `tests/every_offered_version.rs` (every offered version of every served API written and read back), and the raw
requests of `tools/old-profiles-oracle` against real Kafka 1.1, 2.8 and 3.9.1.
