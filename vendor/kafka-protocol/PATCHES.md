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

## 5. TxnOffsetCommit v6 and InitProducerId v6 at Apache Kafka 4.4.0's versions

0.18.0 (Kafka 4.1.0's schemas) has TxnOffsetCommit to v5 and the InitProducerId request to v5. Kafka 4.4.0's
`TxnOffsetCommitRequest.json` and `TxnOffsetCommitResponse.json` add v6 (topics named by id, KIP-516: `TopicId` from v6, `Name`
to v5 only and ignorable; the request's `GenerationId` is renamed `GenerationIdOrMemberEpoch`), and 4.4.0's Java client sends it.
`InitProducerIdRequest.json` has v6 (KIP-939: `Enable2Pc` and `KeepPreparedTxn`) in 4.1.0 and in 4.4.0 alike, marked
`latestVersionUnstable`, so the generator skipped it (it decoded neither flag); the response was already generated to v6.

Generated as patch 4 was, by upstream's generator (`protocol_codegen` at the head of `tychedelia/kafka-protocol-rs`), from
patch 4's input (Kafka 4.1.0's schemas with 4.4.0's six `StreamsGroup*.json`) with the four `TxnOffsetCommit*.json` and
`InitProducerId*.json` of the 4.4.0 source release put in their place, then `rustfmt`. One change was made to the input:
`InitProducerIdRequest.json`'s `"latestVersionUnstable": true` set to `false`, so that v6 is generated; whether v6 is offered is
the broker's decision (doze-kafka offers it only with `--unstable-api-versions`, as Kafka 4.4.0 only with
`unstable.api.versions.enable`). Against the vendored files the output differs in exactly `init_producer_id_request.rs`,
`txn_offset_commit_request.rs` and `txn_offset_commit_response.rs`, which were taken whole, and in `messages.rs` by
`ApiKey::TxnOffsetCommit`'s range (0-6) and patch 3's three ranges, of which only the first was applied. Every other file is
byte for byte what patch 4 left.

The rename is the one source change it asks of the workspace (`generation_id` to `generation_id_or_member_epoch` on
`TxnOffsetCommitRequest`).

Regression test: `crates/kafka-wire/tests/kip516_kip939_versions.rs` (each new field written and read back at its versions, and
left out or refused below them), and `tests/every_offered_version.rs`.
