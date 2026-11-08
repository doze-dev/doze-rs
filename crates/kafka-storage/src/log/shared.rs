//! What the writer and its readers share.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use arc_swap::ArcSwap;
use kafka_wire::ids::Offset;
use tokio::sync::watch;

use crate::fs::Fs;
use crate::segment::SegmentView;

/// How far flushing has got, or why it stopped.
#[derive(Debug, Clone)]
pub(crate) struct SyncState {
    /// Every append numbered up to this is on disk.
    pub synced: u64,
    /// The first flush failure, which takes the partition offline.
    pub failed: Option<Arc<str>>,
}

/// The state readers use without asking the writer: the segment list (replaced whole when it changes, which
/// is rare), the log-start offset, and channels that announce the end offset and the flushes.
pub(crate) struct Shared<F: Fs> {
    pub segments: ArcSwap<Vec<SegmentView<F>>>,
    start: AtomicI64,
    pub end: watch::Sender<Offset>,
    pub sync: watch::Sender<SyncState>,
}

impl<F: Fs> Shared<F> {
    pub(crate) fn new(segments: Vec<SegmentView<F>>, start: Offset, end: Offset) -> Self {
        Self {
            segments: ArcSwap::from_pointee(segments),
            start: AtomicI64::new(start.get()),
            end: watch::Sender::new(end),
            sync: watch::Sender::new(SyncState {
                synced: 0,
                failed: None,
            }),
        }
    }

    pub(crate) fn start(&self) -> Offset {
        Offset::new(self.start.load(Ordering::Acquire)).unwrap_or(Offset::ZERO)
    }

    pub(crate) fn set_start(&self, start: Offset) {
        self.start.store(start.get(), Ordering::Release);
    }

    pub(crate) fn end(&self) -> Offset {
        *self.end.borrow()
    }
}
