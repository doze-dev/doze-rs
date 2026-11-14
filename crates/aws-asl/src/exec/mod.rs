//! Executions. An execution is plain data: the frames of the branches that are running (a flat list; a Parallel branch or a Map item
//! is a frame whose parent is the frame of its state), the next event id, and what the context object says. Nothing else is needed
//! to continue it, so an execution that was waiting (a `Wait`, a task, a token) is written to disk and carried on after a restart.
//!
//! The interpreter changes an execution and says what happened: the history events (with AWS's ids: global, chained per branch),
//! the task calls the service must make, and the end. It never does I/O and never reads a clock: `Env` is handed in.

mod choose;
mod events;
mod fan;
mod inspect;
mod io;
mod items;
mod plan;
mod redrive;
mod run;
mod task;
#[cfg(test)]
mod tests;
mod timers;
mod writer;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::def::{Definition, Kind};
use crate::errors::Failure;

pub use events::Event;
pub use fan::ITEM_READER;
pub use io::context_time;
pub use items::MAP_ITEM;
pub use task::parks;
pub use writer::RESULT_WRITER;

/// Where a frame is between two steps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    /// Its state is to be run.
    #[default]
    Runnable,
    /// A task call is out; its answer comes to [`Exec::deliver`].
    Calling,
    /// A Wait state, until `wake_at`.
    Sleeping,
    /// Waiting to try a task again, until `wake_at`.
    RetryWait,
    /// Waiting for a task token, an activity worker or a job (`.sync`).
    Parked,
    /// A Parallel or Map state waiting for its branches.
    Join,
    /// A Map item held back by `MaxConcurrency`.
    Pending,
    Done,
    Failed,
    /// Stopped with its execution, or because a sibling failed.
    Aborted,
}

impl Status {
    /// Whether the frame has finished.
    #[must_use]
    pub const fn finished(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Aborted)
    }
}

/// One step into a sub-machine: a Parallel state's branch, or a Map state's item processor (`branch` is `None`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hop {
    pub state: String,
    pub branch: Option<u32>,
}

/// One running branch of an execution.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Frame {
    pub id: u32,
    /// `0` for the root.
    pub parent: u32,
    /// The branch or the item index.
    pub index: u32,
    /// Which fan-out of the parent the frame belongs to (a frame may run several Parallel or Map states in turn).
    pub generation: u32,
    pub fanouts: u32,
    pub def: Vec<Hop>,
    /// The current state; empty before the first.
    pub state: String,
    pub status: Status,
    /// The state's raw input, as JSON text.
    pub input: String,
    pub output: Option<String>,
    /// A task's evaluated input, kept so that a retry or a restart calls again with the same one.
    pub task_input: Option<String>,
    pub wake_at: i64,
    pub deadline: i64,
    pub heartbeat_at: i64,
    pub heartbeat_secs: i64,
    pub token: Option<String>,
    pub attempts: Vec<u32>,
    /// The Map item this frame runs, for `$$.Map.Item.Value`.
    pub item: Option<String>,
    pub entered_ms: i64,
    pub entered_event: u64,
    /// The last history event of this frame's chain.
    pub prev_event: u64,
    pub failure: Option<Failure>,
    /// The variables in scope, a JSON object.
    pub vars: String,
    /// Counts this frame's task calls, so that an answer to an earlier call is recognised and ignored.
    pub seq: u32,
    /// A Map state's concurrency limit (0 is none).
    pub limit: u32,
    /// The `...StateStarted` event of a Parallel or Map state, which its branches chain from.
    pub started_event: u64,
    /// A distributed Map's failures tolerated: a count and a percentage.
    pub tolerated: Option<(f64, f64)>,
    /// A distributed Map's Map Run ARN.
    pub map_run: Option<String>,
    /// A distributed Map whose results are being written (its call is the `ResultWriter`'s).
    pub writing: bool,
    /// A distributed Map's item, run as an execution of its own: this frame only stands for it, and its call is that execution.
    pub proxy: bool,
}

/// One execution.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Exec {
    pub arn: String,
    pub name: String,
    pub machine_arn: String,
    pub machine_name: String,
    pub role_arn: String,
    pub start_ms: i64,
    pub input: String,
    pub redrive_count: u32,
    /// The execution's own deadline (`TimeoutSeconds` of the machine), 0 for none.
    pub deadline: i64,
    pub frames: Vec<Frame>,
    pub next_frame: u32,
    pub next_event: u64,
}

