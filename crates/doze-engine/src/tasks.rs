//! Tasks: a command that runs to its end and records how it went (Go's `runtime/tasks.go`). What decides whether a task
//! runs is here, without I/O; its `inputs` are [`inputs`]; its record is [`crate::applied::TaskRecord`].

pub mod inputs;
pub(crate) mod run;

use crate::applied::TaskRecord;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

/// A task as the engine runs it (Go's `runspec.TaskSpec`).
#[derive(Debug, Clone, Default)]
pub struct TaskSpec {
    /// Its run line, as argument words (no shell, PLAN.md D14).
    pub run: Vec<String>,
    /// Where it runs, and what its `inputs` are relative to.
    pub dir: PathBuf,
    /// Added to doze's own environment.
    pub env: BTreeMap<String, String>,
    /// Globs of the files whose change makes it run again; none: it runs once, and again only after a failure.
    pub inputs: Vec<String>,
    /// Runs only when asked for (`doze task`), never because something needs it.
    pub manual: bool,
    /// How long one run may take; `None`: as long as it takes.
    pub timeout: Option<Duration>,
}

impl TaskSpec {
    /// A task running `argv` in `dir`.
    pub fn new<I: IntoIterator<Item = S>, S: Into<String>>(
        argv: I,
        dir: impl Into<PathBuf>,
    ) -> TaskSpec {
        TaskSpec {
            run: argv.into_iter().map(Into::into).collect(),
            dir: dir.into(),
            ..TaskSpec::default()
        }
    }

    /// Sets its `inputs`.
    pub fn inputs(mut self, globs: &[&str]) -> TaskSpec {
        self.inputs = globs.iter().map(|g| (*g).to_string()).collect();
        self
    }
}

/// What to watch for a service that asked to be (Go's `watchSets`): its own globs, and the inputs of every task it
/// waits for, through its needs. A restart runs those tasks first, and a task reruns when its inputs changed, so editing
/// a migration restarts the app after the migration ran. One that did not ask to be watched is not.
pub fn watch_sets(
    stack: &crate::stack::Stack,
    svc: &crate::stack::Service,
) -> Vec<crate::watch::Set> {
    if svc.watch.is_empty() {
        return Vec::new();
    }
    let mut sets = svc.watch.clone();
    let mut seen = std::collections::BTreeSet::new();
    let mut todo: Vec<&str> = svc.needs.iter().rev().map(|n| n.name.as_str()).collect();
    while let Some(name) = todo.pop() {
        if !seen.insert(name) {
            continue;
        }
        let Some(dep) = stack.service(name) else {
            continue;
        };
        if let crate::stack::Kind::Task(t) = &dep.kind
            && !t.inputs.is_empty()
        {
            sets.push(crate::watch::Set {
                dir: t.dir.clone(),
                globs: t.inputs.clone(),
            });
        }
        todo.extend(dep.needs.iter().rev().map(|n| n.name.as_str()));
    }
    sets
}

/// Why a task is asked to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ask {
    /// `doze task <name>`: it runs whatever its state.
    Forced,
    /// Something needs it (a dependant starting, `up`): it runs only if it has to.
    Needed,
}

/// Whether a task has to run, and the words for why (Go's `whyRun`). `needed_by` names the dependant that asked; a
/// `manual` task runs only when forced; `hash` is its inputs' hash now (empty when it lists none).
pub fn why_run(
    ask: Ask,
    manual: bool,
    has_inputs: bool,
    hash: &str,
    last: Option<&TaskRecord>,
    needed_by: Option<&str>,
) -> Option<String> {
    let by = needed_by
        .filter(|b| !b.is_empty())
        .map(|b| format!(" (needed by {b})"))
        .unwrap_or_default();
    if ask == Ask::Forced {
        return Some("asked for".into());
    }
    if manual {
        return None;
    }
    match last {
        None => Some(format!("first run{by}")),
        Some(r) if r.result != "done" => Some(format!("failed last time{by}")),
        Some(r) if has_inputs && r.inputs != hash => Some(format!("inputs changed{by}")),
        Some(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Go's `watchSets`: a watched service also watches the inputs of every task it waits for, through its needs (each
    /// once, depth first); one that did not ask to be watched watches nothing.
    #[test]
    fn a_watched_service_also_watches_the_inputs_of_its_tasks() {
        use crate::process::ProcessSpec;
        use crate::stack::Service;
        let proc = |n: &str| Service::process(n, ProcessSpec::new(["x"], "."));
        let t = |n: &str, g: &str| Service::task(n, TaskSpec::new(["x"], n).inputs(&[g]));
        let stack = crate::stack::Stack {
            name: "t".into(),
            config_name: "doze.hcl".into(),
            data_dir: "/nonexistent".into(),
            state_file: "/nonexistent/s".into(),
            idle_timeout: None,
            extra_env: BTreeMap::new(),
            log_limits: doze_logstore::Options::default(),
            services: vec![
                proc("app")
                    .watch("src", &["*.rs"])
                    .needs("db")
                    .needs("seed"),
                proc("db").needs("migrate"),
                t("migrate", "*.sql"),
                t("seed", "*.csv").needs("migrate"),
                proc("quiet").needs("migrate"),
            ],
        };
        let sets = |n: &str| -> Vec<(String, Vec<String>)> {
            let svc = stack.service(n).cloned().unwrap_or_else(|| proc("none"));
            watch_sets(&stack, &svc)
                .into_iter()
                .map(|s| (s.dir.to_string_lossy().into_owned(), s.globs))
                .collect()
        };
        let set = |d: &str, g: &str| (d.to_string(), vec![g.to_string()]);
        assert_eq!(
            sets("app"),
            [
                set("src", "*.rs"),
                set("migrate", "*.sql"),
                set("seed", "*.csv")
            ]
        );
        assert!(sets("quiet").is_empty());
    }

    fn rec(result: &str, inputs: &str) -> TaskRecord {
        TaskRecord {
            result: result.into(),
            inputs: inputs.into(),
            ..TaskRecord::default()
        }
    }

    /// Go's `whyRun`, each branch: forced, manual, first run, failed, inputs changed, up to date; "needed by" only
    /// when a dependant asked.
    #[test]
    fn a_task_runs_when_it_has_to_and_says_why() {
        let w = |ask, manual, inputs, hash, last: Option<&TaskRecord>, by| {
            why_run(ask, manual, inputs, hash, last, by)
        };
        let done = rec("done", "h1");
        assert_eq!(
            w(Ask::Forced, true, false, "", Some(&done), None).as_deref(),
            Some("asked for")
        );
        assert_eq!(w(Ask::Needed, true, false, "", None, None), None);
        assert_eq!(
            w(Ask::Needed, false, false, "", None, Some("api")).as_deref(),
            Some("first run (needed by api)")
        );
        assert_eq!(
            w(
                Ask::Needed,
                false,
                false,
                "",
                Some(&rec("failed", "")),
                None
            )
            .as_deref(),
            Some("failed last time")
        );
        assert_eq!(
            w(Ask::Needed, false, true, "h2", Some(&done), Some("")).as_deref(),
            Some("inputs changed")
        );
        assert_eq!(w(Ask::Needed, false, true, "h1", Some(&done), None), None);
        assert_eq!(
            w(Ask::Needed, false, false, "h2", Some(&done), None),
            None,
            "no inputs: the hash is not looked at"
        );
    }
}
