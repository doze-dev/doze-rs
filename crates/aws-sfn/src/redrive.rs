//! `RedriveExecution`: a Standard execution that failed, was aborted or timed out, within 14 days of stopping, carries on from where
//! it stopped. A `clientToken` makes a repeat answer the first redrive. The refusals are worded from memory.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::drive::Work;
use crate::model::{Execution, Run};
use crate::req::{Req, secs};

/// How long after stopping an execution may be redriven.
const WINDOW_MS: i64 = 14 * 86_400_000;

/// `redriveStatus` and its reason, as `DescribeExecution` answers them.
pub(crate) fn redrive_status(e: &Execution) -> (&'static str, Option<String>) {
    match e.status {
        Run::Failed | Run::Aborted | Run::TimedOut if !e.express => ("REDRIVABLE", None),
        other => (
            "NOT_REDRIVABLE",
            Some(format!(
                "Execution is {} and cannot be redriven",
                other.name()
            )),
        ),
    }
}

impl Core {
    pub(crate) fn redrive(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let mut e = self.execution(r.s("executionArn"))?.clone();
        let token = r.opt("clientToken").map(str::to_owned);
        if let Some(t) = &token
            && e.redrive_tokens.contains(t)
        {
            return Ok(json!({"redriveDate": secs(e.redrive_ms.unwrap_or(e.start_ms))}));
        }
        let now = self.now_ms();
        let (status, reason) = redrive_status(&e);
        if status != "REDRIVABLE" || e.stop_ms.is_some_and(|s| now - s > WINDOW_MS) {
            let why = reason.unwrap_or_else(|| {
                "Execution can no longer be redriven: it stopped more than 14 days ago".to_owned()
            });
            return Err(ApiError::modeled("ExecutionNotRedrivable", why));
        }
        let Some(def) = self.definition(&e.definition) else {
            return Err(ApiError::modeled(
                "ExecutionNotRedrivable",
                "The execution's definition cannot be read",
            ));
        };
        let env = self.env_of(&e);
        let step = e.exec.redrive(&def, &env);
        e.status = Run::Running;
        e.stop_ms = None;
        e.error = None;
        e.cause = None;
        e.output = None;
        e.redrive_ms = Some(now);
        e.redrive_tokens.extend(token);
        self.absorb(&mut e, &def, step);
        self.notice(&e);
        let arn = e.arn.clone();
        self.store(e);
        self.queue.push_back(Work::Run(arn));
        self.process();
        Ok(json!({"redriveDate": secs(now)}))
    }
}
