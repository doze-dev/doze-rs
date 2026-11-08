//! The task that owns a partition's writer.

use std::time::Duration;

use kafka_storage::fs::Fs;
use kafka_storage::{Appended, PartitionLog, StorageError};
use kafka_wire::batch::{Batch, TimestampType};
use kafka_wire::ids::TimestampMs;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, sleep_until};

use crate::blocking;

/// The most appends taken in one go, so that a partition under load still hears a Close.
const GROUP: usize = 64;

pub(super) enum Command {
    Append(Append),
    /// Flush and close; answers whether that worked.
    Close(oneshot::Sender<bool>),
}

pub(super) struct Append {
    pub(super) batch: Batch,
    pub(super) now: TimestampMs,
    pub(super) clock: TimestampType,
    /// The next flush must cover this append (acks=all).
    pub(super) durable: bool,
    pub(super) reply: oneshot::Sender<Result<Appended, StorageError>>,
}

/// Takes commands until the queue closes or a Close arrives: appends are written in the order they came, then one
/// flush is made if any of them asked to be durable (every producer waiting on it is released by that one
/// flush). Appends that did not ask for it are flushed `fsync_interval` after the first of them, and only while
/// there is something to flush: an idle partition holds no timer.
pub(super) async fn run<F: Fs>(
    mut log: PartitionLog<F>,
    mut queue: mpsc::Receiver<Command>,
    fsync_interval: Duration,
) {
    let mut unflushed_since: Option<Instant> = None;
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
        let (closing, outcome) = blocking::run(|| work(&mut log, group));
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
    group: Vec<Command>,
) -> (Option<oneshot::Sender<bool>>, Flushed) {
    let (mut wrote, mut wants_flush, mut closing) = (false, false, None);
    for command in group {
        match command {
            Command::Append(a) => {
                let result = log.append(std::slice::from_ref(&a.batch), a.now, a.clock);
                if result.is_ok() {
                    wrote = true;
                    wants_flush |= a.durable;
                }
                // The producer waiting on this learns of the append now and of its flush by the ticket.
                let _ = a.reply.send(result);
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
