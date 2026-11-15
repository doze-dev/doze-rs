# Data control, group b: DynamoDB, Kinesis, CloudWatch Logs (2026-11-15)

Each service implements `aws_wire::storage::Storage` on the SQS pattern (`Ask::Data` / `Request::Data` carrying a oneshot,
`Core::data` answering inside the region's actor, `Keyed::keys_on_disk` for the regions, deletes handed to the writer and then the
file compacted with the writer paused, so the deletes are on disk first). Bytes are the encoded rows (key + value) as they sit in
the redb file, counted by reading them, never estimated. Tests empty against a real temp directory and check `freed > 0`, a
restart loads exactly what the live core holds, a dry run removes nothing, and a reset frees the names at once.

## DynamoDB (`crates/aws-dynamodb/src/storage.rs`, test `src/tests/data.rs`)

- **Resource**: a table. **Units**: items.
- **Bytes**: its item rows + its index rows (secondary-index entries and time-to-live entries; an entry holds the key it points
  at, not a copy of the item) + its current stream's records. `items` counts item rows only.
- **Prunes**: none (`Names::prunes` is empty). An item carries no time it was written (no write time is stored with an item and
  none can be inferred), so `before` is refused with `Unsupported` ("a table can be emptied whole only: its items carry no time
  they were written"), as are `prefix`, `old-versions`, `unused`. Stream records do carry a time, but they are already trimmed at
  24 hours; pruning them alone would gain little and leave a consumer gap, so they go only with the table's items.
- **Empty** removes every item, index entry, time-to-live entry and stream record; keeps the table, key schema, indexes, TTL
  setting, stream settings (same stream ARN), PITR, insights, tags. The stream's numbering continues: the last sequence number is
  written into the table record (`Stream::last_seq`, which existed but was never written), and `last_seq()` now takes the larger
  of that and the last record on disk, so a consumer's position stays below every future record across an empty and a restart.
  The time-to-live timer is re-armed from the answer (to none, when nothing with a TTL is left).
- **Notes**: a table whose stream holds records: `in N stream records, kept 24 hours whether or not anything reads them`
  (bytes = those records, no prune: emptying the table removes them).
- **Reset** drops every table (through `drop_table`, immediately: no `DELETING` window), every Contributor Insights setting,
  every row of the stream space (including any left over by the bug below), sequence caches and transaction tokens.
- **Go weak (bettered)**: disabling a table's stream left its records in the file forever, with no reader (records of a stream
  that is not current are refused) and no owner (`DeleteTable` only removed the current stream's records). `UpdateTable` now
  removes them when the stream is switched off. AWS keeps a disabled stream readable for 24 hours; here it was already
  unreadable, so nothing AWS-visible changes. Files written before this fix may still hold such rows; they are not attributed to
  any table in `usage` (no table names their stream id), and `reset` removes them.
- **AWS-correct**: emptying is a local action with no AWS counterpart; nothing AWS-visible changes for tables not emptied.

## Kinesis (`crates/aws-kinesis/src/storage.rs`, test `src/tests/data.rs`)

- **Resource**: a stream. **Units**: records. **Bytes**: its record rows.
- **Prunes**: `before` (arrival time). A sequence number carries the millisecond its record arrived in (`seq >> 24`), the same
  property retention trimming uses, so `before` deletes, per shard, the row range below `ms_seq(before)`.
- **Empty** removes records; keeps the stream, all its shards (open and closed, with their lineage), consumers, retention,
  encryption, monitoring, policy, tags. Each shard's last sequence number lives in the stream record, not in its rows, so the next
  record is numbered above every earlier one.
- **What a consumer sees** (AWS-visible effect): the same as if the records had passed their retention. An iterator taken before
  the empty stays valid (it is a position, not a reference to a record) and its next `GetRecords` returns nothing until a record
  is put, then that record; `TRIM_HORIZON` starts at the first record kept; `AT_SEQUENCE_NUMBER` / `AFTER_SEQUENCE_NUMBER` of a
  removed record are accepted and read on from the next record kept (Kinesis does not check that the number is held). Tested:
  an iterator from before an empty reads `[]`, and a put after numbers above the last one before, also after a restart.
- **Notes**: a stream whose retention is above the default 24 hours, with records: `retention raised to N hours (the default is
  24): older records are kept`, bytes = records older than a day, prune = `before` now − 24 h.
- **Reset** removes every stream, the account-level settings (minimum throughput billing commitment), every record row.

## CloudWatch Logs (`crates/aws-logs/src/storage.rs`, test `src/tests/data.rs`)

- **Resource**: a log group. **Units**: events. **Bytes**: its event rows in all its streams.
- **Prunes**: `before` (the event's timestamp, which is the row key's order, as retention uses).
- **Empty** removes events; keeps the group with retention, KMS key, class, deletion protection, tags, metric filters,
  subscription filters; keeps its **streams, with no events** (decided: as AWS leaves a stream whose events expired; a
  `DescribeLogStreams` then shows it with no first/last event time and `storedBytes` 0). A stream's write count is kept, so its
  sequence token goes on moving. The per-stream held count and `storedBytes` are lowered by exactly what went; a stream with
  events left gets the first kept event's time as its `firstEventTimestamp`.
- **Notes** (one per group at most):
  - no retention and no local bound (`--logs-retention-days 0`): `no retention set: kept forever`, bytes = events older than
    7 days, prune = `before` now − 7 days. Lambda creates its groups this way without the person choosing.
  - no retention with the local bound (the default, 7 days): `no retention set: kept forever on AWS; doze removes events older
    than 7 days (--logs-retention-days) when the group is next written or read`, prune = `before` the bound. The bound is applied
    lazily, so a group nothing touches keeps its old events on disk indefinitely; the note says so and the prune removes them.
  - a retention, with events past it still on disk (same lazy sweep): `events past the retention, removed when the group is next
    written or read`, prune = `before` the cutoff.
- **Reset** removes every group, stream, held count, stored document (resource and account policies, query definitions, data
  protection policies) and every event row.

## Behaviour calls

| Behaviour | Call |
| --- | --- |
| DynamoDB: disabled stream's records removed on disable | Go weak (bettered: they stayed on disk unowned) |
| DynamoDB: stream numbering survives an empty and a restart | Go weak (bettered: `Stream::last_seq` was never written; numbering came from the newest record on disk only) |
| DynamoDB: `before` refused | AWS-correct (no write time exists to honour it) |
| Kinesis: iterators after an empty behave as after retention | AWS-correct (memory: AWS has no "empty", retention is the nearest) |
| Logs: streams kept with no events after an empty | AWS-correct (memory: streams outlive expired events) |
| Logs: local bound noted as lazy | Go weak (bettered: the bound only ran on a touch, silently) |

## Open

- `Storage` boilerplate (the four async methods that ask each region's actor) is repeated per crate as in SQS; a shared helper
  belongs in `aws-wire` (integrator's file), which would bring each `storage.rs` under the 300-line soft limit (DynamoDB 320,
  Kinesis 323, Logs 413; all under the 450 hard limit).
- DynamoDB stream rows orphaned by the old disable bug in existing files are not shown per table; only `reset` removes them.
- No conformance scenario: nothing here is AWS-visible beyond the stream-disable fix, which removed rows no API could read.
