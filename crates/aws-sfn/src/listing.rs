//! `ListExecutions`: the running executions from memory and the finished ones by their summaries (`finished.rs`).

use std::borrow::Cow;

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use crate::arn::{self, Target};
use crate::core::Core;
use crate::finished::Summary;
use crate::model::Run;
use crate::req::{Req, put, secs, token};

impl Core {
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
        // The running executions from memory, the finished ones by their summaries.
        let running = self
            .executions
            .iter()
            .filter(|(_, e)| e.status == Run::Running && !e.express)
            .map(|(k, e)| (k, Cow::Owned(Summary::of(e))));
        let finished = self.index.summaries().map(|(k, s)| (k, Cow::Borrowed(s)));
        let mut all: Vec<(&str, Cow<'_, Summary>)> = running
            .chain(finished)
            .filter(|(_, e)| by_run.is_some() || e.map_run.is_none())
            .filter(|(_, e)| match &filter_machine {
                Some((m, Some(Target::Version(..)))) => {
                    e.machine_arn == *m && e.version_arn.as_deref() == Some(given)
                }
                Some((m, Some(Target::Alias(..)))) => {
                    e.machine_arn == *m && e.alias_arn.as_deref() == Some(given)
                }
                Some((m, _)) => e.machine_arn == *m,
                None => e.map_run.as_deref() == by_run,
            })
            .filter(|(_, e)| status.is_none_or(|s| e.status.name() == s))
            .filter(|(_, e)| match redriven {
                Some("REDRIVEN") => e.redrive_count > 0,
                Some("NOT_REDRIVEN") => e.redrive_count == 0,
                _ => true,
            })
            .collect();
        all.sort_by(|(a, x), (b, y)| y.start_ms.cmp(&x.start_ms).then_with(|| b.cmp(a)));
        let (from, size) = (r.page_from()?, r.page_size(100));
        let page: Vec<Value> = all
            .iter()
            .skip(from)
            .take(size)
            .map(|(arn, e)| item(arn, e))
            .collect();
        let mut out = json!({"executions": page});
        if from + size < all.len() {
            crate::req::set(&mut out, "nextToken", json!(token(from + size)));
        }
        Ok(out)
    }
}

fn item(arn: &str, e: &Summary) -> Value {
    let mut m = Map::new();
    m.insert("executionArn".into(), json!(arn));
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
    if e.redrive_count > 0 {
        m.insert("redriveCount".into(), json!(e.redrive_count));
        put(&mut m, "redriveDate", e.redrive_ms.map(secs));
    }
    Value::Object(m)
}
