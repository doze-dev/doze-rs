//! Where an instance's processes print: a run of the log store (`doze-logstore`, Go's format: `<data>/logs/<name>/`,
//! `rNNNNNN.sNNN.log`, each line stamped and tagged with its stream). A process's stdout and stderr are pipes pumped
//! into the run line by line; doze's own lines (a step's, a hook's) go in under `s:<moment>`.
//!
//! A pipe stays open while anything holds its write end, and a tool that leaves a server behind would hold it forever
//! (W0). Every process here is a doze-os tree, killed whole when it ends, so the pipe closes with it; what still holds
//! one after that (a process that left its group) is cut off when the run closes: its pump is dropped, not waited for.

use doze_logstore::{LineWriter, Run, Store};
use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::task::JoinHandle;

/// How long a closing run waits for its pumps to drain what the processes wrote last.
const DRAIN: Duration = Duration::from_secs(1);

/// The most of a short command's output kept to quote in its error (Go quoted a hook's whole output).
const CAPTURE: usize = 16 * 1024;

/// One start's log. Cheap to clone; every clone is the same run.
#[derive(Debug, Clone)]
pub struct RunLog {
    run: Run,
    pumps: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

/// What a short command printed, the end of it, for its error.
#[derive(Debug, Clone, Default)]
pub(crate) struct Capture(Arc<Mutex<Vec<u8>>>);

impl Capture {
    fn push(&self, bytes: &[u8]) {
        let mut b = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        b.extend_from_slice(bytes);
        let over = b.len().saturating_sub(CAPTURE);
        b.drain(..over);
    }

    /// What was captured, as text.
    pub(crate) fn text(&self) -> String {
        let b = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        String::from_utf8_lossy(&b).into_owned()
    }
}

impl RunLog {
    /// A new run of `name` in `store` (the previous run stays readable; what is over the limits is pruned).
    pub fn begin(store: &Store, name: &str) -> io::Result<RunLog> {
        Ok(RunLog {
            run: store.begin(name)?,
            pumps: Arc::default(),
        })
    }

    /// A new run of `name` in a store of its own at `dir` (the module test harness's one log).
    pub fn open(dir: &std::path::Path, name: &str) -> io::Result<RunLog> {
        RunLog::begin(&Store::open(dir), name)
    }

    /// Every line of this run so far, its texts one per line.
    pub fn text(&self) -> String {
        let from = doze_logstore::Cursor {
            run: self.run.id(),
            ..doze_logstore::Cursor::default()
        };
        let (lines, _) = self
            .run
            .store()
            .read(self.run.instance(), from, usize::MAX)
            .unwrap_or_default();
        let mut out = String::new();
        for l in lines.iter().filter(|l| l.run == self.run.id()) {
            out.push_str(&l.text);
            out.push('\n');
        }
        out
    }

    /// The run it writes.
    pub fn run(&self) -> &Run {
        &self.run
    }

    /// Adds one line doze writes itself on `stream` (`s:hook`, say).
    pub fn line(&self, stream: &str, text: &str) {
        self.run.line(stream, text);
    }

    /// The texts of this run's last `n` lines (Go's `recentLogs`): what a failed start quotes and a `log_line` check
    /// reads.
    pub fn recent(&self, n: usize) -> Vec<String> {
        let store = self.run.store();
        store
            .tail(self.run.instance(), i64::from(self.run.id()), n)
            .map(|lines| lines.into_iter().map(|l| l.text).collect())
            .unwrap_or_default()
    }

    /// Pumps a spawned tree's stdout and stderr into this run, as `o<part>` and `e<part>`, until the run closes.
    pub(crate) fn attach(&self, tree: &mut doze_os::tree::Tree, part: &str) {
        let mut pumps = self.pumps.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(out) = tree.take_stdout() {
            let w = self.run.writer(&format!("{}{part}", doze_logstore::STDOUT));
            pumps.push(tokio::spawn(pump(out, w, None)));
        }
        if let Some(err) = tree.take_stderr() {
            let w = self.run.writer(&format!("{}{part}", doze_logstore::STDERR));
            pumps.push(tokio::spawn(pump(err, w, None)));
        }
    }

    /// Pumps a short command's output into this run on `stream`, also kept in `capture`; the pumps to wait for.
    pub(crate) fn attach_short(
        &self,
        tree: &mut doze_os::tree::Tree,
        stream: &str,
        capture: &Capture,
    ) -> Vec<JoinHandle<()>> {
        let mut out = Vec::new();
        if let Some(o) = tree.take_stdout() {
            let w = self.run.writer(stream);
            out.push(tokio::spawn(pump(o, w, Some(capture.clone()))));
        }
        if let Some(e) = tree.take_stderr() {
            let w = self.run.writer(stream);
            out.push(tokio::spawn(pump(e, w, Some(capture.clone()))));
        }
        out
    }

    /// Ends the run once its processes are gone: the pumps get a second to write what is left, then the run is
    /// closed (a line without its newline recorded as it stands). Closing twice is nothing.
    pub async fn close(&self) {
        let pumps: Vec<JoinHandle<()>> =
            std::mem::take(&mut *self.pumps.lock().unwrap_or_else(PoisonError::into_inner));
        drain(pumps).await;
        self.run.close();
    }
}

/// Waits up to [`DRAIN`] for `pumps`, then drops what is still running.
pub(crate) async fn drain(pumps: Vec<JoinHandle<()>>) {
    let deadline = tokio::time::Instant::now() + DRAIN;
    for p in pumps {
        let abort = p.abort_handle();
        if tokio::time::timeout_at(deadline, p).await.is_err() {
            abort.abort();
        }
    }
}

/// Copies what a pipe carries into the run, line by line; at its end, a last line without a newline is recorded.
async fn pump(mut from: impl AsyncRead + Unpin, to: LineWriter, capture: Option<Capture>) {
    let mut buf = vec![0u8; 8192];
    loop {
        match from.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let got = buf.get(..n).unwrap_or_default();
                to.push(got);
                if let Some(c) = &capture {
                    c.push(got);
                }
            }
        }
    }
    to.finish();
}
