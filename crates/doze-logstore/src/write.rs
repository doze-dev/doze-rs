//! The writer: one [`Run`] per start of an instance, its lines appended to segment files, what is over the limits
//! pruned as it goes, followers woken by every line.

use crate::{Segment, Store, file_name, lock};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// The longest line kept whole. A process that prints this much without a newline has it recorded as a line of its own
/// and the rest as the next (Go kept growing the partial line without a bound).
pub const MAX_LINE: usize = 64 * 1024;

/// One start of an instance, open for writing. Cheap to clone; every clone is the same run.
#[derive(Debug, Clone)]
pub struct Run {
    inner: Arc<RunInner>,
}

#[derive(Debug)]
struct RunInner {
    store: Store,
    instance: String,
    id: u32,
    state: Mutex<RunState>,
}

#[derive(Debug, Default)]
struct RunState {
    file: Option<File>,
    seg: u32,
    size: u64,
    /// Each writer's stream and the line it is assembling.
    partials: Vec<(String, Vec<u8>)>,
    closed: bool,
    /// The lines formatted and not yet written: every complete line of one chunk goes in one write (E2 audit 1, m8).
    /// Reused, so a line costs no allocation.
    buf: Vec<u8>,
}

impl Store {
    /// Starts a new run of `instance` and makes it the one being written. A run still open for it is closed first;
    /// what is over the limits goes.
    pub fn begin(&self, instance: &str) -> io::Result<Run> {
        let old = lock(&self.inner.live).get(instance).cloned();
        if let Some(old) = old {
            old.close();
        }
        std::fs::create_dir_all(self.inner.dir.join(instance))?;
        let segs = self.segments(instance)?;
        let id = segs.last().map_or(1, |s| s.run.saturating_add(1));
        let run = Run {
            inner: Arc::new(RunInner {
                store: self.clone(),
                instance: instance.into(),
                id,
                state: Mutex::new(RunState::default()),
            }),
        };
        {
            let mut st = lock(&run.inner.state);
            st.file = Some(run.open(0)?);
        }
        lock(&self.inner.live).insert(instance.into(), run.clone());
        self.prune(instance, id, 0);
        Ok(run)
    }

