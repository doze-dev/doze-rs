//! The task that owns a partition's writer.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use kafka_storage::fs::Fs;
use kafka_storage::{Appended, PartitionLog, StorageError};
use kafka_wire::ResponseError;
use kafka_wire::batch::{Batch, TimestampType};
use kafka_wire::ids::{Offset, TimestampMs};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, sleep_until};

use crate::blocking;
use crate::producers::{Producers, Stored, Verdict, last_of};
use crate::txn_index::{Open, TxnIndex};

/// The most appends taken in one go, so that a partition under load still hears a Close.
const GROUP: usize = 64;
/// How much of the log's tail is read to relearn its producers after a restart.
const HISTORY_BYTES: u64 = 16 << 20;

pub(super) enum Command {
    Append(Append),
    /// What the partition knows of its transactions and producers.
    Txns(oneshot::Sender<Result<TxnView, StorageError>>),
    /// Flush and close; answers whether that worked.
    Close(oneshot::Sender<bool>),
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
    pub(super) reply: oneshot::Sender<Result<Written, StorageError>>,
}

/// What the writer remembers besides the log.
struct Memory {
    producers: Producers,
    /// The log's recent history has been read in.
    loaded: bool,
    /// The log start the producers were last pruned for.
    pruned_for: i64,
    /// The transactions the log holds, shared with the readers.
    txns: Arc<RwLock<TxnIndex>>,
}

/// Takes commands until the queue closes or a Close arrives: appends are written in the order they came, then one
/// flush is made if any of them asked to be durable (every producer waiting on it is released by that one
/// flush). Appends that did not ask for it are flushed `fsync_interval` after the first of them, and only while
/// there is something to flush: an idle partition holds no timer.
pub(super) async fn run<F: Fs>(
    mut log: PartitionLog<F>,
    mut queue: mpsc::Receiver<Command>,
    fsync_interval: Duration,
    txns: Arc<RwLock<TxnIndex>>,
) {
    let mut unflushed_since: Option<Instant> = None;
    let mut memory = Memory {
        producers: Producers::default(),
        loaded: false,
        pruned_for: 0,
        txns,
    };
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
        let mut group = vec![first];
        while group.len() < GROUP {
            match queue.try_recv() {
                Ok(c) => group.push(c),
                Err(_) => break,
            }
        }
        let (closing, outcome) = blocking::run(|| work(&mut log, &mut memory, group));
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
    group: Vec<Command>,
) -> (Option<oneshot::Sender<bool>>, Flushed) {
    let (mut wrote, mut wants_flush, mut closing) = (false, false, None);
    for command in group {
        match command {
            Command::Append(a) => {
                let result = append_one(log, memory, &a);
                if matches!(result, Ok(Written::Appended(_))) {
                    wrote = true;
                    wants_flush |= a.durable;
                }
                // The producer waiting on this learns of the append now and of its flush by the ticket.
                let _ = a.reply.send(result);
            }
            Command::Txns(reply) => {
                let _ = reply.send(view(log, memory));
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
    if memory.pruned_for != log.start_offset().get() {
        memory.pruned_for = log.start_offset().get();
        memory.producers.forget_below(memory.pruned_for);
    }
    let (id, epoch) = (producer.id.get(), producer.epoch.get());
    let last = last_of(first, a.batch.record_count());
    match memory.producers.check(id, epoch, first, last) {
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
            continue;
        };
        memory.producers.record(
            Stored {
                id: p.id.get(),
                epoch: p.epoch.get(),
                first,
                last: last_of(first, batch.record_count),
                base_offset: batch.base_offset,
                max_timestamp: batch.max_timestamp.get(),
            },
            batch.max_timestamp.get(),
        );
    }
    memory.loaded = true;
    Ok(())
}

mod transactions;
pub(super) use transactions::{read_index, shared};
use transactions::{view, write};
