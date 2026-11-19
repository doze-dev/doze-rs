//! `PutLogAlarm`: an alarm on the results of a CloudWatch Logs scheduled query. It is checked as the model documents it, kept,
//! described (`DescribeAlarms` with `AlarmTypes: [LogAlarm]`), tagged, forced (`SetAlarmState`) and deleted like any alarm, and its
//! history is kept. It is evaluated on its schedule through Logs Insights (`log_eval`, `log_query`). The refusals' words are (memory).

use aws_wire::ApiError;
use serde_json::Value;

use crate::core::Core;
use crate::model::{Kind, Spec};
use crate::req::{Put, Req, validation};

/// The comparisons a log alarm takes (the model's documentation of `ComparisonOperator`).
const COMPARISONS: [&str; 4] = [
    "GreaterThanThreshold",
    "GreaterThanOrEqualToThreshold",
    "LessThanThreshold",
    "LessThanOrEqualToThreshold",
];
/// The most query results an alarm evaluates (the model: 1 through 100), and log lines a notification carries (0 through 50).
const MAX_RESULTS: i64 = 100;
const MAX_LOG_LINES: i64 = 50;

impl Core {
    pub(crate) fn put_log_alarm(&mut self, r: &Req<'_>) -> Result<Value, ApiError> {
        check(r).map_err(validation)?;
        if self
            .alarms
            .get(r.s("AlarmName"))
            .is_some_and(|a| a.kind != Kind::Log)
        {
            // (memory)
            return Err(ApiError::modeled(
                "ResourceConflict",
                format!(
                    "An alarm named {} that is not a log alarm already exists.",
                    r.s("AlarmName")
                ),
            ));
        }
        self.store_alarm(r, Kind::Log, Spec::default())
    }
}

fn check(r: &Req<'_>) -> Result<(), String> {
    let (n, m) = (
        r.int("QueryResultsToEvaluate").unwrap_or(1),
        r.int("QueryResultsToAlarm").unwrap_or(1),
    );
    if n > MAX_RESULTS {
        return Err(format!(
            "QueryResultsToEvaluate must be between 1 and {MAX_RESULTS}."
        ));
    }
    if m > n {
        // Recorded 2026-11-19 (the two spaces are AWS's).
        return Err(
            "QueryResultsToAlarm must be  smaller or equal than QueryResultsToEvaluate".to_owned(),
        );
    }
    let comparison = r.s("ComparisonOperator");
    if !COMPARISONS.contains(&comparison) {
        return Err(format!(
            "The comparison {comparison} is not valid for a log alarm. Valid values: {}.",
            COMPARISONS.join(", ")
        ));
    }
    if let Some(treat) = r.opt("TreatMissingData")
        && !["breaching", "notBreaching", "ignore", "missing"].contains(&treat)
    {
        return Err(format!(
            "The value {treat} is not supported for TreatMissingData parameter. Supported values are [breaching, notBreaching, ignore, missing]."
        ));
    }
    let lines = r.int("ActionLogLineCount").unwrap_or(0);
    if !(0..=MAX_LOG_LINES).contains(&lines) {
        return Err(format!(
            "ActionLogLineCount must be between 0 and {MAX_LOG_LINES}."
        ));
    }
    if lines > 0 && r.opt("ActionLogLineRoleArn").is_none() {
        return Err(
            "ActionLogLineRoleArn is required when ActionLogLineCount is greater than 0."
                .to_owned(),
        );
    }
    schedule(
        r.v.pointer("/ScheduledQueryConfiguration/ScheduleConfiguration")
            .unwrap_or(&Value::Null),
    )
}

/// The schedule: a `rate()` expression, and offsets with the end before the start (the model).
fn schedule(s: &Value) -> Result<(), String> {
    let expr = s
        .get("ScheduleExpression")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let rate = expr
        .strip_prefix("rate(")
        .and_then(|x| x.strip_suffix(')'))
        .and_then(|x| x.trim().split_once(' '))
        .filter(|(n, unit)| {
            n.parse::<u32>().is_ok_and(|n| n > 0)
                && ["minute", "minutes", "hour", "hours", "day", "days"].contains(unit)
        });
    if rate.is_none() {
        return Err(format!(
            "The schedule expression {expr} is not valid: use a rate() expression, for example rate(5 minutes)."
        ));
    }
    let start = s
        .get("StartTimeOffset")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let end = s.get("EndTimeOffset").and_then(Value::as_i64).unwrap_or(0);
    if end < 0 || end >= start {
        return Err("EndTimeOffset must be non-negative and less than StartTimeOffset.".to_owned());
    }
    Ok(())
}

/// A log alarm as `DescribeAlarms` shows it: what was sent, the scheduled query's ARN, and its
/// `EvaluationState` when it has one.
pub(crate) fn shown(mut v: Value, evaluation: Option<&str>) -> Value {
    if let Some(state) = evaluation {
        v.put("EvaluationState", Value::from(state));
    }
    let arn = v
        .get("AlarmArn")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    // (memory) the shape of the ARN of the scheduled query AWS makes for the alarm.
    let query_arn =
        arn.replacen(":cloudwatch:", ":logs:", 1)
            .replacen(":alarm:", ":scheduled-query:alarm-", 1);
    if let Some(Value::Object(q)) = v.get_mut("ScheduledQueryConfiguration") {
        q.insert("QueryARN".to_owned(), Value::String(query_arn));
    }
    // Recorded 2026-11-19: AWS fills in no default for TreatMissingData or ActionLogLineCount; a member not sent is not shown (the
    // evaluation still treats missing data as `missing`).
    v
}
