//! The task that owns a partition's writer.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use kafka_storage::fs::Fs;
use kafka_storage::{Appended, PartitionLog, Retention, StorageError};
use kafka_wire::ResponseError;
use kafka_wire::batch::{Batch, TimestampType};
use kafka_wire::ids::{Offset, TimestampMs};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, sleep_until};

use super::upkeep::Upkeep;
use crate::blocking;
use crate::producers::{FirstSequence, Producers, Stored, Verdict, last_of};
use crate::txn_index::{Open, TxnIndex};

/// The most appends taken in one go, so that a partition under load still hears a Close.
const GROUP: usize = 64;
/// How much of the log's tail is read to relearn its producers after a restart.
const HISTORY_BYTES: u64 = 16 << 20;

pub(super) enum Command<F: Fs> {
    Append(Append),
    /// What the partition knows of its transactions and producers.
    Txns(oneshot::Sender<Result<TxnView, StorageError>>),
    /// Each producer's window of recent batches, for the console.
    Windows(oneshot::Sender<Result<Vec<crate::producers::Window>, StorageError>>),
    /// Move the start of the log up; answers the start now in force.
    Trim(Trim, oneshot::Sender<Result<Offset, StorageError>>),
    /// Remove every record, unless a transaction is open in the log (the console's Empty topic).
    Empty(oneshot::Sender<Result<Emptying, StorageError>>),
    /// Apply retention now; answers the start now in force and when retention could next take something.
    Retain(
        Retention,
        TimestampMs,
        oneshot::Sender<Result<Retained, StorageError>>,
    ),
    /// Plan a round of cleaning, if the log is due one; answers with the count of changes it was judged at.
    Clean(
        kafka_storage::compact::Params,
        oneshot::Sender<Result<(kafka_storage::compact::Planned<F>, u64), StorageError>>,
    ),
    /// Put a round of cleaning in place; answers how many segments it replaced.
    Install(
        kafka_storage::compact::Cleaned,
        oneshot::Sender<Result<usize, StorageError>>,
    ),
    /// Take back the files of a round that will not be put in place.
    Discard(kafka_storage::compact::Cleaned),
    /// Judge appends by this config from now on.
    Configure(kafka_storage::LogConfig),
    /// Flush and close; answers whether that worked.
    Close(oneshot::Sender<bool>),
}

/// How the start of a log is moved up.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Trim {
    /// Delete what retention no longer keeps, judged at `now`.
    Retention(Retention, TimestampMs),
    /// DeleteRecords: everything below the offset, or (`None`) everything.
    Before(Option<Offset>),
}

/// What became of a request to empty a partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Emptying {
    /// Every record is gone: the log starts where it ends, and `offsets` offsets were below that.
    Emptied { offsets: i64 },
    /// A transaction has records in the log and no marker yet: emptying would leave its marker with nothing to end, and its
    /// producer's commit would commit nothing. Nothing was done.
    TxnOpen,
}

/// What a pass of retention left.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Retained {
    /// The log start now in force.
    pub(crate) start: Offset,
    /// When retention could next take something, if nothing is written (`PartitionLog::retention_due`).
    pub(crate) due_ms: Option<i64>,
    /// The writer's count of changes when that was judged.
    pub(crate) changes: u64,
}

/// What became of an append.
#[derive(Debug)]
pub(crate) enum Written {
    /// It was written.
    Appended(Appended),
    /// An idempotent producer's retry of a batch already written: where it went, and its timestamp as stored.
    Duplicate { offset: Offset, timestamp: i64 },
    /// The producer's sequence or epoch is wrong; nothing was written.
    Refused(ResponseError),
}

/// What the partition's writer knows of its open transactions and its producers.
#[derive(Debug, Clone)]
pub(crate) struct TxnView {
    pub(crate) open: Vec<Open>,
    /// (id, epoch, last sequence, last used in ms)
    pub(crate) producers: Vec<(i64, i16, i32, i64)>,
}

pub(super) struct Append {
    pub(super) batch: Batch,
    pub(super) now: TimestampMs,
    pub(super) clock: TimestampType,
    /// The next flush must cover this append (acks=all).
    pub(super) durable: bool,
    /// Where a producer the partition knows no batch of may start.
    pub(super) first: FirstSequence,
    pub(super) reply: oneshot::Sender<Result<Written, StorageError>>,
}

