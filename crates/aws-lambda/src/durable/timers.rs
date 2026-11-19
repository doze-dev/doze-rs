//! What comes due for durable executions: a wait's end, a step's next attempt, a callback's timeout or heartbeat timeout, a failed
//! invocation's next try, the execution timeout, and the end of a closed execution's retention. The region's
//! [`doze_runtime::Deadlines`] task asks for a tick at the soonest of them and sleeps; with none it ends.

use serde_json::json;

use super::{Execution, RUNNING, closed, error_text};
use crate::core::Core;

const DAY_MS: i64 = 86_400_000;

impl Core {
    /// Does whatever is due, and asks for the invocations that are owed. The first tick after the region opened also starts what
    /// was in flight when the process stopped: the invocations (from their last checkpoint) and the chained invokes (again).
    pub(crate) fn durable_tick(&mut self) {
        let now = self.now_ms();
        let first = !self.live.ticked;
        self.live.ticked = true;
        let keys: Vec<String> = self.durable.iter().map(|(k, _)| k.to_owned()).collect();
        for key in keys {
            let Some(e) = self.durable.get_mut(&key) else {
                continue;
            };
            if closed(&e.status) {
                let expired = e
                    .end_ms
                    .is_some_and(|end| end + e.retention_days * DAY_MS <= now);
                if expired {
                    self.durable.remove(&key);
                }
                continue;
            }
            if now >= e.start_ms + e.timeout_secs * 1000 {
                // Memory: the words of AWS's timeout error are not recorded.
                let err = error_text(
                    "ExecutionTimedOut",
                    &format!(
                        "The durable execution ran past its ExecutionTimeout of {} seconds.",
                        e.timeout_secs
                    ),
                );
                e.close(now, "TIMED_OUT", None, Some(err));
                self.live.wake.closed.push(e.arn.clone());
                continue;
            }
            due_operations(e, now);
            if e.retry_at.is_some_and(|at| at <= now) {
                e.retry_at = None;
                e.wake = true;
            }
            if first {
                self.live.wake.chained.extend(in_flight_invokes(e));
            }
            if e.wake {
                self.wake_up(&key);
            }
        }
        self.live.changed = true;
    }

    /// The soonest moment anything of the region's durable executions comes due.
    pub(crate) fn durable_due(&self) -> Option<i64> {
        self.durable.iter().filter_map(|(_, e)| next_of(e)).min()
    }
}

/// Completes the waits that ended, readies the steps whose attempt is due, and times out the callbacks whose time ran out.
fn due_operations(e: &mut Execution, now: i64) {
    let (mut ended, mut ready, mut timed_out) = (Vec::new(), Vec::new(), Vec::new());
    for op in &mut e.ops {
        match (op.kind.as_str(), op.status.as_str()) {
            ("WAIT", "STARTED") if op.wait_end_ms.is_some_and(|t| t <= now) => {
                ended.push(op.id.clone())
            }
            ("STEP", "PENDING") if op.next_attempt_ms.is_some_and(|t| t <= now) => {
                op.status = "READY".into();
                ready.push(op.id.clone());
            }
            ("CALLBACK", "STARTED") => {
                // The checkpoint server's words: "Callback timed out", "Callback timed out on heartbeat".
                if op.callback_deadline_ms.is_some_and(|t| t <= now) {
                    timed_out.push((op.id.clone(), "Callback timed out"));
                } else if op.heartbeat_deadline_ms.is_some_and(|t| t <= now) {
                    timed_out.push((op.id.clone(), "Callback timed out on heartbeat"));
                }
            }
            _ => {}
        }
    }
    for id in ready {
        e.mark(&id);
        e.wake = true;
    }
    for id in ended {
        let Some(op) = e.op_mut(&id) else { continue };
        op.status = "SUCCEEDED".into();
        op.end_ms = Some(now);
        let op = op.clone();
        e.event(
            now,
            "WaitSucceeded",
            Some(&op),
            json!({"Duration": op.wait_secs}),
        );
        e.mark(&id);
        e.wake = true;
    }
    for (id, message) in timed_out {
        if let Some(op) = e.op_mut(&id) {
            op.error = Some(json!({"ErrorMessage": message}).to_string());
        }
        e.complete_callback(&id, "TIMED_OUT", now);
    }
}

/// The chained invokes an execution was waiting on.
fn in_flight_invokes(e: &Execution) -> Vec<super::Chained> {
    e.ops
        .iter()
        .filter(|o| o.kind == "CHAINED_INVOKE" && o.status == "STARTED")
        .map(|o| super::Chained {
            execution: e.arn.clone(),
            op: o.id.clone(),
            function: o.target.clone().unwrap_or_default(),
            payload: o.input.clone().unwrap_or_else(|| "{}".to_owned()),
            account: e.account.clone(),
            endpoint: e.endpoint.clone(),
        })
        .collect()
}

fn next_of(e: &Execution) -> Option<i64> {
    if e.status != RUNNING {
        return e.end_ms.map(|end| end + e.retention_days * DAY_MS);
    }
    let ops = e
        .ops
        .iter()
        .filter_map(|op| match (op.kind.as_str(), op.status.as_str()) {
            ("WAIT", "STARTED") => op.wait_end_ms,
            ("STEP", "PENDING") => op.next_attempt_ms,
            ("CALLBACK", "STARTED") => [op.callback_deadline_ms, op.heartbeat_deadline_ms]
                .into_iter()
                .flatten()
                .min(),
            _ => None,
        });
    ops.chain(
        [Some(e.start_ms + e.timeout_secs * 1000), e.retry_at]
            .into_iter()
            .flatten(),
    )
    .min()
}
