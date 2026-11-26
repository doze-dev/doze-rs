//! Waiting for a folder to change, without looking at it on a timer: kqueue's `EVFILT_VNODE` on the folder where the
//! system has it (macOS, the BSDs). Elsewhere [`Watch::wait`] only waits out its time, and a caller treats every
//! wait as a possible change.
//!
//! A folder, not a file: the files doze watches are replaced by an atomic rename, which gives the name a new inode,
//! so a watch on the old file sees nothing after the first replacement. The rename (and any file made or removed in
//! the folder) is a write to the folder itself, which is what is watched. A write into a file in place is not seen.

use std::io;
use std::path::Path;
use std::time::Duration;

/// A watch on one folder.
#[derive(Debug)]
pub struct Watch {
    imp: imp::Watch,
}

impl Watch {
    /// Watches the folder at `path`, which must exist.
    pub fn dir(path: &Path) -> io::Result<Watch> {
        Ok(Watch {
            imp: imp::Watch::new(path)?,
        })
    }

    /// Waits until something in the folder changes (`true`), or `timeout` passes (`false`). Changes made while nobody
    /// waited are not lost: the next wait returns at once. A folder that is removed or renamed is watched again under
    /// its path once it is back; until then each wait waits out its time.
    pub fn wait(&mut self, timeout: Duration) -> io::Result<bool> {
        self.imp.wait(timeout)
    }

    /// Whether this system wakes a wait when the folder changes. Where it does not, [`Watch::wait`] is a sleep.
    pub fn notifies() -> bool {
        imp::NOTIFIES
    }
}

#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
mod imp {
    use rustix::event::kqueue::{Event, EventFilter, EventFlags, VnodeEvents, kevent, kqueue};
    use rustix::fd::{AsRawFd, OwnedFd};
    use rustix::fs::{Mode, OFlags, open};
    use std::io;
    use std::mem::MaybeUninit;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    pub(super) const NOTIFIES: bool = true;

    /// The queue first: fields drop in order, and the queue must not outlive the folder it watches (`kevent`'s
    /// safety rule).
    #[derive(Debug)]
    pub(super) struct Watch {
        kq: Option<(OwnedFd, OwnedFd)>,
        path: PathBuf,
    }

    const GONE: VnodeEvents = VnodeEvents::DELETE.union(VnodeEvents::RENAME);

    fn arm(path: &Path) -> io::Result<(OwnedFd, OwnedFd)> {
        let dir = open(
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let kq = kqueue()?;
        let flags = VnodeEvents::WRITE | VnodeEvents::EXTEND | VnodeEvents::ATTRIBUTES | GONE;
        let ev = Event::new(
            EventFilter::Vnode {
                vnode: dir.as_raw_fd(),
                flags,
            },
            EventFlags::ADD | EventFlags::CLEAR,
            std::ptr::null_mut(),
        );
        let mut none: [MaybeUninit<Event>; 0] = [];
        // SAFETY: the one descriptor the event names, `dir`, is returned beside the queue and kept as long as it
        // (`Watch` drops the queue first).
        #[allow(unsafe_code)]
        unsafe {
            kevent(&kq, &[ev], &mut none, Some(Duration::ZERO))?;
        }
        Ok((kq, dir))
    }

    impl Watch {
        pub(super) fn new(path: &Path) -> io::Result<Watch> {
            Ok(Watch {
                kq: Some(arm(path)?),
                path: path.to_path_buf(),
            })
        }

        pub(super) fn wait(&mut self, timeout: Duration) -> io::Result<bool> {
            let Some((kq, _dir)) = &self.kq else {
                // The folder went away: look for it again after the wait, and say the wait may have seen a change.
                std::thread::sleep(timeout);
                self.kq = arm(&self.path).ok();
                return Ok(self.kq.is_some());
            };
            let mut got = [const { MaybeUninit::<Event>::uninit() }; 4];
            // SAFETY: no event is added here; the one registered names `_dir`, which lives as long as `kq`.
            #[allow(unsafe_code)]
            let (events, _) = match unsafe { kevent(kq, &[], &mut got, Some(timeout)) } {
                Ok(v) => v,
                Err(rustix::io::Errno::INTR) => return Ok(true),
                Err(e) => return Err(e.into()),
            };
            let gone = events.iter().any(|e| match e.filter() {
                EventFilter::Vnode { flags, .. } => flags.intersects(GONE),
                _ => false,
            });
            if gone {
                // A new folder at the path is a new vnode: watch that one (or nothing, until it is back).
                self.kq = arm(&self.path).ok();
            }
            Ok(!events.is_empty())
        }
    }
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
)))]
mod imp {
    use std::io;
    use std::path::Path;
    use std::time::Duration;

