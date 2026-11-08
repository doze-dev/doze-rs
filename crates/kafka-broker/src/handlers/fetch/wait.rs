//! Waiting for a partition to grow, or for the topics to change under the fetch.

use kafka_storage::fs::Fs;
use kafka_wire::ids::Offset;
use tokio::sync::watch;
use tokio_stream::wrappers::WatchStream;
use tokio_stream::{StreamExt, StreamMap};

use super::{Place, Wanted};

/// Why a wait ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Woke {
    /// A partition grew.
    Grew,
    /// A topic was deleted: what the fetch resolved is out of date.
    Topics,
}

/// What a fetch watches: the end offset of each partition it reads, and the broker's count of changes to its topics.
/// (Both are streams that stay alive between waits: a waiter that is dropped is no longer woken, so asking again
/// from scratch each time would sleep through the change.)
#[derive(Debug)]
pub(super) struct Watching {
    ends: StreamMap<usize, WatchStream<Offset>>,
    topics: WatchStream<u64>,
}

impl Watching {
    /// Streams of the changes from here on, for the partitions that are open. This is done before the first read, so
    /// a produce after the read is a change and a produce before it is read.
    pub(super) fn new<F: Fs>(wanted: &[Wanted<'_, F>], topics: watch::Receiver<u64>) -> Self {
        let ends = wanted
            .iter()
            .flat_map(|w| &w.places)
            .filter_map(|p| match p {
                Place::Open(partition) => Some(partition.reader().watch_end()),
                Place::Error(_) => None,
            })
            .enumerate()
            .map(|(i, end)| (i, WatchStream::from_changes(end)))
            .collect();
        Self {
            ends,
            topics: WatchStream::from_changes(topics),
        }
    }

    /// Resolves when a partition has grown or the topics have changed. With nothing to watch it is pending for ever:
    /// the caller's deadline ends the wait.
    pub(super) async fn changed(&mut self) -> Woke {
        tokio::select! {
            Some(_) = self.topics.next() => Woke::Topics,
            Some(_) = self.ends.next(), if !self.ends.is_empty() => Woke::Grew,
            else => std::future::pending().await,
        }
    }
}
