//! The daemon's lifecycle from the outside (Go's `internal/daemonctl`): start it detached (`doze __daemon`, a session
//! of its own, its output to `doze.log`), wait for its control socket, find it by its pid file, stop it, and the stack
//! lock that `up` and `down` take so two of them never start or stop one stack at once. Callers print; these return
//! values and errors.
//!
//! The layout is Go's, under a stack's run folder (`<home>/projects/<slug>/run`): `doze.admin.sock`, `doze.pid` (the
//! pid on line one), `doze.log` and `blocks.json` (the blocks the daemon started with, for `doze sync`). The lock is the Rust doze's own (`doze.ctl.lock`, an advisory lock the OS drops
//! when its holder dies); Go had none.

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The hidden argument the daemon runs under (Go's `doze __daemon --config <path>`).
pub const ARG: &str = "__daemon";

/// How long a start waits for the socket while the daemon says nothing (Go's 100 × 100 ms, from the start): a daemon
/// that writes a line to its log has that long again from the line.
const START_WAIT: Duration = Duration::from_secs(10);

/// How long a start waits while the daemon's `__fetch` child says a download is under way (a fresh doze home fetches
/// each module before the daemon serves): as long as the daemon itself gives the child, after which it fails the
/// download and exits, which the start sees.
const DOWNLOAD_WAIT: Duration = doze_fetch::child::CHILD_LIMIT;

/// The longest a start waits in all, however busy the daemon says it is.
const START_CEILING: Duration = Duration::from_secs(30 * 60);

/// How long a stop waits for the daemon to go (Go's 200 × 100 ms: past the shutdown's 15 s reap budget).
const STOP_WAIT: Duration = Duration::from_secs(20);

/// A stack's run folder and the files in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunDir(PathBuf);

impl RunDir {
    /// The run folder of the stack whose data folder is `data_dir` (`<data_dir>/run`).
    pub fn of(data_dir: &Path) -> RunDir {
        RunDir(data_dir.join("run"))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// The control socket (Go's `ControlSocketPath`).
    pub fn socket(&self) -> PathBuf {
        self.0.join("doze.admin.sock")
    }

    /// The pid file (Go's `PidFilePath`): the pid, then a newline.
    pub fn pid_file(&self) -> PathBuf {
        self.0.join("doze.pid")
    }

    /// The daemon's log (Go's `LogFilePath`).
    pub fn log_file(&self) -> PathBuf {
        self.0.join("doze.log")
    }

    fn lock_file(&self) -> PathBuf {
        self.0.join("doze.ctl.lock")
    }

    /// The blocks the daemon started with (Go's `daemon.BlocksFile`).
    pub fn blocks_file(&self) -> PathBuf {
        self.0.join("blocks.json")
    }
}

/// Records the blocks `cfg` declares as the daemon starts with them (Go's `recordBlocks`): each block's kind, type and
/// fingerprint, in Go's shape (`{"<name>": {"kind", "type", "fp"}}`). The daemon reads doze.hcl once, when it starts,
/// so `doze sync` compares the file as it is now with this and says which blocks differ and what applying them
/// restarts; `doze migrate` restarts the daemon first when its block differs. Written from doze.hcl's shape (read
/// without modules), the way the CLI reads it, so the two prints are made the same way.
pub fn write_blocks(run: &RunDir, cfg: &doze_config::Config) -> io::Result<()> {
    let map: serde_json::Map<String, serde_json::Value> = cfg
        .block_fingerprints()
        .into_iter()
        .map(|(name, p)| {
            let entry =
                serde_json::json!({ "kind": p.kind.as_str(), "type": p.ty, "fp": p.fingerprint });
            (name, entry)
        })
        .collect();
    let text =
        serde_json::to_string_pretty(&serde_json::Value::Object(map)).map_err(io::Error::other)?;
    std::fs::write(run.blocks_file(), text)
}

/// The blocks the running daemon started with, by name; `None` when there is no record, or it is not one (a Go
/// daemon's prints are made another way, so a caller that knows the daemon is Go's does not compare them).
pub fn blocks(run: &RunDir) -> Option<std::collections::BTreeMap<String, doze_config::BlockPrint>> {
    let text = std::fs::read_to_string(run.blocks_file()).ok()?;
    let serde_json::Value::Object(map) = serde_json::from_str(&text).ok()? else {
        return None;
    };
    map.into_iter()
        .map(|(name, v)| {
            let field = |k: &str| v.get(k).and_then(serde_json::Value::as_str);
            let print = doze_config::BlockPrint {
                kind: doze_config::Kind::parse(field("kind")?)?,
                ty: field("type").unwrap_or_default().to_owned(),
                fingerprint: field("fp")?.to_owned(),
            };
            Some((name, print))
        })
        .collect()
}

/// Whether a daemon accepts control connections here (Go's `Running`).
pub fn running(run: &RunDir) -> bool {
    doze_os::local::LocalStream::connect(&run.socket()).is_ok()
}

/// The running daemon's pid, or `None` (Go's `Pid`: line one of the pid file, and alive).
pub fn pid(run: &RunDir) -> Option<u32> {
    let raw = std::fs::read_to_string(run.pid_file()).ok()?;
    let pid: u32 = raw.lines().next()?.trim().parse().ok()?;
    (pid > 0 && doze_os::process::alive(pid)).then_some(pid)
}

/// Writes this process's pid file (the daemon does, once its socket is up), whole or not at all: it is written beside
/// and renamed into place, so a reader never sees it created but still empty.
pub fn write_pid(run: &RunDir) -> io::Result<()> {
    let file = run.pid_file();
    let tmp = file.with_extension("pid.tmp");
    std::fs::write(&tmp, format!("{}\n", std::process::id()))?;
    std::fs::rename(&tmp, &file)
}

/// What a start needs.
#[derive(Debug, Clone)]
pub struct Start<'a> {
    /// The doze executable (it runs itself as `doze __daemon`).
    pub exe: &'a Path,
    /// The stack's doze.hcl, absolute.
    pub config: &'a Path,
    /// `--var` overrides: the daemon gets them as `DOZE_VAR_<name>` (never on its command line, where `ps` shows
    /// them), as Go's `daemonEnv` did.
    pub vars: &'a [(String, String)],
}

/// Starts the daemon detached and waits for its socket (Go's `Start`); its pid. On a failed start the error quotes the
/// end of its log.
pub fn start(run: &RunDir, s: &Start<'_>) -> Result<u32, String> {
    start_relaying(run, s, &mut |_| {})
}

/// [`start`], handing `say` each line the daemon writes to its log while it is waited for: a first start on a fresh
/// doze home fetches the stack's modules before the daemon serves, and the person waiting should see why.
///
/// The wait is for progress, not for a fixed time (Go gave up after 10 s, which a module download on a slow network
/// outlasts): a daemon that has written nothing for [`START_WAIT`] has failed to start, unless its `__fetch` child said
/// a download is under way ([`doze_fetch::child::busy`]), which it is given [`DOWNLOAD_WAIT`] for; and none is waited
/// for past [`START_CEILING`]. A daemon that exits is reported at once.
pub fn start_relaying(
    run: &RunDir,
    s: &Start<'_>,
    say: &mut dyn FnMut(&str),
) -> Result<u32, String> {
    doze_os::fs::private_dir(run.path())
        .map_err(|e| format!("creating {}: {e}", run.path().display()))?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(run.log_file())
        .map_err(|e| format!("opening {}: {e}", run.log_file().display()))?;
    // This start's lines are the ones after what earlier daemons wrote.
    let mut lines = LogLines::from_end(run.log_file());
    let err = log.try_clone().map_err(|e| e.to_string())?;
    let mut cmd = std::process::Command::new(s.exe);
    cmd.arg(ARG)
        .arg("--config")
        .arg(s.config)
        .stdin(std::process::Stdio::null())
        .stdout(log)
        .stderr(err);
    for (k, v) in s.vars {
        cmd.env(format!("DOZE_VAR_{k}"), v);
    }
    let pid = doze_os::detach::spawn_detached(cmd).map_err(|e| format!("starting daemon: {e}"))?;
    wait_up(run, pid, &mut lines, say)
}

/// Waits for the daemon `pid` started to serve, relaying its log (see [`start_relaying`]); its pid.
fn wait_up(
    run: &RunDir,
    pid: u32,
    lines: &mut LogLines,
    say: &mut dyn FnMut(&str),
) -> Result<u32, String> {
    let t = Instant::now();
    let mut said = Instant::now();
    let mut downloads = 0usize;
    loop {
        // Up means answering and named in the pid file: the daemon writes it just after its socket accepts, and
        // what runs next (`down`, the app) finds the daemon by it.
        if running(run) && self::pid(run).is_some() {
            return Ok(pid);
        }
        for line in lines.read() {
            said = Instant::now();
            match doze_fetch::child::busy(&line) {
                Some(true) => downloads += 1,
                Some(false) => downloads = downloads.saturating_sub(1),
                None => {}
            }
            say(&line);
        }
        if !doze_os::process::alive(pid) {
            // What it wrote as it went.
            for line in lines.read() {
                say(&line);
            }
            return Err(format!(
                "daemon exited during startup: {}",
                this_start(lines)
            ));
        }
        let quiet = if downloads > 0 {
            DOWNLOAD_WAIT
        } else {
            START_WAIT
        };
        if said.elapsed() >= quiet || t.elapsed() >= START_CEILING {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let why = if t.elapsed() >= START_CEILING {
        format!("did not come up within {}", secs(START_CEILING))
    } else if downloads > 0 {
        format!(
            "did not come up: a download it started said nothing for {}",
            secs(DOWNLOAD_WAIT)
        )
    } else if lines.seen == 0 {
        format!("did not come up: it wrote nothing for {}", secs(START_WAIT))
    } else {
        format!(
            "did not come up: it wrote nothing more for {}",
            secs(START_WAIT)
        )
    };
    Err(format!(
        "daemon (pid {pid}, still running) {why}; {}\n(full log: {})",
        this_start(lines),
        run.log_file().display()
    ))
}

/// A wait in words: `10s`, `11m`.
fn secs(d: Duration) -> String {
    match d.as_secs() {
        s if s >= 120 && s % 60 == 0 => format!("{}m", s / 60),
        s => format!("{s}s"),
    }
}

/// What this start's daemon wrote, for an error: its last lines, or that it wrote none (an empty "recent log" under
/// the error said nothing).
fn this_start(lines: &LogLines) -> String {
    if lines.seen == 0 {
        return "its log has nothing from this start".into();
    }
    let last: Vec<&str> = lines.last.iter().map(String::as_str).collect();
    format!("recent log:\n{}", last.join("\n"))
}

/// The complete lines a log gains, read from where it ended (a partial last line waits for its end).
#[derive(Debug)]
struct LogLines {
    path: PathBuf,
    offset: u64,
    pending: Vec<u8>,
    /// How many lines were read.
    seen: usize,
    /// The last [`LogLines::KEEP`] of them.
    last: std::collections::VecDeque<String>,
}

impl LogLines {
    const KEEP: usize = 15;

    fn from_end(path: PathBuf) -> LogLines {
        let offset = std::fs::metadata(&path).map_or(0, |m| m.len());
        LogLines {
            path,
            offset,
            pending: Vec::new(),
            seen: 0,
            last: std::collections::VecDeque::new(),
        }
    }

    fn read(&mut self) -> Vec<String> {
        use std::io::{Read as _, Seek as _};
        let Ok(mut f) = std::fs::File::open(&self.path) else {
            return Vec::new();
        };
        let mut buf = Vec::new();
        if f.seek(io::SeekFrom::Start(self.offset)).is_err() || f.read_to_end(&mut buf).is_err() {
            return Vec::new();
        }
        self.offset += buf.len() as u64;
        self.pending.extend_from_slice(&buf);
        let Some(last) = self.pending.iter().rposition(|b| *b == b'\n') else {
            return Vec::new();
        };
        let rest = self.pending.split_off(last + 1);
        let done = std::mem::replace(&mut self.pending, rest);
        let lines: Vec<String> = String::from_utf8_lossy(&done)
            .split('\n')
            .map(|l| l.trim_end_matches('\r'))
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect();
        self.seen += lines.len();
        for l in &lines {
            if self.last.len() == LogLines::KEEP {
                self.last.pop_front();
            }
            self.last.push_back(l.clone());
        }
        lines
    }
}

/// Asks the daemon to stop and waits for it to go (Go's `Stop`): `Ok(false)` when none was running, `Ok(true)` once it
/// has gone. Where it cannot be asked (Windows), it is ended; its Job objects end its engines.
pub fn stop(run: &RunDir) -> Result<bool, String> {
    let Some(pid) = pid(run) else {
        return Ok(false);
    };
    match doze_os::process::ask_to_stop(pid) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::Unsupported => {
            doze_os::process::kill_recorded(pid).map_err(|e| format!("ending daemon: {e}"))?;
        }
        Err(e) => return Err(format!("signalling daemon: {e}")),
    }
    let t = Instant::now();
    while t.elapsed() < STOP_WAIT {
        if !doze_os::process::alive(pid) {
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!("daemon (pid {pid}) did not stop"))
}

/// The last `n` lines of the daemon's log (Go's `TailLog`).
pub fn tail_log(run: &RunDir, n: usize) -> String {
    let Ok(text) = std::fs::read_to_string(run.log_file()) else {
        return "(no log available)".into();
    };
    let lines: Vec<&str> = text.trim_end_matches('\n').split('\n').collect();
    lines
        .get(lines.len().saturating_sub(n)..)
        .unwrap_or_default()
        .join("\n")
}

/// The stack lock `up` and `down` hold while they start or stop its daemon. Dropped, it is released; a holder that
/// dies releases it too.
#[derive(Debug)]
pub struct StackLock {
    _file: std::fs::File,
}

impl StackLock {
    /// Takes the lock, waiting for whoever holds it.
    pub fn take(run: &RunDir) -> io::Result<StackLock> {
        let file = StackLock::open(run)?;
        file.lock()?;
        Ok(StackLock { _file: file })
    }

    /// Takes the lock if no one holds it; `None` if someone does.
    pub fn try_take(run: &RunDir) -> io::Result<Option<StackLock>> {
        let file = StackLock::open(run)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(StackLock { _file: file })),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(e)) => Err(e),
        }
    }

    fn open(run: &RunDir) -> io::Result<std::fs::File> {
        doze_os::fs::private_dir(run.path())?;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(run.lock_file())?;
        f.flush()?;
        Ok(f)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod blocks_tests {
    use super::*;

    /// The record reads back as written, and Go's file (the same shape, Go's prints) reads too.
    #[test]
    fn the_blocks_record_reads_back_and_reads_gos() {
        let dir = std::env::temp_dir().join(format!("doze-blocks-{}", std::process::id()));
        let run = RunDir::of(&dir);
        std::fs::create_dir_all(run.path()).unwrap();
        let src = "app \"api\" {\n  run = \"sleep 600\"\n}\n";
        std::fs::write(dir.join("doze.hcl"), src).unwrap();
        let path = dir.join("doze.hcl").to_string_lossy().into_owned();
        let cfg =
            crate::entry::with_options("0.0.0", &doze_config::NoModules, Default::default(), |o| {
                doze_config::load_shape(&path, o)
            })
            .unwrap();
        write_blocks(&run, &cfg).unwrap();
        assert_eq!(blocks(&run).unwrap(), cfg.block_fingerprints());
        std::fs::write(
            run.blocks_file(),
            r#"{"db": {"kind": "module", "type": "postgres", "fp": "ab12"}}"#,
        )
        .unwrap();
        let go = blocks(&run).unwrap();
        assert_eq!(go["db"].fingerprint, "ab12");
        std::fs::write(run.blocks_file(), "[]").unwrap();
        assert_eq!(blocks(&run), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
