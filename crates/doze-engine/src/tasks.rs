//! Tasks: a command that runs to its end and records how it went (Go's `runtime/tasks.go`). What decides whether a task
//! runs is here, without I/O; its `inputs` are [`inputs`]; its record is [`crate::applied::TaskRecord`].

pub mod inputs;

use crate::applied::TaskRecord;

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