    /// Deletes everything kept for `instance`, closing its live run first.
    pub fn remove(&self, instance: &str) -> io::Result<()> {
        let live = lock(&self.inner.live).get(instance).cloned();
        if let Some(run) = live {
            run.close();
        }
        match std::fs::remove_dir_all(self.inner.dir.join(instance)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Deletes what is over the limits for `instance`: whole runs beyond the run limit, then the oldest segments while
    /// the total is over the byte limit. The segment being written is never deleted.
    fn prune(&self, instance: &str, live_run: u32, live_seg: u32) {
        let Ok(segs) = self.segments(instance) else {
            return;
        };
        let mut ids: Vec<u32> = segs.iter().map(|s| s.run).collect();
        ids.dedup();
        let oldest_kept = ids
            .len()
            .checked_sub(self.inner.max_runs)
            .and_then(|i| ids.get(i))
            .copied()
            .unwrap_or(0);
        let mut total: u64 = segs.iter().map(|s| s.size).sum();
        for Segment {
            run,
            seg,
            path,
            size,
            ..
        } in segs
        {
            if run == live_run && seg == live_seg {
                continue;
            }
            if (run < oldest_kept || total > self.inner.max_bytes)
                && std::fs::remove_file(&path).is_ok()
            {
                total = total.saturating_sub(size);
            }
        }
    }

    /// Tells followers of `instance` there is something new to read.
    pub(crate) fn wake(&self, instance: &str) {
        if let Some(tx) = lock(&self.inner.changed).get(instance) {
            tx.send_modify(|v| *v = v.wrapping_add(1));
        }
    }

    /// What moves at the next write to `instance`.
    pub(crate) fn waiter(&self, instance: &str) -> tokio::sync::watch::Receiver<u64> {
        lock(&self.inner.changed)
            .entry(instance.into())
            .or_insert_with(|| tokio::sync::watch::channel(0).0)
            .subscribe()
    }

    /// The run of `instance` being written, if any.
    pub fn live(&self, instance: &str) -> Option<Run> {
        lock(&self.inner.live).get(instance).cloned()
    }
}

impl Run {
    /// The run's number: 1 for an instance's first start, counting up.
    pub fn id(&self) -> u32 {
        self.inner.id
    }

    /// The instance it is a run of.
    pub fn instance(&self) -> &str {
        &self.inner.instance
    }

    /// The store it belongs to.
    pub fn store(&self) -> &Store {
        &self.inner.store
    }

    /// Whether it is closed.
    pub fn is_closed(&self) -> bool {
        lock(&self.inner.state).closed
    }

    fn open(&self, seg: u32) -> io::Result<File> {
        let path = self
            .inner
            .store
            .inner
            .dir
            .join(&self.inner.instance)
            .join(file_name(self.inner.id, seg));
        OpenOptions::new().create(true).append(true).open(path)
    }

    /// A writer whose complete lines are recorded on `stream`: what a process's stdout or stderr is pumped into.
    pub fn writer(&self, stream: &str) -> LineWriter {
        let mut st = lock(&self.inner.state);
        st.partials.push((stream.into(), Vec::new()));
        LineWriter {
            run: self.clone(),
            slot: st.partials.len() - 1,
        }
    }

    /// Records one line on `stream`.
    pub fn line(&self, stream: &str, text: &str) {
        let mut st = lock(&self.inner.state);
        self.write_locked(&mut st, stream, text.as_bytes());
        self.flush_locked(&mut st);
    }

    fn write_locked(&self, st: &mut RunState, stream: &str, text: &[u8]) {
        if st.closed || st.file.is_none() {
            return;
        }
        let stamp = crate::stamp(SystemTime::now());
        let len = stamp.len() + stream.len() + text.len() + 3;
        let len = u64::try_from(len).unwrap_or(u64::MAX);
        let store = &self.inner.store;
        if st.size > 0 && st.size.saturating_add(len) > store.inner.max_bytes / 4 {
            // What the old segment has pending goes to it first.
            self.flush_locked(st);
            let next = st.seg.saturating_add(1);
            st.file = self.open(next).ok();
            st.seg = next;
            st.size = 0;
            store.prune(&self.inner.instance, self.inner.id, next);
        }
        st.buf.extend_from_slice(stamp.as_bytes());
        st.buf.push(b' ');
        st.buf.extend_from_slice(stream.as_bytes());
        st.buf.push(b' ');
        st.buf.extend_from_slice(text);
        st.buf.push(b'\n');
        st.size = st.size.saturating_add(len);
    }

    /// Writes the pending lines in one write and wakes the followers.
    fn flush_locked(&self, st: &mut RunState) {
        if st.buf.is_empty() {
            return;
        }
        if let Some(f) = st.file.as_mut() {
            // A failed write loses these lines, as one failed line write did; the run goes on.
            let _ = f.write_all(&st.buf);
        }
        st.buf.clear();
        self.inner.store.wake(&self.inner.instance);
    }

    /// Ends the run: a line still being assembled is recorded as it stands, and the file is closed. The run stays
    /// readable. Closing twice is nothing.
    pub fn close(&self) {
        {
            let mut st = lock(&self.inner.state);
            if st.closed {
                return;
            }
            for i in 0..st.partials.len() {
                let Some((stream, partial)) = st.partials.get_mut(i) else {
                    continue;
                };
                if partial.is_empty() {
                    continue;
                }
                let (stream, text) = (stream.clone(), std::mem::take(partial));
                self.write_locked(&mut st, &stream, &text);
            }
            self.flush_locked(&mut st);
            st.closed = true;
            st.file = None;
        }
        let store = &self.inner.store;
        {
            let mut live = lock(&store.inner.live);
            if live
                .get(&self.inner.instance)
                .is_some_and(|r| Arc::ptr_eq(&r.inner, &self.inner))
            {
                live.remove(&self.inner.instance);
            }
        }
        store.wake(&self.inner.instance);
    }
}

/// Assembles the bytes a process writes into lines of one stream; a trailing `\r` is dropped from each.
#[derive(Debug)]
pub struct LineWriter {
    run: Run,
    slot: usize,
}

impl LineWriter {
    /// Records each complete line in `bytes`; keeps the rest for the next call (or for [`Run::close`]).
    pub fn push(&self, bytes: &[u8]) {
        let run = &self.run;
        let mut st = lock(&run.inner.state);
        let mut rest = bytes;
        while !rest.is_empty() {
            let Some((stream, partial)) = st.partials.get_mut(self.slot) else {
                break;
            };
            let room = MAX_LINE.saturating_sub(partial.len()).max(1);
            let (line, after, whole) = match rest.iter().take(room + 1).position(|b| *b == b'\n') {
                Some(i) => (rest.get(..i), rest.get(i + 1..), true),
                None if rest.len() >= room => (rest.get(..room), rest.get(room..), true),
                None => (Some(rest), None, false),
            };
            partial.extend_from_slice(line.unwrap_or_default());
            rest = after.unwrap_or_default();
            if !whole {
                break;
            }
            let mut text = std::mem::take(partial);
            if text.last() == Some(&b'\r') {
                text.pop();
            }
            let stream = stream.clone();
            run.write_locked(&mut st, &stream, &text);
        }
        run.flush_locked(&mut st);
    }
}

impl LineWriter {
    /// The end of what this writer gets (its pipe closed): a line still being assembled is recorded as it stands, now
    /// rather than when the run closes.
    pub fn finish(&self) {
        let run = &self.run;
        let mut st = lock(&run.inner.state);
        let Some((stream, partial)) = st.partials.get_mut(self.slot) else {
            return;
        };
        if partial.is_empty() {
            return;
        }
        let (stream, text) = (stream.clone(), std::mem::take(partial));
        run.write_locked(&mut st, &stream, &text);
        run.flush_locked(&mut st);
    }
}

impl Write for LineWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.push(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
