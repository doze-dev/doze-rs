//! What a request asks to do, for the authorizer: `cloudformation:<Operation>` on the stack's ARN (`.../stack/<name>/*` when only a
//! name is known, as AWS words its refusal: recorded 2026-11-12) or on `*` for the operations on no stack. The resources a stack
//! makes are authorized by their own services as CloudFormation calls them, acting for the caller.

use aws_model_cloudformation::Operation;
use aws_peers::Need;
use aws_wire::service::Call;
use serde_json::Value;

/// Every thing `call` asks to do.
pub(crate) fn needs(call: &Call) -> Vec<Need> {
    let Some(op) = Operation::from_index(call.op) else {
        return Vec::new();
    };
    let body: Value = serde_json::from_str(&call.body).unwrap_or(Value::Null);
    let action = format!("cloudformation:{op:?}");
    let name = body
        .get("StackName")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let resource = if name.starts_with("arn:") {
        name.to_owned()
    } else if name.is_empty() {
        "*".to_owned()
    } else {
        format!(
            "arn:aws:cloudformation:{}:{}:stack/{name}/*",
            call.caller.region, call.caller.account_id
        )
    };
    vec![Need::new(action, resource)]
}
