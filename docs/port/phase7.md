# Phase 7: topic admin, configs, retention and compaction

Status: DONE (2026-11-10) after two independent audits (the first, at 9eded61, said not done: one blocker, four major; the second,
at b394527, said done, with minors and test gaps that are all closed: `debt.md` section H). Catalogue: behaviours/admin.md and behaviours/compaction.md. Goal: the topic
configuration a client sets is the configuration the broker acts on; a partition count can be raised; a partition's start can be
moved; old data is deleted by age and size on its own; a compacted topic keeps the latest record of each key. The recorded
Apache Kafka 4.3.1 scenarios of these APIs pass (`list-config-append`, the Phase 7 rows of `version-matrix`, `metadata-autocreate`).


## What carries in from earlier phases (`debt.md`)
- `Peek::read` insists that a batch's record count agrees with its last offset delta; compaction breaks that on purpose (a
  thinned batch keeps its span). It is relaxed in 7d, and the log walks (`visit_batches`, `read`, the producer history, the
  transaction index) advance by the last offset delta, not by the record count.
- `segment.bytes`, `index.interval.bytes` and the rest of the topic configs must act on an open log at runtime
  (AlterConfigs), not only at open.
- Rewriting a batch into the topic's `compression.type`.

## Sources
- Apache Kafka 4.3.1 source, read item by item by a research agent (copies in the scratchpad `k431p7/`), because most claims
  of this area in `conformance/claims/topics.md` are "memory", not recorded. Marked **(source)** in the catalogue.
- The recordings: `topic-config-keys`, `list-config-append`, `metadata-autocreate` (its `IncrementalAlterConfigs` step),
  `createtopics-validation`/`-collisions` (the config wording already passes), `version-matrix` (the versions of these APIs),
  `full-disk-{alter-configs,create-partitions,delete-records}` (fault scenarios; the Rust edition tests them with `SimFs`).
- The Go edition (`internal/broker/{configs,alterconfigs,createpartitions,deleterecords}.go`,
  `internal/storage/log/compact.go`, `docs/storage-and-durability.md` "Compaction"); domain inspiration only. Its compaction
  design is the right shape (sealed segments only, below the last stable offset, rewrite and swap, offsets never move,
  tombstones kept for `delete.retention.ms`, aborted data dropped, a marker kept while a record of its transaction is).
- Prior art outside: Kafka's own LogCleaner is the reference; the Rust brokers surveyed in Phase 0 (Nisshi, Crabka) skip
  compaction.

## Design
**Configs.** The topic's own overrides live in its `kafka-meta` record (already, from CreateTopics). A new
`MetaStore::set_topic_config(name, new_overrides)` rewrites the record durably and bumps `topics_version`. The pure table in
`topic_config/` (keys, kinds, Kafka's wording) validates every change; DescribeConfigs answers from the same table (value,
source, `is_default`, read-only, sensitive, type, synonyms, documentation by version). Broker resources (`BROKER`, id or the
empty default) answer the broker's static configuration read-only, and an alter of a broker config is refused in Kafka's
words (`Cannot update these configs dynamically: [...]`). AlterConfigs (full replace) and IncrementalAlterConfigs (SET, DELETE,
APPEND, SUBTRACT) both reduce to "the new overrides map", validated once, written once.

**Acting on a config.** What the log needs (`segment.bytes`, `index.interval.bytes`) is sent to the partition's writer as a
command and takes effect at the next roll; what a request needs (`max.message.bytes`, `message.timestamp.*`,
`compression.type`, `cleanup.policy`) is read from the topic record per request, which is already how Produce does it; what a
sweep needs (`retention.ms`, `retention.bytes`, `segment.ms`, the compaction settings) is read by the sweeper each pass.

