//! The task's inputs in `machine_props` (invariant 11).

use super::*;

/// The starts in flight that are not the task's.
pub(super) fn processes(s: &Shell) -> BTreeMap<u64, String> {
    s.boots
        .iter()
        .filter(|(_, n)| *n != TASK)
        .map(|(b, n)| (*b, n.clone()))
        .collect()
}

/// The task's inputs: a wake, its run deciding to run, its end (0: done, 1: failed, 2: up to date, skipped).
pub(super) fn task_step(m: &mut Machine, s: &mut Shell, now: Now, i: &In) {
    let mine = |s: &Shell, k: usize| -> Option<(u64, String)> {
        let t: BTreeMap<u64, String> = s
            .boots
            .iter()
            .filter(|(_, n)| *n == TASK)
            .map(|(b, n)| (*b, n.clone()))
            .collect();
        pick(&t, k)
    };
    match i {
        In::WakeTask => {
            let (_, fx) = m.wake(TASK, Cause::Command);
            assert!(
                !fx.iter().any(|f| matches!(f, Effect::Journal(_))),
                "a task's wake is journalled only when it runs"
            );
            absorb(s, m, fx);
        }
        In::TaskRun(k) => {
            if let Some((b, _)) = mine(s, *k) {
                let fx = m.task_runs(TASK, b, Cause::Command, "first run");
                absorb(s, m, fx);
            }
        }
        In::TaskEnd(k, how) => {
            if let Some((b, _)) = mine(s, *k) {
                s.boots.remove(&b);
                let fx = match how {
                    0 => m.task_done(TASK, b, Duration::ZERO, now),
                    1 => m.boot_failed(TASK, b, "exit 1", Duration::ZERO, now),
                    _ => m.task_skipped(TASK, b, now),
                };
                absorb(s, m, fx);
            }
        }
        _ => {}
    }
}
