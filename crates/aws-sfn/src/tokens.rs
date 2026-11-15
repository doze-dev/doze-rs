//! `SendTaskSuccess`, `SendTaskFailure` and `SendTaskHeartbeat`: a token is found in the running execution that made it, and its
//! answer delivered there. A token answered already is `TaskTimedOut`, naming the activity (recorded 2026-11-15); a token this
//! service never made is `InvalidToken`, and one of the right shape it does not know `TaskDoesNotExist` (both from memory).

use aws_asl::{Failure, Outcome};
use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::drive::Work;
use crate::model::Run;
use crate::req::Req;

/// How many answered tokens are remembered (a second answer to an older one is `TaskDoesNotExist`).
const SPENT: usize = 1024;

impl Core {
    /// The execution, frame and call waiting on `token`.
    fn by_token(&self, token: &str) -> Result<(String, u32, u32), ApiError> {
        if !token.starts_with("AQ") {
            return Err(ApiError::modeled("InvalidToken", "Invalid token"));
        }
        self.executions
            .iter()
            .filter(|(_, e)| e.status == Run::Running)
            .find_map(|(k, e)| e.exec.by_token(token).map(|f| (k.to_owned(), f.id, f.seq)))
            .ok_or_else(|| self.not_open(token))
    }

    /// Why `token` cannot be answered: it was answered already, or it is not known.
    fn not_open(&self, token: &str) -> ApiError {
        match self.spent.iter().find(|(t, _)| t == token) {
            // An activity's: recorded 2026-11-15. A callback's: (memory).
            Some((_, activity)) if !activity.is_empty() => {
                ApiError::modeled("TaskTimedOut", format!("Task Timed Out: '{activity}'"))
            }
            Some(_) => ApiError::modeled(
                "TaskTimedOut",
                "Task Timed Out: 'Provided task does not exist anymore'",
            ),
            None => ApiError::modeled("TaskDoesNotExist", "Task Does Not Exist"), // (memory)
        }
    }

    fn answer(&mut self, token: &str, outcome: Outcome) -> Result<Value, ApiError> {
        let (execution, frame, seq) = self.by_token(token)?;
        let mut activity = String::new();
        if let Some(e) = self.executions.get_mut(&execution) {
            if let Some(a) = e.activity.iter().find(|a| a.frame == frame) {
                activity.clone_from(&a.activity);
            }
            e.activity.retain(|a| a.frame != frame);
        }
        if self.spent.len() == SPENT {
            self.spent.pop_front();
        }
        self.spent.push_back((token.to_owned(), activity));
        self.queue.push_back(Work::Deliver {
            execution,
            frame,
            seq,
            outcome,
        });
        self.process();
        Ok(json!({}))
    }

    pub(crate) fn task_success(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let output = r.s("output");
        let value = aws_asl::jackson::read(output).map_err(|e| {
            // (memory)
            ApiError::modeled(
                "InvalidOutput",
                format!("Invalid State Machine Execution Output: '{}'", e.sentence),
            )
        })?;
        self.answer(r.s("taskToken"), Outcome::Done(value))
    }

    pub(crate) fn task_failure(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let failure = Failure::new(r.s("error"), r.s("cause"));
        self.answer(r.s("taskToken"), Outcome::Failed(failure))
    }

    pub(crate) fn task_heartbeat(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let (execution, frame, _) = self.by_token(r.s("taskToken"))?;
        let now = self.now_ms();
        if let Some(e) = self.executions.get_mut(&execution) {
            e.exec.heartbeat(frame, now);
        }
        Ok(json!({}))
    }
}
