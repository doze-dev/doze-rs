//! What each instance printed, as the doze daemon keeps it on disk (Go `internal/logstore`, its format kept): under
//! `<project state>/logs/<instance>/`, each start of the instance is a run, and a run is one or more segment files
//! `r000012.s000.log` of plain text, one line per line printed:
//!
//! ```text
//! 2026-10-10T09:15:02.418305Z o listening on :8080
//! ```
//!
//! a UTC timestamp, the stream (`o` stdout, `e` stderr, `s:<step>` a step), and the text. This crate reads it, so the
//! logs of a stopped or crashed instance, or of a stack whose daemon is down, stay readable (`doze logs`). The daemon
//! is the one writer and may be mid-line: only complete lines are read.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// The lines a tail gives when none is asked for (the Go daemon's `logBacklog`).
pub const BACKLOG: usize = 200;

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

/// A log folder: `<project state>/logs`.
#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
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

#[derive(Debug, Clone)]
struct Segment {
    run: u32,
    path: PathBuf,
}

impl Store {
    pub fn open(dir: impl Into<PathBuf>) -> Store {
        Store { dir: dir.into() }
    }

    /// The segments of `instance`, oldest first.
    fn segments(&self, instance: &str) -> io::Result<Vec<Segment>> {
        let entries = match std::fs::read_dir(self.dir.join(instance)) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut out: Vec<(u32, u32, Segment)> = entries
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name();
                let (run, seg) = segment_name(name.to_str()?)?;
                Some((
                    run,
                    seg,
                    Segment {
                        run,
                        path: e.path(),
                    },
                ))
            })
            .collect();
        out.sort_by_key(|(r, s, _)| (*r, *s));
        Ok(out.into_iter().map(|(_, _, s)| s).collect())
    }

    /// The last `n` lines of one run of `instance`: run 0 is the newest, a negative run counts back from it (-1 is
    /// the one before), a positive one is a run by its number. An instance that never printed has no lines.
    pub fn tail(&self, instance: &str, run: i64, n: usize) -> Result<Vec<Line>, Error> {
        let segs = self.segments(instance).map_err(Error::Io)?;
        if segs.is_empty() {
            return Ok(Vec::new());
        }
        let mut ids: Vec<u32> = segs.iter().map(|s| s.run).collect();
        ids.dedup();
        let run = if run <= 0 {
            let back = usize::try_from(run.unsigned_abs()).unwrap_or(usize::MAX);
            let at = ids.len().checked_sub(back + 1);
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
        for sg in segs.iter().rev().filter(|s| s.run == run) {
            let mut lines =
                tail_segment(instance, sg, n.saturating_sub(out.len())).map_err(Error::Io)?;
            lines.append(&mut out);
            out = lines;
            if out.len() >= n {
                break;
            }
        }
        let extra = out.len().saturating_sub(n);
        out.drain(..extra);
        Ok(out)
    }
}

/// `r000012.s003.log` → (12, 3); exactly Go's name, nothing else.
fn segment_name(name: &str) -> Option<(u32, u32)> {
    let rest = name.strip_prefix('r')?.strip_suffix(".log")?;
    let (run, seg) = rest.split_once(".s")?;
    let ok = |s: &str, w: usize| s.len() >= w && s.bytes().all(|b| b.is_ascii_digit());
    if !ok(run, 6) || !ok(seg, 3) {
        return None;
    }
    let (r, s) = (run.parse().ok()?, seg.parse().ok()?);
    (format!("r{r:06}.s{s:03}.log") == name).then_some((r, s))
}

/// The last `n` complete lines of a segment, read backwards in blocks so the cost is what is returned.
fn tail_segment(instance: &str, sg: &Segment, n: usize) -> io::Result<Vec<Line>> {
    const BLOCK: u64 = 64 * 1024;
    let mut f = match File::open(&sg.path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()), // pruned meanwhile
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
    let Some(last) = buf.iter().rposition(|b| *b == b'\n') else {
        return Ok(Vec::new());
    };
    buf.truncate(last);
    let mut out: Vec<Line> = buf
        .split(|b| *b == b'\n')
        .filter_map(|raw| parse_line(instance, sg.run, raw))
        .collect();
    let extra = out.len().saturating_sub(n);
    out.drain(..extra);
    Ok(out)
}

// fuzz:parser fuzz_logline
/// `<stamp> <stream> <text>`; a line whose stamp is not one is skipped, as Go skips it.
pub fn parse_line(instance: &str, run: u32, raw: &[u8]) -> Option<Line> {
    let raw = String::from_utf8_lossy(raw);
    let (stamp, rest) = raw.split_once(' ')?;
    let (stream, text) = rest.split_once(' ').unwrap_or((rest, ""));
    if !is_stamp(stamp) {
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

/// Go's layout `2006-01-02T15:04:05.000000Z`, digit for digit.
fn is_stamp(s: &str) -> bool {
    const LAYOUT: &[u8] = b"dddd-dd-ddTdd:dd:dd.ddddddZ";
    s.len() == LAYOUT.len()
        && s.bytes().zip(LAYOUT).all(|(c, l)| match l {
            b'd' => c.is_ascii_digit(),
            l => c == *l,
        })
}

/// The log folder of a project state folder.
pub fn dir_of(project_state: &Path) -> PathBuf {
    project_state.join("logs")
}

#[cfg(test)]
mod tests;
