//! What each instance printed, as doze keeps it on disk (Go `internal/logstore`, its format kept): under
//! `<project state>/logs/<instance>/`, each start of the instance is a run, and a run is one or more segment files
//! `r000012.s000.log` of plain text, one line per line printed:
//!
//! ```text
//! 2026-10-10T09:15:02.418305Z o listening on :8080
//! ```
//!
//! a UTC timestamp, the stream (`o` stdout, `e` stderr, `s:<step>` a step), and the text. Nothing is held in memory
//! beyond a partial line per writer, so a daemon's footprint does not grow with what its services print, and the logs
//! of a stopped or crashed instance, or of a stack whose daemon is down, stay readable (`doze logs`).
//!
//! The daemon is the one writer ([`Store::begin`], then [`Run`]); it may be mid-line, so readers take only complete
//! lines. A follower ([`Store::follow`]) is woken by the writer, not by a timer, and reads from the file, so it never
//! misses a line however far behind it falls.

mod read;
mod stamp;
mod write;

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

pub use read::{Cursor, RunInfo};
pub use stamp::stamp;
pub use write::{LineWriter, MAX_LINE, Run};

/// The lines a tail gives when none is asked for (the Go daemon's `logBacklog`).
pub const BACKLOG: usize = 200;

/// The stream of what a process wrote to stdout.
pub const STDOUT: &str = "o";
/// The stream of what a process wrote to stderr.
pub const STDERR: &str = "e";
/// A step's stream is this and its moment: `s:setup`.
pub const STEP_PREFIX: &str = "s:";

/// Runs kept per instance, the live one included, when [`Options::max_runs`] is 0.
pub const DEFAULT_MAX_RUNS: usize = 5;
/// Bytes kept per instance across its runs, when [`Options::max_bytes`] is 0.
pub const DEFAULT_MAX_BYTES: u64 = 10 << 20;

/// What is kept, per instance. A run moves to a new segment at a quarter of `max_bytes`; old segments are what pruning
/// deletes, so most of the cap stays in use.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// Runs kept, the live one included; 0 is [`DEFAULT_MAX_RUNS`].
    pub max_runs: usize,
    /// Bytes kept across all runs; 0 is [`DEFAULT_MAX_BYTES`].
    pub max_bytes: u64,
}

/// One line an instance printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub instance: String,
    pub run: u32,
    /// The UTC time as written, `2026-10-10T09:15:02.418305Z`: fixed width, so it sorts as it reads.
    pub stamp: String,
    pub stream: String,
    pub text: String,
}

/// A log folder: `<project state>/logs`. Cheap to clone; every clone is the same store, so a reader sees the writer's
/// live run and is woken by its lines.
#[derive(Debug, Clone)]
pub struct Store {
    inner: Arc<Shared>,
}

#[derive(Debug)]
struct Shared {
    dir: PathBuf,
    max_runs: usize,
    max_bytes: u64,
    /// The run being written, per instance.
    live: Mutex<HashMap<String, Run>>,
    /// Moves on every line an instance gets; followers wait on it.
    changed: Mutex<HashMap<String, tokio::sync::watch::Sender<u64>>>,
}

