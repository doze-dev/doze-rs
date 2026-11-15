//! What a call of the actor asks of the world outside it: task calls to make on other services (they take time, and a service
//! may call back), executions that ended (a `StartSyncExecution` waits for one), activities that have work for a poller, log
//! records to write, and status-change events for EventBridge. The actor never awaits any of these.

use serde_json::Value;

/// A task call to make.
#[derive(Clone, Debug)]
pub(crate) struct Job {
    pub(crate) execution: String,
    pub(crate) frame: u32,
    pub(crate) seq: u32,
    pub(crate) resource: String,
    pub(crate) input: Value,
    pub(crate) token: Option<String>,
    pub(crate) credentials: Option<Value>,
    pub(crate) role_arn: String,
    pub(crate) region: String,
    pub(crate) account: String,
    pub(crate) endpoint: String,
}

/// An execution that ended, with what `StartSyncExecution` answers for it.
#[derive(Clone, Debug)]
pub(crate) struct Ended {
    pub(crate) arn: String,
    pub(crate) answer: Value,
}

/// Log records for one log group.
#[derive(Clone, Debug)]
pub(crate) struct LogWrite {
    pub(crate) group_arn: String,
    pub(crate) stream: String,
    pub(crate) records: Vec<(i64, String)>,
    pub(crate) region: String,
    pub(crate) account: String,
}

/// Everything one call asks for.
#[derive(Debug, Default)]
pub(crate) struct Effects {
    pub(crate) jobs: Vec<Job>,
    pub(crate) ended: Vec<Ended>,
    pub(crate) activities: Vec<String>,
    pub(crate) logs: Vec<LogWrite>,
    /// `Step Functions Execution Status Change` events: `(detail, resources, region, account)`.
    pub(crate) notices: Vec<(Value, String, String, String)>,
    /// A `StartSyncExecution` whose execution has not ended yet.
    pub(crate) sync: Option<String>,
    /// `GetActivityTask` found a task.
    pub(crate) taken: bool,
}
