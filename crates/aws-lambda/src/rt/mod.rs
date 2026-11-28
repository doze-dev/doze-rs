//! Running functions: processes that speak the Runtime API ([`api`]), the workers that supervise them ([`worker`]), the pool that keeps
//! them warm and limits them ([`Runtime`]), and the invocation in flight ([`Running`]).

pub(crate) mod api;
pub(crate) mod check;
pub(crate) mod logger;
pub(crate) mod logs;
mod pool;
pub(crate) mod pump;
pub(crate) mod shims;
pub(crate) mod spec;
pub(crate) mod worker;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use bytes::Bytes;
use doze_runtime::{Clock, Tasks};
use serde_json::json;
use tokio::sync::{OwnedSemaphorePermit, mpsc, oneshot};
use tokio::time::Instant;

use self::api::{Chunk, Work};
use self::pool::{Pool, Slot};
use self::spec::{Creds, Interpreters, Spec};
use self::worker::{Line, Start, Worker};

/// How long a process may take to come up and ask for its first invocation (Lambda gives init 10 s of its own).
const INIT_BUDGET: Duration = Duration::from_secs(10);
/// A process whose credentials end within this long is not given another invocation.
const CREDS_MARGIN_MS: i64 = 300_000;

/// What a pool is configured with.
#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    /// How long a warm process waits for the next invocation before it ends.
    pub idle: Duration,
    /// The most function processes alive at once, of every function; when it is reached, the one idle longest makes room.
    pub max_processes: usize,
    /// Where each runtime family's executable is, when it is not on `PATH`.
    pub interpreters: Interpreters,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            idle: Duration::from_secs(300),
            max_processes: 32,
            interpreters: Interpreters::default(),
        }
    }
}

/// The processes of every function, by function version.
#[derive(Debug)]
pub(crate) struct Runtime {
    tasks: Tasks,
    clock: Arc<dyn Clock>,
    config: RuntimeConfig,
    dir: PathBuf,
    pool: Pool,
    /// Invocations in flight by function, for reserved concurrency.
    inflight: Mutex<HashMap<String, usize>>,
}

/// The invocation, for the runner.
#[derive(Debug)]
pub(crate) struct Ask {
    /// Pool key: the function version and the revision it was configured at.
    pub(crate) key: String,
    /// `region/name`, for reserved concurrency and retirement.
    pub(crate) function: String,
    pub(crate) reserved: Option<i64>,
    pub(crate) spec: Spec,
    pub(crate) payload: Bytes,
    pub(crate) timeout: Duration,
    pub(crate) invoked_arn: String,
    pub(crate) client_context: Option<String>,
    pub(crate) trace_id: String,
}

/// What came of waiting for the next thing from the process.
#[derive(Debug)]
pub(crate) enum Event {
    Chunk(Chunk),
    /// The function ran out of time.
    Timeout,
    /// The process went away.
    Crashed,
}

/// Why an invocation was not started.
#[derive(Debug)]
pub(crate) enum Refused {
    /// The function's reserved concurrency is used up.
    Reserved,
    /// Every process the pool may have is working, and none came free in time.
    Full,
    /// The process could not be started.
    Launch(String),
}

/// An invocation handed to a process.
#[derive(Debug)]
pub(crate) struct Running {
    pub(crate) id: String,
    worker: Arc<Worker>,
    permit: OwnedSemaphorePermit,
    rx: mpsc::Receiver<Chunk>,
    started: Option<oneshot::Receiver<Instant>>,
    begin: Instant,
    began_exec: Option<Instant>,
    pub(crate) timeout: Duration,
    function: String,
    clock: Arc<dyn Clock>,
}

/// How an invocation ended, for the log and the report.
#[derive(Debug)]
pub(crate) struct Finished {
    pub(crate) lines: Vec<Line>,
    pub(crate) duration_ms: f64,
    pub(crate) init_ms: Option<f64>,
    pub(crate) stream: String,
}

impl Runtime {
    pub(crate) fn new(
        tasks: Tasks,
        clock: Arc<dyn Clock>,
        config: RuntimeConfig,
        dir: PathBuf,
    ) -> Self {
        Self {
            tasks,
            clock,
            pool: Pool::new(config.max_processes),
            config,
            dir,
            inflight: Mutex::new(HashMap::new()),
        }
    }

    /// Where each runtime family's executable is, when it is configured.
    pub(crate) fn interpreters(&self) -> &Interpreters {
        &self.config.interpreters
    }

    /// Ends every warm process of `function` (`region/name`), so the next invocation starts one with the new code or configuration.
    pub(crate) fn retire(&self, function: &str) {
        self.pool.each(&format!("{function}/"), |w| w.retire());
    }

