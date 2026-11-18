//! History events as `GetExecutionHistory` answers them. Ids are global; `previousEventId` follows the branch the event belongs to
//! (recorded: `ExecutionStarted` 1←0, the first state's `...StateEntered` 2←0, its `...StateExited` 3←2, `ExecutionSucceeded` 4←3).
//! Payloads in details are JSON *text*, as the SDKs' types say.

use serde_json::{Map, Value, json};

use super::{Exec, Frame};

/// One history event.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub id: u64,
    pub prev: u64,
    pub ms: i64,
    /// `ExecutionStarted`, `PassStateEntered`, ...
    pub kind: String,
    /// The details member's name and its value (`stateEnteredEventDetails`).
    pub details: Option<(String, Value)>,
}

impl Event {
    /// The event as the API answers it, `timestamp` in epoch seconds.
    #[must_use]
    pub fn wire(&self) -> Value {
        let mut m = Map::new();
        #[allow(clippy::cast_precision_loss)]
        m.insert("timestamp".into(), json!(self.ms as f64 / 1000.0));
        m.insert("type".into(), json!(self.kind));
        m.insert("id".into(), json!(self.id));
        m.insert("previousEventId".into(), json!(self.prev));
        if let Some((k, v)) = &self.details {
            m.insert(k.clone(), v.clone());
        }
        Value::Object(m)
    }
}

/// The detail AWS puts beside a payload it did not cut.
pub(crate) fn whole() -> Value {
    json!({"truncated": false})
}

/// Which family a task's events are in: a bare Lambda ARN is `LambdaFunction...`, an activity `Activity...`, the rest `Task...`
/// with the integration's service and action (`resourceType` `sqs`, `resource` `sendMessage.waitForTaskToken`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Family {
    Lambda,
    Activity,
    Task { kind: String, action: String },
}

pub(crate) fn family(resource: &str) -> Family {
    if resource.starts_with("arn:aws:lambda:") {
        return Family::Lambda;
    }
    if resource.starts_with("arn:aws:states:") && resource.contains(":activity:") {
        return Family::Activity;
    }
    let rest = resource.split_once(":::").map_or(resource, |(_, r)| r);
    let (mut kind, mut action) = rest.split_once(':').unwrap_or((rest, ""));
    // `aws-sdk:s3:getObject` is resource type `aws-sdk:s3` and resource `getObject` (memory).
    let sdk;
    if kind == "aws-sdk"
        && let Some((service, act)) = action.split_once(':')
    {
        sdk = format!("aws-sdk:{service}");
        kind = &sdk;
        action = act;
    }
    Family::Task {
        kind: kind.to_owned(),
        action: action.to_owned(),
    }
}

/// Builds events on an execution: each takes the next id and chains on its frame.
pub(crate) struct Recorder<'a> {
    pub(crate) ex: &'a mut Exec,
    pub(crate) out: &'a mut Vec<Event>,
    pub(crate) now: i64,
    pub(crate) region: String,
}

