//! What Step Functions keeps. Records are plain data the store writes as postcard behind a version byte; blocks the service only
//! echoes (logging, tracing and encryption configurations) are kept as the JSON the caller sent.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

/// A state machine. Keyed by its ARN.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Machine {
    pub(crate) arn: String,
    pub(crate) name: String,
    pub(crate) express: bool,
    pub(crate) definition: String,
    pub(crate) role_arn: String,
    pub(crate) description: Option<String>,
    /// `loggingConfiguration`, `tracingConfiguration` and `encryptionConfiguration` as sent.
    pub(crate) logging: Option<String>,
    pub(crate) tracing: Option<String>,
    pub(crate) encryption: Option<String>,
    pub(crate) created_ms: i64,
    pub(crate) updated_ms: i64,
    pub(crate) revision: String,
    pub(crate) tags: BTreeMap<String, String>,
    pub(crate) versions: Vec<Version>,
    pub(crate) next_version: u64,
    pub(crate) aliases: Vec<Alias>,
    /// The endpoint the machine was made through, for the queue URLs its tasks make.
    pub(crate) endpoint: String,
}

/// A published version: the definition and role frozen.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Version {
    pub(crate) number: u64,
    pub(crate) definition: String,
    pub(crate) role_arn: String,
    pub(crate) description: Option<String>,
    pub(crate) revision: String,
    pub(crate) created_ms: i64,
    pub(crate) logging: Option<String>,
    pub(crate) tracing: Option<String>,
    pub(crate) encryption: Option<String>,
}

/// An alias: one or two versions with weights.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Alias {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    /// `(version number, weight)`.
    pub(crate) routes: Vec<(u64, u32)>,
    pub(crate) created_ms: i64,
    pub(crate) updated_ms: i64,
}

/// An activity. Keyed by its ARN.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Activity {
    pub(crate) arn: String,
    pub(crate) name: String,
    pub(crate) created_ms: i64,
    pub(crate) encryption: Option<String>,
    pub(crate) tags: BTreeMap<String, String>,
}

/// How an execution stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Run {
    Running,
    Succeeded,
    Failed,
    TimedOut,
    Aborted,
    PendingRedrive,
}

impl Run {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Running => "RUNNING",
            Self::Succeeded => "SUCCEEDED",
            Self::Failed => "FAILED",
            Self::TimedOut => "TIMED_OUT",
            Self::Aborted => "ABORTED",
            Self::PendingRedrive => "PENDING_REDRIVE",
        }
    }
}

/// One execution. Keyed by its ARN. A running one carries the interpreter's state; a finished one keeps it too, for a redrive.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Execution {
    pub(crate) arn: String,
    pub(crate) name: String,
    pub(crate) machine_arn: String,
    pub(crate) version_arn: Option<String>,
    pub(crate) alias_arn: Option<String>,
    pub(crate) express: bool,
    pub(crate) status: Run,
    pub(crate) start_ms: i64,
    pub(crate) stop_ms: Option<i64>,
    pub(crate) input: String,
    pub(crate) output: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) cause: Option<String>,
    pub(crate) trace_header: Option<String>,
    /// The definition it runs, frozen when it started.
    pub(crate) definition: String,
    pub(crate) role_arn: String,
    pub(crate) exec: aws_asl::Exec,
    pub(crate) endpoint: String,
    /// The Map Run whose item this execution runs.
    pub(crate) map_run: Option<String>,
    pub(crate) redrive_ms: Option<i64>,
    pub(crate) events: u64,
    /// `.sync` parents waiting for this one: `(parent ARN, frame, call)`.
    pub(crate) parents: Vec<(String, u32, u32, u8)>,
    /// The logging configuration it started with.
    pub(crate) logging: Option<String>,
    /// Activity tasks waiting for a worker, or taken by one.
    pub(crate) activity: Vec<ActivityCall>,
    /// The `RedriveExecution` client tokens already used.
    pub(crate) redrive_tokens: Vec<String>,
    /// The mock test case it runs (`<machine ARN>#<case>`), and how often each mocked state was called.
    pub(crate) test_case: Option<String>,
    pub(crate) mock_calls: Vec<(String, u32)>,
}

/// An activity task of an execution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ActivityCall {
    pub(crate) frame: u32,
    pub(crate) seq: u32,
    pub(crate) activity: String,
    pub(crate) input: String,
    pub(crate) token: String,
    pub(crate) taken: bool,
}

/// A Map Run of a distributed Map state. Keyed by its ARN.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct MapRun {
    pub(crate) arn: String,
    pub(crate) execution: String,
    pub(crate) frame: u32,
    pub(crate) status: String,
    pub(crate) start_ms: i64,
    pub(crate) stop_ms: Option<i64>,
    pub(crate) max_concurrency: u32,
    pub(crate) tolerated_count: f64,
    pub(crate) tolerated_percentage: f64,
    /// `(pending, running, succeeded, failed, aborted, total)` when the run ended; read from the execution while it runs.
    pub(crate) counts: [u64; 6],
    pub(crate) machine_arn: String,
}

/// Where the executions stand, kept beside their records so that nothing done per step or per finish reads them all: the running
/// ones (what a wake-up, the next due time and a restart look at) and each machine's finished ones by start time (what `trim` drops
/// the oldest of). It was a scan of every execution, with a clone and a sort of every ARN, on each finish.
#[derive(Debug, Default)]
pub(crate) struct ExecIndex {
    running: BTreeSet<String>,
    finished: HashMap<String, BTreeSet<(i64, String)>>,
}

impl ExecIndex {
    /// The index of `all`.
    pub(crate) fn of<'a>(all: impl Iterator<Item = &'a Execution>) -> Self {
        let mut index = Self::default();
        for e in all {
            index.put(e);
        }
        index
    }

    /// `e` as it is now stored.
    pub(crate) fn put(&mut self, e: &Execution) {
        if e.status == Run::Running {
            self.running.insert(e.arn.clone());
        } else {
            self.running.remove(&e.arn);
            self.finished
                .entry(e.machine_arn.clone())
                .or_default()
                .insert((e.start_ms, e.arn.clone()));
        }
    }

    /// `e` is no longer stored as it was.
    pub(crate) fn forget(&mut self, e: &Execution) {
        self.running.remove(&e.arn);
        if let Some(set) = self.finished.get_mut(&e.machine_arn) {
            set.remove(&(e.start_ms, e.arn.clone()));
            if set.is_empty() {
                self.finished.remove(&e.machine_arn);
            }
        }
    }

    /// The running executions' ARNs.
    pub(crate) fn running(&self) -> impl Iterator<Item = &str> {
        self.running.iter().map(String::as_str)
    }

    /// The finished executions of `machine`, oldest first.
    pub(crate) fn finished(&self, machine: &str) -> impl Iterator<Item = &(i64, String)> {
        self.finished.get(machine).into_iter().flatten()
    }

    /// How many executions of `machine` have finished.
    pub(crate) fn finished_count(&self, machine: &str) -> usize {
        self.finished.get(machine).map_or(0, BTreeSet::len)
    }
}
