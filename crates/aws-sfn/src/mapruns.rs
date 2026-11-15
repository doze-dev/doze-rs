//! Map Runs: one for each distributed Map state that runs, made when its `MapRunStarted` event is kept. Its items are branches of
//! the execution here (on AWS they are child executions), so the counts are read from the execution while it runs and kept when the
//! run ends. `UpdateMapRun` changes the concurrency and the tolerance of a run in flight.

use aws_wire::ApiError;
use serde_json::{Value, json};

use crate::core::Core;
use crate::model::{Execution, MapRun, Run};
use crate::req::{Req, invalid_arn, not_found, secs, token};

impl Core {
    /// Makes and ends Map Run records from a step's events.
    pub(crate) fn note_map_runs(&mut self, e: &Execution, events: &[aws_asl::Event]) {
        for ev in events.iter().filter(|ev| ev.kind == "MapRunStarted") {
            let Some(arn) = ev
                .details
                .as_ref()
                .and_then(|(_, d)| d.get("mapRunArn"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            let Some(f) = e
                .exec
                .frames
                .iter()
                .find(|f| f.map_run.as_deref() == Some(arn))
            else {
                continue;
            };
            let (count, pct) = f.tolerated.unwrap_or((0.0, 0.0));
            self.map_runs.insert(
                arn.to_owned(),
                MapRun {
                    arn: arn.to_owned(),
                    execution: e.arn.clone(),
                    frame: f.id,
                    status: "RUNNING".to_owned(),
                    start_ms: ev.ms,
                    stop_ms: None,
                    max_concurrency: f.limit,
                    tolerated_count: count,
                    tolerated_percentage: pct,
                    counts: [0; 6],
                    machine_arn: e.machine_arn.clone(),
                },
            );
        }
        let failed = events.iter().any(|ev| ev.kind == "MapRunFailed");
        let ended = failed || events.iter().any(|ev| ev.kind == "MapRunSucceeded");
        if ended {
            self.close_runs(e, if failed { "FAILED" } else { "SUCCEEDED" }, false);
        }
    }

    /// Ends the running Map Runs of `e` whose state is over (or all of them, when the execution stopped).
    pub(crate) fn close_runs(&mut self, e: &Execution, status: &str, all: bool) {
        let now = self.now_ms();
        let open: Vec<String> = self
            .map_runs
            .iter()
            .filter(|(_, m)| m.execution == e.arn && m.status == "RUNNING")
            .map(|(k, _)| k.to_owned())
            .collect();
        for k in open {
            let Some(m) = self.map_runs.get_mut(&k) else {
                continue;
            };
            let still = e.exec.frame(m.frame).is_some_and(|f| {
                f.status == aws_asl::Status::Join && f.map_run.as_deref() == Some(k.as_str())
            });
            if still && !all {
                continue;
            }
            m.counts = e.exec.map_counts(m.frame);
            status.clone_into(&mut m.status);
            m.stop_ms = Some(now);
        }
    }

    fn map_run(&self, given: &str) -> Result<&MapRun, ApiError> {
        if crate::arn::parts(given).is_none_or(|p| p.kind != "mapRun") {
            return Err(invalid_arn("Arn", given));
        }
        self.map_runs
            .get(given)
            .ok_or_else(|| not_found(format!("Resource not found: '{given}'")))
    }

    pub(crate) fn describe_map_run(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let m = self.map_run(r.s("mapRunArn"))?;
        let live = self
            .executions
            .get(&m.execution)
            .filter(|e| e.status == Run::Running && m.status == "RUNNING");
        let c = live.map_or(m.counts, |e| e.exec.map_counts(m.frame));
        let counts = json!({
            "pending": c[0], "running": c[1], "succeeded": c[2], "failed": c[3], "timedOut": 0, "aborted": c[4],
            "total": c[5], "resultsWritten": c[2] + c[3], "failuresNotRedrivable": 0, "pendingRedrive": 0,
        });
        let mut out = json!({
            "mapRunArn": m.arn, "executionArn": m.execution, "status": m.status, "startDate": secs(m.start_ms),
            "maxConcurrency": m.max_concurrency, "toleratedFailurePercentage": m.tolerated_percentage,
            "toleratedFailureCount": m.tolerated_count as i64, "itemCounts": counts, "executionCounts": counts,
            "redriveCount": 0,
        });
        if let (Some(s), Some(o)) = (m.stop_ms, out.as_object_mut()) {
            o.insert("stopDate".into(), secs(s));
        }
        Ok(out)
    }

    pub(crate) fn list_map_runs(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let e = self.execution(r.s("executionArn"))?.arn.clone();
        let mut all: Vec<&MapRun> = self
            .map_runs
            .iter()
            .map(|(_, m)| m)
            .filter(|m| m.execution == e)
            .collect();
        all.sort_by_key(|m| std::cmp::Reverse(m.start_ms));
        let (from, size) = (r.page_from()?, r.page_size(100));
        let page: Vec<Value> = all
            .iter()
            .skip(from)
            .take(size)
            .map(|m| {
                let mut v = json!({"executionArn": m.execution, "mapRunArn": m.arn, "stateMachineArn": m.machine_arn, "startDate": secs(m.start_ms)});
                if let (Some(s), Some(o)) = (m.stop_ms, v.as_object_mut()) {
                    o.insert("stopDate".into(), secs(s));
                }
                v
            })
            .collect();
        let mut out = serde_json::Map::new();
        out.insert("mapRuns".into(), Value::Array(page));
        if from + size < all.len() {
            out.insert("nextToken".into(), json!(token(from + size)));
        }
        Ok(Value::Object(out))
    }

    pub(crate) fn update_map_run(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        let m = self.map_run(r.s("mapRunArn"))?.clone();
        let limit = r
            .int("maxConcurrency")
            .map(|n| u32::try_from(n).unwrap_or(0));
        let count = r.int("toleratedFailureCount").map(|n| n as f64);
        let pct =
            r.v.get("toleratedFailurePercentage")
                .and_then(Value::as_f64);
        if let Some(rec) = self.map_runs.get_mut(&m.arn) {
            if let Some(l) = limit {
                rec.max_concurrency = l;
            }
            if let Some(c) = count {
                rec.tolerated_count = c;
            }
            if let Some(p) = pct {
                rec.tolerated_percentage = p;
            }
        }
        let Some(mut e) = self
            .executions
            .get(&m.execution)
            .cloned()
            .filter(|e| e.status == Run::Running)
        else {
            return Ok(json!({}));
        };
        let Some(def) = self.definition(&e.definition) else {
            return Ok(json!({}));
        };
        let env = self.env_of(&e);
        let step = e.exec.update_map(&def, m.frame, limit, (count, pct), &env);
        self.absorb(&mut e, &def, step);
        self.store(e);
        self.process();
        Ok(json!({}))
    }
}
