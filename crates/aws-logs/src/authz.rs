//! What a request asks to do, for the authorizer: the IAM action (`logs:PutLogEvents`) and the ARN of the log group or stream it names.

use aws_model_cloudwatch_logs::Operation;
use aws_peers::Need;
use aws_wire::service::Call;
use serde_json::Value;

use crate::model::group_arn;
use crate::req::name_of_identifier;

/// Every thing `call` asks to do.
pub(crate) fn needs(call: &Call) -> Vec<Need> {
    use Operation as O;
    let Some(op) = Operation::from_index(call.op) else {
        return Vec::new();
    };
    let body: Value = serde_json::from_str(&call.body).unwrap_or(Value::Null);
    let text = |key: &str| body.get(key).and_then(Value::as_str);
    let (region, account) = (&call.caller.region, &call.caller.account_id);
    let action = format!("logs:{op:?}");
    let group = text("logGroupName")
        .map(str::to_owned)
        .or_else(|| text("logGroupIdentifier").map(name_of_identifier))
        .or_else(|| {
            text("resourceArn")
                .filter(|a| a.contains(":log-group:"))
                .map(name_of_identifier)
        });
    let resource = match (op, group) {
        (
            O::DescribeLogGroups
            | O::ListLogGroups
            | O::TestMetricFilter
            | O::DescribeQueryDefinitions
            | O::DescribeAccountPolicies
            | O::DescribeResourcePolicies
            | O::PutResourcePolicy
            | O::DeleteResourcePolicy
            | O::PutAccountPolicy
            | O::DeleteAccountPolicy
            | O::PutQueryDefinition
            | O::DeleteQueryDefinition,
            _,
        )
        | (_, None) => "*".to_owned(),
        (O::PutLogEvents | O::GetLogEvents | O::DeleteLogStream, Some(g)) => format!(
            "{}:log-stream:{}",
            group_arn(region, account, &g),
            text("logStreamName").unwrap_or_default()
        ),
        (O::CreateLogStream | O::DescribeLogStreams | O::FilterLogEvents, Some(g)) => {
            format!("{}:*", group_arn(region, account, &g))
        }
        (_, Some(g)) => group_arn(region, account, &g),
    };
    vec![Need::new(action, resource)]
}
