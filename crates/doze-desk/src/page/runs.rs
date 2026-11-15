//! A task's runs, as its page lists them: when, why, how long, how each ended, and each one's run in the logs.

use super::time::{self, Zone};
use crate::convert;
use doze_control as wire;
use doze_display as words;
use std::time::SystemTime;

/// One run of a task, newest first on the page.
#[derive(Debug, Clone, PartialEq)]
pub struct RunRow {
    /// Counts the runs kept, the oldest 1.
    pub seq: usize,
    /// The run's number in its logs (the daemon's); 0 when its lines are gone.
    pub log: u32,
    pub when: String,
    pub ago: String,
    pub why: String,
    pub took: String,
    pub secs: f64,
    pub ok: bool,
    pub done: bool,
    pub exit: i64,
}

/// A task's runs from the journal, newest first.
pub fn runs(
    events: &[wire::Event],
    v: &words::Instance,
    now: SystemTime,
    zone: Zone,
) -> Vec<RunRow> {
    let events: Vec<words::Event> = events.iter().map(convert::event).collect();
    let runs = words::run_history(&events, &v.name, v.task.as_ref());
    let n = runs.len();
    runs.iter()
        .enumerate()
        .rev()
        .map(|(i, r)| {
            let mut exit = i64::from(r.done && !r.ok);
            if i + 1 == n
                && r.done
                && let Some(t) = &v.task
            {
                exit = t.exit_code;
            }
            RunRow {
                seq: i + 1,
                log: 0,
                when: time::when(r.at, now, zone),
                ago: now.duration_since(r.at).map(words::ago).unwrap_or_default(),
                why: r.why.clone().into_owned(),
                took: if r.done && !r.took.is_zero() {
                    time::tenths(r.took)
                } else {
                    String::new()
                },
                secs: r.took.as_secs_f64(),
                ok: r.ok,
                done: r.done,
                exit,
            }
        })
        .collect()
}

/// Each run kept, matched to its run in the logs from the newest: the two count differently, and older lines may be
/// gone.
pub fn match_runs(rows: &mut [RunRow], log_runs: &[u32]) {
    let n = rows.len();
    for r in rows.iter_mut() {
        let back = n - r.seq;
        r.log = log_runs
            .len()
            .checked_sub(1 + back)
            .and_then(|j| log_runs.get(j))
            .copied()
            .unwrap_or(0);
    }
}
