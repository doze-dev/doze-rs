//! The writer (Go's `journal.Journal`): events appended to the live file with the next sequence number, the file
//! rotated to `<path>.1` when it would pass its cap, and every event handed to subscribers. Nothing is kept in memory:
//! a daemon at rest holds an open file and a sequence number.

use crate::{DEFAULT_MAX_BYTES, Event, encode, read_file, rotated};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use time::OffsetDateTime;
use tokio::sync::broadcast;

/// How many events a subscriber may fall behind before it misses some (it sees a gap in `seq` and fills it with
/// [`Journal::since`]).
pub const SUBSCRIBER_BUFFER: usize = 256;

/// The journal of one stack, open for writing. Cheap to clone; every clone is the same journal.
#[derive(Debug, Clone)]
pub struct Journal {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    path: PathBuf,
    max_bytes: u64,
    state: Mutex<State>,
    subs: broadcast::Sender<Event>,
}

#[derive(Debug)]
struct State {
    file: Option<File>,
    size: u64,
    seq: u64,
}

fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn ends_with_newline(path: &Path) -> io::Result<bool> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = File::open(path)?;
    f.seek(SeekFrom::End(-1))?;
    let mut last = [0u8; 1];
    f.read_exact(&mut last)?;
    Ok(last == *b"\n")
}

impl Journal {
    /// Opens (creating it and its folder) the journal at `path`; a `max_bytes` of 0 is [`DEFAULT_MAX_BYTES`]. The
    /// sequence goes on from the last event on disk, so `seq` is unique across daemon restarts.
    pub fn open(path: &Path, max_bytes: u64) -> io::Result<Journal> {
        let max_bytes = if max_bytes == 0 {
            DEFAULT_MAX_BYTES
        } else {
            max_bytes
        };
        // The last event is in the live file, or in the rotated one if the live file is new.
        let mut seq = 0;
        for p in [rotated(path), path.to_path_buf()] {
            if let Some(e) = read_file(&p)?.last() {
                seq = e.seq;
            }
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut file = open_append(path)?;
        let mut size = file.metadata()?.len();
        // A write cut short by a crash ends without a newline: end it, so the next event is a line of its own (Go
        // appended to the torn line, and the next event was lost with it).
        if size > 0 && !ends_with_newline(path)? {
            file.write_all(b"\n")?;
            size = size.saturating_add(1);
        }
        Ok(Journal {
            inner: Arc::new(Inner {
                path: path.into(),
                max_bytes,
                state: Mutex::new(State {
                    file: Some(file),
                    size,
                    seq,
                }),
                subs: broadcast::channel(SUBSCRIBER_BUFFER).0,
            }),
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The file.
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    /// Stamps `e` with the next sequence number (and the time, if it has none), writes it, hands it to subscribers,
    /// and returns it. A write that fails is not retried: the event still reaches subscribers.
    pub fn append(&self, mut e: Event) -> Event {
        let mut st = self.lock();
        st.seq = st.seq.saturating_add(1);
        e.seq = st.seq;
        if e.time.is_none() {
            e.time = Some(OffsetDateTime::now_utc());
        }
        if st.file.is_some() {
            let mut line = encode(&e);
            line.push('\n');
            let len = line.len() as u64;
            if st.size > 0 && st.size.saturating_add(len) > self.inner.max_bytes {
                self.rotate(&mut st);
            }
            if let Some(f) = st.file.as_mut()
                && f.write_all(line.as_bytes()).is_ok()
            {
                st.size = st.size.saturating_add(len);
            }
        }
        // Sent under the lock, so subscribers see events in sequence order.
        let _ = self.inner.subs.send(e.clone());
        e
    }

    /// Moves the live file aside, replacing the previous rotated one, and starts a new one.
    fn rotate(&self, st: &mut State) {
        st.file = None;
        let _ = std::fs::rename(&self.inner.path, rotated(&self.inner.path));
        st.file = open_append(&self.inner.path).ok();
        st.size = 0;
    }

    /// Every event on disk with a sequence number above `seq`, oldest first; `since(0)` is the whole journal.
    pub fn since(&self, seq: u64) -> io::Result<Vec<Event>> {
        let _st = self.lock();
        crate::since(&self.inner.path, seq)
    }

    /// The last `n` events on disk, oldest first.
    pub fn tail(&self, n: usize) -> io::Result<Vec<Event>> {
        let _st = self.lock();
        crate::tail(&self.inner.path, n)
    }

    /// Every event appended from now on. A subscriber that falls more than [`SUBSCRIBER_BUFFER`] behind is told how
    /// many it missed (`RecvError::Lagged`) and can fill the gap with [`Journal::since`]. Dropping it unsubscribes.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.inner.subs.subscribe()
    }

    /// Closes the file. Appends after it still reach subscribers.
    pub fn close(&self) {
        self.lock().file = None;
    }
}
