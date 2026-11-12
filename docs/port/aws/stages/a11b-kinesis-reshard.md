# A11b: Kinesis resharding and account settings

Branch `aws-kinesis-reshard`, 2026-11-13. Closes the gap the integrator's parity check found against the Go edition: `SplitShard`,
`MergeShards`, `UpdateShardCount`, `DescribeAccountSettings` and `UpdateAccountSettings` were refused by name (PLAN E9); the owner's bar
("at least as good as the Go edition, covering everything it covered") overrules E9. `PLAN.md`, `STATUS.md`, `scoreboard.md` and `weight.md`
are not edited here; the integrator folds this in.

## What was built (all in `crates/aws-kinesis`)

- **`SplitShard`**: closes the parent at the current end of the stream and opens two children. The split key is the first hash of the upper
  child (the lower ends one below it); it must lie after the parent's start and at or before its end. Children name the parent in
  `ParentShardId`. The parent gets an `EndingSequenceNumber` at once, even if it never held a record (this is the recorded behaviour).
- **`MergeShards`**: both shards must be open and must be neighbours (a gap would drop hash space); the child spans both ranges, names
  `ShardToMerge` as `ParentShardId` and the other as `AdjacentParentShardId`.
- **`UpdateShardCount`** (`UNIFORM_SCALING`): closes every open shard and re-tiles the hash space in equal ranges. A child names the shard
  that owned its first hash as parent and, if it spans two, the one that owned its last hash as adjacent parent. Doubling therefore splits
  each shard in two, halving merges pairs (exactly the AWS shapes); other counts are the same rule applied to uneven overlaps (three
  or more overlapped parents name only two). The answer carries `StreamName`, `CurrentShardCount`, `TargetShardCount`, `StreamARN`.
- **Closed shards**: `GetRecords` on a closed shard reads to its last record; the read that exhausts it answers with no
  `NextShardIterator`, `MillisBehindLatest` 0 and `ChildShards` (`ShardId`, `ParentShards`, `HashKeyRange`). An iterator made before the
  split keeps working. Closed shards are forgotten (listing, iterators) once their last record is past the stream's retention.
- **`UPDATING`**: `DescribeStream`, `DescribeStreamSummary` and `ListStreams` report `UPDATING` for one second after a reshard, `ACTIVE`
  otherwise; a reshard on a stream that is not `ACTIVE` is `ResourceInUseException`. Reads and writes continue meanwhile. The status is
  derived from the children's opening times (a sequence number carries its arrival time), so no field was added to the stored records and
  no timer exists.
- **`ListShards` `ShardFilter`**: `AFTER_SHARD_ID`, `AT_TRIM_HORIZON`, `FROM_TRIM_HORIZON`, `AT_LATEST`, `AT_TIMESTAMP`, `FROM_TIMESTAMP`
  (the model's own definitions). The filter travels in `NextToken`, so a later page is cut the same way.
- **`DescribeAccountSettings` / `UpdateAccountSettings`**: the minimum throughput billing commitment as a persisted row (a new `account`
  table in `kinesis.redb`), moving through the model's states: `DISABLED` -> `ENABLED` (24 h minimum, `EarliestAllowedEndAt`) ->
  `ENABLED_UNTIL_EARLIEST_ALLOWED_END` (a disable asked for early, cancelled by enabling again) -> `DISABLED` (`EndedAt`). It bills nothing
  and nothing local reads it. (Go answered a constant; this is the contract in the model.)
- Sequence numbers: a put never takes a number below the shard's starting number (a child opened in the same millisecond starts a little above
  the clock floor). Children start above everything their parents held and never share a starting number.

## Scoreboard

`cargo build --profile release-aws -p doze-aws`, then
`UV_BIN=... cargo xtask conformance --cmd "$PWD/target/release-aws/doze-aws --listen {addr} --data-dir {data} --account 111122223333" kinesis`:

