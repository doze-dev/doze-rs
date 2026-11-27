//! Large buffers that are kept for the next use instead of being handed back to the allocator: the frames of big requests
//! (the server reads a produce request into one) and the reads of a fetch (the log reads a partition's records into one).
//!
//! A produce request is one large allocation, used once and freed; so was a fetch's read of up to a megabyte a partition. On macOS the system allocator keeps freed
//! large blocks, so a burst of big requests leaves the process holding all of it. Here a freed buffer goes back
//! on a shelf (bounded, by count and by bytes) and the next frame of that size takes it, so the process holds
//! about what is in flight plus the shelf, and nothing is allocated per frame once it is warm.
//!
//! The shelf is for a burst, not for ever: once no frame has been taken from it or given back to it for [`QUIET`], what is on
//! it is freed ([`Pool::trim_when_quiet`], a task of each server), so a broker that took a burst of big produce requests an
//! hour ago does not still hold up to 64 MiB for them.

use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::Duration;

use bytes::Bytes;
use tokio::sync::Notify;
use tokio::time::Instant;

/// Frames smaller than this are not pooled: the allocator serves them well. Bigger than `MAX`, a frame is read in
/// chunks as it arrives, so that what a peer *claims* it will send is not allocated before it sends it.
pub const MIN: usize = 1 << 16;
/// Size classes are the powers of two from `MIN` up to this; a bigger frame is allocated and freed as usual.
pub const MAX: usize = 1 << 24;
const CLASSES: usize = (MAX.trailing_zeros() - MIN.trailing_zeros() + 1) as usize;
/// The most buffers kept per class, and the most bytes kept over all of them.
const PER_CLASS: usize = 16;
const KEPT_BYTES: usize = 64 << 20;
/// How long the shelf is kept with no frame taken from it or given back to it.
pub const QUIET: Duration = Duration::from_secs(60);

/// Whether `len` bytes go in a pooled buffer: from `MIN` to `MAX`, both included.
#[must_use]
pub fn pooled(len: usize) -> bool {
    (MIN..=MAX).contains(&len)
}

/// The shelves, one per size class, and how many bytes are on them. One lock covers both: two locks taken in two
/// orders by `take` and `give_back` hung the broker (found by the Phase 3 re-audit).
#[derive(Default)]
struct Shelves {
    classes: [Vec<Vec<u8>>; CLASSES],
    kept: usize,
    /// When a buffer was last taken from a shelf or put on one.
    used_at: Option<Instant>,
}

/// The pool every connection and every read shares.
pub struct Pool {
    shelves: Mutex<Shelves>,
    /// Rung when the shelves go from empty to holding something: the trimmer waits for it, with no timer, while they are
    /// empty.
    filled: Notify,
}

impl std::fmt::Debug for Pool {
    /// How much is kept, not the buffers.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pool")
            .field("kept", &lock(&self.shelves).kept)
            .finish_non_exhaustive()
    }
}

/// The pool every connection and every read shares.
pub static POOL: LazyLock<Pool> = LazyLock::new(Pool::new);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The shelf for a buffer of exactly `capacity`, if it is one of the classes.
fn shelf_of(capacity: usize) -> Option<usize> {
    (capacity.is_power_of_two() && (MIN..=MAX).contains(&capacity))
        .then(|| (capacity.trailing_zeros() - MIN.trailing_zeros()) as usize)
}

impl Pool {
    fn new() -> Self {
        Self {
            shelves: Mutex::new(Shelves::default()),
            filled: Notify::new(),
        }
    }

    /// Frees what is on the shelves if nothing has been taken or given back for `quiet`. When the shelves hold something
    /// still, the time they will have been quiet that long.
    fn trim_if_quiet(&self, quiet: Duration) -> Option<Instant> {
        let mut shelves = lock(&self.shelves);
        if shelves.kept == 0 {
            return None;
        }
        let at = shelves.used_at.map_or_else(Instant::now, |t| t + quiet);
        if Instant::now() < at {
            return Some(at);
        }
        let freed = std::mem::take(&mut shelves.classes);
        shelves.kept = 0;
        drop(shelves);
        drop(freed);
        None
    }

