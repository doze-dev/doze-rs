//! What a request asks to do, for the authorizer: the IAM action (`states:StartExecution`) and the ARN acted on. Making or
//! re-pointing a machine also passes its role (`iam:PassRole`), and tags given at creation are a second thing to ask for.

use aws_model_sfn::Operation;
use aws_model_sfn::tables::TABLES;
use aws_peers::Need;
use aws_peers::input::In;
use aws_wire::service::Call;

/// Every thing `call` asks to do.
pub(crate) fn needs(call: &Call) -> Vec<Need> {
    use Operation as O;
    let Some(op) = Operation::from_index(call.op) else {
        return Vec::new();
    };
    let body = In::of(&TABLES, call);
    let action = format!("states:{op:?}");
    let resource = resource(op, &body, &call.caller.region, &call.caller.account_id);
    let on = if resource.is_empty() {
        "*".to_owned()
    } else {
        resource.clone()
    };
    let mut needs = vec![Need::new(&action, on)];
    if matches!(
        op,
        O::CreateStateMachine | O::UpdateStateMachine | O::TestState
    ) && let Some(role) = body.get("roleArn").and_then(|v| v.as_str())
    {
        needs.push(Need::new("iam:PassRole", role).with(
            "iam:PassedToService",
            vec!["states.amazonaws.com".to_owned()],
        ));
    }
    if matches!(op, O::CreateStateMachine | O::CreateActivity)
        && body
            .get("tags")
            .and_then(|v| v.as_array())
            .is_some_and(|t| !t.is_empty())
    {
        needs.push(Need::new("states:TagResource", resource));
    }
    needs
}

/// The ARN a request acts on (`*` for one that acts on none).
fn resource(op: Operation, body: &In<'_>, region: &str, account: &str) -> String {
    use Operation as O;
    let text = |m: &str| {
        body.get(m)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_owned()
    };
    match op {
        O::CreateStateMachine => crate::arn::machine(region, account, &text("name")),
        O::CreateActivity => crate::arn::activity(region, account, &text("name")),
        O::CreateStateMachineAlias => body
            .pointer("/routingConfiguration/0/stateMachineVersionArn")
            .and_then(|v| v.as_str())
            .map(|v| {
                format!(
                    "{}:{}",
                    v.rsplit_once(':').map_or(v, |(m, _)| m),
                    text("name")
                )
            })
            .unwrap_or_default(),
        O::DescribeExecution
        | O::StopExecution
        | O::GetExecutionHistory
        | O::RedriveExecution
        | O::DescribeStateMachineForExecution
        | O::ListMapRuns => text("executionArn"),
        O::DescribeActivity | O::DeleteActivity | O::GetActivityTask => text("activityArn"),
        O::DeleteStateMachineVersion => text("stateMachineVersionArn"),
        O::DescribeStateMachineAlias | O::UpdateStateMachineAlias | O::DeleteStateMachineAlias => {
            text("stateMachineAliasArn")
        }
        O::DescribeMapRun | O::UpdateMapRun => text("mapRunArn"),
        O::TagResource | O::UntagResource | O::ListTagsForResource => text("resourceArn"),
        O::ListStateMachines
        | O::ListActivities
        | O::ValidateStateMachineDefinition
        | O::TestState
        | O::SendTaskSuccess
        | O::SendTaskFailure
        | O::SendTaskHeartbeat => "*".to_owned(),
        _ => text("stateMachineArn"),
    }
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
            dir: std::env::temp_dir().join("doze-aws-sfn-authz-unused"),
            idle: std::time::Duration::from_secs(60),
            tasks: Tasks::new(),
            clock: Arc::new(SystemClock),
            ext: None,
        };
        let h = Harness::new(crate::Sfn::new(Arc::new(ctx)));
        h.call(h.op(op), body).unwrap()
    }

    #[test]
    fn an_alias_is_asked_for_on_the_machine_its_first_route_names_and_a_machine_passes_its_role() {
        let alias = needs(&call(
            "CreateStateMachineAlias",
            &json!({"name": "live", "routingConfiguration": [
                {"stateMachineVersionArn": "arn:aws:states:us-east-1:000000000000:stateMachine:m:1", "weight": 100}
            ]}),
        ));
        assert_eq!(
            alias[0].resource,
            "arn:aws:states:us-east-1:000000000000:stateMachine:m:live"
        );
        let made = needs(&call(
            "CreateStateMachine",
            &json!({"name": "m", "definition": "{}", "roleArn": "arn:aws:iam::000000000000:role/r",
                "tags": [{"key": "k", "value": "v"}]}),
        ));
        let on: Vec<(&str, &str)> = made
            .iter()
            .map(|n| (n.action.as_str(), n.resource.as_str()))
            .collect();
        assert_eq!(
            on,
            [
                (
                    "states:CreateStateMachine",
                    "arn:aws:states:us-east-1:000000000000:stateMachine:m"
                ),
                ("iam:PassRole", "arn:aws:iam::000000000000:role/r"),
                (
                    "states:TagResource",
                    "arn:aws:states:us-east-1:000000000000:stateMachine:m"
                ),
            ]
        );
    }
}
