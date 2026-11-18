//! The actor's side of a running operation: what a job reports is written to the stack's record and its events, and an operation the
//! process stopped in the middle of is marked failed when the region opens again.

use std::collections::HashSet;
use std::sync::{Mutex, PoisonError};

use crate::core::Core;
use crate::model::{Draft, Event, Progress, Stack};

/// The stacks whose operation runs in this process (an actor that was reaped while one runs must not mark it failed on reopening).
static RUNNING: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Marks a stack's operation as running in this process, or not.
pub(crate) fn set_running(stack_id: &str, running: bool) {
    let mut g = RUNNING.lock().unwrap_or_else(PoisonError::into_inner);
    let set = g.get_or_insert_with(HashSet::new);
    if running {
        set.insert(stack_id.to_owned());
    } else {
        set.remove(stack_id);
    }
}

fn is_running(stack_id: &str) -> bool {
    RUNNING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .is_some_and(|s| s.contains(stack_id))
}

/// A time as AWS writes an event's: `2026-11-12T10:00:00.123Z`.
pub(crate) fn iso_ms(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let base = aws_core::time::format_iso8601(secs);
    let base = base.trim_end_matches(".000Z");
    format!("{base}.{:03}Z", ms.rem_euclid(1000))
}

/// Whether a status is final (nothing is running).
pub(crate) fn terminal(status: &str) -> bool {
    !status.ends_with("_IN_PROGRESS")
}

impl Core {
    /// Adds an event to a stack's trail.
    pub(crate) fn push_event(&mut self, stack_id: &str, d: Draft) {
        let now = self.tick();
        let Some(stack) = self.stacks.get_mut(stack_id) else {
            return;
        };
        stack.events += 1;
        let seq = stack.events;
        let is_stack = d.logical == stack.name
            && d.ty == "AWS::CloudFormation::Stack"
            && d.physical == stack.id;
        let id = if is_stack {
            aws_seal::uuid_v4().unwrap_or_default()
        } else {
            format!("{}-{}-{}", d.logical, d.status, iso_ms(now))
        };
        let event = Event {
            stack_id: stack.id.clone(),
            id,
            stack_name: stack.name.clone(),
            op_id: stack.last_op.as_ref().map(|(o, _)| o.clone()),
            logical: d.logical,
            physical: d.physical,
            ty: d.ty,
            at_ms: now,
            status: d.status,
            reason: d.reason,
            props: d.props,
            token: None,
            detailed: d.detailed,
        };
        if !stack.notification_arns.is_empty() {
            let message = crate::notify::message(&event, &stack.caller_account());
            for arn in &stack.notification_arns {
                self.outbox.push((arn.clone(), message.clone()));
            }
        }
        let key = event_key(stack_id, seq);
        let (logical, status, is_detailed) = (
            event.logical.clone(),
            event.status.clone(),
            event.detailed.is_some(),
        );
        self.events.insert(key, event);
        // A resource's status and time follow its newest event.
        if !is_stack
            && !is_detailed
            && let Some(r) = self
                .stacks
                .get_mut(stack_id)
                .and_then(|s| s.resources.get_mut(&logical))
        {
            r.updated_ms = now;
            if status.ends_with("COMPLETE") {
                r.status = status;
            }
        }
    }

    /// The time, never the same twice in one region (events and resources are told apart by it, as on AWS where they are
    /// seconds apart).
    pub(crate) fn tick(&mut self) -> i64 {
        let now = self.now_ms().max(self.last_ms + 1);
        self.last_ms = now;
        now
    }

    /// Writes what a job reports.
    pub(crate) fn progress(&mut self, p: Progress) {
        let now = self.now_ms();
        let Some(stack) = self.stacks.get_mut(&p.stack_id) else {
            return;
        };
        for (logical, res) in p.resources {
            match res {
                Some(r) => {
                    stack.resources.insert(logical, r);
                }
                None => {
                    stack.resources.remove(&logical);
                }
            }
        }
        if let Some((status, reason)) = &p.status {
            stack.status.clone_from(status);
            stack.reason.clone_from(reason);
            if status == "DELETE_COMPLETE" {
                stack.deleted_ms = Some(now);
            }
        }
        if let Some(f) = p.finish {
            finish(stack, f);
        }
        for d in p.events {
            self.push_event(&p.stack_id, d);
        }
        self.settle_change_set(&p.stack_id);
    }

    /// The change set a finished operation executed takes its final execution status.
    fn settle_change_set(&mut self, stack_id: &str) {
        let Some(stack) = self.stacks.get(stack_id) else {
            return;
        };
        if !terminal(&stack.status) {
            return;
        }
        let Some(cs) = stack.change_set_id.clone() else {
            return;
        };
        let failed = stack.status.contains("ROLLBACK") || stack.status.ends_with("FAILED");
        if let Some(c) = self.change_sets.get(&cs)
            && c.execution == "EXECUTE_IN_PROGRESS"
            && let Some(c) = self.change_sets.get_mut(&cs)
        {
            c.execution = if failed {
                "EXECUTE_FAILED"
            } else {
                "EXECUTE_COMPLETE"
            }
            .into();
        }
    }

    /// Marks the operations a stopped process left in progress as failed (`open` runs this).
    pub(crate) fn interrupted(&mut self) {
        let stuck: Vec<(String, String)> = self
            .stacks
            .iter()
            .filter(|(id, s)| {
                s.status.ends_with("_IN_PROGRESS")
                    && s.status != "REVIEW_IN_PROGRESS"
                    && !is_running(id)
            })
            .map(|(id, s)| (id.to_owned(), s.status.clone()))
            .collect();
        for (id, status) in stuck {
            let next = match status.as_str() {
                "CREATE_IN_PROGRESS" => "CREATE_FAILED",
                "ROLLBACK_IN_PROGRESS" => "ROLLBACK_FAILED",
                "DELETE_IN_PROGRESS" => "DELETE_FAILED",
                "UPDATE_COMPLETE_CLEANUP_IN_PROGRESS" => "UPDATE_COMPLETE",
                s if s.starts_with("UPDATE_ROLLBACK") => "UPDATE_ROLLBACK_FAILED",
                _ => "UPDATE_FAILED",
            };
            let reason = "doze-aws stopped while this operation was in progress";
            let (name, sid) = match self.stacks.get_mut(&id) {
                Some(s) => {
                    s.status = next.to_owned();
                    s.reason = Some(reason.to_owned());
                    (s.name.clone(), s.id.clone())
                }
                None => continue,
            };
            self.push_event(&id, Draft::stack(&name, &sid, next, Some(reason)));
        }
    }
}

/// The key of a stack's `seq`-th event: events of one stack sort together, oldest first.
pub(crate) fn event_key(stack_id: &str, seq: u64) -> String {
    format!("{stack_id}#{seq:010}")
}

fn finish(stack: &mut Stack, f: crate::model::Finish) {
    if let Some(o) = f.outputs {
        stack.outputs = o;
    }
    if let Some((body, processed, params, description)) = f.restore {
        stack.body = body;
        stack.processed = processed;
        stack.params = params;
        stack.description = description;
    }
    if let Some(i) = f.imports {
        stack.imports = i;
    }
    if !matches!(
        stack.status.as_str(),
        "UPDATE_FAILED" | "UPDATE_ROLLBACK_FAILED"
    ) {
        stack.previous = None;
    }
    let _ = f.change_set;
}