    /// Frees the shelves each time they have been quiet for [`QUIET`], until `stop` resolves. Waits on nothing but the bell
    /// while they are empty.
    pub async fn trim_when_quiet(
        &'static self,
        quiet: Duration,
        stop: impl std::future::Future<Output = ()>,
    ) {
        let mut stop = std::pin::pin!(stop);
        loop {
            let next = self.trim_if_quiet(quiet);
            let wait = async {
                match next {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => self.filled.notified().await,
                }
            };
            tokio::select! {
                () = &mut stop => return,
                () = wait => {}
            }
        }
    }

    /// An empty buffer with room for `len` bytes (a frame is appended to it).
    #[must_use]
    pub fn take(&'static self, len: usize) -> Vec<u8> {
        let mut buf = self.take_kept(len);
        buf.clear();
        buf
    }

    /// A buffer of `len` bytes, holding whatever it held (a read fills it): one taken from a shelf is not zeroed again, as a
    /// new one has to be. The caller cuts it to what it filled, so nothing it held before is ever handed on.
    #[must_use]
    pub fn take_filled(&'static self, len: usize) -> Vec<u8> {
        let mut buf = self.take_kept(len);
        buf.resize(len, 0);
        buf
    }

    /// A buffer of the class for `len`, from its shelf if one is there, as it was given back.
    fn take_kept(&'static self, len: usize) -> Vec<u8> {
        let class = len.next_power_of_two().max(MIN);
        let kept = shelf_of(class).and_then(|s| {
            let mut shelves = lock(&self.shelves);
            let buf = shelves.classes.get_mut(s)?.pop()?;
            shelves.kept -= buf.capacity();
            shelves.used_at = Some(Instant::now());
            Some(buf)
        });
        kept.unwrap_or_else(|| Vec::with_capacity(if class <= MAX { class } else { len }))
    }

    /// Puts a buffer back on its shelf as it is (its bytes are not cleared: [`Pool::take`] empties it, and a read's
    /// [`Pool::take_filled`] writes over it).
    fn give_back(&self, buf: Vec<u8>) {
        let Some(s) = shelf_of(buf.capacity()) else {
            return;
        };
        let mut shelves = lock(&self.shelves);
        let capacity = buf.capacity();
        if shelves.kept + capacity > KEPT_BYTES {
            return;
        }
        if let Some(shelf) = shelves.classes.get_mut(s)
            && shelf.len() < PER_CLASS
        {
            shelf.push(buf);
            let was_empty = shelves.kept == 0;
            shelves.kept += capacity;
            shelves.used_at = Some(Instant::now());
            drop(shelves);
            if was_empty {
                self.filled.notify_one();
            }
        }
    }

    /// The bytes of `buf` as a `Bytes` whose buffer returns to the shelf when the last copy is dropped.
    #[must_use]
    pub fn share(&'static self, buf: Vec<u8>) -> Bytes {
        Bytes::from_owner(Pooled { buf, pool: self })
    }
}

struct Pooled {
    buf: Vec<u8>,
    pool: &'static Pool,
}

impl AsRef<[u8]> for Pooled {
    fn as_ref(&self) -> &[u8] {
        &self.buf
    }
}

impl Drop for Pooled {
    fn drop(&mut self) {
        self.pool.give_back(std::mem::take(&mut self.buf));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pool of the test's own, so that tests running side by side do not take each other's buffers.
    fn pool() -> &'static Pool {
        Box::leak(Box::new(Pool::new()))
    }

    fn on_shelf(pool: &Pool, class: usize) -> usize {
        lock(&pool.shelves).classes[class].len()
    }

    #[test]
    fn a_buffer_given_back_is_the_one_taken_next_and_the_shelves_stay_bounded() {
        let pool = pool();
        let first = pool.take(100_000);
        assert_eq!(first.capacity(), 1 << 17, "rounded up to its class");
        let at = first.as_ptr();
        drop(pool.share(first));
        let again = pool.take(70_000);
        assert_eq!(again.as_ptr(), at, "same class, same buffer");
        // Buffers past the shelf's count are freed, not kept.
        let many: Vec<_> = (0..PER_CLASS + 4).map(|_| pool.take(MIN)).collect();
        drop(many.into_iter().map(|b| pool.share(b)).collect::<Vec<_>>());
        assert!(on_shelf(pool, 0) <= PER_CLASS);
        assert!(lock(&pool.shelves).kept <= KEPT_BYTES);
    }

