//! Starting, stopping, describing and listing executions. `StartExecution` with the name of an execution that is still running and
//! the same input answers that execution (recorded: a finished one is `ExecutionAlreadyExists`, whatever the input).

use aws_asl::{Exec, Start};
use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::arn::{self, Target};
use crate::core::Core;
use crate::drive::Work;
use crate::model::{Execution, Run};
use crate::req::{
    Req, invalid_arn, invalid_name, no_execution, no_machine, put, secs, token, valid_name,
};

/// The largest input (AWS's 256 KiB payload quota).
const MOST_INPUT: usize = 262_144;

/// What an execution starts from, once the machine, version or alias is resolved.
pub(crate) struct Launch {
    pub(crate) machine_arn: String,
    pub(crate) machine_name: String,
    pub(crate) version_arn: Option<String>,
    pub(crate) alias_arn: Option<String>,
    pub(crate) definition: String,
    pub(crate) role_arn: String,
    pub(crate) express: bool,
    pub(crate) logging: Option<String>,
    pub(crate) endpoint: String,
}

/// Who an execution is: its ARN and name, its input, and what started it.
pub(crate) struct Begin {
    pub(crate) arn: String,
    pub(crate) name: String,
    pub(crate) input: String,
    pub(crate) trace: Option<String>,
    /// A `.sync` parent: `(ARN, frame, call, .sync:2)`.
    pub(crate) parent: Option<(String, u32, u32, bool)>,
    pub(crate) test_case: Option<String>,
}

/// The record of a new execution.
fn record(l: Launch, b: Begin, exec: Exec, now: i64) -> Execution {
    Execution {
        arn: b.arn,
        name: b.name,
        machine_arn: l.machine_arn,
        version_arn: l.version_arn,
        alias_arn: l.alias_arn,
        express: l.express,
        status: Run::Running,
        start_ms: now,
        stop_ms: None,
        input: b.input,
        output: None,
        error: None,
        cause: None,
        trace_header: b.trace,
        definition: l.definition,
        role_arn: l.role_arn,
        exec,
        endpoint: l.endpoint,
        map_run: None,
        redrive_ms: None,
        events: 0,
        parents: b.parent.into_iter().collect(),
        logging: l.logging,
        activity: Vec::new(),
        redrive_tokens: Vec::new(),
        test_case: b.test_case,
        mock_calls: Vec::new(),
    }
}

impl Core {
    /// Resolves a state machine, version or alias ARN (an alias picks one of its versions by weight).
    pub(crate) fn launch(&self, given: &str) -> Result<Launch, ApiError> {
        let (p, t) = arn::target(given).ok_or_else(|| invalid_arn("Arn", given))?;
        let key = arn::machine(p.region, p.account, t.machine());
        let m = self.machines.get(&key).ok_or_else(|| no_machine(given))?;
        let mut l = Launch {
            machine_arn: m.arn.clone(),
            machine_name: m.name.clone(),
            version_arn: None,
            alias_arn: None,
            definition: m.definition.clone(),
            role_arn: m.role_arn.clone(),
            express: m.express,
            logging: m.logging.clone(),
            endpoint: m.endpoint.clone(),
        };
        let number = match &t {
            Target::Machine(_) => return Ok(l),
            Target::Version(_, n) => *n,
            Target::Alias(_, a) => {
                let alias = m
                    .aliases
                    .iter()
                    .find(|x| &x.name == a)
                    .ok_or_else(|| no_machine(given))?;
                l.alias_arn = Some(given.to_owned());
                crate::aliases::pick(&alias.routes, self.now_ms())
            }
        };
        let v = m
            .versions
            .iter()
            .find(|v| v.number == number)
            .ok_or_else(|| no_machine(given))?;
        l.version_arn = Some(format!("{}:{number}", m.arn));
        l.definition.clone_from(&v.definition);
        l.role_arn.clone_from(&v.role_arn);
        Ok(l)
    }

