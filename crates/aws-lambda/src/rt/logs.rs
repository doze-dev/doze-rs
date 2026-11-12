//! A function's log: what AWS writes to `/aws/lambda/<name>`, in the shape AWS writes it. Every invocation is `START`, the lines the
//! function printed, `END` and `REPORT` (or the JSON platform events when the function's log format is JSON), in a stream named for
//! the day and the process (`2026/11/13/[$LATEST]<32 hex>`). They reach CloudWatch Logs over the peer bus when it is running.

use std::collections::HashSet;
use std::sync::{Mutex, PoisonError};

use aws_core::time::format_iso8601;
use aws_peers::Peers;
use aws_wire::service::Caller;
use serde_json::json;

use super::worker::Line;

/// `2026/11/13/[$LATEST]5f0c…`: the day, the version and a fresh id, as Lambda names the stream of a new execution environment.
pub(crate) fn stream_name(version: &str, now_ms: i64) -> String {
    let day = format_iso8601(now_ms.div_euclid(1000)).chars().take(10).collect::<String>().replace('-', "/");
    let id = aws_seal::uuid_v4().unwrap_or_default().replace('-', "");
    format!("{day}/[{version}]{id}")
}

/// What one invocation reports.
pub(crate) struct Report<'a> {
    pub(crate) id: &'a str,
    pub(crate) version: &'a str,
    pub(crate) duration_ms: f64,
    pub(crate) init_ms: Option<f64>,
    pub(crate) memory: i64,
    /// `error` or `timeout` when it did not succeed.
    pub(crate) status: Option<&'a str>,
    pub(crate) json: bool,
}

/// The events of one invocation: its start, the lines it printed, its end and its report.
pub(crate) fn events(start_ms: i64, end_ms: i64, lines: &[Line], r: &Report<'_>) -> Vec<(i64, String)> {
    let billed = r.duration_ms.ceil();
    let mut out = Vec::new();
    if r.json {
        out.push((start_ms, json!({"time": iso(start_ms), "type": "platform.start", "record": {"requestId": r.id, "version": r.version}}).to_string()));
    } else {
        out.push((start_ms, format!("START RequestId: {} Version: {}\n", r.id, r.version)));
    }
    for (at, line) in lines {
        out.push((*at, if r.json { line.clone() } else { format!("{line}\n") }));
    }
    if r.json {
        let mut metrics = json!({"durationMs": r.duration_ms, "billedDurationMs": billed, "memorySizeMB": r.memory});
        if let (Some(init), Some(m)) = (r.init_ms, metrics.as_object_mut()) {
            m.insert("initDurationMs".into(), json!(init));
        }
        let mut record = json!({"requestId": r.id, "metrics": metrics, "status": r.status.unwrap_or("success")});
        record["status"] = json!(r.status.unwrap_or("success"));
        out.push((end_ms, json!({"time": iso(end_ms), "type": "platform.report", "record": record}).to_string()));
        return out;
    }
    out.push((end_ms, format!("END RequestId: {}\n", r.id)));
    let mut report = format!(
        "REPORT RequestId: {}\tDuration: {:.2} ms\tBilled Duration: {billed:.0} ms\tMemory Size: {} MB",
        r.id, r.duration_ms, r.memory
    );
    if let Some(init) = r.init_ms {
        report.push_str(&format!("\tInit Duration: {init:.2} ms"));
    }
    if let Some(status) = r.status.filter(|s| *s == "timeout") {
        report.push_str(&format!("\tStatus: {status}"));
    }
    report.push('\n');
    out.push((end_ms, report));
    out
}

fn iso(ms: i64) -> String {
    format!("{}.{:03}Z", format_iso8601(ms.div_euclid(1000)).trim_end_matches('Z').split('.').next().unwrap_or_default(), ms.rem_euclid(1000))
}

/// Where a function's output goes.
#[derive(Debug, Default)]
pub(crate) struct Logger {
    /// Groups and streams already created, so they are made once.
    made: Mutex<HashSet<String>>,
}

impl Logger {
    /// Writes `events` to the function's log group and stream. A failure is logged and does not fail the invocation.
    pub(crate) async fn write(&self, peers: Option<&Peers>, caller: &Caller, group: &str, stream: &str, events: Vec<(i64, String)>) {
        if events.is_empty() {
            return;
        }
        let Some(peers) = peers else { return };
        let acting = Caller { service: Some("lambda".to_owned()), ..caller.clone() };
        let key = format!("{}/{group}/{stream}", caller.region);
        let known = self.made.lock().unwrap_or_else(PoisonError::into_inner).contains(&key);
        if !known {
            for (op, body) in [
                ("CreateLogGroup", json!({"logGroupName": group})),
                ("CreateLogStream", json!({"logGroupName": group, "logStreamName": stream})),
            ] {
                if let Err(e) = peers.call("lambda", &acting, "logs", op, &body).await
                    && e.code != "ResourceAlreadyExistsException"
                {
                    tracing::warn!("lambda could not write its log: {op}: {}", e.message);
                    return;
                }
            }
            self.made.lock().unwrap_or_else(PoisonError::into_inner).insert(key.clone());
        }
        let rows: Vec<_> = events.iter().map(|(t, m)| json!({"timestamp": t, "message": m})).collect();
        let put = json!({"logGroupName": group, "logStreamName": stream, "logEvents": rows});
        if let Err(e) = peers.call("lambda", &acting, "logs", "PutLogEvents", &put).await {
            // The group or stream was deleted under a warm process: the next write makes them again.
            self.made.lock().unwrap_or_else(PoisonError::into_inner).remove(&key);
            tracing::warn!("lambda could not write its log: {}", e.message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_invocation_is_start_lines_end_and_report() {
        let lines = vec![(5, "hello".to_owned())];
        let r = Report { id: "r1", version: "$LATEST", duration_ms: 1.234, init_ms: Some(80.0), memory: 128, status: None, json: false };
        let e = events(1, 9, &lines, &r);
        let text: Vec<&str> = e.iter().map(|(_, m)| m.as_str()).collect();
        assert_eq!(text[0], "START RequestId: r1 Version: $LATEST\n");
        assert_eq!(text[1], "hello\n");
        assert_eq!(text[2], "END RequestId: r1\n");
        assert_eq!(text[3], "REPORT RequestId: r1\tDuration: 1.23 ms\tBilled Duration: 2 ms\tMemory Size: 128 MB\tInit Duration: 80.00 ms\n");
    }

    #[test]
    fn the_stream_is_named_for_the_day_and_the_version() {
        let s = stream_name("$LATEST", 1_700_000_000_000);
        assert!(s.starts_with("2023/11/14/[$LATEST]"), "{s}");
        assert_eq!(s.len(), "2023/11/14/[$LATEST]".len() + 32);
    }
}
