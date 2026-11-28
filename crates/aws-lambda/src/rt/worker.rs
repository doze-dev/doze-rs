//! One function process and the Runtime API it talks to. A worker is started for a function version, runs invocations one at a time,
//! and ends when it has been idle too long, when it is killed (a timeout, a retirement, room for another), when it exits on its own,
//! or at shutdown; however it ends, its process group goes with it. Three tasks serve it: the Runtime API's listener, one reader of
//! both its pipes (`pump.rs`), and its supervisor (the exit, the idle time). Every task is tracked, so a stop leaves no child behind.

use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use doze_runtime::{Clock, Tasks};
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::sync::{OwnedSemaphorePermit, mpsc};
use tokio_util::sync::CancellationToken;

use super::api::{Shared, Work, serve};
use super::pump::{Held, Lines, pump};
use super::spec::{Spec, environment, launch};

/// The order processes were given back in, across the pool.
static TURNS: AtomicU64 = AtomicU64::new(0);

/// A line the process wrote, with when we read it.
pub(crate) type Line = (i64, String);

/// A running (or just ended) function process.
#[derive(Debug)]
pub(crate) struct Worker {
    pub(crate) spec: Spec,
    pub(crate) shared: Arc<Shared>,
    work: mpsc::Sender<Work>,
    busy: AtomicBool,
    dead: AtomicBool,
    retired: AtomicBool,
    kill: CancellationToken,
    held: Mutex<Held>,
    seen: AtomicU64,
    /// Second handles on the read ends of its pipes, to ask how much is still unread (`settle`).
    #[cfg(unix)]
    pipes: Vec<std::os::fd::OwnedFd>,
    last_used_ms: AtomicI64,
    /// When it was last given back, as a turn of [`TURNS`]: two processes given back in the same millisecond are still in order.
    last_used_turn: AtomicU64,
    /// Whether it has served an invocation (the first one pays for the start and reports it as `Init Duration`).
    pub(crate) warm: AtomicBool,
    /// How the process ended, once it has.
    pub(crate) exit: Mutex<Option<String>>,
}

/// Why a process could not be started.
#[derive(Debug)]
pub(crate) struct LaunchError(pub(crate) String);

/// What a worker is started with besides its spec.
pub(crate) struct Start<'a> {
    pub(crate) tasks: &'a Tasks,
    pub(crate) clock: &'a Arc<dyn Clock>,
    pub(crate) idle: Duration,
    /// Its place under the pool's cap on processes, given back when the process has ended.
    pub(crate) place: OwnedSemaphorePermit,
}

impl Worker {
    /// Starts the process for `spec`.
    ///
    /// # Errors
    /// The command cannot be had or run (no interpreter, no `bootstrap`).
    pub(crate) async fn start(spec: Spec, s: Start<'_>) -> Result<Arc<Self>, LaunchError> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|e| LaunchError(e.to_string()))?;
        let api = listener
            .local_addr()
            .map_err(|e| LaunchError(e.to_string()))?
            .to_string();
        let mut child = spawn(&spec, &api)?;
        let (out, err) = (child.stdout.take(), child.stderr.take());
        let (tx, rx) = mpsc::channel(8);
        let shared = Arc::new(Shared::new(rx));
        let worker = Arc::new(Self {
            spec,
            shared: Arc::clone(&shared),
            work: tx,
            busy: AtomicBool::new(false),
            dead: AtomicBool::new(false),
            retired: AtomicBool::new(false),
            kill: CancellationToken::new(),
            held: Mutex::new(Held::default()),
            seen: AtomicU64::new(0),
            #[cfg(unix)]
            pipes: second_handles(out.as_ref(), err.as_ref()),
            last_used_ms: AtomicI64::new(s.clock.now_ms()),
            last_used_turn: AtomicU64::new(TURNS.fetch_add(1, Ordering::Relaxed)),
            warm: AtomicBool::new(false),
            exit: Mutex::new(None),
        });
        if let (Some(out), Some(err)) = (out, err) {
            let (w, clock) = (Arc::clone(&worker), Arc::clone(s.clock));
            let ended = worker.kill.clone().cancelled_owned();
            let keep = move |line| w.push(clock.now_ms(), line);
            s.tasks
                .spawn(pump(Lines::new(out), Lines::new(err), ended, keep));
        }
        s.tasks.spawn(serve(
            listener,
            shared,
            worker.kill.clone(),
            s.tasks.clone(),
        ));
        let watch = Watch {
            worker: Arc::clone(&worker),
            clock: Arc::clone(s.clock),
            idle: s.idle,
            stop: s.tasks.stopping(),
            place: s.place,
        };
        s.tasks.spawn(supervise(child, watch));
        Ok(worker)
    }