    pub(crate) fn start_execution(&mut self, r: &Req<'_>, sync: bool) -> Result<Value, ApiError> {
        // `<machine ARN>#<case>` runs a test case of the mock configuration (Step Functions Local's convention).
        let (given, case) = match r.s("stateMachineArn").split_once('#') {
            Some((a, c)) => (a, Some(c.to_owned())),
            None => (r.s("stateMachineArn"), None),
        };
        let l = self.launch(given)?;
        if sync && !l.express {
            // (memory)
            return Err(ApiError::modeled(
                "StateMachineTypeNotSupported",
                "This operation is not supported by this type of state machine",
            ));
        }
        let input = r.opt("input").unwrap_or("{}").to_owned();
        check_input(&input)?;
        let name = r.opt("name").map_or_else(crate::util::uuid, str::to_owned);
        if !valid_name(&name) {
            return Err(invalid_name(&name));
        }
        let arn = self.execution_arn(&l, &name, r.region(), r.account());
        if let Some(found) = self.executions.get(&arn) {
            if found.status == Run::Running && found.input == input && !sync {
                return Ok(json!({"executionArn": found.arn, "startDate": secs(found.start_ms)}));
            }
            return Err(ApiError::modeled(
                "ExecutionAlreadyExists",
                format!("Execution Already Exists: '{arn}'"),
            ));
        }
        let trace = r.opt("traceHeader").map(str::to_owned);
        let e = self.begin(
            l,
            Begin {
                arn: arn.clone(),
                name,
                input,
                trace,
                parent: None,
                test_case: case,
            },
        );
        let start = e.start_ms;
        self.store(e);
        self.queue.push_back(Work::Run(arn.clone()));
        self.process();
        if !sync {
            return Ok(json!({"executionArn": arn, "startDate": secs(start)}));
        }
        match self.effects.ended.iter().find(|x| x.arn == arn) {
            Some(done) => Ok(sync_shape(&done.answer, r.opt("includedData"))),
            None => {
                self.effects.sync = Some(arn);
                Ok(Value::Null)
            }
        }
    }

    fn execution_arn(&self, l: &Launch, name: &str, region: &str, account: &str) -> String {
        if l.express {
            arn::express(region, account, &l.machine_name, name, &crate::util::uuid())
        } else {
            arn::execution(region, account, &l.machine_name, name)
        }
    }

    /// A new execution with its `ExecutionStarted` event kept.
    pub(crate) fn begin(&mut self, l: Launch, b: Begin) -> Execution {
        let now = self.now_ms();
        let def = self.definition(&l.definition);
        let mut extra = Vec::new();
        if let Some(a) = &l.alias_arn {
            extra.push(("stateMachineAliasArn".to_owned(), json!(a)));
        }
        if let Some(v) = &l.version_arn {
            extra.push(("stateMachineVersionArn".to_owned(), json!(v)));
        }
        let start = Start {
            arn: b.arn.clone(),
            name: b.name.clone(),
            machine_arn: l
                .version_arn
                .clone()
                .unwrap_or_else(|| l.machine_arn.clone()),
            machine_name: l.machine_name.clone(),
            role_arn: l.role_arn.clone(),
            input: b.input.clone(),
            started_extra: extra,
        };
        let started = def.as_ref().map(|d| Exec::start(d, start, now));
        let (exec, first) = started.map_or((Exec::default(), None), |(x, ev)| (x, Some(ev)));
        let mut e = record(l, b, exec, now);
        if let (Some(d), Some(first)) = (def, first) {
            let step = aws_asl::Step {
                events: vec![first],
                ..aws_asl::Step::default()
            };
            self.absorb(&mut e, &d, step);
        }
        self.notice(&e);
        e
    }

    /// An execution that this region keeps, by ARN, with AWS's refusals for an ARN that is not one.
    pub(crate) fn execution(&self, given: &str) -> Result<&Execution, ApiError> {
        let p = arn::parts(given).ok_or_else(|| invalid_arn("Arn", given))?;
        if p.kind == "express" {
            // (memory)
            return Err(invalid_arn(
                "Arn",
                "Resource type not valid in this context: express",
            ));
        }
        if p.kind != "execution" {
            return Err(invalid_arn("Arn", given));
        }
        self.executions
            .get(given)
            .ok_or_else(|| no_execution(given))
    }

    pub(crate) fn describe_execution(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let e = self.execution(r.s("executionArn"))?;
        Ok(describe(e))
    }

