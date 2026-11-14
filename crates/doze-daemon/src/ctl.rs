//! The daemon's lifecycle from the outside (Go's `internal/daemonctl`): start it detached (`doze __daemon`, a session
//! of its own, its output to `doze.log`), wait for its control socket, find it by its pid file, stop it, and the stack
//! lock that `up` and `down` take so two of them never start or stop one stack at once. Callers print; these return
//! values and errors.
//!
//! The layout is Go's, under a stack's run folder (`<home>/projects/<slug>/run`): `doze.admin.sock`, `doze.pid` (the
//! pid on line one) and `doze.log`. The lock is the Rust doze's own (`doze.ctl.lock`, an advisory lock the OS drops
//! when its holder dies); Go had none.

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The hidden argument the daemon runs under (Go's `doze __daemon --config <path>`).
pub const ARG: &str = "__daemon";

/// How long a start waits for the socket (Go's 100 × 100 ms).
const START_WAIT: Duration = Duration::from_secs(10);

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

/// Writes this process's pid file (the daemon does, once its socket is up).
pub fn write_pid(run: &RunDir) -> io::Result<()> {
    std::fs::write(run.pid_file(), format!("{}\n", std::process::id()))
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
    doze_os::fs::private_dir(run.path())
        .map_err(|e| format!("creating {}: {e}", run.path().display()))?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(run.log_file())
        .map_err(|e| format!("opening {}: {e}", run.log_file().display()))?;
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
    let t = Instant::now();
    while t.elapsed() < START_WAIT {
        if running(run) {
            return Ok(pid);
        }
        if !doze_os::process::alive(pid) {
            return Err(format!(
                "daemon exited during startup:\n{}",
                tail_log(run, 15)
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!(
        "daemon did not come up within 10s; recent log:\n{}\n(full log: {})",
        tail_log(run, 15),
        run.log_file().display()
    ))
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
