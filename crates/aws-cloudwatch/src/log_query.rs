//! A log alarm's scheduled query, run through the peer bus as Logs Insights: `StartQuery`, then `GetQueryResults` until the query has
//! ended. This runs in a task the alarm timer starts for the run (and only for it), never in the actor. Insights answers through
//! `GetQueryResults` alone, so the task asks again while the query runs: at once, then less often, up to a second apart, and gives
//! up after a minute (ours; a local query over one window takes milliseconds).

use std::time::Duration;

use aws_peers::Peers;
use aws_wire::service::Caller;
use serde_json::{Value, json};

use crate::log_eval::LogRun;

/// The longest a run waits for its query (ours).
const GIVE_UP: Duration = Duration::from_secs(60);
/// The first and the longest pause between two asks.
const FIRST_PAUSE: Duration = Duration::from_millis(10);
const LONGEST_PAUSE: Duration = Duration::from_secs(1);

/// The aggregated values a run's query found (one per contributor; none when it found no rows), or why it failed.
pub(crate) async fn run(peers: &Peers, caller: &Caller, run: &LogRun) -> Result<Vec<f64>, String> {
    let body = json!({
        "logGroupIdentifiers": run.groups,
        "startTime": run.range_ms.0.div_euclid(1000),
        "endTime": run.range_ms.1.div_euclid(1000),
        "queryString": run.query,
    });
    let started = peers
        .call("monitoring", caller, "logs", "StartQuery", &body)
        .await
        .map_err(|e| format!("{}: {}", e.code, e.message))?;
    let id = started
        .get("queryId")
        .and_then(Value::as_str)
        .ok_or("StartQuery gave no query id")?
        .to_owned();
    let deadline = tokio::time::Instant::now() + GIVE_UP;
    let mut pause = FIRST_PAUSE;
    loop {
        let got = peers
            .call(
                "monitoring",
                caller,
                "logs",
                "GetQueryResults",
                &json!({"queryId": id}),
            )
            .await
            .map_err(|e| format!("{}: {}", e.code, e.message))?;
        match got
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "Complete" => return Ok(values(&got, run)),
            "Failed" | "Cancelled" | "Timeout" | "Unknown" => {
                let status = got
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                return Err(format!("the query ended {status}"));
            }
            _ => {}
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("the query did not finish within a minute".to_owned());
        }
        tokio::time::sleep(pause).await;
        pause = (pause * 2).min(LONGEST_PAUSE);
    }
}

/// The aggregate of each result row: the column named for it, else the first number that is not a `by` field.
fn values(results: &Value, run: &LogRun) -> Vec<f64> {
    let rows = results
        .get("results")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    rows.iter()
        .filter_map(|row| {
            let cells: Vec<(&str, &str)> = row
                .as_array()?
                .iter()
                .filter_map(|c| Some((c.get("field")?.as_str()?, c.get("value")?.as_str()?)))
                .collect();
            let named = cells
                .iter()
                .find(|(f, _)| *f == run.value_field)
                .and_then(|(_, v)| v.parse().ok());
            named.or_else(|| {
                cells
                    .iter()
                    .filter(|(f, _)| !run.by.iter().any(|b| b == f) && *f != "@ptr")
                    .find_map(|(_, v)| v.parse().ok())
            })
        })
        .collect()
}
