# A10: DynamoDB (stage note, 2026-11-12)

Branch `aws-a10-dynamodb`, built beside the other A-stages in its own worktree. The integrator folds this note into `PLAN.md`,
`STATUS.md`, `scoreboard.md` and `weight.md`; none of those was touched here.

## What was built

Two new crates and three small changes to shared ones.

- **`aws-expr`** (3,120 lines with tests, no I/O, depends on `aws-core`, `serde`, `serde_json`): DynamoDB's data model and the five
  expression languages. `Number` is an exact decimal (up to 38 digits, exponent -130 to 125, add and subtract, order-preserving key
  bytes); `Value` and `Item` with the JSON wire form, sizes (the documented per-type counts) and depth; the lexer; one parser for
  condition and filter expressions (`AND`, `OR`, `NOT`, parentheses, the six comparisons, `BETWEEN`, `IN`, `attribute_exists`,
  `attribute_not_exists`, `attribute_type`, `begins_with`, `contains`, `size`); the update parser (`SET` with `+`, `-`,
  `if_not_exists`, `list_append`; `REMOVE`; `ADD`; `DELETE`; each clause once; overlapping and conflicting paths refused); projection;
  key conditions checked against the key schema; the 573 reserved words (a compiled-in text file, nothing built at start);
  `#name` and `:value` resolution with the unused-names and unused-values checks. A syntax error is reported before an undefined name
  or a reserved word, as AWS reads the whole text first. Fuzz target `fuzz_expression` (marked `// fuzz:parser`; `cargo xtask fuzz-check`
  passes; 2.6 million inputs in 60 seconds on nightly, no finding). 30 unit tests.