/// A task call the service makes for a frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Dispatch {
    pub frame: u32,
    pub seq: u32,
    pub state: String,
    pub resource: String,
    pub input: Value,
    pub token: Option<String>,
    /// `Credentials` of the state, evaluated.
    pub credentials: Option<Value>,
    pub timeout_secs: Option<f64>,
    pub heartbeat_secs: Option<i64>,
}

/// How an execution ended.
#[derive(Clone, Debug, PartialEq)]
pub enum End {
    Succeeded(String),
    Failed(Failure),
    TimedOut(Failure),
}

/// What one call of the interpreter did.
#[derive(Clone, Debug, Default)]
pub struct Step {
    pub events: Vec<Event>,
    pub dispatches: Vec<Dispatch>,
    pub end: Option<End>,
}

/// What a task call answered.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// The task finished with this result.
    Done(Value),
    /// The task failed.
    Failed(Failure),
    /// The call was made and the task goes on (`.waitForTaskToken`, `.sync`): the frame waits, and the history says `TaskSubmitted`
    /// with the call's answer.
    Submitted(Value),
}

/// What the context object says of an execution, given when it starts.
#[derive(Clone, Debug, Default)]
pub struct Start {
    pub arn: String,
    pub name: String,
    pub machine_arn: String,
    pub machine_name: String,
    pub role_arn: String,
    pub input: String,
    /// Members of `ExecutionStarted`'s details beyond `input`, `inputDetails` and `roleArn` (alias and version ARNs).
    pub started_extra: Vec<(String, Value)>,
}

impl Exec {
    /// A new execution, with its `ExecutionStarted` event (id 1; the first state's events chain from 0, as recorded).
    #[must_use]
    pub fn start(def: &Definition, s: Start, now_ms: i64) -> (Self, Event) {
        let mut ex = Self {
            arn: s.arn,
            name: s.name,
            machine_arn: s.machine_arn,
            machine_name: s.machine_name,
            role_arn: s.role_arn,
            start_ms: now_ms,
            input: s.input,
            deadline: def
                .timeout_seconds
                .map_or(0, |t| now_ms + (t * 1000.0) as i64),
            next_frame: 2,
            next_event: 1,
            ..Self::default()
        };
        ex.frames.push(Frame {
            id: 1,
            input: ex.input.clone(),
            vars: "{}".to_owned(),
            ..Frame::default()
        });
        let mut details = serde_json::Map::new();
        details.insert("input".into(), Value::String(ex.input.clone()));
        details.insert(
            "inputDetails".into(),
            serde_json::json!({"truncated": false}),
        );
        details.insert("roleArn".into(), Value::String(ex.role_arn.clone()));
        for (k, v) in s.started_extra {
            details.insert(k, v);
        }
        let first = Event {
            id: 1,
            prev: 0,
            ms: now_ms,
            kind: "ExecutionStarted".to_owned(),
            details: Some((
                "executionStartedEventDetails".to_owned(),
                Value::Object(details),
            )),
        };
        ex.next_event = 2;
        (ex, first)
    }

    /// The frame `id`.
    #[must_use]
    pub fn frame(&self, id: u32) -> Option<&Frame> {
        self.frames.iter().find(|f| f.id == id)
    }

    pub(crate) fn frame_mut(&mut self, id: u32) -> Option<&mut Frame> {
        self.frames.iter_mut().find(|f| f.id == id)
    }

    /// The frame waiting on `token`.
    #[must_use]
    pub fn by_token(&self, token: &str) -> Option<&Frame> {
        self.frames.iter().find(|f| {
            f.token.as_deref() == Some(token)
                && matches!(f.status, Status::Parked | Status::Calling)
        })
    }

    /// Whether every frame has finished.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.frames.iter().all(|f| f.status.finished())
    }
}

/// What every step of the interpreter reads: the definition and the environment.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Cx<'a> {
    pub(crate) def: &'a Definition,
    pub(crate) env: &'a crate::env::Env,
}

/// The (sub-)machine a frame runs in.
pub(crate) fn sub<'a>(def: &'a Definition, hops: &[Hop]) -> Option<&'a Definition> {
    let mut cur = def;
    for h in hops {
        let s = cur.state(&h.state)?;
        cur = match (&s.kind, h.branch) {
            (Kind::Parallel { branches }, Some(b)) => branches.get(usize::try_from(b).ok()?)?,
            (Kind::Map(m), None) => &m.processor,
            _ => return None,
        };
    }
    Some(cur)
}
