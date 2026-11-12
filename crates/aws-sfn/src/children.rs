//! `arn:aws:states:::states:startExecution`, plain, `.sync`, `.sync:2` and `.waitForTaskToken`: the child runs in this region's
//! actor, so starting it needs no I/O, and a `.sync` parent is told when the child ends (the link is kept on the child, on disk, so
//! it survives a restart). A child of a stopped parent keeps running, as on AWS.

use aws_asl::{Dispatch, Failure, Outcome};
use serde_json::{Map, Value, json};

use crate::core::Core;
use crate::drive::Work;
use crate::model::{Execution, Run};

/// `StepFunctions.<Code>Exception`, the name a failed call of the integration takes (memory).
fn error_name(code: &str) -> String {
    if code.ends_with("Exception") {
        format!("StepFunctions.{code}")
    } else {
        format!("StepFunctions.{code}Exception")
    }
}

impl Core {
    pub(crate) fn start_child(&mut self, parent: &Execution, d: &Dispatch) {
        let mode = d
            .resource
            .trim_start_matches("arn:aws:states:::states:startExecution");
        let outcome = self.start_child_now(parent, d, mode);
        self.queue.push_back(Work::Deliver {
            execution: parent.arn.clone(),
            frame: d.frame,
            seq: d.seq,
            outcome,
        });
    }

    fn start_child_now(&mut self, parent: &Execution, d: &Dispatch, mode: &str) -> Outcome {
        let target = d
            .input
            .get("StateMachineArn")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let launch = match self.launch(target) {
            Ok(l) => l,
            Err(e) => return Outcome::Failed(Failure::new(error_name(&e.code), e.message)),
        };
        let input = match d.input.get("Input") {
            None | Some(Value::Null) => "{}".to_owned(),
            Some(Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
        };
        if let Err(e) = crate::executions::check_input(&input) {
            return Outcome::Failed(Failure::new(error_name(&e.code), e.message));
        }
        let name = d
            .input
            .get("Name")
            .and_then(Value::as_str)
            .map_or_else(crate::util::uuid, str::to_owned);
        let Some(p) = crate::arn::parts(&parent.arn) else {
            return Outcome::Failed(Failure::runtime("The execution's ARN cannot be read"));
        };
        let arn = if launch.express {
            crate::arn::express(
                p.region,
                p.account,
                &launch.machine_name,
                &name,
                &crate::util::uuid(),
            )
        } else {
            crate::arn::execution(p.region, p.account, &launch.machine_name, &name)
        };
        if self.executions.get(&arn).is_some() {
            return Outcome::Failed(Failure::new(
                error_name("ExecutionAlreadyExists"),
                format!("Execution Already Exists: '{arn}'"),
            ));
        }
        let sync = mode.starts_with(".sync");
        let link = sync.then(|| (parent.arn.clone(), d.frame, d.seq, mode == ".sync:2"));
        let begin = crate::executions::Begin {
            arn: arn.clone(),
            name,
            input,
            trace: None,
            parent: link,
            test_case: parent.test_case.clone(),
        };
        let child = self.begin(launch, begin);
        let started = json!({"ExecutionArn": arn, "StartDate": child.start_ms});
        self.store(child);
        self.queue.push_back(Work::Run(arn));
        if sync || mode == ".waitForTaskToken" {
            Outcome::Submitted(started)
        } else {
            Outcome::Done(started)
        }
    }
}

/// What a `.sync` parent is told when its child ends: the child's description (input and output as text for `.sync`, as JSON for
/// `.sync:2`), or `States.TaskFailed` with that description as the cause (memory).
pub(crate) fn outcome_for_parent(child: &Execution, sync2: bool, now: i64) -> Outcome {
    let doc = |text: &str| -> Value {
        if sync2 {
            serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_owned()))
        } else {
            Value::String(text.to_owned())
        }
    };
    let mut m = Map::new();
    m.insert("ExecutionArn".into(), json!(child.arn));
    m.insert("Input".into(), doc(&child.input));
    m.insert("InputDetails".into(), json!({"Included": true}));
    m.insert("Name".into(), json!(child.name));
    if let Some(o) = &child.output {
        m.insert("Output".into(), doc(o));
        m.insert("OutputDetails".into(), json!({"Included": true}));
    }
    m.insert("StartDate".into(), json!(child.start_ms));
    m.insert("StateMachineArn".into(), json!(child.machine_arn));
    m.insert("Status".into(), json!(child.status.name()));
    m.insert("StopDate".into(), json!(child.stop_ms.unwrap_or(now)));
    if let Some(e) = &child.error {
        m.insert("Error".into(), json!(e));
    }
    if let Some(c) = &child.cause {
        m.insert("Cause".into(), json!(c));
    }
    let described = Value::Object(m);
    if child.status == Run::Succeeded {
        Outcome::Done(described)
    } else {
        Outcome::Failed(Failure::new(
            aws_asl::errors::TASK_FAILED,
            described.to_string(),
        ))
    }
}