**Retention.** One broker task (`log.retention.check.interval.ms`, default 5 minutes, a test knob like the offsets one) walks
the topics: an open partition is asked through its writer (`enforce_retention`, which exists in storage), a closed one is
judged from its files (`enforce_closed`, which exists). `segment.ms` rolls the active segment at the next append once its
wall-clock age (since its first write) passes it; an idle partition keeps its segment until then (ADM-D7).

**DeleteRecords.** Storage already moves a log start (`PartitionLog::delete_before`). The handler adds Kafka's checks, the
response's low watermark, the producer-state and transaction-index effects of the start moving (both are pruned by the start),
and the read-committed effect.

**CreatePartitions.** The topic record gains partitions (count only grows), the directories are made durably, the new
partitions open on first use; validate-only changes nothing; assignments are checked and ignored on a one-broker cluster.

**Compaction** (storage `compact/`, broker `cleaner`). A pure planner chooses the logs and segments from sizes, ages, the
dirty ratio and the lags; an executor reads the dirty sealed segments (below the last stable offset) into a key to latest
offset map, rewrites each eligible segment to a `.tmp` file beside it and puts it in place a few segments at a time (the index
removed first, the log renamed over, the index renamed in: the sequence that is safe against a stop at any point, tested with
`SimFs` stops at every operation as the log's own recovery is), and removes a segment left with nothing but its ending batch
(the first segment excepted: CMP-D1). Offsets never move; batches keep base offset and offset span, so records leave gaps; the empty batch
that holds the last offset or producer sequence is kept. Tombstones stay for `delete.retention.ms`; aborted data goes; a marker
stays while a record of its transaction does. The reader, the walk and the index accept gaps. Produce refuses a keyless
record on a compacted topic (INVALID_RECORD, per-record errors). `compact,delete` compacts first and retains after.

## Slices
| | Slice | Gate |
|---|---|---|
| 7a | Retention sweeper for logs (open and closed), `segment.ms`; DeleteRecords (API 21, v0-2) | oracle `version-matrix` rows for DeleteRecords; tests of the sweep with `SimFs` time; full-disk DeleteRecords with `SimFs` |
| 7b | Config APIs: DescribeConfigs v0-4, AlterConfigs v0-2, IncrementalAlterConfigs v0-1, topic and broker resources; `set_topic_config`; configs acting on a live log | oracle `list-config-append`, `metadata-autocreate`, `topic-config-keys`, `version-matrix` rows; crash sweep covers the config write |
| 7c | CreatePartitions (API 37, v0-3) | oracle `version-matrix` row; stop sweep over the directory creation |
| 7d | Compaction: `Peek` and the walks accept gaps; planner; executor and swap with crash recovery; cleaner task; keyless refusal; compression rewrite | simulation: random keyed workloads with stops at every operation against a model (latest per key survives, offsets unchanged, no reader sees a half-swapped log); real clients on a compacted topic in `interop/` |
| 7e | Catalogue, weight, interop, independent audit, debt register updated | gates as before |

Gates as before: fmt, clippy `-D warnings` (run before every commit, never chained after it), tests, lint-size, weight (binary
4.2 MB, 115 packages, 8 MB idle: compaction and the config APIs must fit), the oracle, interop. Commit per slice, no push.

## Risks
- Compaction is the largest correctness risk of the port so far: a swap that loses a record, or an index that disagrees with a
  rewritten log, is silent data loss. The stop-at-every-operation sweep and a model comparison are the evidence; the audit gets
  the property statement first.
- A rewritten batch under a different codec changes its CRC and size: the cleaner works on whole batches, the wire crate's
  recode is already there.
- The cleaner and a transaction: only below the last stable offset, and the transaction index must be consistent with the
  rewritten log (aborted ranges refer to offsets that stay; a marker is kept while a record of its transaction is).
- Configs that Kafka accepts and a single broker cannot honour (`min.insync.replicas` above 1, replication settings): accepted
  and shown, with the effect Kafka has on one broker where there is one (acks=all below the minimum is NOT_ENOUGH_REPLICAS).
