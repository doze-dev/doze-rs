//! Writes handed to the writer and not yet committed, so a read sees them (read-your-writes).
//!
//! A service that queues a write need not wait for its commit before it takes the next call: a read of the database ([`Db::get`],
//! [`Db::scan`], a [`RowTable`]'s reads) sees what was last committed with every queued write of that database laid over it, in the
//! order the writer will apply them. An entry leaves when its commit is flushed (the file then holds it), or when it fails: a
//! commit that fails drops every entry, and the database takes no more (the writer refuses its writes from then on, see
//! [`GroupCommit`](crate::GroupCommit)), so a read never sees a write the file will not have.
//!
//! The overlay is taken before the read transaction begins: an entry leaves only after its commit returned, so whatever is not in
//! the overlay is in the transaction, and an entry both in the overlay and committed is laid over itself, which changes nothing.
//!
//! [`Db::get`]: crate::Db::get
//! [`Db::scan`]: crate::Db::scan
//! [`RowTable`]: crate::rows::RowTable

use std::collections::{BTreeMap, VecDeque};
use std::ops::Bound;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::Notify;

/// One queued change, as a read sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change<'a> {
    /// A row put under a key.
    Put(&'a [u8], &'a [u8]),
    /// A row removed.
    Delete(&'a [u8]),
    /// Every row from the first key up to (not including) the second.
    DeleteRange(&'a [u8], &'a [u8]),
}

/// The writes of one queued operation, as a read sees them.
pub trait Overlaid: Send + Sync + 'static {
    /// Calls `f` with each change to `table`, in the order the commit applies them.
    fn changes(&self, table: &str, f: &mut dyn FnMut(Change<'_>));
}

#[derive(Default)]
struct Inner {
    next: u64,
    live: VecDeque<(u64, Arc<dyn Overlaid>)>,
    /// A commit failed: nothing is laid over the file any more (the writer refuses the database's writes).
    failed: bool,
}

/// The queued, not yet committed writes of one database. Cloning shares it (the writer holds a clone of each job's).
#[derive(Clone, Default)]
pub struct Pending(Arc<(Mutex<Inner>, Notify)>);

impl std::fmt::Debug for Pending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pending")
            .field("queued", &self.lock().live.len())
            .finish()
    }
}

impl Pending {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Lays `writes` over the file until [`Pending::done`] of the number it answers; `None` once a commit failed.
    pub(crate) fn add(&self, writes: Arc<dyn Overlaid>) -> Option<u64> {
        let mut inner = self.lock();
        if inner.failed {
            return None;
        }
        let id = inner.next;
        inner.next += 1;
        inner.live.push_back((id, writes));
        Some(id)
    }

    /// Entry `id` is on disk, or its operation failed alone: it is no longer laid over the file.
    pub(crate) fn done(&self, id: u64) {
        let mut inner = self.lock();
        if let Some(at) = inner.live.iter().position(|(of, _)| *of == id) {
            inner.live.remove(at);
        }
        drop(inner);
        self.0.1.notify_waiters();
    }

    /// A commit failed: every entry is dropped, and none is taken from now on.
    pub(crate) fn fail(&self) {
        let mut inner = self.lock();
        inner.failed = true;
        inner.live.clear();
        drop(inner);
        self.0.1.notify_waiters();
    }

    /// The writer took the database again (after a compaction), and accepts its writes again: so does the overlay.
    pub(crate) fn revive(&self) {
        self.lock().failed = false;
    }

    /// Whether no write is waiting for its commit.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lock().live.is_empty()
    }

    /// The queued writes as they are now, oldest first, to lay over a read begun after this.
    #[must_use]
    pub fn view(&self) -> View {
        View(
            self.lock()
                .live
                .iter()
                .map(|(_, w)| Arc::clone(w))
                .collect(),
        )
    }

    /// Waits until every write queued before this call has been committed (or has failed). For a read that cannot see the
    /// overlay (a raw read transaction, a count kept beside a write number).
    pub async fn settled(&self) {
        let last = self.lock().next;
        loop {
            let notified = self.0.1.notified();
            if self.lock().live.front().is_none_or(|(id, _)| *id >= last) {
                return;
            }
            notified.await;
        }
    }
}

/// The rows a [`View::merge`] reads: those of `table` from `lo` up to (not including) `hi`, last first when `reverse`.
#[derive(Clone, Copy, Debug)]
pub struct Span<'a> {
    /// The table.
    pub table: &'a str,
    /// The first key.
    pub lo: &'a [u8],
    /// Past the last key.
    pub hi: &'a [u8],
    /// Last first.
    pub reverse: bool,
}

