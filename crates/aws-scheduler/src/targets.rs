//! What a target's ARN means. A **templated** target is a resource ARN (a queue, a function, a topic, a bus, a state machine, a
//! stream) and calls that service's one documented operation; a **universal** target is `arn:aws:scheduler:::aws-sdk:<service>:<action>`
//! and calls any write operation of a service, with `Input` as the request.

use aws_wire::ApiError;
use serde_json::Value;

use crate::req::invalid;

/// A target, by what its ARN names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind<'a> {
    /// A resource of `service` (the ARN's service part: `sqs`, `lambda`, `states`...).
    Templated { service: &'a str },
    /// `aws-sdk:<sdk>:<action>`.
    Universal { sdk: &'a str, action: &'a str },
}

const UNIVERSAL: &str = "arn:aws:scheduler:::aws-sdk:";

/// The prefixes of read-only actions a universal target may not name (EventBridge Scheduler User Guide, "Using universal targets",
/// read 2026-11-14).
const READ_ONLY: &[&str] = &[
    "get",
    "describe",
    "list",
    "poll",
    "receive",
    "search",
    "scan",
    "query",
    "select",
    "read",
    "lookup",
    "discover",
    "validate",
    "batchGet",
    "batchDescribe",
    "batchRead",
    "transactGet",
    "adminGet",
    "adminList",
    "testMigration",
    "retrieve",
    "testConnection",
    "translateDocument",
    "isAuthorized",
    "batchIsAuthorized",
    "invokeModel",
    "testMapping",
    "testParsing",
    "testState",
];

/// What the target ARN names; `None` for an ARN neither form fits.
pub(crate) fn kind(arn: &str) -> Option<Kind<'_>> {
    if let Some(rest) = arn.strip_prefix(UNIVERSAL) {
        let (sdk, action) = rest.split_once(':')?;
        if sdk.is_empty() || action.is_empty() || action.contains(':') {
            return None;
        }
        return Some(Kind::Universal { sdk, action });
    }
    let mut parts = arn.splitn(6, ':');
    let (Some("arn"), Some(_partition), Some(service), Some(_region), Some(_account), Some(_res)) = (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) else {
        return None;
    };
    (!service.is_empty()).then_some(Kind::Templated { service })
}

/// The service a universal target's SDK id runs as on the bus, for the services doze-aws runs.
pub(crate) fn bus_name(sdk: &str) -> Option<&'static str> {
    Some(match sdk {
        "sqs" => "sqs",
        "sns" => "sns",
        "lambda" => "lambda",
        "sfn" => "states",
        "eventbridge" => "events",
        "dynamodb" => "dynamodb",
        "s3" => "s3",
        "kinesis" => "kinesis",
        "ssm" => "ssm",
        "secretsmanager" => "secretsmanager",
        "kms" => "kms",
        "iam" => "iam",
        "sts" => "sts",
        "cloudwatch" => "monitoring",
        "cloudwatchlogs" => "logs",
        "cloudformation" => "cloudformation",
        "scheduler" => "scheduler",
        _ => return None,
    })
}

/// Refuses a target AWS would refuse when the schedule is made: an ARN of neither form, or a universal target naming a read-only
/// action. A target on a service doze-aws does not run is kept (AWS accepts it) and said so in the log.
pub(crate) fn check(target: &Value) -> Result<(), ApiError> {
    let arn = target
        .get("Arn")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match kind(arn) {
        // (memory)
        None => Err(invalid(format!("Invalid target ARN {arn}."))),
        Some(Kind::Universal { sdk, action }) => {
            let lower = action.to_ascii_lowercase();
            if READ_ONLY
                .iter()
                .any(|p| lower.starts_with(&p.to_ascii_lowercase()))
            {
                // (memory) AWS refuses read-only actions for universal targets.
                return Err(invalid(format!(
                    "Unsupported action {action} for service {sdk}: EventBridge Scheduler does not invoke read-only API actions."
                )));
            }
            if bus_name(sdk).is_none() {
                tracing::warn!(
                    "scheduler: the universal target {arn} names {sdk}, which doze-aws does not run; the schedule is kept and its invocations will fail"
                );
            }
            Ok(())
        }
        Some(Kind::Templated { service }) => {
            if !matches!(
                service,
                "sqs" | "sns" | "lambda" | "states" | "events" | "kinesis"
            ) {
                tracing::warn!(
                    "scheduler: the target {arn} is on {service}, which doze-aws does not deliver to; the schedule is kept and its invocations will fail"
                );
            }
            Ok(())
        }
    }
}