    pub(crate) fn stop_execution(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let mut e = self.execution(r.s("executionArn"))?.clone();
        if e.status != Run::Running {
            return Ok(json!({"stopDate": secs(e.stop_ms.unwrap_or(e.start_ms))}));
        }
        let env = self.env_of(&e);
        let (step, _tokens) = e.exec.abort(r.opt("error"), r.opt("cause"), &env);
        if let Some(def) = self.definition(&e.definition) {
            self.absorb(&mut e, &def, step);
        }
        e.status = Run::Aborted;
        e.error = r.opt("error").map(str::to_owned);
        e.cause = r.opt("cause").map(str::to_owned);
        e.stop_ms = Some(self.now_ms());
        e.activity.clear();
        self.ended(&mut e);
        let stop = e.stop_ms.unwrap_or_default();
        self.store(e);
        self.process();
        Ok(json!({"stopDate": secs(stop)}))
    }

    pub(crate) fn list_executions(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let by_run = r.opt("mapRunArn").filter(|s| !s.is_empty());
        let given = r.s("stateMachineArn");
        let filter_machine = if by_run.is_none() {
            let l = self.launch(given)?;
            if l.express {
                // (memory)
                return Err(ApiError::modeled(
                    "StateMachineTypeNotSupported",
                    "This operation is not supported by this type of state machine",
                ));
            }
            Some((l.machine_arn, arn::target(given).map(|(_, t)| t)))
        } else {
            None
        };
        let status = r.opt("statusFilter");
        let redriven = r.opt("redriveFilter");
        let mut all: Vec<&Execution> = self
            .executions
            .iter()
            .map(|(_, e)| e)
            .filter(|e| !e.express)
            .filter(|e| match &filter_machine {
                Some((m, Some(Target::Version(..)))) => {
                    e.machine_arn == *m && e.version_arn.as_deref() == Some(given)
                }
                Some((m, Some(Target::Alias(..)))) => {
                    e.machine_arn == *m && e.alias_arn.as_deref() == Some(given)
                }
                Some((m, _)) => e.machine_arn == *m,
                None => e.map_run.as_deref() == by_run,
            })
            .filter(|e| status.is_none_or(|s| e.status.name() == s))
            .filter(|e| match redriven {
                Some("REDRIVEN") => e.exec.redrive_count > 0,
                Some("NOT_REDRIVEN") => e.exec.redrive_count == 0,
                _ => true,
            })
            .collect();
        all.sort_by(|a, b| b.start_ms.cmp(&a.start_ms).then_with(|| b.arn.cmp(&a.arn)));
        let (from, size) = (r.page_from()?, r.page_size(100));
        let page: Vec<Value> = all.iter().skip(from).take(size).map(|e| item(e)).collect();
        let mut out = json!({"executions": page});
        if from + size < all.len() {
            crate::req::set(&mut out, "nextToken", json!(token(from + size)));
        }
        Ok(out)
    }
}

/// `InvalidExecutionInput` for input that is not JSON, worded as recorded.
pub(crate) fn check_input(input: &str) -> Result<(), ApiError> {
    if input.len() > MOST_INPUT {
        // (memory)
        return Err(ApiError::modeled(
            "InvalidExecutionInput",
            "Input size exceeds the maximum size of 262144 bytes",
        ));
    }
    aws_asl::jackson::read(input).map(|_| ()).map_err(|e| {
        ApiError::modeled(
            "InvalidExecutionInput",
            format!("Invalid State Machine Execution Input: '{}'", e.sentence),
        )
    })
}

