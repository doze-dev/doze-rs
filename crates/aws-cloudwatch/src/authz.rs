//! What a request asks to do, for the authorizer: the IAM action (`cloudwatch:PutMetricAlarm`) and the ARN of the alarm or dashboard.

use aws_model_cloudwatch::Operation;
use aws_peers::Need;
use aws_wire::service::Call;
use serde_json::Value;

use crate::alarms::alarm_arn;

/// Every thing `call` asks to do.
pub(crate) fn needs(call: &Call) -> Vec<Need> {
    use Operation as O;
    let Some(op) = Operation::from_index(call.op) else {
        return Vec::new();
    };
    let body: Value = serde_json::from_str(&call.body).unwrap_or(Value::Null);
    let text = |key: &str| body.get(key).and_then(Value::as_str);
    let (region, account) = (&call.caller.region, &call.caller.account_id);
    let action = format!("cloudwatch:{op:?}");
    let alarm = |name: &str| Need::new(&action, alarm_arn(region, account, name));
    let dashboard = |name: &str| {
        Need::new(
            &action,
            format!("arn:aws:cloudwatch::{account}:dashboard/{name}"),
        )
    };
    match op {
        O::PutMetricAlarm | O::PutCompositeAlarm | O::SetAlarmState => text("AlarmName")
            .map(|n| vec![alarm(n)])
            .unwrap_or_default(),
        O::DeleteAlarms | O::EnableAlarmActions | O::DisableAlarmActions => body
            .get("AlarmNames")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(alarm)
            .collect(),
        O::PutDashboard | O::GetDashboard => text("DashboardName")
            .map(|n| vec![dashboard(n)])
            .unwrap_or_default(),
        O::DeleteDashboards => body
            .get("DashboardNames")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(dashboard)
            .collect(),
        O::TagResource | O::UntagResource | O::ListTagsForResource => text("ResourceARN")
            .map(|a| vec![Need::new(&action, a)])
            .unwrap_or_default(),
        _ => vec![Need::new(&action, "*")],
    }
}
