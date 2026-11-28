//! Message move tasks: redriving messages out of a dead-letter queue.
//!
//! The wording is AWS's, from `test_move_task_*` and `test_an_arn_elsewhere` (recorded 2026-11-09). A task has no thread:
//! it advances when the next call arrives, by the time that has passed, which is all a task nobody is watching needs.
//!
//! Modelled from the recordings, not documented: the first message moves at once and then the rate applies; a moved message's
//! receive count is reset (else the source's redrive policy would send it straight back). A finished task reports what it
//! moved: the 2026-11-09 recording showed 0 moved for a completed one-message task, the 2026-11-15 one showed 1, so the 0 was
//! AWS's counter catching up, not a rule, and copying it only misled a developer watching a redrive.

use aws_model_sqs::keys::{
    CancelMessageMoveTaskRequest as Cancel, CancelMessageMoveTaskResult as Cancelled,
    ListMessageMoveTasksRequest as List, ListMessageMoveTasksResult as Listed,
    ListMessageMoveTasksResultEntry as Entry, StartMessageMoveTaskRequest as Start,
    StartMessageMoveTaskResult as Started,
};
use aws_wire::ApiError;
use aws_wire::service::{Call, Reply};
use serde::{Deserialize, Serialize};

use crate::core::{Core, invalid_parameter, missing, reply, request};
use crate::model::Message;

/// How fast a task moves when the caller names no rate.
const DEFAULT_PER_SECOND: i64 = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Status {
    Running,
    Completed,
    Cancelled,
    /// A message had no queue to return to. Last, so a database written before it existed reads as it did.
    Failed,
}

impl Status {
    const fn name(self) -> &'static str {
        match self {
            Self::Running => "RUNNING",
            Self::Completed => "COMPLETED",
            Self::Cancelled => "CANCELLED",
            Self::Failed => "FAILED",
        }
    }
}

/// The only reason a task fails (SQS Developer Guide, "Dead-letter queue redrive"; `FailureReason` in the model): the next
/// message was sent straight to the dead-letter queue, so it names no queue it came from, and no destination was given.
const COULD_NOT_DETERMINE_SOURCE: &str = "CouldNotDetermineMessageSource";

/// One task.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Task {
    handle: String,
    source_arn: String,
    destination_arn: Option<String>,
    per_second: Option<i64>,
    started_ms: i64,
    to_move: u64,
    moved: u64,
    status: Status,
    finished_ms: Option<i64>,
}

fn not_found() -> ApiError {
    ApiError::new(
        404,
        "ResourceNotFoundException",
        "The resource that you specified for the SourceArn parameter doesn't exist.",
    )
}

/// The queue name an SQS ARN of this account names. An ARN of another service is a different refusal; one of another account,
/// or of a queue that is not there, is not found.
fn queue_of<'a>(call: &Call, arn: &'a str) -> Result<&'a str, ApiError> {
    let parts: Vec<&str> = arn.split(':').collect();
    let [_, _, service, _, account, name] = parts.as_slice() else {
        return Err(wrong_format());
    };
    if *service != "sqs" {
        return Err(wrong_format());
    }
    if *account != call.caller.account_id {
        return Err(not_found());
    }
    Ok(name)
}

fn wrong_format() -> ApiError {
    invalid_parameter(
        "You must use this format to specify the SourceArn: arn:<partition>:<service>:<region>:<account-id>:<resource-id>",
    )
}

impl Core {
    /// Lets every running task move the messages the time since it began allows.
    pub(crate) fn advance_tasks(&mut self, now_ms: i64) {
        for i in 0..self.tasks.len() {
            let Some(task) = self.tasks.get(i) else {
                continue;
            };
            if task.status != Status::Running {
                continue;
            }
            let rate = task.per_second.unwrap_or(DEFAULT_PER_SECOND).max(1);
            let elapsed_ms = (now_ms - task.started_ms).max(0);
            let allowed = u64::try_from(elapsed_ms * rate / 1000).unwrap_or(0) + 1;
            let (source, destination, already, to_move) = (
                task.source_arn.clone(),
                task.destination_arn.clone(),
                task.moved,
                task.to_move,
            );
            let want = allowed.min(to_move).saturating_sub(already);
            let (moved, sourceless) =
                self.move_messages(&source, destination.as_deref(), want, now_ms);
            if let Some(task) = self.tasks.get_mut(i) {
                task.moved += moved;
                if sourceless {
                    task.status = Status::Failed;
                    task.finished_ms = Some(now_ms);
                } else if task.moved >= task.to_move || moved < want {
                    task.status = Status::Completed;
                    task.finished_ms = Some(now_ms);
                }
            }
        }
    }

    /// Moves up to `count` messages from the queue `source` names, each to `destination` or else back to the queue it came
    /// from. Returns how many moved, and whether the next message has no queue to return to (it and the ones after it stay
    /// where they are: the task fails).
    fn move_messages(
        &mut self,
        source: &str,
        destination: Option<&str>,
        count: u64,
        now_ms: i64,
    ) -> (u64, bool) {
        let Some(name) = source.rsplit(':').next() else {
            return (0, false);
        };
        let mut moving: Vec<Message> = Vec::new();
        let mut sourceless = false;
        if let Some(q) = self.queue_named(name) {
            let window = usize::try_from(count)
                .unwrap_or(usize::MAX)
                .min(q.messages.len());
            let take = if destination.is_some() {
                window
            } else {
                q.messages
                    .iter()
                    .take(window)
                    .take_while(|m| m.source_arn.is_some())
                    .count()
            };
            sourceless = take < window;
            moving.extend(q.messages.take_front(take));
        }
        let mut moved = 0;
        for mut m in moving {
            let target = destination
                .map(str::to_owned)
                .or_else(|| m.source_arn.clone());
            m.handles.clear();
            m.receive_count = 0;
            m.visible_at_ms = now_ms;
            m.source_arn = None;
            let home = target.as_deref().and_then(|a| a.rsplit(':').next());
            match home.and_then(|n| self.queue_named(n)) {
                Some(q) => {
                    q.messages.push(m);
                    moved += 1;
                }
                None => {
                    if let Some(q) = self.queue_named(name) {
                        q.messages.push(m);
                    }
                }
            }
        }
        (moved, sourceless)
    }

