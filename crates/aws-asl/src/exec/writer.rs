//! A distributed Map's `ResultWriter`: when the run is over, its results are handed to the service to write (they go to S3, which is
//! I/O), and what the service answers (`{MapRunArn, ResultWriterDetails}`) is the state's result.

use serde_json::{Value, json};

use super::io::{View, parse};
use super::{Cx, Dispatch, Exec, Frame, Status, Step};
use crate::def::{Kind, Lang, State};
use crate::errors::Failure;

/// The resource of the call that writes a distributed Map's results.
pub const RESULT_WRITER: &str = "doze:resultWriter";

/// One item's result, as AWS writes it in the result files (memory: the member names).
fn item(ex: &Exec, k: &Frame) -> Value {
    let status = match k.status {
        Status::Done => "SUCCEEDED",
        Status::Failed => "FAILED",
        _ => "ABORTED",
    };
    let mut v = json!({
        "ExecutionArn": format!("{}/{}", ex.arn, k.index), "Input": k.input, "InputDetails": {"Included": true},
        "Name": k.index.to_string(), "Status": status, "StateMachineArn": ex.machine_arn,
    });
    if let Some(m) = v.as_object_mut() {
        if let Some(o) = &k.output {
            m.insert("Output".into(), json!(o));
            m.insert("OutputDetails".into(), json!({"Included": true}));
        }
        if let Some(f) = &k.failure {
            m.insert("Error".into(), json!(f.error));
            m.insert("Cause".into(), json!(f.cause));
        }
    }
    v
}

impl Exec {
    /// Asks the service to write a distributed Map's results, when the state has a `ResultWriter`; `false` when it has none.
    pub(crate) fn write_results(
        &mut self,
        cx: Cx<'_>,
        id: u32,
        state: &State,
        kids: &[&Frame],
        step: &mut Step,
    ) -> bool {
        let Kind::Map(m) = &state.kind else {
            return false;
        };
        let (true, Some(writer)) = (m.distributed, &m.result_writer) else {
            return false;
        };
        let Some(f) = self.frame(id).cloned() else {
            return false;
        };
        let view = View {
            ex: self,
            frame: &f,
            state,
            env: cx.env,
        };
        let raw = parse(&f.input);
        let params = match writer.get("Parameters") {
            None => Ok(json!({})),
            Some(p) if state.lang == Lang::Jsonata => view
                .jsonata("Parameters", p, &view.states(&raw, &[]))
                .map(|v| v.unwrap_or(Value::Null)),
            Some(p) => view
                .effective(&raw)
                .and_then(|eff| view.template(p, &eff, None)),
        };
        let params = match params {
            Ok(p) => p,
            Err(e) => {
                self.failed(
                    cx,
                    id,
                    state,
                    Failure::new(crate::errors::RESULT_WRITER_FAILED, e.cause),
                    step,
                );
                return true;
            }
        };
        let results: Vec<Value> = kids.iter().map(|k| item(self, k)).collect();
        let input = json!({
            "Resource": writer.get("Resource"), "Parameters": params, "WriterConfig": writer.get("WriterConfig"),
            "MapRunArn": f.map_run, "ExecutionArn": self.arn, "Results": results,
        });
        let Some(fm) = self.frame_mut(id) else {
            return true;
        };
        fm.seq += 1;
        fm.status = Status::Calling;
        fm.writing = true;
        fm.task_input = Some(input.to_string());
        let d = Dispatch {
            frame: id,
            seq: fm.seq,
            state: state.name.clone(),
            resource: RESULT_WRITER.to_owned(),
            input,
            token: None,
            credentials: None,
            timeout_secs: None,
            heartbeat_secs: None,
        };
        Self::push_dispatch(step, d);
        true
    }
}