/// Why a tail has nothing to give.
#[derive(Debug)]
pub enum Error {
    /// `--run -N` past what is kept.
    OnlyRuns {
        instance: String,
        kept: usize,
    },
    /// No run with that number.
    NoRun {
        instance: String,
        run: u32,
    },
    Io(io::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::OnlyRuns { instance, kept } => {
                write!(f, "{instance} has only {kept} run(s) kept")
            }
            Error::NoRun { instance, run } => write!(f, "{instance} has no run {run}"),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

/// One file of a run.
#[derive(Debug, Clone)]
pub(crate) struct Segment {
    pub(crate) run: u32,
    pub(crate) seg: u32,
    pub(crate) path: PathBuf,
    pub(crate) size: u64,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Store {
    /// The store rooted at `dir`, with the default limits. Nothing is created until something is written.
    pub fn open(dir: impl Into<PathBuf>) -> Store {
        Store::with_options(dir, Options::default())
    }

    /// The store rooted at `dir`, keeping what `opts` says.
    pub fn with_options(dir: impl Into<PathBuf>, opts: Options) -> Store {
        let max_runs = match opts.max_runs {
            0 => DEFAULT_MAX_RUNS,
            n => n,
        };
        let max_bytes = match opts.max_bytes {
            0 => DEFAULT_MAX_BYTES,
            n => n,
        };
        Store {
            inner: Arc::new(Shared {
                dir: dir.into(),
                max_runs,
                max_bytes,
                live: Mutex::new(HashMap::new()),
                changed: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// The folder.
    pub fn dir(&self) -> &Path {
        &self.inner.dir
    }

    /// The segments of `instance`, oldest first.
    pub(crate) fn segments(&self, instance: &str) -> io::Result<Vec<Segment>> {
        let entries = match std::fs::read_dir(self.inner.dir.join(instance)) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut out: Vec<Segment> = entries
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name();
                let (run, seg) = segment_name(name.to_str()?)?;
                let size = e.metadata().ok()?.len();
                Some(Segment {
                    run,
                    seg,
                    path: e.path(),
                    size,
                })
            })
            .collect();
        out.sort_by_key(|s| (s.run, s.seg));
        Ok(out)
    }

    /// The last `n` lines of one run of `instance`: run 0 is the newest, a negative run counts back from it (-1 is
    /// the one before), a positive one is a run by its number. An instance that never printed has no lines.
    pub fn tail(&self, instance: &str, run: i64, n: usize) -> Result<Vec<Line>, Error> {
        self.tail_at(instance, run, n).map(|(lines, _)| lines)
    }

    /// [`Store::tail`], and the cursor just past the lines: where a follower goes on from.
    pub fn tail_at(
        &self,
        instance: &str,
        run: i64,
        n: usize,
    ) -> Result<(Vec<Line>, Cursor), Error> {
        let segs = self.segments(instance).map_err(Error::Io)?;
        if segs.is_empty() {
            return Ok((Vec::new(), Cursor::default()));
        }
        let mut ids: Vec<u32> = segs.iter().map(|s| s.run).collect();
        ids.dedup();
        let run = if run <= 0 {
            let back = usize::try_from(run.unsigned_abs()).unwrap_or(usize::MAX);
            let at = ids.len().checked_sub(back.saturating_add(1));
            match at.and_then(|i| ids.get(i)) {
                Some(id) => *id,
                None => {
                    return Err(Error::OnlyRuns {
                        instance: instance.into(),
                        kept: ids.len(),
                    });
                }
            }
        } else {
            u32::try_from(run).unwrap_or(u32::MAX)
        };
        if !ids.contains(&run) {
            return Err(Error::NoRun {
                instance: instance.into(),
                run,
            });
        }
        let mut out: Vec<Line> = Vec::new();
        let mut end: Option<Cursor> = None;
        for sg in segs.iter().rev().filter(|s| s.run == run) {
            let (mut lines, off) =
                tail_segment(instance, sg, n.saturating_sub(out.len())).map_err(Error::Io)?;
            end.get_or_insert(Cursor {
                run: sg.run,
                seg: sg.seg,
                offset: off,
            });
            lines.append(&mut out);
            out = lines;
            if out.len() >= n {
                break;
            }
        }
        let extra = out.len().saturating_sub(n);
        out.drain(..extra);
        Ok((out, end.unwrap_or_default()))
    }
}

/// `r000012.s003.log` → (12, 3); exactly Go's name, nothing else.
pub(crate) fn segment_name(name: &str) -> Option<(u32, u32)> {
    let rest = name.strip_prefix('r')?.strip_suffix(".log")?;
    let (run, seg) = rest.split_once(".s")?;
    let ok = |s: &str, w: usize| s.len() >= w && s.bytes().all(|b| b.is_ascii_digit());
    if !ok(run, 6) || !ok(seg, 3) {
        return None;
    }
    let (r, s) = (run.parse().ok()?, seg.parse().ok()?);
    (file_name(r, s) == name).then_some((r, s))
}

/// The file of segment `seg` of run `run`.
pub(crate) fn file_name(run: u32, seg: u32) -> String {
    format!("r{run:06}.s{seg:03}.log")
}

/// The last `n` complete lines of a segment, read backwards in blocks so the cost is what is returned, and the offset
/// just past the last of them.
fn tail_segment(instance: &str, sg: &Segment, n: usize) -> io::Result<(Vec<Line>, u64)> {
    const BLOCK: u64 = 64 * 1024;
    let mut f = match File::open(&sg.path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((Vec::new(), 0)), // pruned meanwhile
        Err(e) => return Err(e),
    };
    let size = f.metadata()?.len();
    let mut start = size;
    let mut buf: Vec<u8> = Vec::new();
    while start > 0 && buf.iter().filter(|b| **b == b'\n').count() <= n {
        let step = BLOCK.min(start);
        start -= step;
        let mut chunk = vec![0; usize::try_from(step).unwrap_or(0)];
        f.seek(SeekFrom::Start(start))?;
        f.read_exact(&mut chunk)?;
        chunk.extend_from_slice(&buf);
        buf = chunk;
    }
    if start > 0 {
        // The buffer begins mid-line: drop up to the first newline.
        let first = buf
            .iter()
            .position(|b| *b == b'\n')
            .map_or(buf.len(), |i| i + 1);
        buf.drain(..first);
    }
    // Only complete lines: stop before one still being written.
    let from = size.saturating_sub(buf.len() as u64);
    let Some(last) = buf.iter().rposition(|b| *b == b'\n') else {
        return Ok((Vec::new(), from));
    };
    let end = from.saturating_add(last as u64).saturating_add(1);
    buf.truncate(last);
    let mut out: Vec<Line> = buf
        .split(|b| *b == b'\n')
        .filter_map(|raw| parse_line(instance, sg.run, raw))
        .collect();
    let extra = out.len().saturating_sub(n);
    out.drain(..extra);
    Ok((out, end))
}

// fuzz:parser fuzz_logline
/// `<stamp> <stream> <text>`; a line whose stamp is not one is skipped, as Go skips it.
pub fn parse_line(instance: &str, run: u32, raw: &[u8]) -> Option<Line> {
    let raw = String::from_utf8_lossy(raw);
    let (stamp, rest) = raw.split_once(' ')?;
    let (stream, text) = rest.split_once(' ').unwrap_or((rest, ""));
    if !stamp::is_stamp(stamp) {
        return None;
    }
    Some(Line {
        instance: instance.to_owned(),
        run,
        stamp: stamp.to_owned(),
        stream: stream.to_owned(),
        text: text.to_owned(),
    })
}

/// The log folder of a project state folder.
pub fn dir_of(project_state: &Path) -> PathBuf {
    project_state.join("logs")
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_guards;
#[cfg(test)]
mod tests_write;