- **`aws-dynamodb`** (4,760 lines, 54 tests): the service, `Keyed<Core>` (one actor per region, home region `us-east-1`), nothing opened
  until the first request.
  - **Storage.** One redb file per region with four tables: the table records (loaded when the service opens: schema, indexes, stream,
    TTL, tags, PITR; nothing else is in memory), the items, the index entries and the stream records. Items are read from the file when a
    request needs them. Keys are byte strings that sort as DynamoDB sorts (strings and binary by bytes, numbers by value) and end by
    themselves, so a partition is a prefix. A secondary index is materialised (a global index is built by `UpdateTable` with a backfill),
    its entry holds the item's primary key and the item is fetched on read, so an index never holds a stale copy; the time-to-live
    index is the same kind of entry under index id 0. Every call's writes go in one transaction through the group-commit writer and the
    actor waits for the commit before it takes the next call (read-your-writes), so a batch or a transaction is one flush.
  - **Operations built (24 of 58):** `CreateTable`, `DescribeTable`, `UpdateTable`, `DeleteTable`, `ListTables`, `PutItem`, `GetItem`,
    `UpdateItem`, `DeleteItem`, `Query`, `Scan`, `BatchGetItem`, `BatchWriteItem`, `TransactGetItems`, `TransactWriteItems`,
    `DescribeTimeToLive`, `UpdateTimeToLive`, `DescribeContinuousBackups`, `UpdateContinuousBackups`, `TagResource`, `UntagResource`,
    `ListTagsOfResource`, `DescribeLimits`, `DescribeEndpoints` (answers the address the caller used, so endpoint discovery stays here).
  - **Refused by name** (`Service::serves` is false: 501 `NotImplemented`, `X-Doze-Not-Built`, before the request is read; 34 operations):
    global tables (8, with their settings and replica auto scaling), backups and restore (6), exports and imports (6), PartiQL
    (`ExecuteStatement`, `BatchExecuteStatement`, `ExecuteTransaction`), contributor insights (3), Kinesis streaming destinations (4),
    resource policies (3), `SearchVectors`. A test checks that `serves` agrees with the dispatch for all 58 operations.
  - **Legacy parameters are refused by name, never ignored:** `Expected`, `ConditionalOperator`, `AttributeUpdates`,
    `AttributesToGet` (also inside `BatchGetItem`'s tables), `KeyConditions`, `QueryFilter`, `ScanFilter` answer 501 `NotImplemented`
    naming the parameter and what replaces it. Also refused by name: a global index with a multi-attribute key, `ResourcePolicy`
    on `CreateTable`, and `ReplicaUpdates`, `MultiRegionConsistency`, `GlobalTableWitnessUpdates`, `VectorIndexUpdates` on `UpdateTable`.
  - **Tables:** the async states AWS reports. `CreateTable` answers `CREATING`, `DeleteTable` `DELETING`, `UpdateTable` `UPDATING`, a new
    global index is `CREATING` with `Backfilling`. Nothing waits for a timer: the states settle at the start of the next call, after a
    window that defaults to zero and is turned up by `DOZE_DDB_CREATE_MS`, `DOZE_DDB_UPDATE_MS`, `DOZE_DDB_DELETE_MS` (so a waiter passes
    on its first probe, and a test that wants to see the in-between state can). Schema rules (HASH first, definitions exactly the key
    attributes, index counts and names, projection rules, billing rules, the 3 to 255 name rule, deletion protection), global indexes
    added and removed by `UpdateTable` (backfilled on creation), local indexes, tags (50 at most, `aws:` refused), table class,
    warm and on-demand throughput echoed, the three answers that carry a description differ as the recording shows.
  - **Items:** put, get, update, delete with condition expressions, `ReturnValues` (`ALL_OLD`, `ALL_NEW`, `UPDATED_OLD`, `UPDATED_NEW`),
    `ReturnValuesOnConditionCheckFailure` (the item rides in the error body), key rules (missing, wrong type, empty, 2,048 and 1,024
    bytes), the 400 KB item limit, 32 levels of nesting, index key types checked at write, `ReturnConsumedCapacity` (totals only).
  - **Query and Scan:** key conditions with every sort-key operator, ascending and descending, `Limit`, `ExclusiveStartKey` and
    `LastEvaluatedKey` (also on indexes, where the key carries both the table's and the index's attributes), 1 MB pages, filter after the
    limit (`Count` against `ScannedCount`), projection, `Select` (all four, with AWS's refusals for a global index), consistent read
    refused on a global index, a local index fetches the whole item, a filter on a key attribute refused in a query, parallel scan
    segments.
  - **Batches:** `BatchWriteItem` (25, duplicates refused, one bad request writes nothing), `BatchGetItem` (100, duplicates refused,
    16 MB then `UnprocessedKeys`).
  - **Transactions:** all or nothing; `TransactionCanceledException` with the reason of each action in the message and in
    `CancellationReasons` (with the item when asked); one operation per item; 100 actions; 4 MB; `ClientRequestToken` replay for ten
    minutes (same request answers done, a different one is `IdempotentParameterMismatchException`). `TransactionConflict` never arises:
    the actor serialises everything.
  - **TTL:** the setting round-trips, the one-hour rule and the already-enabled/disabled refusals; expired items are deleted by a sweep
    at the start of the next call (at most once a second, 500 at a time) from the TTL index, so a read is never a scan; the stream record
    carries `userIdentity` of the service.
  - **Streams:** `UpdateTable` and `CreateTable` stream specifications (enable twice and disable with none are refused; each enabling is
    a new stream with a new label and ARN), records with the images the view type asks for, written only when an item changed, 24
    hours of retention, sequence numbers that continue across a restart. `Dynamodb::stream_records(region, stream_arn, after, limit)`
    reads them and `Dynamodb::stream_arrived()` resolves when a call has appended (the notification the plan asks for; an event source
    mapping in A13 awaits it). `Record::to_event` gives the Lambda record shape.
  - **PITR:** `UpdateContinuousBackups` and `DescribeContinuousBackups` round-trip the setting, the period (1 to 35, default 35) and the
    restorable window; `RestoreTableToPointInTime` is refused by name.
  - **Authorization** (`authz.rs`): `dynamodb:<Operation>` on the table ARN, the index ARN for a query or scan of an index, each table of
    a batch, the action of each part of a transaction (`PutItem`, `UpdateItem`, `DeleteItem`, `ConditionCheckItem`, `GetItem`), `*` for
    `ListTables`, `DescribeLimits` and `DescribeEndpoints`, the given ARN for tags. `dynamodb:LeadingKeys` and `dynamodb:Attributes`
    are **not supplied** (a policy that tests them finds them absent). Asked in `handle` the way the other services do; registered on
    the gateway and the peer bus in `doze-aws`.
- **Shared crates changed** (small, defaults keep the other services as they were):
  - `aws-wire`: `ApiError::with_body` and a boxed `BodyFields` carry extra members of an awsJson error body (`CancellationReasons`, the
    `Item` of a failed condition); boxed because a bare `Vec` pushed `ApiError` over clippy's 128-byte `result_large_err` limit in every
    crate. `Service::invalid_in(violation, body)`, default `invalid`, for a service that words a refusal by quoting the request.
  - `aws-gateway`: calls `invalid_in` where it called `invalid`. `aws-testkit`: the same.
  - `doze-aws`: the dependency and the registration in `run.rs`.

## Scoreboard

Conformance account `111122223333`, run as the earlier entries do (release binary, `UV_BIN` set to a real uv):

```
cargo xtask conformance --cmd "target/release/doze-aws --listen {addr} --data-dir {data} --account 111122223333" dynamodb
module           labels  exact  wording  behaviour  unverified
dynamodb             84     53        0          0          31
total: 84 labels, 63% exact, 63% ignoring wording
```

**Read it as: 53 of 53 recorded labels exact (100%; the Go edition on the same recordings: 68% exact, 91% ignoring wording). The 31 other
labels are in 8 scenarios written before AWS answered them (`test_dynamodb.py`, below the 2026-11-12 comment); they pass against
doze-aws and are UNVERIFIED until recorded.** The recording needs the owner's recorder policy to allow `dynamodb:UpdateTable`,
`UpdateContinuousBackups`, `DescribeContinuousBackups`, `TagResource`, `UntagResource`, `ListTagsOfResource` and `TransactGetItems`
on `doze-rec-*` tables (the policy generator has not been read for them here); PITR and the stream specification cost cents at most.

## What the recording taught

- `DescribeTable` is not `CreateTable` is not `DeleteTable`: create has no `WarmThroughput`, no `LastUpdateToPayPerRequestDateTime` and
  no `Backfilling`; describe has all three (warm throughput 12,000 reads and 4,000 writes per second on demand, on the table and on
  each global index); delete carries neither the schema nor the creation time. Each is built as recorded.
- `AttributeDefinitions` come back sorted by name (`at, owner, pk` for a request that said `pk, owner, at`).
- The count prefix is not uniform: `1 validation error detected:` leads the update expression's refusals (syntax, reserved word,
  undefined value, unused value) and an empty set, and does **not** lead the key condition's or any "One or more parameter values were
  invalid" text.
- An empty set says `An string set  may not be empty` (a double space).
- `Requested resource not found` has no suffix on an item operation and `: Table: <name> not found` on `DescribeTable`.
- A cancelled transaction names the reasons in its message: `[None, ConditionalCheckFailed]`.
- A batch of 26 is refused with the Java SDK's print of the whole request (`WriteRequest(putRequest=PutRequest(item={pk=AttributeValue(s=k0,
  n=null, ...`), which only the body can produce; that is why `Service::invalid_in` exists.
- A page with `Limit` reports `LastEvaluatedKey` even when the limit is the end of the data; a filter is applied after the limit.
- Items come back with sets sorted and numbers normalised (`1.50` is `1.5`).

## Wording that is not recorded

Every module of the service says so at the top, and `(memory)` marks the individual cases. Unrecorded: every table-schema refusal
(`HASH` first, definitions mismatch, index names and counts, billing combinations), the table-name pattern text, deletion protection,
"Table is being created/updated/deleted", `At least one of ProvisionedThroughput ...` (recalled, high confidence), the stream already
enabled and not enabled texts, "TimeToLive is already disabled" and the one-hour text, the tag texts, the item and key limit texts
(the 2,048-byte text with its missing space is recalled, high confidence), the nesting text, duplicate set members, number syntax and
range texts, `Query condition missed key schema element`, `Query key condition not supported`, the filter-on-a-key text, the start-key
texts, `Segment` texts, `ReturnValues` texts, the transaction duplicate-item text and the 4 MB text, `Too many items requested` for
batch writes, the idempotent-mismatch text, every runtime update-expression refusal (`An operand in the update expression has an
incorrect data type`, `The provided expression refers to an attribute that does not exist in the item`, `Type mismatch for attribute to
update`), overlap against conflict in `Two document paths ...`, `Invalid function name`, operand type and count texts, the `BETWEEN`
texts, the 4 KB expression text, and the legacy-parameter text (doze's own). Behaviour that is `(memory)` too: `<>` with a missing
attribute is true; `size()` counts characters; how a number is printed outside a modest range (plain between 1E-6 and 1E37, scientific
beyond); warm throughput for a provisioned table; consumed capacity units; the restorable window; the stream sequence number (21
digits) and event id; the stream label format; the order of keys in a Java print of a map.

## Decisions taken here and what is still open

1. **Transitional states have a window of zero by default.** A waiter passes on the first probe. The in-between state is visible only
   in the answer that starts it, unless `DOZE_DDB_*_MS` is set. (Owner: a CLI flag, or a `doze.hcl` setting, instead of environment
   variables?)
2. **Time to live is a sweep at the start of the next call**, not a timer: nothing wakes while idle. Expired items linger until the
   next call after their time, as on AWS (which takes up to days). The `Expiring` mechanism was not needed.
3. **Writes wait for their commit inside the actor.** One flush per call (a batch or a transaction is one call), and calls are
   serialised. The alternative (several calls per flush) means the commit and the read-modify-write on the writer thread; revisit if a
   workload of concurrent single puts shows the cost.
4. **`DescribeTable` counts items and bytes from the file** (AWS refreshes them every six hours); on a large table that is a scan.
5. **The DynamoDB Streams wire API is not built.** `ListStreams`, `DescribeStream`, `GetShardIterator` and `GetRecords` belong to a
   different service (`DynamoDBStreams_20120810`) with no model in `models/`; adding it means a model file and a codegen run. The stream is
   complete behind it: `stream_records` and `stream_arrived` are what the Lambda mapping (A13) needs.
6. **Not built, named in the plan row:** the DynamoDB-to-Kinesis destination (Kinesis is A11; the four operations are refused by name)
   and the opt-in fault injectors. PartiQL is refused by name; it is a parser of its own.
7. **`ItemCollectionMetrics`** is not returned (only a table with a local index returns it, and only when asked). **`ConsumedCapacity`**
   is reported as totals for the item operations and queries and not at all for batches and transactions; `INDEXES` is answered as `TOTAL`.
8. **Item order on a scan** is the order of the keys' bytes, not AWS's hash order (AWS promises none).
9. **A disabled stream's records are dropped**, where AWS keeps them readable for 24 hours; nothing reads them here.
10. **Global index key and hash-key type changes are not allowed after creation**, and an item that cannot be indexed at backfill (a key
    of the wrong type) is left out of the index.

## Tests

`cargo test -p aws-expr -p aws-dynamodb`: 30 + 54. Every recorded behaviour above has a test; the restart-after-every-call scenario
(`every_call_leaves_what_the_next_one_reads`), a restart that keeps tables, items, a global index, a stream and its sequence, tags and
PITR, the regions kept apart through the service, the notification, the 1 MB page, the 4 MB transaction, the 16 MB batch get and the
refusal list against `serves` are among them. A real process was started, written to over boto3 (30 items, a global index, a stream,
a transaction), stopped with SIGTERM and started again: the query, the index query, the item count and the stream ARN came back.

## Weight (ready to paste)

With DynamoDB the whole binary is 5,851,040 B (macOS arm64 release, thin LTO; 5,097,200 B before), 202 packages in the weight tool's
count (198 before; three are the new crates `aws-dynamodb`, `aws-expr`, `aws-model-dynamodb`, the fourth was not traced) and idles at
3,456 KB (3.3 MB before). No third-party crate was added: `redb`, `postcard`, `serde_json` and `tokio::sync` were already linked. The cost
is the DynamoDB model's tables, the expression engine and the service: +753,840 B. The limits in `xtask/weight-limits.txt` stand
(7,000,000 B, 203 packages, 6,144 KB); one package is left, so the next service moves the package limit. Nothing is opened until the
first request, items stay on disk, and the only data kept in memory while the service is live is one record per table.

## Gates

See the report that accompanies this note for the exit code of `cargo xtask gates` at the last commit.
