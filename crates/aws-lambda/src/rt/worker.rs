//! One function process and the Runtime API it talks to. A worker is started for a function version, runs invocations one at a time,
//! and ends when it has been idle too long, when it is killed (a timeout, a retirement), when it exits on its own, or at shutdown.
//! Every task it starts is tracked, so a stop leaves no child behind.

use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use doze_runtime::{Clock, Tasks};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::api::{Shared, Work, serve};
use super::spec::{Spec, environment, launch};

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
    lines: Mutex<Vec<Line>>,
    seen: AtomicU64,
    last_used_ms: AtomicI64,
    /// Whether it has served an invocation (the first one pays for the start and reports it as `Init Duration`).
    pub(crate) warm: AtomicBool,
    /// How the process ended, once it has.
    pub(crate) exit: Mutex<Option<String>>,
}

/// Why a process could not be started.
#[derive(Debug)]
pub(crate) struct LaunchError(pub(crate) String);

impl Worker {
    /// Starts the process for `spec`.
    ///
    /// # Errors
    /// The command cannot be had or run (no interpreter, no `bootstrap`).
    pub(crate) async fn start(
        spec: Spec,
        tasks: &Tasks,
        clock: &Arc<dyn Clock>,
        idle: Duration,
    ) -> Result<Arc<Self>, LaunchError> {
        let (program, args) = launch(&spec).map_err(LaunchError)?;
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|e| LaunchError(e.to_string()))?;
        let api = listener
            .local_addr()
            .map_err(|e| LaunchError(e.to_string()))?
            .to_string();
        let mut command = Command::new(&program);
        command
            .args(&args)
            .env_clear()
            .envs(environment(&spec, &api))
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
        // Its own process group, so that ending it ends what it started too.
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command
            .spawn()
            .map_err(|e| LaunchError(format!("{}: {e}", program.display())))?;
        let (tx, rx) = mpsc::channel(8);
        let shared = Arc::new(Shared::new(rx));
        let now = clock.now_ms();
        let worker = Arc::new(Self {
            spec,
            shared: Arc::clone(&shared),
            work: tx,
            busy: AtomicBool::new(false),
            dead: AtomicBool::new(false),
            retired: AtomicBool::new(false),
            kill: CancellationToken::new(),
            lines: Mutex::new(Vec::new()),
            seen: AtomicU64::new(0),
            last_used_ms: AtomicI64::new(now),
            warm: AtomicBool::new(false),
            exit: Mutex::new(None),
        });
        let stop = tasks.stopping();
        if let Some(out) = child.stdout.take() {
            tasks.spawn(pump(out, Arc::clone(&worker), Arc::clone(clock)));
        }
        if let Some(err) = child.stderr.take() {
            tasks.spawn(pump(err, Arc::clone(&worker), Arc::clone(clock)));
        }
        tasks.spawn(serve(listener, shared, worker.kill.clone(), tasks.clone()));
        tasks.spawn(supervise(child, Arc::clone(&worker), stop.clone()));
        tasks.spawn(reap_idle(
            Arc::clone(&worker),
            Arc::clone(clock),
            idle,
            stop,
        ));
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
        self.busy.store(false, Ordering::Release);
        if self.retired.load(Ordering::Acquire) {
            self.kill();
        }
    }

    pub(crate) fn is_dead(&self) -> bool {
        self.dead.load(Ordering::Acquire)
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

    /// What the process has written since the last call, once its output has stopped arriving (a line printed just before the
    /// response may still be on its way through the pipe).
    pub(crate) async fn take_lines(&self) -> Vec<Line> {
        for _ in 0..10 {
            let before = self.seen.load(Ordering::Acquire);
            tokio::time::sleep(Duration::from_millis(2)).await;
            if self.seen.load(Ordering::Acquire) == before {
                break;
            }
        }
        std::mem::take(&mut *self.lines.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// What the process has written since the last call, at once (for the log while the invocation runs).
    pub(crate) fn drain(&self) -> Vec<Line> {
        std::mem::take(&mut *self.lines.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn push(&self, at: i64, line: String) {
        self.lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((at, line));
        self.seen.fetch_add(1, Ordering::AcqRel);
    }
}

/// Reads a pipe line by line into the worker.
async fn pump<R: AsyncRead + Unpin>(pipe: R, worker: Arc<Worker>, clock: Arc<dyn Clock>) {
    let mut lines = BufReader::new(pipe).lines();
    loop {
        tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(line)) => worker.push(clock.now_ms(), line),
                _ => return,
            },
            // The process is over: what it wrote last is still in the pipe, but something it started may hold the other end open, so
            // only a moment is spent on it.
            () = worker.kill.cancelled() => {
                let tail = async { while let Ok(Some(line)) = lines.next_line().await { worker.push(clock.now_ms(), line); } };
                drop(tokio::time::timeout(Duration::from_millis(100), tail).await);
                return;
            }
        }
    }
}

/// Waits for the process to end, or ends it when asked.
async fn supervise(mut child: tokio::process::Child, worker: Arc<Worker>, stop: CancellationToken) {
    tokio::select! {
        status = child.wait() => {
            if let Ok(status) = status {
                tracing::debug!(function = %worker.spec.name, "function process exited: {status}");
                *worker.exit.lock().unwrap_or_else(PoisonError::into_inner) = Some(status.to_string());
            }
        }
        () = worker.kill.cancelled() => end(&mut child).await,
        () = stop.cancelled() => end(&mut child).await,
    }
    worker.dead.store(true, Ordering::Release);
    // A process that ended mid-invocation closes the channel its caller waits on.
    worker.shared.drop_current();
    worker.kill.cancel();
}

/// Ends the process and what it started.
async fn end(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        // WHY: the group is killed through kill(1) because signalling a group needs a libc call, which this crate does not make.
        drop(
            Command::new("kill")
                .arg("-KILL")
                .arg("--")
                .arg(format!("-{pid}"))
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await,
        );
    }
    drop(child.start_kill());
    drop(child.wait().await);
}

/// Ends the worker once it has been idle for `idle`; sleeps until the moment that could happen, and does nothing while it works.
async fn reap_idle(
    worker: Arc<Worker>,
    clock: Arc<dyn Clock>,
    idle: Duration,
    stop: CancellationToken,
) {
    let idle_ms = i64::try_from(idle.as_millis()).unwrap_or(i64::MAX);
    loop {
        let wait = if worker.busy.load(Ordering::Acquire) {
            idle_ms
        } else {
            let since = clock.now_ms() - worker.last_used_ms.load(Ordering::Acquire);
            if since >= idle_ms {
                worker.kill();
                return;
            }
            idle_ms - since
        };
        let sleep = Duration::from_millis(u64::try_from(wait).unwrap_or(1).max(1));
        tokio::select! {
            () = tokio::time::sleep(sleep) => {}
            () = worker.kill.cancelled() => return,
            () = stop.cancelled() => return,
        }
    }
}
