//! Numbers for connections, handed out only while an observer is installed.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::observer::{ConnId, Observer, Opened, Watch};

/// Where connection numbers come from. With no observer installed nothing is ever taken from it.
#[derive(Debug, Default)]
pub(crate) struct Numbers(AtomicU64);

impl Numbers {
    /// What a new connection holds of `observer`, if there is one: the next number, and the observer told that the
    /// connection opened. With no observer this is one `Option` test.
    pub(crate) fn watch(
        &self,
        observer: Option<&Arc<dyn Observer>>,
        peer: Option<SocketAddr>,
    ) -> Option<Watch> {
        let observer = observer?;
        let id = ConnId(self.0.fetch_add(1, Ordering::Relaxed) + 1);
        observer.opened(&Opened { id, peer });
        Some(Watch {
            observer: Arc::clone(observer),
            conn: id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Quiet;
    impl Observer for Quiet {
        fn opened(&self, _: &Opened) {}
        fn begin(&self, _: &crate::observer::Call<'_>) -> crate::observer::Pending {
            crate::observer::Pending::new(())
        }
        fn end(&self, _: crate::observer::Pending, _: &crate::observer::Observed<'_>) {}
        fn closed(&self, _: ConnId, _: &str) {}
    }

    #[test]
    fn numbers_start_at_one_and_none_are_taken_without_an_observer() {
        let numbers = Numbers::default();
        assert!(numbers.watch(None, None).is_none());
        let observer: Arc<dyn Observer> = Arc::new(Quiet);
        let a = numbers.watch(Some(&observer), None).unwrap();
        let b = numbers.watch(Some(&observer), None).unwrap();
        assert_eq!((a.conn, b.conn), (ConnId(1), ConnId(2)));
    }
}