    /// How many processes of `function` (`region/name`) are running, whatever version they run.
    pub(crate) fn alive(&self, function: &str) -> usize {
        let mut n = 0;
        self.pool.each(&format!("{function}/"), |w| {
            n += usize::from(!w.is_dead() && !w.is_retired());
        });
        n
    }

    /// How many version entries the pool keeps (for tests).
    #[cfg(test)]
    pub(crate) fn slots(&self) -> usize {
        self.pool.slots()
    }

    /// How many connections each live process of `function` opened to its Runtime API (for tests).
    #[cfg(test)]
    pub(crate) fn connections(&self, function: &str) -> Vec<usize> {
        let mut out = Vec::new();
        self.pool.each(&format!("{function}/"), |w| {
            if !w.is_dead() {
                out.push(w.shared.connections.load(Ordering::Relaxed));
            }
        });
        out
    }

    /// Starts the invocation on a warm process or a new one.
    ///
    /// # Errors
    /// See [`Refused`].
    pub(crate) async fn begin(&self, mut ask: Ask) -> Result<Running, Refused> {
        if !self.take_inflight(&ask.function, ask.reserved) {
            return Err(Refused::Reserved);
        }
        let slot = self.pool.slot(&ask.key);
        let Ok(permit) = Arc::clone(&slot.permits).acquire_owned().await else {
            self.drop_inflight(&ask.function);
            return Err(Refused::Launch("the pool is closed".to_owned()));
        };
        let now = self.clock.now_ms();
        let warm = {
            let mut workers = slot.workers.lock().unwrap_or_else(PoisonError::into_inner);
            workers.retain(|w| !w.is_dead());
            workers.iter().find(|w| fresh(w, now) && w.claim()).cloned()
        };
        let worker = match warm {
            Some(w) => w,
            None => match self.spawn(&mut ask, &slot).await {
                Ok(w) => w,
                Err(why) => {
                    self.drop_inflight(&ask.function);
                    return Err(why);
                }
            },
        };
        let (tx, rx) = mpsc::channel(32);
        let (started_tx, started_rx) = oneshot::channel();
        let id = aws_seal::uuid_v4().unwrap_or_default();
        let deadline_ms = now + i64::try_from(ask.timeout.as_millis()).unwrap_or(i64::MAX);
        let work = Work {
            id: id.clone(),
            payload: ask.payload,
            deadline_ms,
            invoked_arn: ask.invoked_arn,
            trace_id: ask.trace_id,
            client_context: ask.client_context,
            started: started_tx,
            sink: tx,
        };
        if !worker.send(work).await {
            worker.kill();
            self.drop_inflight(&ask.function);
            return Err(Refused::Launch(
                "the function process went away before it was given the invocation".to_owned(),
            ));
        }
        Ok(Running {
            id,
            worker,
            permit,
            rx,
            started: Some(started_rx),
            begin: Instant::now(),
            began_exec: None,
            timeout: ask.timeout,
            function: ask.function,
            clock: Arc::clone(&self.clock),
        })
    }

    async fn spawn(&self, ask: &mut Ask, slot: &Slot) -> Result<Arc<Worker>, Refused> {
        shims::ensure(&self.dir).map_err(|e| {
            Refused::Launch(format!("the runtime clients could not be written: {e}"))
        })?;
        ask.spec.shims = shims::dir(&self.dir);
        ask.spec.interpreters = self.config.interpreters.clone();
        ask.spec.log_stream = logs::stream_name(&ask.spec.version, self.clock.now_ms());
        let place = self.pool.place().await.ok_or(Refused::Full)?;
        let start = Start {
            tasks: &self.tasks,
            clock: &self.clock,
            idle: self.config.idle,
            place,
        };
        let worker = Worker::start(ask.spec.clone(), start)
            .await
            .map_err(|e| Refused::Launch(e.0))?;
        assert_claimed(&worker);
        slot.workers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Arc::clone(&worker));
        Ok(worker)
    }

    fn take_inflight(&self, function: &str, reserved: Option<i64>) -> bool {
        let mut m = self.inflight.lock().unwrap_or_else(PoisonError::into_inner);
        let n = m.entry(function.to_owned()).or_insert(0);
        if reserved.is_some_and(|r| i64::try_from(*n).unwrap_or(i64::MAX) >= r) {
            return false;
        }
        *n += 1;
        true
    }

    fn drop_inflight(&self, function: &str) {
        let mut m = self.inflight.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(n) = m.get_mut(function) {
            *n = n.saturating_sub(1);
        }
    }

    /// Invocations in flight now: of `function` (`region/name`), and of every function of its region (for `ConcurrentExecutions`).
    pub(crate) fn concurrency(&self, function: &str) -> (usize, usize) {
        let m = self.inflight.lock().unwrap_or_else(PoisonError::into_inner);
        let region = function.split_once('/').map_or("", |(r, _)| r);
        let total = m
            .iter()
            .filter(|(k, _)| k.split_once('/').is_some_and(|(r, _)| r == region))
            .map(|(_, n)| *n)
            .sum();
        (m.get(function).copied().unwrap_or(0), total)
    }

    /// The invocation finished: the count of reserved concurrency goes down.
    pub(crate) fn done(&self, function: &str) {
        self.drop_inflight(function);
    }
}

