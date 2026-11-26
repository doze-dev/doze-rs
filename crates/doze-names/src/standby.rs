//! How a peer that lost the resolver's or the front door's bind waits to try again.
//!
//! Every doze program on the machine is a peer, and all but one stand by, so the wait is paid many times over: Go
//! (and this port until 2026-11-26) tried the bind every 2 s, in every loop of every standing-by program, idle or
//! not (the network daemon and each stack's daemon, three loops apiece). A holder that lets go cleanly writes the
//! registry as it does (its record goes), so where the system can say when a folder changes ([`Watch`]) the standby
//! waits for that instead, and tries again at once and a few times just after (the record goes a moment before the
//! socket closes). For a holder that died without writing anything there is still a timer, [`STANDBY`] rather than
//! 2 s; where nothing can be watched, the 2 s timer as before.

use doze_os::fs::Watch;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokio::sync::watch;

/// Where nothing says the holder let go: Go's interval.
pub(crate) const RETRY: Duration = Duration::from_secs(2);
/// For a holder that died without writing, where the folder is watched: how long the zone (or the front door) may be
/// dark after a `kill -9` of the program serving it. The network daemon serves the resolver from boot and launchd
/// starts it again at once, so in practice it is a stack's daemon killed while it held the front door.
pub(crate) const STANDBY: Duration = Duration::from_secs(10);
/// After the registry changed: the holder's record goes just before its socket closes.
const SOON: Duration = Duration::from_millis(100);
const SOON_TRIES: u32 = 5;

/// One waiter's state.
#[derive(Debug)]
pub(crate) struct Standby {
    changes: Option<watch::Receiver<u64>>,
    soon: u32,
}

impl Standby {
    /// Waits on the registry at `registry` (its folder), where that can be watched.
    pub(crate) fn new(registry: Option<&Path>) -> Standby {
        Standby {
            changes: registry.and_then(Path::parent).and_then(changes),
            soon: 0,
        }
    }

    /// Waits until it is worth trying the bind again; `true` when asked to stop instead.
    pub(crate) async fn wait(&mut self, stop: &mut watch::Receiver<bool>) -> bool {
        let wait = self.next_wait();
        tokio::select! {
            _ = stop.changed() => true,
            () = tokio::time::sleep(wait) => false,
            () = changed(&mut self.changes) => {
                // A change: try soon, a few times. The watcher ending instead: try now, then on the short timer.
                if self.changes.is_some() {
                    self.soon = SOON_TRIES;
                }
                false
            }
        }
    }

    fn next_wait(&mut self) -> Duration {
        if self.soon > 0 {
            self.soon -= 1;
            SOON
        } else if self.changes.is_some() {
            STANDBY
        } else {
            RETRY
        }
    }
}

/// Returns at the next change, or at once when the watcher has ended (and then forgets it, so the waiter goes back
/// to [`RETRY`]); never, when there is no watcher.
async fn changed(rx: &mut Option<watch::Receiver<u64>>) {
    let Some(r) = rx else {
        return std::future::pending().await;
    };
    if r.changed().await.is_err() {
        *rx = None;
    }
}

/// The receivers of every watched folder in this process. The relay thread holds the only sender, so a watcher that
/// fails closes the channel and every waiter hears it.
static WATCHED: OnceLock<Mutex<BTreeMap<PathBuf, watch::Receiver<u64>>>> = OnceLock::new();

/// The changes to `dir`, from one thread per folder per process that waits on it (and never wakes while nothing is
/// written). `None` where the system cannot say, or the folder cannot be watched.
fn changes(dir: &Path) -> Option<watch::Receiver<u64>> {
    if !Watch::notifies() {
        return None;
    }
    let mut all = WATCHED
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // A folder whose watcher ended is watched afresh.
    if let Some(rx) = all.get(dir).filter(|rx| rx.has_changed().is_ok()) {
        return Some(rx.clone());
    }
    let mut w = Watch::dir(dir).ok()?;
    let rx = relay(move || w.wait(Duration::from_secs(24 * 3600)))?;
    all.insert(dir.to_path_buf(), rx.clone());
    Some(rx)
}

/// A thread that calls `wait` until it fails, counting the changes it reports. When it fails the thread ends and the
/// sender with it, which closes the channel.
fn relay(
    mut wait: impl FnMut() -> std::io::Result<bool> + Send + 'static,
) -> Option<watch::Receiver<u64>> {
    let (tx, rx) = watch::channel(0u64);
    std::thread::Builder::new()
        .name("doze-names-watch".into())
        .spawn(move || {
            while let Ok(changed) = wait() {
                if changed {
                    tx.send_modify(|n| *n = n.wrapping_add(1));
                }
            }
        })
        .ok()?;
    Some(rx)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[tokio::test]
    async fn a_change_to_the_registry_wakes_a_standby_at_once() {
        let home = crate::registry::tests::home();
        let file = home.join(crate::registry::FILE_NAME);
        let mut s = Standby::new(Some(&file));
        let (_stop_tx, mut stop) = watch::channel(false);
        if !Watch::notifies() {
            assert!(s.changes.is_none());
            return;
        }
        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            std::fs::write(home.join("names.json.tmp"), "{}").unwrap();
            std::fs::rename(home.join("names.json.tmp"), file).unwrap();
        });
        let t = std::time::Instant::now();
        assert!(!s.wait(&mut stop).await);
        assert!(t.elapsed() < Duration::from_secs(2), "woken by the timer");
        writer.join().unwrap();
        // Then it tries again soon, a few times, before going back to waiting.
        let t = std::time::Instant::now();
        assert!(!s.wait(&mut stop).await);
        assert!(t.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn a_stop_ends_the_wait() {
        let mut s = Standby::new(None);
        let (tx, mut stop) = watch::channel(false);
        tx.send(true).unwrap();
        assert!(s.wait(&mut stop).await);
    }

    #[tokio::test]
    async fn a_watcher_that_fails_sends_the_waiters_back_to_the_short_timer() {
        let mut calls = 0;
        let rx = relay(move || {
            calls += 1;
            if calls < 3 {
                Ok(true)
            } else {
                Err(std::io::Error::other("the folder's watch went away"))
            }
        })
        .unwrap();
        let mut s = Standby {
            changes: Some(rx),
            soon: 0,
        };
        let (_tx, mut stop) = watch::channel(false);
        let t = std::time::Instant::now();
        // Whatever changes were reported first, the end is heard, not waited out on the 10 s timer.
        while s.changes.is_some() {
            assert!(!s.wait(&mut stop).await);
            assert!(
                t.elapsed() < Duration::from_secs(3),
                "the end was never heard"
            );
        }
        s.soon = 0;
        assert_eq!(s.next_wait(), RETRY);
    }
}
