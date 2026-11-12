//! What a request asks to do, for the authorizer: the IAM action (`states:StartExecution`) and the ARN acted on. Making or
//! re-pointing a machine also passes its role (`iam:PassRole`), and tags given at creation are a second thing to ask for.

use aws_model_sfn::Operation;
use aws_peers::Need;
use aws_wire::service::Call;
use serde_json::Value;

use crate::core::Core;

/// Every thing `call` asks to do.
pub(crate) fn needs(call: &Call) -> Vec<Need> {
    Core::needs_of(call)
}

impl Core {
    /// Every thing `call` asks to do.
    fn needs_of(call: &Call) -> Vec<Need> {
        use Operation as O;
        let Some(op) = Operation::from_index(call.op) else {
            return Vec::new();
        };
        let body: Value = serde_json::from_str(&call.body).unwrap_or(Value::Null);
        let text = |m: &str| {
            body.get(m)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let (region, account) = (&call.caller.region, &call.caller.account_id);
        let action = format!("states:{op:?}");
        let resource = match op {
            O::CreateStateMachine => crate::arn::machine(region, account, &text("name")),
            O::CreateActivity => crate::arn::activity(region, account, &text("name")),
            O::CreateStateMachineAlias => body
                .pointer("/routingConfiguration/0/stateMachineVersionArn")
                .and_then(Value::as_str)
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
            O::DescribeStateMachineAlias
            | O::UpdateStateMachineAlias
            | O::DeleteStateMachineAlias => text("stateMachineAliasArn"),
            O::DescribeMapRun | O::UpdateMapRun => text("mapRunArn"),
            O::TagResource | O::UntagResource | O::ListTagsForResource => text("resourceArn"),
            O::ListStateMachines
            | O::ListActivities
            | O::ValidateStateMachineDefinition
            | O::TestState => "*".to_owned(),
            O::SendTaskSuccess | O::SendTaskFailure | O::SendTaskHeartbeat => "*".to_owned(),
            _ => text("stateMachineArn"),
        };
        let mut needs = vec![Need::new(
            &action,
            if resource.is_empty() {
                "*".to_owned()
            } else {
                resource.clone()
            },
        )];
        if matches!(
            op,
            O::CreateStateMachine | O::UpdateStateMachine | O::TestState
        ) && let Some(role) = body.get("roleArn").and_then(Value::as_str)
        {
            needs.push(Need::new("iam:PassRole", role).with(
                "iam:PassedToService",
                vec!["states.amazonaws.com".to_owned()],
            ));
        }
        if matches!(op, O::CreateStateMachine | O::CreateActivity)
            && body
                .get("tags")
                .and_then(Value::as_array)
                .is_some_and(|t| !t.is_empty())
        {
            needs.push(Need::new("states:TagResource", resource));
        }
        needs
    }
}
