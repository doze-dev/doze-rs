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
        let wait = if self.soon > 0 {
            self.soon -= 1;
            SOON
        } else if self.changes.is_some() {
            STANDBY
        } else {
            RETRY
        };
        tokio::select! {
            _ = stop.changed() => true,
            () = tokio::time::sleep(wait) => false,
            () = changed(&mut self.changes) => {
                self.soon = SOON_TRIES;
                false
            }
        }
    }
}

async fn changed(rx: &mut Option<watch::Receiver<u64>>) {
    // A watcher that ended (it never does while its folder exists) leaves the timer to do the work.
    let ended = match rx {
        Some(r) => r.changed().await.is_err(),
        None => true,
    };
    if ended {
        *rx = None;
        std::future::pending::<()>().await;
    }
}

/// The changes to `dir`, from one thread per folder per process that waits on it (and never wakes while nothing is
/// written). `None` where the system cannot say, or the folder cannot be watched.
fn changes(dir: &Path) -> Option<watch::Receiver<u64>> {
    static WATCHED: OnceLock<Mutex<BTreeMap<PathBuf, watch::Sender<u64>>>> = OnceLock::new();
    if !Watch::notifies() {
        return None;
    }
    let mut all = WATCHED
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(tx) = all.get(dir) {
        return Some(tx.subscribe());
    }
    let mut w = Watch::dir(dir).ok()?;
    let (tx, rx) = watch::channel(0u64);
    let sender = tx.clone();
    std::thread::Builder::new()
        .name("doze-names-watch".into())
        .spawn(move || {
            loop {
                match w.wait(Duration::from_secs(24 * 3600)) {
                    Ok(true) => sender.send_modify(|n| *n = n.wrapping_add(1)),
                    Ok(false) => {}
                    // Nothing to wait on: the waiters fall back to their timer.
                    Err(_) => return,
                }
            }
        })
        .ok()?;
    all.insert(dir.to_path_buf(), tx);
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
}