    /// Takes the worker for an invocation, if it is alive and free.
    pub(crate) fn claim(&self) -> bool {
        !self.dead.load(Ordering::Acquire)
            && !self.retired.load(Ordering::Acquire)
            && self
                .busy
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }

    /// Gives the worker back after an invocation.
    pub(crate) fn release(&self, now_ms: i64) {
        self.last_used_ms.store(now_ms, Ordering::Release);
        self.last_used_turn
            .store(TURNS.fetch_add(1, Ordering::Relaxed), Ordering::Release);
        self.busy.store(false, Ordering::Release);
        if self.retired.load(Ordering::Acquire) {
            self.kill();
        }
    }

    pub(crate) fn is_dead(&self) -> bool {
        self.dead.load(Ordering::Acquire)
    }

    /// Whether it is alive and waiting for an invocation, and since when (the millisecond, then the turn within it): what the pool's
    /// cap ends first. The millisecond alone tied a warm invocation answered within the millisecond another process was given back,
    /// and the tie went to whichever process the map listed first.
    pub(crate) fn idle_since(&self) -> Option<(i64, u64)> {
        let free = !self.busy.load(Ordering::Acquire) && !self.is_dead();
        free.then(|| {
            (
                self.last_used_ms.load(Ordering::Acquire),
                self.last_used_turn.load(Ordering::Acquire),
            )
        })
    }

    /// Whether it has been told to end once it is idle.
    pub(crate) fn is_retired(&self) -> bool {
        self.retired.load(Ordering::Acquire)
    }

    /// Ends the process once it is idle: its code or configuration has been replaced.
    pub(crate) fn retire(&self) {
        self.retired.store(true, Ordering::Release);
        if !self.busy.load(Ordering::Acquire) {
            self.kill();
        }
    }

    /// Resolves when the process has ended or been ended.
    pub(crate) async fn kill_signal(&self) {
        self.kill.cancelled().await;
    }

    /// Ends the process.
    pub(crate) fn kill(&self) {
        self.dead.store(true, Ordering::Release);
        self.kill.cancel();
    }

    /// Hands an invocation to the process. `false` when the process is gone.
    pub(crate) async fn send(&self, work: Work) -> bool {
        self.work.send(work).await.is_ok()
    }

    /// Waits until what the process wrote before now has been read: its pipes hold nothing unread (a line printed just before the
    /// response is in the pipe when the response arrives). No sleep: the reader is let run until the pipes are empty, a bounded
    /// number of times, since a process that never stops printing never empties them.
    pub(crate) async fn settle(&self) {
        #[cfg(unix)]
        for _ in 0..200 {
            let unread = self
                .pipes
                .iter()
                .any(|fd| rustix::io::ioctl_fionread(fd).is_ok_and(|n| n > 0));
            if !unread {
                return;
            }
            tokio::task::yield_now().await;
        }
        // Without FIONREAD: until a few milliseconds pass with no new line.
        #[cfg(not(unix))]
        for _ in 0..10 {
            let before = self.seen.load(Ordering::Acquire);
            tokio::time::sleep(Duration::from_millis(2)).await;
            if self.seen.load(Ordering::Acquire) == before {
                break;
            }
        }
    }

