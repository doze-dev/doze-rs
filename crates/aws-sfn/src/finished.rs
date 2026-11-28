//! Where the executions stand, and the finished ones, which live on disk.
//!
//! A running execution is held whole in memory: every step, wake-up and answer works on it. A finished one is read rarely (a describe,
//! its history, a redrive, a list), so once its record is staged it leaves memory ([`doze_store::records::Records::evict`]) and only
//! its [`Summary`] stays: what `ListExecutions` shows and filters on, and what `trim` and the data view count. A describe reads the
//! record from the database, which sees it while its commit is still queued (`doze_store::pending`). 5,000 finished executions with
//! 10 KB inputs held 67 to 69 MB at the peak; weight.md, "Workstream 7a", has what they hold now.
//!
//! The definition an execution runs is shared ([`Def`]): every execution of a machine held a copy of its text.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::model::{Execution, Run};

/// A definition's text, shared by the executions that run it. Written as the text it holds, so the record's format is the one it was
/// as a `String` (postcard writes both alike): no new record version.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Def(pub(crate) Arc<str>);

impl std::ops::Deref for Def {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

impl Serialize for Def {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Def {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self(String::deserialize(d)?.into()))
    }
}

/// The texts in use, so an execution started or read back shares the copy the others hold.
#[derive(Debug, Default)]
pub(crate) struct Defs(HashSet<Arc<str>>);

impl Defs {
    /// The shared copy of `text`.
    pub(crate) fn share(&mut self, text: &str) -> Def {
        if let Some(found) = self.0.get(text) {
            return Def(Arc::clone(found));
        }
        // Texts nothing holds any more go when the set grows (one held only here has a count of one).
        if self.0.len() >= 64 {
            self.0.retain(|t| Arc::strong_count(t) > 1);
        }
        let shared: Arc<str> = text.into();
        self.0.insert(Arc::clone(&shared));
        Def(shared)
    }

    /// `def` as the shared copy (one read back from disk has its own).
    pub(crate) fn adopt(&mut self, def: &mut Def) {
        *def = self.share(&def.0);
    }
}

/// What is kept in memory of a finished execution.
#[derive(Clone, Debug)]
pub(crate) struct Summary {
    pub(crate) name: String,
    pub(crate) machine_arn: String,
    pub(crate) version_arn: Option<String>,
    pub(crate) alias_arn: Option<String>,
    pub(crate) status: Run,
    pub(crate) start_ms: i64,
    pub(crate) stop_ms: Option<i64>,
    pub(crate) map_run: Option<String>,
    pub(crate) redrive_count: u32,
    pub(crate) redrive_ms: Option<i64>,
    /// The length of its row on disk (0 until it is staged).
    pub(crate) bytes: u64,
}

impl Summary {
    pub(crate) fn of(e: &Execution) -> Self {
        Self {
            name: e.name.clone(),
            machine_arn: e.machine_arn.clone(),
            version_arn: e.version_arn.clone(),
            alias_arn: e.alias_arn.clone(),
            status: e.status,
            start_ms: e.start_ms,
            stop_ms: e.stop_ms,
            map_run: e.map_run.clone(),
            redrive_count: e.exec.redrive_count,
            redrive_ms: e.redrive_ms,
            bytes: 0,
        }
    }
}

/// Where the executions stand, kept beside their records so that nothing done per step or per finish reads them all: the running
/// ones (what a wake-up, the next due time and a restart look at), each machine's finished ones by start time (what `trim` drops
/// the oldest of), and the finished ones' summaries.
#[derive(Debug, Default)]
pub(crate) struct ExecIndex {
    running: BTreeSet<String>,
    finished: HashMap<String, BTreeSet<(i64, String)>>,
    summaries: HashMap<String, Summary>,
}

impl ExecIndex {
    /// `e` as it is now stored.
    pub(crate) fn put(&mut self, e: &Execution) {
        self.forget(&e.arn);
        if e.status == Run::Running {
            self.running.insert(e.arn.clone());
        } else {
            self.put_finished(e.arn.clone(), Summary::of(e));
        }
    }

    /// A finished execution, by its summary.
    pub(crate) fn put_finished(&mut self, arn: String, s: Summary) {
        self.finished
            .entry(s.machine_arn.clone())
            .or_default()
            .insert((s.start_ms, arn.clone()));
        self.summaries.insert(arn, s);
    }

    /// The execution `arn` is no longer stored as it was.
    pub(crate) fn forget(&mut self, arn: &str) {
        self.running.remove(arn);
        let Some(s) = self.summaries.remove(arn) else {
            return;
        };
        if let Some(set) = self.finished.get_mut(&s.machine_arn) {
            set.remove(&(s.start_ms, arn.to_owned()));
            if set.is_empty() {
                self.finished.remove(&s.machine_arn);
            }
        }
    }

    /// The finished execution's row is `bytes` long.
    pub(crate) fn sized(&mut self, arn: &str, bytes: usize) {
        if let Some(s) = self.summaries.get_mut(arn) {
            s.bytes = bytes as u64;
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

    /// The summary of a finished execution.
    pub(crate) fn summary(&self, arn: &str) -> Option<&Summary> {
        self.summaries.get(arn)
    }

    /// Every finished execution, by ARN.
    pub(crate) fn summaries(&self) -> impl Iterator<Item = (&str, &Summary)> {
        self.summaries.iter().map(|(k, s)| (k.as_str(), s))
    }

    /// Every execution is forgotten (a reset).
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}