/// A freshly started worker is claimed for the invocation that started it.
fn assert_claimed(w: &Worker) {
    let claimed = w.claim();
    debug_assert!(claimed);
}

/// Whether a worker can take another invocation: it is not near the end of its credentials.
fn fresh(w: &Worker, now_ms: i64) -> bool {
    let end = w.spec.creds.expires_ms;
    end == 0 || end - now_ms > CREDS_MARGIN_MS
}

impl Running {
    /// Waits until the process has fetched the invocation; `Err` with the reason when it never does (it failed to start, or took
    /// longer than the init budget).
    pub(crate) async fn started(&mut self) -> Result<(), InitFailure> {
        let Some(rx) = self.started.take() else {
            return Ok(());
        };
        tokio::select! {
            got = tokio::time::timeout(INIT_BUDGET, rx) => match got {
                // When the process fetched it (it may have before this is polled), so `Duration` is the function's own time.
                Ok(Ok(at)) => {
                    self.began_exec = Some(at);
                    Ok(())
                }
                Ok(Err(_)) => Err(self.died_in_init()),
                Err(_) => Err(InitFailure::Timeout),
            },
            () = self.worker.kill_signal() => Err(self.died_in_init()),
        }
    }

    fn died_in_init(&self) -> InitFailure {
        let reported = self
            .worker
            .shared
            .init_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        InitFailure::Exited(reported)
    }

    /// The next thing the process says, or that the time ran out or the process went away.
    pub(crate) async fn next(&mut self) -> Event {
        let deadline = self.began_exec.unwrap_or(self.begin) + self.timeout;
        match tokio::time::timeout_at(deadline, self.rx.recv()).await {
            Ok(Some(chunk)) => Event::Chunk(chunk),
            Ok(None) => Event::Crashed,
            Err(_) => Event::Timeout,
        }
    }

    /// The process, for reading what it writes while the invocation runs.
    pub(crate) fn tap(&self) -> Arc<Worker> {
        Arc::clone(&self.worker)
    }

    /// How the process ended, if it has.
    pub(crate) fn exit_status(&self) -> Option<String> {
        self.worker
            .exit
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Ends the invocation: the process is given back to the pool when it is `healthy` and ended otherwise; the output it wrote is
    /// handed over.
    pub(crate) async fn finish(self, healthy: bool, rt: &Runtime) -> Finished {
        let exec = self.began_exec.map_or(Duration::ZERO, |t| t.elapsed());
        let init = self.began_exec.map(|t| t.duration_since(self.begin));
        // The lines are taken before the process is given back, so none of the next invocation's are among them.
        self.worker.settle().await;
        let lines = self.worker.drain();
        if healthy {
            self.worker.release(self.clock.now_ms());
            rt.pool.notify_free();
        } else {
            self.worker.kill();
        }
        let first = !self.worker.warm.swap(true, Ordering::AcqRel);
        rt.done(&self.function);
        drop(self.permit);
        Finished {
            lines,
            duration_ms: exec.as_secs_f64() * 1000.0,
            init_ms: first.then(|| init.map_or(0.0, |d| d.as_secs_f64() * 1000.0)),
            stream: self.worker.spec.log_stream.clone(),
        }
    }
}

/// Why a process never fetched its invocation.
#[derive(Debug)]
pub(crate) enum InitFailure {
    /// It did not ask within the init budget.
    Timeout,
    /// It ended first; the error document it reported with `init/error`, if it did.
    Exited(Option<Bytes>),
}

/// The error document for a timeout: `{"errorMessage":"<time> <id> Task timed out after 3.00 seconds"}`.
pub(crate) fn timeout_document(id: &str, at: &str, timeout: Duration) -> Bytes {
    Bytes::from(json!({"errorMessage": format!("{at} {id} Task timed out after {:.2} seconds", timeout.as_secs_f64())}).to_string())
}

/// Credentials for a function that runs as no role.
pub(crate) fn test_credentials() -> Creds {
    Creds {
        key: "test".to_owned(),
        secret: "test".to_owned(),
        token: String::new(),
        expires_ms: 0,
    }
}
