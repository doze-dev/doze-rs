# Phase 6: idempotent producers and transactions

Status: audited twice (2026-11-09): the first said NOT DONE (a deadlock and six majors; `debt.md` A), the second NOT DONE (three majors, small); both sets fixed. See the closure entry in `PLAN.md`. Catalogue:
behaviours/transactions.md (written as slices land). Goal: an idempotent producer's retried batch is recognised and not
written twice; a transactional producer (franz-go, librdkafka, sarama, the Java client's classic protocol) writes to several
partitions and commits consumer offsets atomically; a `read_committed` consumer sees all of a transaction or none of it;
the recorded Apache Kafka 4.3.1 transaction scenarios pass.

Sources: the Go edition (`internal/broker/idempotency.go`, `txn.go`, `txnapis.go`, `producerid.go`, `docs/transactions.md`;
domain inspiration only), the recordings `endtxn-decisions`, `endtxn-while-preparing`, `txn-empty-id`,
`idempotent-write-from-topic-write`, `produce-validation` (its `unknown-pid-seq-*` steps, deferred from Phase 3), the
`full-disk-*` fault scenarios (a Go-harness fault knob; the Rust edition tests the same with `SimFs`), and
`conformance/claims/api.md`. The classic protocol only, as the Go edition: Produce is offered no newer than v9, so clients
that would use KIP-890 transaction v2 fall back to it.

## Design

**Idempotence** (`kafka-broker/src/producers/`, pure): per partition, per producer id, the epoch and the last five batches'
sequence ranges with the offset each was given (Kafka's in-flight window). `check(pid, epoch, first, last) -> Verdict`
(`Accept`, `Duplicate{offset, timestamp}`, `DuplicateOld`, `OutOfOrder`, `Fenced`) and `record`. A producer idle for 15
minutes is forgotten (Kafka's `producer.id.expiration.ms`), with a hard ceiling per partition. The check, the append and the
record are one step under the partition's lock, so two retries cannot both append. After a restart the state is rebuilt from
the last 16 MiB of the log (`PartitionLog::producer_history`), so a batch whose acknowledgement was lost in the stop is
still recognised.

**Control batches and the transaction index** (`kafka-wire` builds and reads control batches; `kafka-storage` scans): a
marker is a control batch with the producer's id and epoch, one record keyed COMMIT or ABORT. A partition's index of open
transactions (producer -> first offset) and aborted ranges is rebuilt from batch headers when the partition is first
produced to or read `read_committed` after a start, and kept as batches are appended. The **last stable offset** is the
first offset of the oldest open transaction, else the log end. A partition that never held a transactional batch is not
scanned.

**The coordinator** (`kafka-broker/src/txn/`): one record per `transactional.id` in `kafka-meta` (producer id, epoch,
timeout, state, partitions, offsets groups), written *before* the broker acts, so a stop in the middle of ending a
transaction is finished on the next start. The states are Empty, Ongoing, PrepareCommit, PrepareAbort, CompleteCommit and CompleteAbort. What a record allows is decided by pure
functions (`txn::rules`), and each id has an entry behind a lock of its own, not an actor. EndTxn answers success only when every marker is flushed and, for a commit,
the offsets are stored; a request that comes while a decided transaction's markers are being written waits for them (Kafka answers
CONCURRENT_TRANSACTIONS for those milliseconds), and is then answered INVALID_TXN_STATE for the other decision or success
for a repeat of the same. A transaction
still open at a restart is left open (its producer may be alive), its timeout counted from when it began, as before the stop. A transaction
that outlives `transaction.timeout.ms` (at most 15 minutes) is aborted by the broker and the producer fenced (epoch raised).

**APIs:** InitProducerId (with a transactional id: becomes the owner, fences the previous owner and aborts what it left
open), AddPartitionsToTxn, AddOffsetsToTxn, TxnOffsetCommit, EndTxn; Produce refuses a transactional batch unless the
producer owns the id and the partition was added; Fetch `read_committed` (cap at the LSO, aborted list) and ListOffsets
(latest = LSO under `read_committed`); DescribeProducers, DescribeTransactions, ListTransactions. WriteTxnMarkers is internal.

## Slices

| | Slice | Gate |
|---|---|---|
| 6a | Pure producer-sequence tracker; `producer_history` from the log; Produce checks, records, answers a duplicate with the original offset; producer id expiry | unit + simulation tests; oracle `produce-validation` (unknown-pid-seq steps), `idempotent-write-from-topic-write` |
| 6b | Control batches (wire); the partition transaction index, LSO and aborted ranges, rebuilt from the log; marker append | wire round trips; index rebuilt = index maintained (property) |
| 6c | `kafka-meta` txn records (crash sweep); the pure `TxnMachine` | unit tests with synthetic time, simulation of invariants |
| 6d | Coordinator actors, InitProducerId (transactional), AddPartitionsToTxn, AddOffsetsToTxn, TxnOffsetCommit, EndTxn with markers, timeouts, start-up recovery | oracle `endtxn-decisions`, `endtxn-while-preparing`, `txn-empty-id`; `SimFs` durability tests |
| 6e | `read_committed` Fetch and ListOffsets; DescribeProducers/Transactions, ListTransactions | broker tests; wire at every version |
| 6f | franz-go (`GroupTransactSession`), librdkafka and sarama transactional producers with `read_committed` consumers in `interop/` | the checkpoint |
| 6g | weight, catalogue, independent audit | |

Gates as before (fmt, clippy -D warnings, tests, lint-size, weight 4.2 MB / 115 packages, oracle). Commit per slice.

## Risks

- The marker and the decision ordering across a crash (the decision is stored first; markers are idempotent because a
  partition skips a marker for a producer with no open transaction).
- Aborted-transaction ranges and the LSO under concurrent appends: one lock per partition covers check, append, index.
- Real clients: librdkafka and franz-go transactions exercise AddOffsetsToTxn/TxnOffsetCommit ordering the recordings do
  not; budget for 6f.
