//! What a request asks to do, for the authorizer: the IAM action (`events:PutRule`), the ARN acted on, and a bus's own policy when
//! the thing is a bus. `PutEvents` is judged once for each bus its entries name.

use aws_model_eventbridge::Operation;
use aws_peers::Need;
use aws_wire::service::Call;
use serde_json::Value;

use crate::core::Core;
use crate::model::key;
use crate::names::{DEFAULT_BUS, archive_arn, bus_arn, bus_name, replay_arn, rule_arn};

impl Core {
    /// Every thing `call` asks to do. Empty for a request that is not understood (it is refused on its own terms).
    pub(crate) fn needs(&self, call: &Call) -> Vec<Need> {
        use Operation as O;
        let Some(op) = Operation::from_index(call.op) else {
            return Vec::new();
        };
        let body: Value = serde_json::from_str(&call.body).unwrap_or(Value::Null);
        let text = |m: &str| body.get(m).and_then(Value::as_str).unwrap_or_default();
        let (region, account) = (&call.caller.region, &call.caller.account_id);
        let action = format!("events:{op:?}");
        let bus_of = |m: &str| {
            let given = Some(text(m))
                .filter(|b| !b.is_empty())
                .unwrap_or(DEFAULT_BUS);
            bus_name(given).unwrap_or(given).to_owned()
        };
        let on_bus = |name: &str| {
            let policy = self
                .buses
                .get(&key(account, &[name]))
                .and_then(|b| b.policy.clone());
            Need::new(&action, bus_arn(region, account, name)).with_policy(policy)
        };
        let rule = |bus_member: &str, name: &str| {
            Need::new(
                &action,
                rule_arn(region, account, &bus_of(bus_member), name),
            )
        };
        let anything = || vec![Need::new(&action, "*")];
        let mut needs = match op {
            O::PutEvents => self.put_events_needs(&body, &on_bus),
            O::CreateEventBus | O::DeleteEventBus | O::UpdateEventBus => {
                vec![on_bus(&bus_of("Name"))]
            }
            O::DescribeEventBus => vec![on_bus(&bus_of("Name"))],
            O::PutPermission | O::RemovePermission => vec![on_bus(&bus_of("EventBusName"))],
            O::PutRule | O::DeleteRule | O::DescribeRule | O::EnableRule | O::DisableRule => {
                vec![rule("EventBusName", text("Name"))]
            }
            O::PutTargets | O::RemoveTargets | O::ListTargetsByRule => {
                vec![rule("EventBusName", text("Rule"))]
            }
            O::TagResource | O::UntagResource | O::ListTagsForResource => {
                vec![Need::new(&action, text("ResourceARN"))]
            }
            O::CreateArchive | O::UpdateArchive | O::DescribeArchive | O::DeleteArchive => {
                vec![Need::new(
                    &action,
                    archive_arn(region, account, text("ArchiveName")),
                )]
            }
            O::StartReplay | O::DescribeReplay | O::CancelReplay => {
                vec![Need::new(
                    &action,
                    replay_arn(region, account, text("ReplayName")),
                )]
            }
            O::CreateConnection
            | O::UpdateConnection
            | O::DeleteConnection
            | O::DescribeConnection
            | O::DeauthorizeConnection => {
                vec![Need::new(
                    &action,
                    format!(
                        "arn:aws:events:{region}:{account}:connection/{}/*",
                        text("Name")
                    ),
                )]
            }
            O::CreateApiDestination
            | O::UpdateApiDestination
            | O::DeleteApiDestination
            | O::DescribeApiDestination => {
                vec![Need::new(
                    &action,
                    format!(
                        "arn:aws:events:{region}:{account}:api-destination/{}/*",
                        text("Name")
                    ),
                )]
            }
            _ => anything(),
        };
        // Tags given when a rule or a bus is made are a second thing to ask for.
        if matches!(op, O::PutRule | O::CreateEventBus)
            && body
                .get("Tags")
                .and_then(Value::as_array)
                .is_some_and(|t| !t.is_empty())
        {
            let target = if op == O::PutRule {
                rule_arn(region, account, &bus_of("EventBusName"), text("Name"))
            } else {
                bus_arn(region, account, text("Name"))
            };
            needs.push(Need::new("events:TagResource", target));
        }
        needs
    }

    /// One need for each distinct bus the entries name.
    fn put_events_needs(&self, body: &Value, on_bus: &dyn Fn(&str) -> Need) -> Vec<Need> {
        let mut buses: Vec<String> = body
            .get("Entries")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|e| {
                let given = e
                    .get("EventBusName")
                    .and_then(Value::as_str)
                    .filter(|b| !b.is_empty());
                let given = given.unwrap_or(DEFAULT_BUS);
                bus_name(given).unwrap_or(given).to_owned()
            })
            .collect();
        buses.sort();
        buses.dedup();
        buses.iter().map(|b| on_bus(b)).collect()
    }
}