impl Recorder<'_> {
    /// Records `kind` on frame `frame` (0 records on the root's chain).
    pub(crate) fn add(&mut self, frame: u32, kind: &str, details: Option<(&str, Value)>) -> u64 {
        let id = self.ex.next_event;
        self.ex.next_event += 1;
        let f = self.ex.frame_mut(frame.max(1));
        let prev = f.as_ref().map_or(0, |f| f.prev_event);
        if let Some(f) = f {
            f.prev_event = id;
        }
        self.out.push(Event {
            id,
            prev,
            ms: self.now,
            kind: kind.to_owned(),
            details: details.map(|(k, v)| (k.to_owned(), v)),
        });
        id
    }

    pub(crate) fn entered(&mut self, f: &Frame, ty: &str) -> u64 {
        self.add(
            f.id,
            &format!("{ty}StateEntered"),
            Some((
                "stateEnteredEventDetails",
                json!({"name": f.state, "input": f.input, "inputDetails": whole()}),
            )),
        )
    }

    pub(crate) fn exited(
        &mut self,
        frame: u32,
        state: &str,
        ty: &str,
        output: &str,
        assigned: Option<&Map<String, Value>>,
    ) {
        let mut d = json!({"name": state, "output": output, "outputDetails": whole()});
        if let (Some(vars), Some(m)) = (assigned, d.as_object_mut()) {
            let texts: Map<String, Value> = vars
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.to_string())))
                .collect();
            m.insert("assignedVariables".into(), Value::Object(texts));
            m.insert("assignedVariablesDetails".into(), whole());
        }
        self.add(
            frame,
            &format!("{ty}StateExited"),
            Some(("stateExitedEventDetails", d)),
        );
    }

    /// `TaskScheduled` and `TaskStarted` (or the Lambda or activity pair) for a call.
    pub(crate) fn scheduled(
        &mut self,
        frame: u32,
        resource: &str,
        input: &str,
        timeout: Option<f64>,
        heartbeat: Option<i64>,
    ) {
        let region = self.region.clone();
        match family(resource) {
            Family::Lambda => {
                let mut d = json!({"resource": resource, "input": input, "inputDetails": whole()});
                if let (Some(t), Some(m)) = (timeout, d.as_object_mut()) {
                    m.insert("timeoutInSeconds".into(), json!(t as i64));
                }
                self.add(
                    frame,
                    "LambdaFunctionScheduled",
                    Some(("lambdaFunctionScheduledEventDetails", d)),
                );
                self.add(frame, "LambdaFunctionStarted", None);
            }
            Family::Activity => {
                let mut d = json!({"resource": resource, "input": input, "inputDetails": whole()});
                if let Some(m) = d.as_object_mut() {
                    if let Some(t) = timeout {
                        m.insert("timeoutInSeconds".into(), json!(t as i64));
                    }
                    if let Some(h) = heartbeat {
                        m.insert("heartbeatInSeconds".into(), json!(h));
                    }
                }
                self.add(
                    frame,
                    "ActivityScheduled",
                    Some(("activityScheduledEventDetails", d)),
                );
            }
            Family::Task { kind, action } => {
                let mut d = json!({"resourceType": kind, "resource": action, "region": region, "parameters": input});
                if let Some(m) = d.as_object_mut() {
                    if let Some(t) = timeout {
                        m.insert("timeoutInSeconds".into(), json!(t as i64));
                    }
                    if let Some(h) = heartbeat {
                        m.insert("heartbeatInSeconds".into(), json!(h));
                    }
                }
                self.add(
                    frame,
                    "TaskScheduled",
                    Some(("taskScheduledEventDetails", d)),
                );
                self.add(
                    frame,
                    "TaskStarted",
                    Some((
                        "taskStartedEventDetails",
                        json!({"resourceType": kind, "resource": action}),
                    )),
                );
            }
        }
    }

    pub(crate) fn submitted(&mut self, frame: u32, resource: &str, output: &str) {
        if let Family::Task { kind, action } = family(resource) {
            self.add(
                frame,
                "TaskSubmitted",
                Some((
                    "taskSubmittedEventDetails",
                    json!({"resourceType": kind, "resource": action, "output": output, "outputDetails": whole()}),
                )),
            );
        }
    }

    pub(crate) fn succeeded(&mut self, frame: u32, resource: &str, output: &str) {
        let (kind, key, mut d) = match family(resource) {
            Family::Lambda => (
                "LambdaFunctionSucceeded",
                "lambdaFunctionSucceededEventDetails",
                json!({}),
            ),
            Family::Activity => (
                "ActivitySucceeded",
                "activitySucceededEventDetails",
                json!({}),
            ),
            Family::Task { kind, action } => (
                "TaskSucceeded",
                "taskSucceededEventDetails",
                json!({"resourceType": kind, "resource": action}),
            ),
        };
        if let Some(m) = d.as_object_mut() {
            m.insert("output".into(), json!(output));
            m.insert("outputDetails".into(), whole());
        }
        self.add(frame, kind, Some((key, d)));
    }

    /// The family's `...Failed` or, for a timeout, `...TimedOut`.
    pub(crate) fn failed(&mut self, frame: u32, resource: &str, error: &str, cause: &str) {
        let timed_out =
            error == crate::errors::TIMEOUT || error == crate::errors::HEARTBEAT_TIMEOUT;
        let (prefix, lower, mut d) = match family(resource) {
            Family::Lambda => ("LambdaFunction", "lambdaFunction", json!({})),
            Family::Activity => ("Activity", "activity", json!({})),
            Family::Task { kind, action } => (
                "Task",
                "task",
                json!({"resourceType": kind, "resource": action}),
            ),
        };
        let what = if timed_out { "TimedOut" } else { "Failed" };
        if let Some(m) = d.as_object_mut() {
            m.insert("error".into(), json!(error));
            m.insert("cause".into(), json!(cause));
        }
        let key = format!("{lower}{what}EventDetails");
        self.add(frame, &format!("{prefix}{what}"), Some((&key, d)));
    }
}

#[cfg(test)]
mod family_tests {
    use super::*;

    #[test]
    fn families() {
        assert_eq!(
            family("arn:aws:lambda:us-east-1:1:function:f"),
            Family::Lambda
        );
        assert_eq!(
            family("arn:aws:states:us-east-1:1:activity:a"),
            Family::Activity
        );
        assert_eq!(
            family("arn:aws:states:::sqs:sendMessage.waitForTaskToken"),
            Family::Task {
                kind: "sqs".into(),
                action: "sendMessage.waitForTaskToken".into()
            }
        );
        assert_eq!(
            family("arn:aws:states:::aws-sdk:s3:getObject"),
            Family::Task {
                kind: "aws-sdk:s3".into(),
                action: "getObject".into()
            }
        );
    }
}
