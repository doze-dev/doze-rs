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

/// The shelves, one per size class.
pub(crate) struct Pool {
    shelves: [Mutex<Vec<Vec<u8>>>; CLASSES],
    kept: Mutex<usize>,
}

/// The pool every connection shares.
pub(crate) static POOL: LazyLock<Pool> = LazyLock::new(|| Pool {
    shelves: std::array::from_fn(|_| Mutex::new(Vec::new())),
    kept: Mutex::new(0),
});

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The shelf for a buffer of exactly `capacity`, if it is one of the classes.
fn shelf_of(capacity: usize) -> Option<usize> {
    (capacity.is_power_of_two() && (MIN..=MAX).contains(&capacity))
        .then(|| (capacity.trailing_zeros() - MIN.trailing_zeros()) as usize)
}

impl Pool {
    /// An empty buffer with room for `len` bytes.
    pub(crate) fn take(&'static self, len: usize) -> Vec<u8> {
        let class = len.next_power_of_two().max(MIN);
        if let Some(shelf) = shelf_of(class).and_then(|s| self.shelves.get(s))
            && let Some(buf) = lock(shelf).pop()
        {
            *lock(&self.kept) -= buf.capacity();
            return buf;
        }
        Vec::with_capacity(if class <= MAX { class } else { len })
    }

    fn give_back(&self, mut buf: Vec<u8>) {
        let Some(shelf) = shelf_of(buf.capacity()).and_then(|s| self.shelves.get(s)) else {
            return;
        };
        let mut kept = lock(&self.kept);
        let mut on_shelf = lock(shelf);
        if on_shelf.len() < PER_CLASS && *kept + buf.capacity() <= KEPT_BYTES {
            buf.clear();
            *kept += buf.capacity();
            on_shelf.push(buf);
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

    #[test]
    fn a_buffer_given_back_is_the_one_taken_next_and_the_shelves_stay_bounded() {
        let first = POOL.take(100_000);
        assert_eq!(first.capacity(), 1 << 17, "rounded up to its class");
        let at = first.as_ptr();
        drop(POOL.share(first));
        let again = POOL.take(70_000);
        assert_eq!(again.as_ptr(), at, "same class, same buffer");
        // Buffers past the shelf's count are freed, not kept.
        let many: Vec<_> = (0..PER_CLASS + 4).map(|_| POOL.take(MIN)).collect();
        drop(many.into_iter().map(|b| POOL.share(b)).collect::<Vec<_>>());
        assert!(lock(&POOL.shelves[0]).len() <= PER_CLASS);
        assert!(*lock(&POOL.kept) <= KEPT_BYTES);
    }

    #[test]
    fn a_frame_too_big_for_any_class_is_not_kept() {
        let big = POOL.take(MAX + 1);
        assert!(shelf_of(big.capacity()).is_none());
        drop(POOL.share(big));
        assert!(*lock(&POOL.kept) <= KEPT_BYTES);
    }
}
