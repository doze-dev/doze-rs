//! Request-frame buffers that are kept for the next frame instead of being handed back to the allocator.
//!
//! A produce request is one large allocation, used once and freed. On macOS the system allocator keeps freed
//! large blocks, so a burst of big requests leaves the process holding all of it. Here a freed buffer goes back
//! on a shelf (bounded, by count and by bytes) and the next frame of that size takes it, so the process holds
//! about what is in flight plus the shelf, and nothing is allocated per frame once it is warm.

use std::sync::{LazyLock, Mutex, PoisonError};

use bytes::Bytes;

/// Frames smaller than this are not pooled: the allocator serves them well. Bigger than `MAX`, a frame is read in
/// chunks as it arrives, so that what a peer *claims* it will send is not allocated before it sends it.
pub(crate) const MIN: usize = 1 << 16;
/// Size classes are the powers of two from `MIN` up to this; a bigger frame is allocated and freed as usual.
pub(crate) const MAX: usize = 1 << 24;
const CLASSES: usize = (MAX.trailing_zeros() - MIN.trailing_zeros() + 1) as usize;
/// The most buffers kept per class, and the most bytes kept over all of them.
const PER_CLASS: usize = 16;
const KEPT_BYTES: usize = 64 << 20;

/// Whether a frame of `len` bytes is read into a pooled buffer: from `MIN` to `MAX`, both included.
pub(crate) fn pooled(len: usize) -> bool {
    (MIN..=MAX).contains(&len)
}

/// The shelves, one per size class, and how many bytes are on them. One lock covers both: two locks taken in two
/// orders by `take` and `give_back` hung the broker (found by the Phase 3 re-audit).
#[derive(Default)]
struct Shelves {
    classes: [Vec<Vec<u8>>; CLASSES],
    kept: usize,
}

/// The pool every connection shares.
pub(crate) struct Pool {
    shelves: Mutex<Shelves>,
}

/// The pool every connection shares.
pub(crate) static POOL: LazyLock<Pool> = LazyLock::new(Pool::new);

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
        }
    }

    /// An empty buffer with room for `len` bytes.
    pub(crate) fn take(&'static self, len: usize) -> Vec<u8> {
        let class = len.next_power_of_two().max(MIN);
        let kept = shelf_of(class).and_then(|s| {
            let mut shelves = lock(&self.shelves);
            let buf = shelves.classes.get_mut(s)?.pop()?;
            shelves.kept -= buf.capacity();
            Some(buf)
        });
        kept.unwrap_or_else(|| Vec::with_capacity(if class <= MAX { class } else { len }))
    }

    fn give_back(&self, mut buf: Vec<u8>) {
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
            buf.clear();
            shelf.push(buf);
            shelves.kept += capacity;
        }
    }

    /// The bytes of `buf` as a `Bytes` whose buffer returns to the shelf when the last copy is dropped.
    pub(crate) fn share(&'static self, buf: Vec<u8>) -> Bytes {
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
