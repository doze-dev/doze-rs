//! What a request asks to do, for the authorizer: `cloudformation:<Operation>` on the stack's ARN (`.../stack/<name>/*` when only a
//! name is known, as AWS words its refusal: recorded 2026-11-12) or on `*` for the operations on no stack. The resources a stack
//! makes are authorized by their own services as CloudFormation calls them, acting for the caller.

use aws_model_cloudformation::Operation;
use aws_model_cloudformation::tables::TABLES;
use aws_peers::Need;
use aws_peers::input::In;
use aws_wire::service::Call;

/// Every thing `call` asks to do.
pub(crate) fn needs(call: &Call) -> Vec<Need> {
    let Some(op) = Operation::from_index(call.op) else {
        return Vec::new();
    };
    let body = In::of(&TABLES, call);
    let action = format!("cloudformation:{op:?}");
    let name = body
        .get("StackName")
        .and_then(|v| v.as_str())
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use std::sync::Arc;

    use aws_testkit::Harness;
    use doze_runtime::{Ctx, SystemClock, Tasks};
    use serde_json::{Value, json};

    use super::*;

    fn call(op: &str, body: &Value) -> Call {
        let ctx = Ctx {
            dir: std::env::temp_dir().join("doze-aws-cloudformation-authz-unused"),
            idle: std::time::Duration::from_secs(60),
            tasks: Tasks::new(),
            clock: Arc::new(SystemClock),
            ext: None,
        };
        let h = Harness::new(crate::Cloudformation::new(Arc::new(ctx)));
        h.call(h.op(op), body).unwrap()
    }

    #[test]
    fn a_stack_named_by_name_is_asked_for_on_its_arn_pattern() {
        let named = needs(&call("DescribeStacks", &json!({"StackName": "app"})));
        assert_eq!(named[0].action, "cloudformation:DescribeStacks");
        assert_eq!(
            named[0].resource,
            "arn:aws:cloudformation:us-east-1:000000000000:stack/app/*"
        );
        assert_eq!(needs(&call("DescribeStacks", &json!({})))[0].resource, "*");
    }
}
