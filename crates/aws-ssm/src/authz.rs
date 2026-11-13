//! What a request asks to do, for the authorizer: the IAM action (`ssm:GetParameter`) and the ARN of each parameter it names.

use aws_model_ssm::Operation;
use aws_peers::Need;
use aws_wire::service::Call;
use serde_json::Value;

/// Every thing `call` asks to do.
pub(crate) fn needs(call: &Call) -> Vec<Need> {
    let Some(op) = Operation::from_index(call.op) else {
        return Vec::new();
    };
    let body: Value = serde_json::from_str(&call.body).unwrap_or(Value::Null);
    let (region, account) = (&call.caller.region, &call.caller.account_id);
    let action = format!("ssm:{op:?}");
    let on = |name: &str| {
        // A version or label selector (`name:3`) is not part of the ARN.
        let bare = name.split_once(':').map_or(name, |(n, _)| n);
        Need::new(
            action.clone(),
            format!(
                "arn:aws:ssm:{region}:{account}:parameter/{}",
                bare.trim_start_matches('/')
            ),
        )
    };
    let one = |key: &str| body.get(key).and_then(Value::as_str).map(on);
    match op {
        Operation::GetParameters | Operation::DeleteParameters => body
            .get("Names")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(on)
            .collect(),
        Operation::GetParametersByPath => one("Path").into_iter().collect(),
        Operation::AddTagsToResource
        | Operation::RemoveTagsFromResource
        | Operation::ListTagsForResource => {
            // Only parameters are tagged here.
            one("ResourceId").into_iter().collect()
        }
        Operation::DescribeParameters => vec![Need::new(
            action.clone(),
            format!("arn:aws:ssm:{region}:{account}:*"),
        )],
        _ => one("Name").into_iter().collect(),
    }
}
