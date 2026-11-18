# dynamodb-ops: the 19 unbuilt DynamoDB operations and the legacy parameters (stage note, 2026-11-18)

Branch `aws-dynamodb-ops`, on `main` at `f864c8dd9`. The integrator folds this note into `PLAN.md`, `STATUS.md` and `scoreboard.md`;
none of those was touched here. AWS is the only reference: each behaviour cites the model (`models/dynamodb.json`) or the developer
guide, and every wording not seen recorded is marked `(memory)` in the code beside it. **Nothing here is recorded**: the
`doze-rec-main` SSO session had expired (`Token has expired and refresh failed`), so every new scenario is UNVERIFIED until the
integrator records it.

## 0. Plumbing (`core.rs`, `work.rs`, `extras.rs`, `disk.rs`)

- One new meta table, `extras`, holds every new small record under a key that says what it is (`backup/<arn>`, `export/<arn>`,
  `import/<arn>`, `policy/<arn>`, `kinesis/<table>`, `restore/<table>`), so the table record keeps its on-disk shape. Two new redb
  spaces: `backup` (a backup's items under its id) and `pitr` (the recovery log). A file made before them gets the tables on open.
- **Work outside the actor** (`work.rs`): a call can leave work (send records to Kinesis, check a stream exists, write an export to
  S3, read an import from S3). `Dynamodb::ask` does it after the actor answers and hands what it found back as `Request::Done`; the
  caller is answered once that is in. The actor never waits on another service (S3 can start a function that calls DynamoDB back).
  The time-to-live timer's answer carries Kinesis records too, sent by the timer's callback. No thread, no timer.
- `finish` and `housekeeping` share `save()`; a refused call drops what it staged (`pending` and `work`).

## 1. Legacy parameters (`legacy.rs`)

`AttributesToGet`, `KeyConditions`, `QueryFilter`, `ScanFilter`, `Expected`, `ConditionalOperator` and `AttributeUpdates` (and
`AttributesToGet` per table of `BatchGetItem`) are translated into the expression they stand for before the call is handled, with
placeholders of their own (`#l0`, `:l0`), and then run by the same code as an expression request. They were refused by name before.

- Rules from the developer guide ("Legacy conditional parameters"): mixing a legacy and an expression parameter is refused (the guide:
  "DynamoDB does not allow mixing legacy conditional parameters and expression parameters in a single call"); operand counts per
  operator (`NULL`/`NOT_NULL` none, `BETWEEN` two, `IN` one or more, the rest one); `Exists: false` is `attribute_not_exists`, a bare
  `Value` or `Exists: true` with one is equality; `KeyConditions` take `EQ LE LT GE GT BEGINS_WITH BETWEEN` only; `AttributeUpdates`
  `PUT` (default) sets, `ADD` adds a number or set, `DELETE` without a value removes and with a set subtracts; an update of only
  `DELETE` actions on an item that is not there makes nothing ("The operation succeeds, but DynamoDB does not create a new item").
- Wording, all `(memory)`: `Can not use both expression and non-expression parameters in the same request: Non-expression
  parameters: {AttributesToGet} Expression parameters: {ProjectionExpression}`; `ExpressionAttributeNames can only be specified when
  using expressions`; `One or more parameter values were invalid: Invalid number of argument(s) for the EQ ComparisonOperator`;
  `Attempted conditional constraint is not an indexable operation`; `ComparisonOperator BEGINS_WITH is not valid for N
  AttributeValue type`; `ConditionalOperator can only be used when Filter or Expected has two or more elements`; `Only DELETE action
  is allowed when no attribute value is specified`; `Cannot specify the AttributesToGet when choosing to get ALL_ATTRIBUTES`; and the
  `Exists`/`Value` ones. `NE` and `NOT_CONTAINS` on a missing attribute behave as the expressions `<>` and `NOT contains` do (the
  guide does not say).

## 2. Backups (`backup.rs`, `restore.rs`)

- `CreateBackup` copies the table's items into the backup space and its table record into the backup; `DescribeBackup` answers
  `BackupDetails`, `SourceTableDetails` and `SourceTableFeatureDetails` (indexes, stream, time to live, encryption, as the model lists);
  `ListBackups` filters by table, type (`USER` default, `SYSTEM`, `ALL`; `AWS_BACKUP` is empty) and time range, and pages by
  `ExclusiveStartBackupArn`; `DeleteBackup` answers `DELETED` and frees the copy. The ARN is `<table arn>/backup/<14 digits of ms>-<8
  hex>` (memory for the form). Answers `CREATING` once; describe says `AVAILABLE`.
- Deleting a table with point-in-time recovery on makes a `SYSTEM` backup (the model), named `<table>$DeletedTableBackup` (memory),
  with `BackupExpiryDateTime` 35 days on, removed by housekeeping when that passes.
- `RestoreTableFromBackup` makes a new table from the backup's record and items, indexes rebuilt; without stream, time to live, tags,
  recovery or deletion protection (the model's "must manually set up" list). `GlobalSecondaryIndexOverride` and
  `LocalSecondaryIndexOverride` keep the indexes they name (each must exist), `BillingModeOverride`, `ProvisionedThroughputOverride`,
  `OnDemandThroughputOverride` and `SSESpecificationOverride` apply. `RestoreSummary` (`SourceBackupArn`, `SourceTableArn`,
  `RestoreDateTime`) is on the answer (`RestoreInProgress: true`, `CREATING`) and on later describes (`false`).
- `doze-aws data` lists each backup as a resource of its own (`<table>/backup/<id>`, it outlives its table), and empties it by that
  name; a reset removes every backup.

## 3. Point-in-time recovery (`pitr.rs`) and `RestoreTableToPointInTime`

- What history exists: the table's stream holds 24 hours only when a stream is on and in the view the table chose, so it is not the
  basis. While recovery is on, every change of an item (writes, deletes, time-to-live deletions, imports) files the item **as it was
  before** under the table's id and the change's time: the recovery log. The table at time T is today's items with every item changed
  after T put back as its first later entry says. One map of the changed keys is held while a restore reads; nothing in memory at rest.
- **The window is a day** (`LOG_MS`), whatever `RecoveryPeriodInDays` says (weight: five weeks of every write is not a laptop's
  business). `EarliestRestorableDateTime` reports `max(recovery enabled, now - min(period, 1 day))` honestly; stale entries are trimmed
  100 at a time as the table is written; turning recovery off drops the log (AWS forgets the history too), and so does deleting the
  table. `LatestRestorableDateTime` is the present (AWS: about five minutes ago): listed as a difference.
- `RestoreTableToPointInTime`: by `SourceTableName` or `SourceTableArn` (one), `RestoreDateTime` or `UseLatestRestorableTime` (one);
  recovery off is `PointInTimeRecoveryUnavailableException`, a time outside the window `InvalidRestoreTimeException`; the new table
  takes the source's current settings (the model) and the items as they were.
- `doze-aws data` counts the log with its table, with a note.

## 4. Exports and imports (`export.rs`, `import.rs`, `ion.rs`)

- `ExportTableToPointInTime` (recovery must be on), `DescribeExport`, `ListExports`. Full exports read `items_at(ExportTime)`;
  incremental ones (`IncrementalExportSpecification`, 15 minutes to 24 hours (memory), `NEW_AND_OLD_IMAGES` or `NEW_IMAGE`) read the
  log: one line per item changed in the window with `Keys`, `NewImage`, `OldImage` and `Metadata.WriteTimestampMicros`, nothing for
  an item made and deleted inside it (the guide's table).
- Layout, from the developer guide ("DynamoDB table export output format"): `<prefix>/AWSDynamoDB/<export id>/_started` (empty),
  `manifest-summary.json` (`version`, `exportArn`, `startTime`, `endTime`, `tableArn`, `tableId`, `exportTime`, `s3Bucket`, `s3Prefix`,
  `s3SseAlgorithm`, `s3SseKmsKeyId`, `manifestFilesS3Key`, `billedSizeBytes`, `itemCount`, `outputFormat`, and for incremental
  `exportType`, `exportFromTime`, `exportToTime`, `outputView`), `manifest-files.json` (JSON lines: `itemCount`, `md5Checksum`,
  `etag`, `dataFileS3Key`), `manifest-summary.md5`, `manifest-files.md5`, and `data/<26 chars>.json.gz` (incremental: the shared
  `AWSDynamoDB/data/`). The guide is inconsistent about the checksum files (`.md5` in its tree, `.checksum` in its list); `.md5` with
  the base64 MD5 of the manifest is (memory), and so are the keys being whole S3 keys (prefix included). One data file: a table here
  is one partition.
- Formats: DynamoDB JSON (`{"Item": ...}` lines) and Ion text (`$ion_1_0 {Item:{...}}`; numbers as decimals `103.`, `6d2`; sets as
  `$dynamodb_SS::[...]`; binary as blobs), gzip. `ion.rs` is a writer and a bounded reader of the text form (no external crate),
  unit-tested against the guide's example; the import readers (JSON lines, Ion, CSV) have a fuzz target, `fuzz_import` (450,000
  runs, no finding).
- `ImportTable`, `DescribeImport`, `ListImports`: the table is made `CREATING`; every object under the prefix is read outside the
  actor, unpacked (`GZIP`, `NONE`), parsed (DynamoDB JSON lines; Ion lines or a list; CSV per the guide: header first unless
  `HeaderList`, RFC 4180 quotes, the key attributes of table and indexes typed by their definitions and everything else a string,
  empty columns left out), and the items written in one go. Items that cannot be read or do not fit the key schema count in
  `ErrorCount`. No source (no bucket, no objects) is `FAILED` and the table goes (memory). `ZSTD` and an export's
  `FilterSpecification` are refused by name: no zstd decoder is linked (size), and the filter is a newer feature (open).
- Client tokens (both): a repeat within 8 hours answers the same task; a different request is `ExportConflictException` /
  `ImportConflictException` (the model).
- S3 is called as DynamoDB over the bus (`acting`), so the bucket's own policy is not consulted; `NoSuchBucket` is
  `FailureCode: S3NoSuchBucket` (memory).

## 5. Resource policies (`policy.rs`)

- `PutResourcePolicy`, `GetResourcePolicy`, `DeleteResourcePolicy` on a table or its current stream, and `ResourcePolicy` on
  `CreateTable` (it was refused). Checked as IAM checks a resource policy (`aws_iam_policy::check`, `Kind::Resource`) and against the
  model's 20 KB; `ExpectedRevisionId` (`NO_POLICY` for none) holds a put or delete to a revision, else `PolicyNotFoundException`; the
  same document again keeps its revision (the model); deleting with no policy answers no revision (the model).
- **Evaluated**: `Core::needs` hands each request's needs to IAM with the policy of the table (also for its indexes, as the model
  says) or the stream; the Streams API (`aws-dynamodb-streams`) now asks `Dynamodb::policy_of` for the stream's policy. Tested with
  IAM in force: a user allowed `dynamodb:*` by its own policy is refused `GetItem` and an index `Query` by the table's deny, and reads
  once the policy is deleted. A table's policies go with it.
- Departure: the account root (a key IAM did not issue) is allowed everything by doze-aws's IAM, so a deny binds users and roles only.

## 6. Kinesis streaming destinations (`kinesis.rs`)

- `EnableKinesisStreamingDestination` answers `ENABLING`; the stream is looked up (`DescribeStreamSummary`) outside the actor and the
  destination is `ACTIVE`, or `ENABLE_FAILED` with `DestinationStatusDescription`, before the call answers. `Disable...` answers
  `DISABLING` and is `DISABLED`; `Update...` changes `ApproximateCreationDateTimePrecision` (`UPDATING`, then `ACTIVE`); `Describe...`
  lists them. One live destination per table (memory).
- Every change on a table with an `ACTIVE` destination goes to `PutRecords` (500 a call) after the call: `awsRegion`, `eventID`,
  `eventName`, `userIdentity` (the service for a time-to-live deletion), `recordFormat: application/json`, `tableName`,
  `eventSource: aws:dynamodb`, and `dynamodb` with `ApproximateCreationDateTime` (milliseconds, or microseconds when asked: the guide),
  `ApproximateCreationDateTimePrecision`, `Keys`, `NewImage`, `OldImage`, `SizeBytes`. The guide shows only the consumer's reads of
  `dynamodb.*`; the top-level members and the partition key (the item's partition key value) are (memory).

## 7. Tests and scenarios

- Crate tests: `tests/legacy.rs`, `tests/backups.rs` (backup round trip, overrides, point-in-time restore and its window, `SYSTEM`
  backups expiring, a restart), `tests/policies.rs` (revisions; IAM in force), `tests/s3_kinesis.rs` (S3, Kinesis and IAM on one bus:
  export layout and manifests, Ion export, CSV import, failures, Kinesis records); `ion.rs` and `import.rs` unit tests.
  `refusals.rs` no longer lists the built operations; the old "legacy parameters are refused" test is gone (they run now).
- boto3 (`conformance/tests/test_dynamodb.py`), **all UNVERIFIED** (67 labels, every one passes against doze-aws):
  `test_legacy_reads`, `test_legacy_writes`, `test_legacy_refusals`, `test_backup_lifecycle`, `test_backup_refusals`,
  `test_point_in_time_restore`, `test_resource_policy_lifecycle`, `test_export_and_import`, `test_export_refusals`,
  `test_kinesis_streaming_destination` (one Kinesis shard for the test's length). The 116 recorded labels score as before (115
  exact; `test_streams_api` is the one known behaviour difference).

## Open

- Recording every scenario above (the SSO session had expired), and with it every (memory) wording.
- `FilterSpecification` on exports; `ZSTD` imports (needs a decoder: size budget); an import's CloudWatch Logs error log.
- Resource-policy deny for the account root (an IAM-wide decision, not DynamoDB's).
- AWS's `BackupInUseException` / `TableInUseException` while a backup or restore runs: nothing here is ever in progress.