```
module           labels  exact  wording  behaviour  unverified
kinesis              58     30        0          0          28
total: 58 labels, 51% exact, 51% ignoring wording
```

Before: 36 labels, 27 exact, 9 unverified. The 3 recorded labels of `test_a_split_closes_the_parent_even_if_it_never_held_a_record`
(`split`, `parent-closed`, `shards`) are now reached and exact: **30 of 30 recorded labels exact**, nothing exact lost, 0 behaviour,
deviations list still empty. The other 19 new labels are UNVERIFIED (scenarios written ahead of a recording, 2026-11-13):
`test_a_merge_names_both_parents_and_a_closed_shard_reads_to_its_end` (gap, self, absent-shard and closed-shard refusals, merge, shards,
both parents drained with their `ChildShards`), `test_a_split_hands_a_records_shard_over_to_its_children` (out-of-range key, closed-shard
split refusal, drain, `AT_LATEST` listing, routing after), `test_a_new_shard_count_re_tiles_the_stream` (scale up, shards, scale past double,
summary), `test_account_settings` (describe only: enabling a commitment on a real account bills it for a day, so the scenario does not
update). The recorder policy will need `kinesis:SplitShard`, `MergeShards`, `UpdateShardCount`, `DescribeAccountSettings`.

## (memory): worded or ruled from memory, not from a recording

Each is marked `(memory)` in a code comment.

- `UPDATING`: how long it lasts (1 s), that a reshard is refused while it lasts and the words
  `Stream <name> under account <account> not ACTIVE, instead in state UPDATING`.
- Every split/merge refusal: the words and codes for a split key out of range, a closed shard, a shard merged with itself, a merge of a
  closed shard, a merge of non-neighbours (`... are not an adjacent pair of shards eligible for merging.`). The not-found wording for an
  unknown shard reuses the recorded `GetShardIterator` one. Whether the split key bounds are `(start, end]` is Go's rule, not recorded.
- `UpdateShardCount`: `LimitExceededException` for more than double or less than half (the model documents both as default limits), the
  `ON_DEMAND` refusal (`ValidationException`), and that asking for the current count is accepted and changes nothing.
- Which parent is `ParentShardId` in a merge (the shard to merge) and the `ParentShards` order in `ChildShards`.
- That the read exhausting a closed shard carries both its last records and the missing iterator (rather than a following empty read).
- `ShardFilter` details: `AT_TRIM_HORIZON` as "open at the start of what is kept", the `FROM_*` variants, and the refusals for a missing
  `ShardId` or `Timestamp`.
- Account settings: the whole state machine, since no recording exists (the model text is the source).

## Not built

`UpdateShardCount` lineage for uneven counts is an approximation of AWS's pairwise plan (above). Split/merge on an `ON_DEMAND` stream is
accepted (not known to be refused). Still refused by name: `SubscribeToShard` (HTTP/2), channels,
`UpdateStreamRecordDistributionStrategy`.

## Tests

`aws-kinesis`: 37 (was 21): 16 new in `tests/reshard.rs`, `tests/scaling.rs` and `tests/state.rs` (recorded split shape; `UPDATING` and the second-reshard refusal; reading a closed
shard to its end with and without a `Limit`, and an iterator made before the split; routing and ordering after a split; split-point and
shard refusals; merge, merge refusals; doubling, halving, uneven counts; count bounds, `ON_DEMAND`, unknown stream; the 500-shard limit;
every `ShardFilter` type, their refusals and filter-preserving paging; retention forgetting a closed shard; restart after a reshard; the
commitment's states and restart) and the refused-by-name test reworked.

## Weight

`cargo xtask weight doze-aws` (darwin/arm64, `release-aws`): **5,404,928 B, 220 packages, 3,632 KB idle (3,584 on an earlier run: noise), verdict ok** against
`6,800,000 B / 224 packages`. No new third-party package (`billing.rs` and `reshard.rs` use only crates already linked). Nothing runs until a
request: no task and no timer; `UPDATING` is computed on read.
