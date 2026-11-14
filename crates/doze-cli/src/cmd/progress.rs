//! `up`'s progress for each boot, on stderr (Go `bootOne` and `relayBootProgress`): "→ name waking…" as it is asked,
//! the daemon log's slow-part lines (fetching, downloading, verifying, extracting) relayed as they are written, and on
//! a terminal the elapsed seconds on the waking line from 3 s, the line cleared when the answer comes (the "awake" line
//! that ends it is the result, on stdout). The log is read from where it ended when the boot was asked.

use crate::paint::Paint;
use std::io::{Read as _, Seek as _, SeekFrom, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// How often the log is read (Go's ticker).
const EVERY: Duration = Duration::from_millis(250);
/// When the elapsed seconds start to show.
const TICK_FROM: Duration = Duration::from_secs(3);
/// The daemon's own prefixes, stripped so a relayed line reads as the CLI's.
const PREFIXES: [&str; 5] = ["doze: ", "› ", "→ ", "✓ ", "✗ "];
const CLEAR: &str = "\r\x1b[K";

/// One boot's progress.
pub(crate) struct Boot {
    log: PathBuf,
    offset: u64,
    pending: Vec<u8>,
    status: String,
    tty: bool,
    paint: Paint,
    start: Instant,
    read: Instant,
}

impl Boot {
    /// Says "→ name waking…" (left open on a terminal, for the tick) and marks where the log ends.
    pub(crate) fn begin(
        err: &mut dyn Write,
        paint: Paint,
        (log, tty): (PathBuf, bool),
        name: &str,
    ) -> Boot {
        let status = format!("{} {name} waking…", paint.dim("→"));
        let _ = if tty {
            write!(err, "{status}")
        } else {
            writeln!(err, "{status}")
        };
        let _ = err.flush();
        let offset = std::fs::metadata(&log).map_or(0, |m| m.len());
        let now = Instant::now();
        Boot {
            log,
            offset,
            pending: Vec::new(),
            status,
            tty,
            paint,
            start: now,
            read: now,
        }
    }

    /// Called while the answer is awaited: every 250 ms, the new progress lines, then the tick.
    pub(crate) fn poll(&mut self, err: &mut dyn Write) {
        if self.read.elapsed() < EVERY {
            return;
        }
        self.read = Instant::now();
        for line in self.new_lines() {
            if !progress_line(&line) {
                continue;
            }
            let line = PREFIXES
                .iter()
                .fold(line.as_str(), |l, p| l.strip_prefix(p).unwrap_or(l));
            let msg = format!("  {}", self.paint.dim(&format!("› {line}")));
            let _ = if self.tty {
                writeln!(err, "{CLEAR}{msg}")
            } else {
                writeln!(err, "{msg}")
            };
        }
        let elapsed = self.start.elapsed();
        if self.tty && elapsed >= TICK_FROM {
            let secs = format!(" {}s", elapsed.as_secs());
            let _ = write!(err, "{CLEAR}{}{}", self.status, self.paint.dim(&secs));
        }
        let _ = err.flush();
    }

    /// The answer came: the open waking line is cleared on a terminal (the result line replaces it).
    pub(crate) fn end(self, err: &mut dyn Write) {
        if self.tty {
            let _ = write!(err, "{CLEAR}");
            let _ = err.flush();
        }
    }

    /// The complete lines written since the last read; a partial last line waits for its end.
    fn new_lines(&mut self) -> Vec<String> {
        let Ok(mut f) = std::fs::File::open(&self.log) else {
            return Vec::new();
        };
        let mut buf = Vec::new();
        if f.seek(SeekFrom::Start(self.offset)).is_err() || f.read_to_end(&mut buf).is_err() {
            return Vec::new();
        }
        self.offset += buf.len() as u64;
        self.pending.extend_from_slice(&buf);
        let Some(last) = self.pending.iter().rposition(|b| *b == b'\n') else {
            return Vec::new();
        };
        let rest = self.pending.split_off(last + 1);
        let done = std::mem::replace(&mut self.pending, rest);
        String::from_utf8_lossy(&done)
            .split('\n')
            .map(|l| l.trim_end_matches('\r'))
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect()
    }
}

/// The log lines worth relaying during a boot (Go `bootProgressLine`): the slow parts, not the supervision chatter.
fn progress_line(line: &str) -> bool {
    ["downloading", "verifying", "fetching", "extracting"]
        .iter()
        .any(|w| line.contains(w))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn relays_the_slow_parts_once_each_and_waits_for_a_line_to_end() {
        let dir = std::env::temp_dir().join(format!("dz-progress-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("doze.log");
        std::fs::write(&log, "doze: downloading before the boot\n").unwrap();
        let mut err = Vec::new();
        let mut b = Boot::begin(&mut err, Paint::new(false), (log.clone(), false), "db");
        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        f.write_all(b"doze: downloading valkey 8.1\nsupervising db\n\xff verifying")
            .unwrap();
        b.read = Instant::now().checked_sub(EVERY).unwrap();
        b.poll(&mut err);
        f.write_all(b" sha256\r\n").unwrap();
        b.read = Instant::now().checked_sub(EVERY).unwrap();
        b.poll(&mut err);
        b.end(&mut err);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            String::from_utf8(err).unwrap(),
            "→ db waking…\n  › downloading valkey 8.1\n  › \u{fffd} verifying sha256\n"
        );
    }
}