    #[test]
    fn frames_from_the_smallest_class_to_the_largest_are_pooled_and_none_others() {
        assert!(!pooled(MIN - 1) && pooled(MIN) && pooled(MIN + 1));
        assert!(pooled(MAX - 1) && pooled(MAX) && !pooled(MAX + 1));
        assert!(!pooled(0) && !pooled(usize::MAX));
        // Every length that is pooled has a class, and the buffer is as big as the class.
        let pool = pool();
        for len in [MIN, MIN + 1, 1 << 20, MAX] {
            assert!(shelf_of(pool.take(len).capacity()).is_some(), "{len}");
        }
    }

    /// A read's buffer comes back from the shelf as it was, not zeroed again, at the length asked for; and a frame's buffer
    /// comes back empty whatever the read before it left in it.
    #[test]
    fn a_reads_buffer_is_reused_without_zeroing_and_a_frames_is_handed_out_empty() {
        let pool = pool();
        let mut first = pool.take_filled(100_000);
        assert_eq!(first.len(), 100_000);
        first.fill(7);
        let at = first.as_ptr();
        drop(pool.share(first));
        let again = pool.take_filled(70_000);
        assert_eq!(
            (again.as_ptr(), again.len()),
            (at, 70_000),
            "the same buffer, cut to the length asked for"
        );
        drop(pool.share(again));
        let frame = pool.take(90_000);
        assert_eq!(
            (frame.as_ptr(), frame.len()),
            (at, 0),
            "the same buffer, empty for a frame"
        );
    }

    #[test]
    fn a_frame_too_big_for_any_class_is_not_kept() {
        let pool = pool();
        let big = pool.take(MAX + 1);
        assert!(shelf_of(big.capacity()).is_none());
        drop(pool.share(big));
        assert!(lock(&pool.shelves).kept <= KEPT_BYTES);
    }

    #[test]
    fn what_is_kept_is_bounded_by_bytes_as_well_as_by_count() {
        let pool = pool();
        // 16 buffers of 16 MiB would be 256 MiB: the byte bound stops the shelf at four.
        let big: Vec<_> = (0..PER_CLASS).map(|_| pool.take(MAX)).collect();
        drop(big.into_iter().map(|b| pool.share(b)).collect::<Vec<_>>());
        assert!(lock(&pool.shelves).kept <= KEPT_BYTES);
        assert!(on_shelf(pool, CLASSES - 1) <= KEPT_BYTES / MAX);
    }

    #[tokio::test(start_paused = true)]
    async fn the_shelves_are_freed_after_a_quiet_minute_and_the_trimmer_waits_for_nothing_while_they_are_empty()
     {
        let pool = pool();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let trimmer = tokio::spawn(pool.trim_when_quiet(QUIET, async {
            let _ = stopped.await;
        }));
        drop(pool.share(pool.take(1 << 20)));
        assert_eq!(lock(&pool.shelves).kept, 1 << 20, "kept for the next frame");
        tokio::time::sleep(QUIET - Duration::from_secs(1)).await;
        // Used again: the quiet minute starts over.
        drop(pool.share(pool.take(1 << 20)));
        tokio::time::sleep(QUIET - Duration::from_secs(1)).await;
        assert_eq!(
            lock(&pool.shelves).kept,
            1 << 20,
            "not quiet for a minute yet"
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(lock(&pool.shelves).kept, 0, "freed");
        assert!(lock(&pool.shelves).classes.iter().all(Vec::is_empty));
        // Empty: a day goes by with the trimmer parked; a buffer given back starts it again.
        tokio::time::sleep(Duration::from_secs(24 * 3600)).await;
        drop(pool.share(pool.take(MIN)));
        tokio::time::sleep(QUIET + Duration::from_secs(1)).await;
        assert_eq!(lock(&pool.shelves).kept, 0);
        let _ = stop.send(());
        trimmer.await.unwrap();
    }

    #[test]
    fn many_threads_taking_and_giving_back_do_not_hang() {
        let pool = pool();
        // Two locks taken in two orders once froze exactly this. A watchdog turns a hang into a failure.
        let (done, finished) = std::sync::mpsc::channel();
        for t in 0..8 {
            let done = done.clone();
            std::thread::spawn(move || {
                for i in 0..20_000 {
                    let len = MIN << ((t + i) % 4);
                    let buf = pool.take(len);
                    drop(pool.share(buf));
                }
                let _ = done.send(());
            });
        }
        for _ in 0..8 {
            finished
                .recv_timeout(std::time::Duration::from_secs(60))
                .expect("a thread finished: the pool did not deadlock");
        }
    }
}