/// What the writer remembers besides the log.
struct Memory {
    producers: Producers,
    /// The log's recent history has been read in.
    loaded: bool,
    /// The transactions the log holds, shared with the readers.
    txns: Arc<RwLock<TxnIndex>>,
    /// How far the last round of cleaning that was put in place got. The aborted transactions that ended before it have no
    /// record left; they are let go one round later, so that a reader that began on the old segments is not told less than
    /// what it reads.
    cleaned_to: i64,
    /// Counts the changes this writer makes, for the chores that leave an unchanged partition alone.
    upkeep: Arc<Upkeep>,
}

/// Takes commands until the queue closes or a Close arrives: appends are written in the order they came, then one
/// flush is made if any of them asked to be durable (every producer waiting on it is released by that one
/// flush). Appends that did not ask for it are flushed `fsync_interval` after the first of them, and only while
/// there is something to flush: an idle partition holds no timer.
pub(super) async fn run<F: Fs>(
    mut log: PartitionLog<F>,
    mut queue: mpsc::Receiver<Command<F>>,
    fsync_interval: Duration,
    txns: Arc<RwLock<TxnIndex>>,
    upkeep: Arc<Upkeep>,
) {
    let mut unflushed_since: Option<Instant> = None;
    let mut memory = Memory {
        producers: Producers::default(),
        loaded: false,
        txns,
        cleaned_to: 0,
        upkeep,
    };
    // The commands of one wakeup, kept across wakeups so a busy partition allocates once. It starts empty and grows to
    // the largest group seen: an idle partition holds nothing.
    let mut group: Vec<Command<F>> = Vec::new();
    loop {
        let next = match unflushed_since {
            Some(since) => tokio::select! {
                c = queue.recv() => c,
                () = sleep_until(since + fsync_interval) => {
                    // A failed flush is the log's to remember: it refuses the next write.
                    let _ = blocking::run(|| log.sync());
                    unflushed_since = None;
                    continue;
                }
            },
            None => queue.recv().await,
        };
        let Some(first) = next else { break };
        group.push(first);
        while group.len() < GROUP {
            match queue.try_recv() {
                Ok(c) => group.push(c),
                Err(_) => break,
            }
        }
        let (closing, outcome) = blocking::run(|| work(&mut log, &mut memory, &mut group));
        match outcome {
            Flushed::Everything => unflushed_since = None,
            Flushed::NotYet => {
                unflushed_since.get_or_insert_with(Instant::now);
            }
            Flushed::NothingWritten => {}
        }
        if let Some(done) = closing {
            let clean = blocking::run(|| log.close()).is_ok();
            let _ = done.send(clean);
            return;
        }
    }
    // Every handle is gone: flush what there is.
    let _ = blocking::run(|| log.close());
}

enum Flushed {
    /// A flush after the appends covered everything.
    Everything,
    /// Something was written and not flushed.
    NotYet,
    /// Nothing was written.
    NothingWritten,
}

fn work<F: Fs>(
    log: &mut PartitionLog<F>,
    memory: &mut Memory,
    group: &mut Vec<Command<F>>,
) -> (Option<oneshot::Sender<bool>>, Flushed) {
    let (mut wrote, mut wants_flush, mut closing) = (false, false, None);
    for command in group.drain(..) {
        match command {
            Command::Append(a) => {
                let result = append_one(log, memory, &a);
                if matches!(result, Ok(Written::Appended(_))) {
                    memory.upkeep.changed();
                    wrote = true;
                    wants_flush |= a.durable;
                }
                // The producer waiting on this learns of the append now and of its flush by the ticket.
                let _ = a.reply.send(result);
            }
            Command::Windows(reply) => {
                let _ = reply.send(transactions::windows(log, memory));
            }
            Command::Txns(reply) => {
                let _ = reply.send(view(log, memory));
            }
            Command::Trim(how, reply) => {
                memory.upkeep.changed();
                let _ = reply.send(trim(log, memory, how));
            }
            Command::Empty(reply) => {
                memory.upkeep.changed();
                let _ = reply.send(transactions::empty(log, memory));
            }
            Command::Retain(policy, now, reply) => {
                let _ = reply.send(retain(log, memory, policy, now));
            }
            Command::Clean(params, reply) => {
                let planned = cleaning::plan(log, memory, &params);
                let _ = reply.send(planned.map(|p| (p, memory.upkeep.changes())));
            }
            Command::Install(cleaned, reply) => {
                let installed = log.install_cleaned(&cleaned);
                if installed.is_ok() {
                    memory.upkeep.changed();
                    memory
                        .txns
                        .write()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .forget_before(memory.cleaned_to);
                    memory.cleaned_to = memory.cleaned_to.max(cleaned.clean_to);
                }
                let _ = reply.send(installed);
            }
            Command::Discard(cleaned) => log.discard_cleaned(&cleaned),
            Command::Configure(config) => {
                memory.upkeep.changed();
                log.set_config(config);
            }
            Command::Close(done) => closing = Some(done),
        }
    }
    let flushed = if wants_flush {
        // One flush for the whole group. Its failure is carried by the tickets of the appends it covered.
        let _ = log.sync();
        Flushed::Everything
    } else if wrote {
        Flushed::NotYet
    } else {
        Flushed::NothingWritten
    };
    (closing, flushed)
}

