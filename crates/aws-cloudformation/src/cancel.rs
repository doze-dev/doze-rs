//! `CancelUpdateStack`: an update in progress is told to stop, and its job rolls the stack back. The model: "Cancels an update on
//! the specified stack. If the call completes successfully, the stack rolls back the update and reverts to the previous stack
//! configuration. You can cancel only stacks that are in the UPDATE_IN_PROGRESS state."
//!
//! How: every update job has a flag here, by stack id, while it runs. Cancelling sets it; the job starts nothing new once it is set,
//! and what waits (a wait condition, a custom resource's answer) stops waiting. What is already being called finishes (local calls
//! take milliseconds), and the rollback puts it back. Nothing here runs at idle: the table is empty unless an update is running.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{Value, json};
use tokio::sync::Notify;

use aws_wire::ApiError;

use crate::core::Core;
use crate::errors::{no_stack, validation};
use crate::req::Req;

/// One update's flag.
#[derive(Debug, Default)]
struct Flag {
    set: AtomicBool,
    notify: Notify,
}

static FLAGS: Mutex<Option<HashMap<String, Arc<Flag>>>> = Mutex::new(None);

fn flag(stack_id: &str, make: bool) -> Option<Arc<Flag>> {
    let mut g = FLAGS.lock().unwrap_or_else(PoisonError::into_inner);
    let map = g.get_or_insert_with(HashMap::new);
    if make {
        return Some(Arc::clone(map.entry(stack_id.to_owned()).or_default()));
    }
    map.get(stack_id).cloned()
}

/// An update of `stack_id` starts: it can be cancelled until [`unwatch`]. The job is started before the call that asked for the
/// update is answered, so no cancel can come first.
pub(crate) fn watch(stack_id: &str) {
    drop(flag(stack_id, true));
}

/// The update ended, or its rollback began (a rollback is not cancelled).
pub(crate) fn unwatch(stack_id: &str) {
    let mut g = FLAGS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(map) = g.as_mut() {
        map.remove(stack_id);
    }
}

/// Tells the update of `stack_id` to stop.
pub(crate) fn cancel(stack_id: &str) {
    if let Some(f) = flag(stack_id, false) {
        f.set.store(true, Ordering::SeqCst);
        f.notify.notify_waiters();
    }
}

/// Whether the update of `stack_id` was told to stop.
pub(crate) fn is_cancelled(stack_id: &str) -> bool {
    flag(stack_id, false).is_some_and(|f| f.set.load(Ordering::SeqCst))
}

/// Resolves when the update of `stack_id` is cancelled; never, for a stack with no update running.
pub(crate) async fn cancelled(stack_id: &str) {
    let Some(f) = flag(stack_id, false) else {
        return std::future::pending().await;
    };
    loop {
        let notified = f.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if f.set.load(Ordering::SeqCst) {
            return;
        }
        notified.await;
    }
}

/// `fut`'s output, or `None` when the update of `stack_id` is cancelled first.
pub(crate) async fn unless_cancelled<F: Future>(stack_id: &str, fut: F) -> Option<F::Output> {
    tokio::select! {
        out = fut => Some(out),
        () = cancelled(stack_id) => None,
    }
}

/// The status reason of a resource whose work a cancel cut off (memory: AWS's words for a resource stopped because the operation
/// ended).
pub(crate) fn cut_off(creating: bool) -> String {
    if creating {
        "Resource creation cancelled".into()
    } else {
        "Resource update cancelled".into()
    }
}

impl Core {
    pub(crate) fn cancel_update_stack(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let name = r.s("StackName");
        let Some(stack) = self.find(name).filter(|s| s.status != "DELETE_COMPLETE") else {
            return Err(no_stack(name));
        };
        if stack.status != "UPDATE_IN_PROGRESS" {
            // (memory) AWS's refusal names no status.
            return Err(validation(
                "CancelUpdateStack cannot be called from current stack status",
            ));
        }
        let id = stack.id.clone();
        cancel(&id);
        // Nested stacks updating as part of this update stop with it.
        let children: Vec<String> = self
            .stacks
            .iter()
            .filter(|(_, s)| {
                s.status == "UPDATE_IN_PROGRESS"
                    && (s.parent_id.as_deref() == Some(id.as_str())
                        || s.root_id.as_deref() == Some(id.as_str()))
            })
            .map(|(_, s)| s.id.clone())
            .collect();
        for child in children {
            cancel(&child);
        }
        Ok(json!({}))
    }
}