/// `DescribeExecution`'s answer.
pub(crate) fn describe(e: &Execution) -> Value {
    let mut m = Map::new();
    m.insert("executionArn".into(), json!(e.arn));
    m.insert("stateMachineArn".into(), json!(e.machine_arn));
    m.insert("name".into(), json!(e.name));
    m.insert("status".into(), json!(e.status.name()));
    m.insert("startDate".into(), secs(e.start_ms));
    put(&mut m, "stopDate", e.stop_ms.map(secs));
    m.insert("input".into(), json!(e.input));
    m.insert("inputDetails".into(), json!({"included": true}));
    if let Some(o) = &e.output {
        m.insert("output".into(), json!(o));
        m.insert("outputDetails".into(), json!({"included": true}));
    }
    put(&mut m, "error", e.error.clone().map(Value::String));
    put(&mut m, "cause", e.cause.clone().map(Value::String));
    put(
        &mut m,
        "traceHeader",
        e.trace_header.clone().map(Value::String),
    );
    put(&mut m, "mapRunArn", e.map_run.clone().map(Value::String));
    put(
        &mut m,
        "stateMachineVersionArn",
        e.version_arn.clone().map(Value::String),
    );
    put(
        &mut m,
        "stateMachineAliasArn",
        e.alias_arn.clone().map(Value::String),
    );
    m.insert("redriveCount".into(), json!(e.exec.redrive_count));
    put(&mut m, "redriveDate", e.redrive_ms.map(secs));
    let (status, reason) = crate::redrive::redrive_status(e);
    m.insert("redriveStatus".into(), json!(status));
    put(&mut m, "redriveStatusReason", reason.map(Value::String));
    Value::Object(m)
}

fn item(e: &Execution) -> Value {
    let mut m = Map::new();
    m.insert("executionArn".into(), json!(e.arn));
    m.insert("stateMachineArn".into(), json!(e.machine_arn));
    m.insert("name".into(), json!(e.name));
    m.insert("status".into(), json!(e.status.name()));
    m.insert("startDate".into(), secs(e.start_ms));
    put(&mut m, "stopDate", e.stop_ms.map(secs));
    put(&mut m, "mapRunArn", e.map_run.clone().map(Value::String));
    put(
        &mut m,
        "stateMachineVersionArn",
        e.version_arn.clone().map(Value::String),
    );
    put(
        &mut m,
        "stateMachineAliasArn",
        e.alias_arn.clone().map(Value::String),
    );
    if e.exec.redrive_count > 0 {
        m.insert("redriveCount".into(), json!(e.exec.redrive_count));
        put(&mut m, "redriveDate", e.redrive_ms.map(secs));
    }
    Value::Object(m)
}

/// `StartSyncExecution`'s answer, without the data when `includedData` is `METADATA_ONLY`.
pub(crate) fn sync_shape(answer: &Value, included: Option<&str>) -> Value {
    let mut a = answer.clone();
    if included == Some("METADATA_ONLY")
        && let Some(m) = a.as_object_mut()
    {
        m.remove("input");
        m.remove("output");
        m.insert("inputDetails".into(), json!({"included": false}));
        if m.contains_key("outputDetails") {
            m.insert("outputDetails".into(), json!({"included": false}));
        }
    }
    a
}

impl Core {
    /// What `StartSyncExecution` answers for an execution that ended.
    pub(crate) fn sync_answer(&self, e: &Execution) -> Value {
        let mut m = Map::new();
        m.insert("executionArn".into(), json!(e.arn));
        m.insert("stateMachineArn".into(), json!(e.machine_arn));
        m.insert("name".into(), json!(e.name));
        m.insert("startDate".into(), secs(e.start_ms));
        m.insert("stopDate".into(), secs(e.stop_ms.unwrap_or(e.start_ms)));
        m.insert("status".into(), json!(e.status.name()));
        put(&mut m, "error", e.error.clone().map(Value::String));
        put(&mut m, "cause", e.cause.clone().map(Value::String));
        m.insert("input".into(), json!(e.input));
        m.insert("inputDetails".into(), json!({"included": true}));
        if let Some(o) = &e.output {
            m.insert("output".into(), json!(o));
            m.insert("outputDetails".into(), json!({"included": true}));
        }
        put(
            &mut m,
            "traceHeader",
            e.trace_header.clone().map(Value::String),
        );
        let ms = e.stop_ms.unwrap_or(e.start_ms) - e.start_ms;
        // (memory) Express bills by 100 ms and 64 MB steps.
        m.insert(
            "billingDetails".into(),
            json!({"billedMemoryUsedInMB": 64, "billedDurationInMilliseconds": ((ms + 99) / 100).max(1) * 100}),
        );
        Value::Object(m)
    }
}
