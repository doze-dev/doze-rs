//! What a request asks to do, for the authorizer: the IAM action (`scheduler:CreateSchedule`) and the ARN acted on. Making or
//! changing a schedule also passes its execution role, which needs `iam:PassRole` on the role (documented).

use aws_model_scheduler::Operation;
use aws_peers::Need;
use aws_wire::service::Call;
use serde_json::Value;

use crate::core::Core;
use crate::model::{DEFAULT_GROUP, group_arn, schedule_arn};

impl Core {
    /// Every thing `call` asks to do.
    pub(crate) fn needs(&self, call: &Call) -> Vec<Need> {
        use Operation as O;
        let Some(op) = Operation::from_index(call.op) else {
            return Vec::new();
        };
        let body: Value = serde_json::from_str(&call.body).unwrap_or(Value::Null);
        let text = |m: &str| body.get(m).and_then(Value::as_str).unwrap_or_default();
        let (region, account) = (&call.caller.region, &call.caller.account_id);
        let action = format!("scheduler:{op:?}");
        let group = Some(text("GroupName"))
            .filter(|g| !g.is_empty())
            .unwrap_or(DEFAULT_GROUP);
        let schedule = || Need::new(&action, schedule_arn(region, account, group, text("Name")));
        let mut needs = match op {
            O::CreateSchedule | O::UpdateSchedule | O::GetSchedule | O::DeleteSchedule => {
                vec![schedule()]
            }
            O::CreateScheduleGroup | O::GetScheduleGroup | O::DeleteScheduleGroup => {
                vec![Need::new(&action, group_arn(region, account, text("Name")))]
            }
            O::TagResource | O::UntagResource | O::ListTagsForResource => {
                vec![Need::new(&action, text("ResourceArn"))]
            }
            O::ListSchedules | O::ListScheduleGroups => vec![Need::new(&action, "*")],
        };
        if matches!(op, O::CreateSchedule | O::UpdateSchedule)
            && let Some(role) = body.pointer("/Target/RoleArn").and_then(Value::as_str)
        {
            needs.push(Need::new("iam:PassRole", role));
        }
        needs
    }
}