    /// What the process has written since the last call.
    pub(crate) fn drain(&self) -> Vec<Line> {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    fn push(&self, at: i64, line: String) {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((at, line));
        self.seen.fetch_add(1, Ordering::AcqRel);
    }
}

/// Starts the process, in a process group of its own, so that ending it ends what it started too.
fn spawn(spec: &Spec, api: &str) -> Result<tokio::process::Child, LaunchError> {
    let (program, args) = launch(spec).map_err(LaunchError)?;
    let mut command = Command::new(&program);
    command
        .args(&args)
        .env_clear()
        .envs(environment(spec, api))
        .current_dir(if spec.root.is_dir() {
            spec.root.clone()
        } else {
            spec.root
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_default()
        })
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    command
        .spawn()
        .map_err(|e| LaunchError(format!("{}: {e}", program.display())))
}

/// Second handles on the pipes' read ends (a duplicate shares the pipe, and its unread count, without taking from it).
#[cfg(unix)]
fn second_handles(
    out: Option<&tokio::process::ChildStdout>,
    err: Option<&tokio::process::ChildStderr>,
) -> Vec<std::os::fd::OwnedFd> {
    use std::os::fd::AsFd;
    let mut fds = Vec::new();
    fds.extend(out.and_then(|o| o.as_fd().try_clone_to_owned().ok()));
    fds.extend(err.and_then(|e| e.as_fd().try_clone_to_owned().ok()));
    fds
}

/// What the supervisor watches the process with.
struct Watch {
    worker: Arc<Worker>,
    clock: Arc<dyn Clock>,
    idle: Duration,
    stop: CancellationToken,
    place: OwnedSemaphorePermit,
}

/// Waits for the process to end, or ends it: when asked, at shutdown, or once it has been idle for `idle` (it sleeps until the moment
/// that could happen, and does nothing while the process works). However it ends, its group is killed, so nothing it started stays.
async fn supervise(mut child: tokio::process::Child, w: Watch) {
    let pid = child.id();
    let idle_ms = i64::try_from(w.idle.as_millis()).unwrap_or(i64::MAX);
    let worker = &w.worker;
    let ours = loop {
        let wait = if worker.busy.load(Ordering::Acquire) {
            idle_ms
        } else {
            let since = w.clock.now_ms() - worker.last_used_ms.load(Ordering::Acquire);
            if since >= idle_ms {
                break true;
            }
            idle_ms - since
        };
        let sleep = Duration::from_millis(u64::try_from(wait).unwrap_or(1).max(1));
        tokio::select! {
            status = child.wait() => {
                if let Ok(status) = status {
                    tracing::debug!(function = %worker.spec.name, "function process exited: {status}");
                    *worker.exit.lock().unwrap_or_else(PoisonError::into_inner) = Some(status.to_string());
                }
                break false;
            }
            () = worker.kill.cancelled() => break true,
            () = w.stop.cancelled() => break true,
            () = tokio::time::sleep(sleep) => {}
        }
    };
    kill_group(pid);
    if ours {
        drop(child.start_kill());
        drop(child.wait().await);
    }
    worker.dead.store(true, Ordering::Release);
    // A process that ended mid-invocation closes the channel its caller waits on.
    worker.shared.drop_current();
    worker.kill.cancel();
    drop(w.place);
}

/// Kills the process group `pid` leads (a group outlives its leader while a member is alive, so its id is not reused before).
fn kill_group(pid: Option<u32>) {
    #[cfg(unix)]
    if let Some(pid) = pid
        .and_then(|p| i32::try_from(p).ok())
        .and_then(rustix::process::Pid::from_raw)
    {
        // ESRCH: nothing of the group is left; that is the usual answer and not worth a word.
        if let Err(e) = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL)
            && e != rustix::io::Errno::SRCH
        {
            tracing::debug!("the function's process group could not be killed: {e}");
        }
    }
    #[cfg(not(unix))]
    drop(pid);
}