    pub(super) const NOTIFIES: bool = false;

    #[derive(Debug)]
    pub(super) struct Watch;

    impl Watch {
        pub(super) fn new(path: &Path) -> io::Result<Watch> {
            if !path.is_dir() {
                return Err(io::Error::from(io::ErrorKind::NotFound));
            }
            Ok(Watch)
        }

        pub(super) fn wait(&mut self, timeout: Duration) -> io::Result<bool> {
            std::thread::sleep(timeout);
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::time::Instant;

    fn folder(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("doze-os-watch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_replacing_rename_wakes_the_wait_and_quiet_waits_out_its_time() {
        let d = folder("rename");
        std::fs::write(d.join("names.json"), "{}").unwrap();
        let mut w = Watch::dir(&d).unwrap();
        let t = Instant::now();
        assert!(
            !w.wait(Duration::from_millis(50)).unwrap(),
            "nothing changed"
        );
        assert!(t.elapsed() >= Duration::from_millis(45));
        if !Watch::notifies() {
            return;
        }
        // Made while nobody waits: the next wait still sees it, at once. That wake is the temporary file's.
        std::fs::write(d.join("names.json.tmp"), "{\"a\":1}").unwrap();
        let t = Instant::now();
        assert!(w.wait(Duration::from_secs(5)).unwrap());
        assert!(t.elapsed() < Duration::from_secs(1));
        assert!(
            !w.wait(Duration::from_millis(50)).unwrap(),
            "the temporary file's change was not consumed"
        );
        // The rename over the watched name alone, from another thread while it waits: a wake of its own.
        let d2 = d.clone();
        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            std::fs::rename(d2.join("names.json.tmp"), d2.join("names.json")).unwrap();
        });
        let t = Instant::now();
        assert!(
            w.wait(Duration::from_secs(5)).unwrap(),
            "the rename never woke it"
        );
        assert!(
            t.elapsed() < Duration::from_secs(2),
            "woken by the rename, not the time"
        );
        writer.join().unwrap();
        assert_eq!(
            std::fs::read_to_string(d.join("names.json")).unwrap(),
            "{\"a\":1}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_folder_made_again_is_watched_again() {
        let d = folder("again");
        let mut w = Watch::dir(&d).unwrap();
        std::fs::remove_dir_all(&d).unwrap();
        let woke = w.wait(Duration::from_secs(2)).unwrap();
        assert!(woke || !Watch::notifies(), "the removal is a change");
        std::fs::create_dir_all(&d).unwrap();
        // The first wait after finds the folder again; after that a write inside it wakes the wait.
        let _ = w.wait(Duration::from_millis(20)).unwrap();
        if Watch::notifies() {
            std::fs::write(d.join("x"), "1").unwrap();
            let t = Instant::now();
            assert!(w.wait(Duration::from_secs(5)).unwrap());
            assert!(t.elapsed() < Duration::from_secs(1));
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_missing_folder_is_refused() {
        assert!(Watch::dir(Path::new("/nonexistent/doze-os-watch")).is_err());
    }
}