/// Queued writes taken at one moment ([`Pending::view`]).
#[derive(Default)]
pub struct View(Vec<Arc<dyn Overlaid>>);

impl std::fmt::Debug for View {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("View").field(&self.0.len()).finish()
    }
}

/// What the queued writes make of a range: each key put or deleted (`None`) in it, and the ranges deleted in it.
#[derive(Default)]
struct Within {
    keys: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
    cleared: Vec<(Vec<u8>, Vec<u8>)>,
}

impl Within {
    /// Whether a row of the file under `key` is hidden by a queued write.
    fn hides(&self, key: &[u8]) -> bool {
        self.keys.contains_key(key)
            || self
                .cleared
                .iter()
                .any(|(lo, hi)| lo.as_slice() <= key && key < hi.as_slice())
    }
}

impl View {
    /// Whether there is nothing queued.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// What the queued writes say of `key` in `table`: `Some(Some(row))` put, `Some(None)` deleted, `None` not touched (the file
    /// answers).
    #[must_use]
    pub fn get(&self, table: &str, key: &[u8]) -> Option<Option<Vec<u8>>> {
        let mut seen = None;
        for w in &self.0 {
            // The row is copied for each queued put of the key: a key has few.
            w.changes(table, &mut |c| match c {
                Change::Put(k, v) if k == key => seen = Some(Some(v.to_vec())),
                Change::Delete(k) if k == key => seen = Some(None),
                Change::DeleteRange(lo, hi) if lo <= key && key < hi => seen = Some(None),
                _ => {}
            });
        }
        seen
    }

    fn within(&self, table: &str, lo: &[u8], hi: &[u8]) -> Within {
        let mut out = Within::default();
        for w in &self.0 {
            w.changes(table, &mut |c| match c {
                Change::Put(k, v) if lo <= k && k < hi => {
                    out.keys.insert(k.to_vec(), Some(v.to_vec()));
                }
                Change::Delete(k) if lo <= k && k < hi => {
                    out.keys.insert(k.to_vec(), None);
                }
                Change::DeleteRange(from, to) if from < hi && lo < to => {
                    let (from, to) = (from.max(lo), to.min(hi));
                    let doomed: Vec<Vec<u8>> = out
                        .keys
                        .range::<[u8], _>((Bound::Included(from), Bound::Excluded(to)))
                        .map(|(k, _)| k.clone())
                        .collect();
                    for k in doomed {
                        out.keys.remove(&k);
                    }
                    out.cleared.push((from.to_vec(), to.to_vec()));
                }
                _ => {}
            });
        }
        out
    }

    /// Calls `each` with the rows of `span` as the file's rows (`disk`, in the span's order) and the queued writes make them, in key
    /// order (last first when reversed), until it returns `false`. `row` reads a row of `disk` as its key and value.
    ///
    /// # Errors
    /// The first error `disk` yields.
    pub fn merge<R, E>(
        &self,
        span: Span<'_>,
        disk: impl Iterator<Item = Result<R, E>>,
        row: impl Fn(&R) -> (&[u8], &[u8]),
        mut each: impl FnMut(&[u8], &[u8]) -> bool,
    ) -> Result<(), E> {
        let Span {
            table,
            lo,
            hi,
            reverse,
        } = span;
        let within = self.within(table, lo, hi);
        let mut puts: Vec<(&[u8], &[u8])> = within
            .keys
            .iter()
            .filter_map(|(k, v)| v.as_deref().map(|v| (k.as_slice(), v)))
            .collect();
        if reverse {
            puts.reverse();
        }
        // Whether a queued row comes before (or at) the file's row, in the order asked.
        let first = |queued: &[u8], filed: &[u8]| {
            if reverse {
                queued >= filed
            } else {
                queued <= filed
            }
        };
        let mut queued = puts.into_iter().peekable();
        for r in disk {
            let r = r?;
            let (k, v) = row(&r);
            while let Some((qk, qv)) = queued.next_if(|(qk, _)| first(qk, k)) {
                if !each(qk, qv) {
                    return Ok(());
                }
            }
            if within.hides(k) {
                continue;
            }
            if !each(k, v) {
                return Ok(());
            }
        }
        for (qk, qv) in queued {
            if !each(qk, qv) {
                break;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