/// Applies retention, and says when it could next take something. A pass that took something is a change.
fn retain<F: Fs>(
    log: &mut PartitionLog<F>,
    memory: &mut Memory,
    policy: Retention,
    now: TimestampMs,
) -> Result<Retained, StorageError> {
    let before = log.start_offset();
    let start = trim(log, memory, Trim::Retention(policy, now))?;
    if start != before {
        memory.upkeep.changed();
    }
    Ok(Retained {
        start,
        due_ms: log.retention_due(policy)?,
        changes: memory.upkeep.changes(),
    })
}

/// Judges the batch by what is known of its producer, if it has one, writes it if it is to be, and remembers it.
fn append_one<F: Fs>(
    log: &mut PartitionLog<F>,
    memory: &mut Memory,
    a: &Append,
) -> Result<Written, StorageError> {
    let Some(producer) = a.batch.producer() else {
        return write(log, memory, a).map(Written::Appended);
    };
    // A batch with a producer and no sequence is a transaction marker: it is not sequenced, but it raises the producer's epoch.
    let Some(first) = producer.base_sequence.and_then(|s| i32::try_from(s).ok()) else {
        let appended = write(log, memory, a)?;
        if !memory.loaded {
            relearn(log, memory)?;
        }
        memory
            .producers
            .marked(producer.id.get(), producer.epoch.get(), a.now.get());
        return Ok(Written::Appended(appended));
    };
    if !memory.loaded {
        relearn(log, memory)?;
    }
    let (id, epoch) = (producer.id.get(), producer.epoch.get());
    let last = last_of(first, a.batch.record_count());
    match memory.producers.check_from(id, epoch, first, last, a.first) {
        Verdict::Accept => {
            let appended = write(log, memory, a)?;
            // The newest timestamp as stored: the batch's own, or the time of the append if the topic says so.
            let stored = match a.clock {
                TimestampType::LogAppend => a.now.get(),
                _ => a.batch.max_timestamp().get(),
            };
            memory.producers.record(
                Stored {
                    id,
                    epoch,
                    first,
                    last,
                    base_offset: appended.first_offset.get(),
                    max_timestamp: stored,
                },
                a.now.get(),
            );
            Ok(Written::Appended(appended))
        }
        Verdict::Duplicate { offset, timestamp } => Ok(Written::Duplicate {
            offset: Offset::new(offset).unwrap_or(Offset::ZERO),
            timestamp,
        }),
        Verdict::OutOfOrder => Ok(Written::Refused(ResponseError::OutOfOrderSequenceNumber)),
        Verdict::Fenced => Ok(Written::Refused(ResponseError::InvalidProducerEpoch)),
    }
}

/// Reads the producers' recent batches back from the log, once: a producer retrying a batch whose acknowledgement was
/// lost in a stop must be recognised, not written twice.
fn relearn<F: Fs>(log: &PartitionLog<F>, memory: &mut Memory) -> Result<(), StorageError> {
    for batch in log.reader().producer_history(HISTORY_BYTES)? {
        let Some(p) = batch.producer else { continue };
        let Some(first) = p.base_sequence.and_then(|s| i32::try_from(s).ok()) else {
            // A marker: the producer is at least at its epoch (a fence raised it), and starts its sequences again. If it is
            // all that is left of the producer, the sequences it ended are not known.
            memory.producers.marked_in_history(
                p.id.get(),
                p.epoch.get(),
                batch.max_timestamp.get(),
            );
            continue;
        };
        memory.producers.record(
            Stored {
                id: p.id.get(),
                epoch: p.epoch.get(),
                first,
                // The span of sequences is the span of offsets, whatever compaction left in the batch.
                last: last_of(first, batch.last_offset_delta.saturating_add(1)),
                base_offset: batch.base_offset,
                max_timestamp: batch.max_timestamp.get(),
            },
            batch.max_timestamp.get(),
        );
    }
    memory.loaded = true;
    Ok(())
}

mod cleaning;
mod transactions;
pub(super) use transactions::{read_index, shared};
use transactions::{trim, view, write};
