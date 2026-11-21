//! The Logs Insights queries of the process: each run's state and answer, kept outside the region actors (an idle actor is reaped;
//! a query outlives it) and in memory only, bounded: the newest [`MAX_QUERIES`] queries, and at most [`MAX_ROWS`] answer rows
//! across them. AWS keeps a query's answer for days; here a restart forgets them (a difference, in the compatibility notes).

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, PoisonError};

use crate::insights::run::Output;

/// The most queries kept.
const MAX_QUERIES: usize = 200;
/// The most answer rows kept across them.
const MAX_ROWS: usize = 1_000_000;
/// The most queries scheduled or running at once in an account (the model: "up to 100 concurrent CloudWatch Logs insights queries").
pub(crate) const MAX_CONCURRENT: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Scheduled,
    Running,
    Complete,
    Failed,
    Cancelled,
}

impl Status {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Scheduled => "Scheduled",
            Self::Running => "Running",
            Self::Complete => "Complete",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }

    pub(crate) const fn live(self) -> bool {
        matches!(self, Self::Scheduled | Self::Running)
    }
}

/// What a run has found so far.
#[derive(Debug)]
pub(crate) struct State {
    pub(crate) status: Status,
    pub(crate) rows: Vec<Vec<(String, String)>>,
    pub(crate) scanned: u64,
    pub(crate) matched: u64,
    pub(crate) bytes: u64,
    pub(crate) duration_ms: u64,
    pub(crate) failure: Option<String>,
}

/// One query.
#[derive(Debug)]
pub(crate) struct QueryRun {
    pub(crate) id: String,
    pub(crate) region: String,
    pub(crate) account: String,
    pub(crate) query: String,
    pub(crate) groups: Vec<String>,
    /// The groups as the request named them (names or ARNs): what `ListLogGroupsForQuery` answers.
    pub(crate) listed: Vec<String>,
    pub(crate) created_ms: i64,
    /// Set by `StopQuery`; the reader looks at it between events.
    pub(crate) cancel: AtomicBool,
    pub(crate) state: Mutex<State>,
    /// For a scheduled query's run, how its delivery to S3 went.
    pub(crate) delivery: Mutex<Option<Delivery>>,
}

/// A scheduled run's delivery: the model's `ActionStatus`, the object written, and why it failed.
#[derive(Clone, Debug)]
pub(crate) struct Delivery {
    pub(crate) status: &'static str,
    pub(crate) key: String,
    pub(crate) error: Option<String>,
}

impl QueryRun {
    pub(crate) fn new(
        id: String,
        (region, account): (&str, &str),
        query: &str,
        (groups, listed): (Vec<String>, Vec<String>),
        created_ms: i64,
    ) -> Self {
        Self {
            id,
            region: region.to_owned(),
            account: account.to_owned(),
            query: query.to_owned(),
            groups,
            listed,
            created_ms,
            cancel: AtomicBool::new(false),
            state: Mutex::new(State {
                status: Status::Scheduled,
                rows: Vec::new(),
                scanned: 0,
                matched: 0,
                bytes: 0,
                duration_ms: 0,
                failure: None,
            }),
            delivery: Mutex::new(None),
        }
    }

    pub(crate) fn delivery(&self) -> Option<Delivery> {
        self.delivery
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn set_delivery(&self, d: Delivery) {
        *self.delivery.lock().unwrap_or_else(PoisonError::into_inner) = Some(d);
    }

    pub(crate) fn with_state<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut s)
    }

    pub(crate) fn status(&self) -> Status {
        self.with_state(|s| s.status)
    }

    /// Moves a live run on; a run already ended (stopped) stays as it is.
    pub(crate) fn set_status(&self, status: Status) {
        self.with_state(|s| {
            if s.status.live() {
                s.status = status;
            }
        });
    }

    /// The answer of a run that read to its end.
    pub(crate) fn finish(&self, out: Output, scanned: u64, bytes: u64, duration_ms: u64) {
        self.with_state(|s| {
            s.scanned = scanned;
            s.bytes = bytes;
            s.matched = out.matched;
            s.duration_ms = duration_ms;
            if s.status == Status::Cancelled {
                return;
            }
            if let Some(why) = out.failed {
                s.status = Status::Failed;
                s.failure = Some(why);
            } else {
                s.status = Status::Complete;
                s.rows = out.rows;
            }
        });
    }
}

/// Every query of the process.
#[derive(Debug)]
pub(crate) struct Registry {
    runs: Mutex<Vec<Arc<QueryRun>>>,
    /// The readers' turns (`insights::job::WORKERS`).
    pub(crate) turns: Arc<tokio::sync::Semaphore>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            runs: Mutex::new(Vec::new()),
            turns: Arc::new(tokio::sync::Semaphore::new(crate::insights::job::WORKERS)),
        }
    }
}

impl Registry {
    fn runs(&self) -> std::sync::MutexGuard<'_, Vec<Arc<QueryRun>>> {
        self.runs.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// How many of the account's queries are scheduled or running.
    pub(crate) fn live(&self, account: &str) -> usize {
        self.runs()
            .iter()
            .filter(|r| r.account == account && r.status().live())
            .count()
    }

    /// Keeps a new run, letting the oldest ended ones go past the bounds.
    pub(crate) fn add(&self, run: Arc<QueryRun>) {
        let mut runs = self.runs();
        runs.push(run);
        let mut rows: usize = runs.iter().map(|r| r.with_state(|s| s.rows.len())).sum();
        while runs.len() > MAX_QUERIES || rows > MAX_ROWS {
            let Some(i) = runs.iter().position(|r| !r.status().live()) else {
                break;
            };
            let gone = runs.remove(i);
            rows = rows.saturating_sub(gone.with_state(|s| s.rows.len()));
        }
    }

    /// The query with this id in this region and account.
    pub(crate) fn get(&self, id: &str, region: &str, account: &str) -> Option<Arc<QueryRun>> {
        self.runs()
            .iter()
            .find(|r| r.id == id && r.region == region && r.account == account)
            .cloned()
    }

    /// The region's queries, newest first.
    pub(crate) fn of(&self, region: &str, account: &str) -> Vec<Arc<QueryRun>> {
        let mut out: Vec<Arc<QueryRun>> = self
            .runs()
            .iter()
            .filter(|r| r.region == region && r.account == account)
            .cloned()
            .collect();
        out.reverse();
        out
    }
}
