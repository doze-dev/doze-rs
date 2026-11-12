//! Log records and status-change events, sent outside the actor. Records go to CloudWatch Logs (`CreateLogStream`, then
//! `PutLogEvents`; a group that does not exist is reported once in doze's own log). Status changes go to the account's default event
//! bus as `aws.states` / `Step Functions Execution Status Change`. A service that is not running is skipped without complaint.

use std::sync::Arc;

use aws_peers::Peers;
use aws_wire::service::Caller;
use serde_json::{Value, json};

use crate::effects::LogWrite;

pub(crate) async fn send(
    peers: Arc<Peers>,
    caller: Caller,
    logs: Vec<LogWrite>,
    notices: Vec<(Value, String, String, String)>,
) {
    for w in logs {
        write(&peers, &caller, w).await;
    }
    for (detail, resource, region, account) in notices {
        let as_account = Caller {
            region,
            account_id: account,
            ..caller.clone()
        };
        #[allow(clippy::cast_precision_loss)]
        let entry = json!({
            "Source": "aws.states", "DetailType": "Step Functions Execution Status Change",
            "Detail": detail.to_string(), "Resources": [resource],
        });
        if let Err(e) = peers
            .call(
                "states",
                &as_account,
                "events",
                "PutEvents",
                &json!({"Entries": [entry]}),
            )
            .await
            && !e.message.starts_with("no service events")
        {
            tracing::debug!(
                "step functions: the status event was not put on the default bus: {}",
                e.message
            );
        }
    }
}

async fn write(peers: &Peers, caller: &Caller, w: LogWrite) {
    let group = w
        .group_arn
        .split_once(":log-group:")
        .map_or(w.group_arn.as_str(), |(_, g)| g)
        .trim_end_matches(":*");
    let as_account = Caller {
        region: w.region.clone(),
        account_id: w.account.clone(),
        ..caller.clone()
    };
    let made = peers
        .call(
            "states",
            &as_account,
            "logs",
            "CreateLogStream",
            &json!({"logGroupName": group, "logStreamName": w.stream}),
        )
        .await;
    if let Err(e) = &made
        && !e.code.contains("AlreadyExists")
    {
        if !e.message.starts_with("no service logs") {
            tracing::warn!("step functions: cannot log to {group}: {}", e.message);
        }
        return;
    }
    let events: Vec<Value> = w
        .records
        .iter()
        .map(|(ms, m)| json!({"timestamp": ms, "message": m}))
        .collect();
    if let Err(e) = peers
        .call(
            "states",
            &as_account,
            "logs",
            "PutLogEvents",
            &json!({"logGroupName": group, "logStreamName": w.stream, "logEvents": events}),
        )
        .await
    {
        tracing::warn!(
            "step functions: the log records for {group} were not written: {}",
            e.message
        );
    }
}