    pub(crate) fn start_message_move_task(&mut self, call: &Call) -> Reply {
        let v = request::<Start>(call)?;
        let source = v
            .get(Start::SOURCE_ARN)
            .ok_or_else(|| missing("SourceArn"))?;
        let name = queue_of(call, source)?;
        let to_move = self
            .queues
            .get(name)
            .map(|q| q.messages.len())
            .ok_or_else(not_found)?;
        if !self
            .queues
            .values()
            .any(|q| q.redrive().is_some_and(|r| r.target_arn == source))
        {
            return Err(invalid_parameter(
                "Source queue must be configured as a Dead Letter Queue.",
            ));
        }
        if self
            .tasks
            .iter()
            .any(|t| t.source_arn == source && t.status == Status::Running)
        {
            return Err(invalid_parameter(
                "There is already a task running. Only one active task is allowed for a source queue arn at a given time.",
            ));
        }
        let per_second = v.get(Start::MAX_NUMBER_OF_MESSAGES_PER_SECOND);
        if per_second.is_some_and(|n| !(1..=500).contains(&n)) {
            // Unrecorded wording.
            return Err(invalid_parameter(
                "Value for parameter MaxNumberOfMessagesPerSecond is invalid. Reason: You must enter a number that's between 1 and 500.",
            ));
        }
        let handle = crate::handles::random_hex::<24>();
        self.tasks.push(Task {
            handle: handle.clone(),
            source_arn: source.to_owned(),
            destination_arn: v.get(Start::DESTINATION_ARN).map(str::to_owned),
            per_second,
            started_ms: self.now_ms(),
            to_move: to_move as u64,
            moved: 0,
            status: Status::Running,
            finished_ms: None,
        });
        // The first message moves as the task begins; the rest follow as time allows.
        let now = self.now_ms();
        self.advance_tasks(now);
        reply::<Started>(|o| {
            o.put(Started::TASK_HANDLE, &handle);
        })
    }

    pub(crate) fn list_message_move_tasks(&mut self, call: &Call) -> Reply {
        let v = request::<List>(call)?;
        let source = v
            .get(List::SOURCE_ARN)
            .ok_or_else(|| missing("SourceArn"))?;
        let name = queue_of(call, source)?;
        if !self.queues.contains_key(name) {
            return Err(not_found());
        }
        let limit = match v.get(List::MAX_RESULTS) {
            None => 1,
            Some(n) if (1..=10).contains(&n) => usize::try_from(n).unwrap_or(1),
            Some(_) => {
                return Err(invalid_parameter(
                    "Value for parameter MaxResults is invalid. Reason: You must enter a number that's between 1 and 10.",
                ));
            }
        };
        let shown: Vec<&Task> = self
            .tasks
            .iter()
            .rev()
            .filter(|t| t.source_arn == source)
            .take(limit)
            .collect();
        reply::<Listed>(|o| {
            if shown.is_empty() {
                return;
            }
            o.put_list(Listed::RESULTS, |l| {
                for t in &shown {
                    l.push(|e| write_entry(e, t));
                }
            });
        })
    }

    pub(crate) fn cancel_message_move_task(&mut self, call: &Call) -> Reply {
        let v = request::<Cancel>(call)?;
        let handle = v
            .get(Cancel::TASK_HANDLE)
            .ok_or_else(|| missing("TaskHandle"))?;
        let now = self.now_ms();
        self.advance_tasks(now);
        let task = self
            .tasks
            .iter_mut()
            .find(|t| t.handle == handle)
            .ok_or_else(|| invalid_parameter("Value for parameter TaskHandle is invalid."))?;
        if task.status == Status::Running {
            task.status = Status::Cancelled;
            task.finished_ms = Some(now);
        }
        let moved = i64::try_from(task.moved).unwrap_or(i64::MAX);
        reply::<Cancelled>(|o| {
            o.put_int(Cancelled::APPROXIMATE_NUMBER_OF_MESSAGES_MOVED, moved);
        })
    }
}

fn write_entry(e: &mut aws_schema::build::Object<'_, Entry>, t: &Task) {
    // The handle is shown while the task runs.
    if t.status == Status::Running {
        e.put(Entry::TASK_HANDLE, &t.handle);
    }
    e.put(Entry::STATUS, t.status.name())
        .put(Entry::SOURCE_ARN, &t.source_arn);
    if let Some(d) = &t.destination_arn {
        e.put(Entry::DESTINATION_ARN, d);
    }
    if let Some(n) = t.per_second {
        e.put_int(Entry::MAX_NUMBER_OF_MESSAGES_PER_SECOND, n);
    }
    e.put_int(
        Entry::APPROXIMATE_NUMBER_OF_MESSAGES_MOVED,
        i64::try_from(t.moved).unwrap_or(i64::MAX),
    );
    e.put_int(
        Entry::APPROXIMATE_NUMBER_OF_MESSAGES_TO_MOVE,
        i64::try_from(t.to_move).unwrap_or(i64::MAX),
    );
    if t.status == Status::Failed {
        e.put(Entry::FAILURE_REASON, COULD_NOT_DETERMINE_SOURCE);
    }
    e.put_int(Entry::STARTED_TIMESTAMP, t.started_ms);
}
