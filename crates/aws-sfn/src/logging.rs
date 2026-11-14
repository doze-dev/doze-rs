//! A machine's `loggingConfiguration`: every history event the level lets through is written to the log group it names, as the JSON
//! record AWS vends (`id`, `type`, `details`, `previous_event_id`, `event_timestamp`, `execution_arn`, `redrive_count`; the field
//! names and their string values are from memory). `ALL` writes everything, `ERROR` the events that report a failure, `FATAL` only
//! the execution's own failure; `includeExecutionData: false` leaves the payloads out. One stream per execution. The writing is done
//! outside the actor, through CloudWatch Logs on the peer bus.

use serde_json::{Map, Value, json};

use crate::core::Core;
use crate::effects::LogWrite;
use crate::model::Execution;

fn wanted(level: &str, kind: &str) -> bool {
    match level {
        "ALL" => true,
        "ERROR" => {
            kind.ends_with("Failed") || kind.ends_with("TimedOut") || kind.ends_with("Aborted")
        }
        "FATAL" => matches!(
            kind,
            "ExecutionFailed" | "ExecutionTimedOut" | "ExecutionAborted"
        ),
        _ => false,
    }
}

impl Core {
    pub(crate) fn log_events(&mut self, e: &Execution, events: &[aws_asl::Event]) {
        let Some(config) = e
            .logging
            .as_deref()
            .and_then(|l| serde_json::from_str::<Value>(l).ok())
        else {
            return;
        };
        let level = config.get("level").and_then(Value::as_str).unwrap_or("OFF");
        let data = config
            .get("includeExecutionData")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let Some(group) = config
            .pointer("/destinations/0/cloudWatchLogsLogGroup/logGroupArn")
            .and_then(Value::as_str)
        else {
            return;
        };
        let records: Vec<(i64, String)> = events
            .iter()
            .filter(|ev| wanted(level, &ev.kind))
            .map(|ev| (ev.ms, record(e, ev, data)))
            .collect();
        if records.is_empty() {
            return;
        }
        let (region, account) = crate::arn::parts(&e.arn)
            .map(|p| (p.region.to_owned(), p.account.to_owned()))
            .unwrap_or_default();
        let machine = e.machine_arn.rsplit(':').next().unwrap_or_default();
        let id = e.arn.rsplit(':').next().unwrap_or_default();
        let day = aws_asl::time::format(e.start_ms);
        let stream = format!(
            "states/{machine}/{}/{id}",
            day.get(..10).unwrap_or_default()
        );
        self.effects.logs.push(LogWrite {
            group_arn: group.to_owned(),
            stream,
            records,
            region,
            account,
        });
    }
}

fn record(e: &Execution, ev: &aws_asl::Event, data: bool) -> String {
    let mut details = ev
        .details
        .as_ref()
        .map_or_else(|| json!({}), |(_, d)| d.clone());
    if !data && let Some(d) = details.as_object_mut() {
        for k in ["input", "output", "parameters"] {
            d.remove(k);
        }
    }
    let mut m = Map::new();
    m.insert("id".into(), json!(ev.id.to_string()));
    m.insert("type".into(), json!(ev.kind));
    m.insert("details".into(), details);
    m.insert("previous_event_id".into(), json!(ev.prev.to_string()));
    m.insert("event_timestamp".into(), json!(ev.ms.to_string()));
    m.insert("execution_arn".into(), json!(e.arn));
    m.insert(
        "redrive_count".into(),
        json!(e.exec.redrive_count.to_string()),
    );
    Value::Object(m).to_string()
}
