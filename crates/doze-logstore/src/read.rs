//! Reading by position and following: a [`Cursor`] addresses a line across runs and segments, and a follower is woken
//! by the writer, then reads from the file.

use crate::{Line, Segment, Store, lock, parse_line};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};

/// A position in an instance's log: the start of a line, or the end of the newest segment. The default is the start of
/// the oldest line kept.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cursor {
    pub run: u32,
    pub seg: u32,
    pub offset: u64,
}

impl Cursor {
    fn before(&self, run: u32, seg: u32) -> bool {
        (self.run, self.seg) < (run, seg)
    }
}

/// One kept run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunInfo {
    pub id: u32,
    /// The stamp of its first line; empty if it printed nothing.
    pub started: String,
    pub bytes: u64,
    /// Still being written.
    pub live: bool,
}

/// How many lines a follower reads at a time.
const BATCH: usize = 512;

impl Store {
    /// The runs kept for `instance`, oldest first.
    pub fn runs(&self, instance: &str) -> io::Result<Vec<RunInfo>> {
        let segs = self.segments(instance)?;
        let live = lock(&self.inner.live).get(instance).map(crate::Run::id);
        let mut out: Vec<RunInfo> = Vec::new();
        for sg in &segs {
            if out.last().is_none_or(|r| r.id != sg.run) {
                let (first, _) = read_segment(instance, sg, 0, 1)?;
                out.push(RunInfo {
                    id: sg.run,
                    started: first
                        .into_iter()
                        .next()
                        .map(|l| l.stamp)
                        .unwrap_or_default(),
                    bytes: 0,
                    live: live == Some(sg.run),
                });
            }
            if let Some(r) = out.last_mut() {
                r.bytes = r.bytes.saturating_add(sg.size);
            }
        }
        Ok(out)
    }

    /// Up to `max` complete lines of `instance` from `from` on, across runs, and the cursor to continue from. A cursor
    /// into something since deleted continues from the oldest line kept after it.
    pub fn read(
        &self,
        instance: &str,
        from: Cursor,
        max: usize,
    ) -> io::Result<(Vec<Line>, Cursor)> {
        let segs = self.segments(instance)?;
        let mut out: Vec<Line> = Vec::new();
        let mut next = from;
        for sg in &segs {
            if next.before(sg.run, sg.seg) {
                next = Cursor {
                    run: sg.run,
                    seg: sg.seg,
                    offset: 0,
                };
            } else if (next.run, next.seg) != (sg.run, sg.seg) {
                continue; // before the cursor
            }
            let (mut lines, end) =
                read_segment(instance, sg, next.offset, max.saturating_sub(out.len()))?;
            out.append(&mut lines);
            next.offset = end;
            if out.len() >= max {
                break;
            }
        }
        Ok((out, next))
    }

    /// Hands every line of `instance` from `from` on to `emit`, and keeps going as lines are written, into new runs
    /// too, until `emit` returns `false`, a read fails, or the future is dropped. The files are read on tokio's blocking
    /// threads, a batch at a time, so a long backlog never holds a runtime worker (E2 audit 1, m9). Call it within a
    /// tokio runtime.
    pub async fn follow(
        &self,
        instance: &str,
        mut from: Cursor,
        mut emit: impl FnMut(Line) -> bool,
    ) -> io::Result<()> {
        loop {
            // Taken before reading, so a line written between the read and the wait still wakes this follower.
            let mut wait = self.waiter(instance);
            wait.borrow_and_update();
            loop {
                let (store, inst) = (self.clone(), instance.to_string());
                let (lines, next) =
                    tokio::task::spawn_blocking(move || store.read(&inst, from, BATCH))
                        .await
                        .map_err(io::Error::other)??;
                let n = lines.len();
                for l in lines {
                    if !emit(l) {
                        return Ok(());
                    }
                }
                from = next;
                if n < BATCH {
                    break;
                }
            }
            if wait.changed().await.is_err() {
                return Ok(());
            }
        }
    }
}

/// Up to `max` complete lines of one segment from `offset`, and the offset just past the last one read.
pub(crate) fn read_segment(
    instance: &str,
    sg: &Segment,
    mut offset: u64,
    max: usize,
) -> io::Result<(Vec<Line>, u64)> {
    if max == 0 {
        return Ok((Vec::new(), offset));
    }
    let mut f = match File::open(&sg.path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((Vec::new(), offset)), // pruned meanwhile
        Err(e) => return Err(e),
    };
    f.seek(SeekFrom::Start(offset))?;
    let mut r = BufReader::with_capacity(64 * 1024, f);
    let mut out = Vec::new();
    let mut raw = Vec::new();
    while out.len() < max {
        raw.clear();
        let n = r.read_until(b'\n', &mut raw)?;
        let Some(body) = raw.strip_suffix(b"\n") else {
            break; // the end, or a line still being written: stop before it
        };
        offset = offset.saturating_add(u64::try_from(n).unwrap_or(0));
        out.extend(parse_line(instance, sg.run, body));
    }
    Ok((out, offset))
}
