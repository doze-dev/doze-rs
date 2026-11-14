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
        let link = sync.then(|| {
            let kind = if mode == ".sync:2" { SYNC2 } else { SYNC };
            (parent.arn.clone(), d.frame, d.seq, kind)
        });
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

    /// A distributed Map's item, started as an execution of the item processor, listed with the Map Run's ARN. Its name is fixed by the
    /// run, the item and the call, so a call made again after a restart finds the execution it already started.
    pub(crate) fn start_item(&mut self, parent: &Execution, d: &Dispatch) {
        let text = |k: &str| {
            d.input
                .get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let run = text("MapRunArn");
        let index = d.input.get("Index").and_then(Value::as_u64).unwrap_or(0);
        let Some(p) = crate::arn::parts(&parent.arn) else {
            return;
        };
        // (memory) AWS names a Map Run's executions `<machine>/<label>:<id>`.
        let label = run
            .rsplit_once(':')
            .map_or("", |(head, _)| head)
            .rsplit('/')
            .next()
            .unwrap_or_default();
        let machine = parent.machine_arn.rsplit(':').next().unwrap_or_default();
        let id = run.rsplit(':').next().unwrap_or_default();
        let name = format!(
            "{}-{index}-{}",
            id.chars().take(36).collect::<String>(),
            d.seq
        );
        let arn = crate::arn::execution(p.region, p.account, &format!("{machine}/{label}"), &name);
        if let Some(done) = self
            .executions
            .get(&arn)
            .filter(|c| c.status != Run::Running)
        {
            let outcome = outcome_for_parent(done, ITEM, self.now_ms());
            self.queue.push_back(Work::Deliver {
                execution: parent.arn.clone(),
                frame: d.frame,
                seq: d.seq,
                outcome,
            });
            return;
        }
        if self.executions.get(&arn).is_some() {
            return;
        }
        let launch = item_launch(parent, machine, &d.input);
        let input = d
            .input
            .get("Input")
            .and_then(Value::as_str)
            .unwrap_or("{}")
            .to_owned();
        let begin = crate::executions::Begin {
            arn: arn.clone(),
            name,
            input,
            trace: None,
            parent: Some((parent.arn.clone(), d.frame, d.seq, ITEM)),
            test_case: parent.test_case.clone(),
        };
        let mut child = self.begin(launch, begin);
        child.map_run = Some(run);
        self.store(child);
        self.queue.push_back(Work::Run(arn));
    }

    /// Stops the running executions of the Map Runs `parent` owns.
    pub(crate) fn stop_items(&mut self, parent: &str) {
        let runs: Vec<String> = self
            .map_runs
            .iter()
            .filter(|(_, m)| m.execution == parent)
            .map(|(k, _)| k.to_owned())
            .collect();
        let items: Vec<String> = self
            .executions
            .iter()
            .filter(|(_, e)| {
                e.status == Run::Running && e.map_run.as_ref().is_some_and(|r| runs.contains(r))
            })
            .map(|(k, _)| k.to_owned())
            .collect();
        for k in items {
            let Some(mut e) = self.executions.get(&k).cloned() else {
                continue;
            };
            let env = self.env_of(&e);
            let (step, _) = e.exec.abort(None, None, &env);
            if let Some(def) = self.definition(&e.definition) {
                self.absorb(&mut e, &def, step);
            }
            e.status = Run::Aborted;
            e.stop_ms = Some(self.now_ms());
            e.parents.clear();
            self.ended(&mut e);
            self.store(e);
        }
    }
}

/// What a distributed Map's item runs: the item processor, as the parent's role, Standard or Express as the Map says.
fn item_launch(parent: &Execution, machine: &str, input: &Value) -> crate::executions::Launch {
    let text = |k: &str| {
        input
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    crate::executions::Launch {
        machine_arn: parent.machine_arn.clone(),
        machine_name: machine.to_owned(),
        version_arn: None,
        alias_arn: None,
        definition: text("Definition"),
        role_arn: parent.role_arn.clone(),
        express: text("ExecutionType") == "EXPRESS",
        logging: None,
        endpoint: parent.endpoint.clone(),
    }
}

/// How a parent waits for a child: `.sync`, `.sync:2`, or as a distributed Map's item.
pub(crate) const SYNC: u8 = 0;
pub(crate) const SYNC2: u8 = 1;
pub(crate) const ITEM: u8 = 2;

/// What a parent is told when its child ends. A Map item: its output, or its error and cause. A `.sync` task: the child's description
/// (input and output as text for `.sync`, as JSON for `.sync:2`), or `States.TaskFailed` with that description as the cause (memory).
pub(crate) fn outcome_for_parent(child: &Execution, mode: u8, now: i64) -> Outcome {
    if mode == ITEM {
        return match (&child.status, &child.output) {
            (Run::Succeeded, Some(o)) => {
                Outcome::Done(serde_json::from_str(o).unwrap_or(Value::Null))
            }
            _ => Outcome::Failed(Failure::new(
                child
                    .error
                    .clone()
                    .unwrap_or_else(|| child.status.name().to_owned()),
                child.cause.clone().unwrap_or_default(),
            )),
        };
    }
    let doc = |text: &str| -> Value {
        if mode == SYNC2 {
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
