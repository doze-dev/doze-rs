//! What a call acted on, read from its decoded input: one generic table of the input members that name a resource, shared by
//! the gateway (a call from the wire) and the bus (a call between services), so every service is named the same way without a
//! line of its own.
//!
//! Only the input's top-level members are looked at, in the order of [`MEMBERS`]: the first one present names the resource. A
//! member is matched by its model name, case aside (Step Functions writes `stateMachineArn`, Logs `logGroupName`). Nothing is
//! copied: the value is borrowed from the call's body or arena.

use aws_schema::arena::{Arena, NIL, Val};
use aws_schema::tables::Tables;

/// The input members that name what a call acts on, most specific first. `Name` is near the end: it names the thing a create makes
/// (`CreateTopic`, `CreateSecret`, SSM's `PutParameter`), and in an input that has a more specific member it is something else.
/// After it comes a rule's bus, which names where a rule lives, not the rule (`PutRule` has `Name` and `EventBusName`,
/// `PutTargets` `Rule` and `EventBusName`): it names the call only when nothing else does. (`GroupName` stays above: it is an
/// IAM group's name first, and `AttachGroupPolicy` acts on the group; a schedule in a group of its own is named by its group.)
pub const MEMBERS: &[&str] = &[
    "QueueUrl",
    "QueueName",
    "TableName",
    "FunctionName",
    "TopicArn",
    "TargetArn",
    "SubscriptionArn",
    "StreamName",
    "StreamARN",
    "LogGroupName",
    "LogGroupIdentifier",
    "StateMachineArn",
    "ExecutionArn",
    "SecretId",
    "KeyId",
    // `AttachRolePolicy` and its kin name a policy and who it goes to: the call is the policy's (its page lists what attached it). A group
    // membership (`AddUserToGroup`) is the group's.
    "PolicyArn",
    "RoleName",
    "GroupName",
    "UserName",
    "StackName",
    "RestApiId",
    "ApiId",
    "Rule",
    "ScheduleName",
    "AlarmName",
    "Bucket",
    "Name",
    "EventBusName",
    // STS's `AssumeRole` acts on the role it names; last, because other inputs carry a role's ARN beside what they act on
    // (`CreateStack`'s `RoleARN`).
    "RoleArn",
];

/// The member of `MEMBERS` that names the resource of a call with input `root` (decoded from `body` into `arena`), and its
/// value; `None` when the input has none of them, or it is not a string.
#[must_use]
pub fn resource_of<'a>(
    tables: &'static Tables,
    arena: &'a Arena,
    body: &'a str,
    root: u32,
) -> Option<(&'static str, &'a str)> {
    if root == NIL {
        return None;
    }
    let Val::Obj(first) = arena.node(root)?.val else {
        return None;
    };
    let mut best: Option<(usize, &'static str, &'a str)> = None;
    let mut at = first;
    while at != NIL {
        let node = arena.node(at)?;
        if let Some(member) = tables.member(node.member)
            && let Some(rank) = MEMBERS
                .iter()
                .position(|m| m.eq_ignore_ascii_case(member.name))
            && best.is_none_or(|(r, _, _)| rank < r)
            && let Some(value) = arena.str_of(body, node.val).filter(|v| !v.is_empty())
        {
            best = Some((rank, member.name, value));
        }
        at = node.next;
    }
    best.map(|(_, member, value)| (member, value))
        .or_else(|| events_bus(tables, arena, body, first))
}

/// What `PutEvents` acts on: it has no member of its own at the top, only `Entries`, so its first entry names it, by the bus it is
/// put on (`default` for an entry that names none). An input that has other members of `MEMBERS` never comes here.
fn events_bus<'a>(
    tables: &'static Tables,
    arena: &'a Arena,
    body: &'a str,
    first: u32,
) -> Option<(&'static str, &'a str)> {
    let mut at = first;
    while at != NIL {
        let node = arena.node(at)?;
        if tables.member(node.member)?.name == "Entries"
            && let Val::Seq(entry) = node.val
            && let Val::Obj(mut field) = arena.node(entry)?.val
        {
            let mut seen = false;
            while field != NIL {
                let f = arena.node(field)?;
                let name = tables.member(f.member)?.name;
                if name == "EventBusName"
                    && let Some(bus) = arena.str_of(body, f.val).filter(|b| !b.is_empty())
                {
                    return Some(("EventBusName", bus));
                }
                seen |= name == "DetailType";
                field = f.next;
            }
            return seen.then_some(("EventBusName", "default"));
        }
        at = node.next;
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use aws_schema::json::decode;
    use aws_schema::patterns::Patterns;
    use aws_wire::service::Service;

    use super::*;

    fn named(op: &str, body: &str) -> Option<(&'static str, String)> {
        let dir = std::env::temp_dir().join(format!("doze-aws-naming-{}", std::process::id()));
        let ctx = doze_runtime::Ctx {
            dir,
            idle: std::time::Duration::from_secs(60),
            tasks: doze_runtime::Tasks::new(),
            clock: std::sync::Arc::new(doze_runtime::SystemClock),
            ext: None,
        };
        let kms = aws_kms::Kms::new(std::sync::Arc::new(ctx));
        let tables = kms.tables();
        let shape = tables
            .op(tables.find_op(op).unwrap())
            .unwrap()
            .input
            .unwrap();
        let mut arena = Arena::new();
        let root = decode(tables, &Patterns::new(tables), shape, body, &mut arena).unwrap();
        resource_of(tables, &arena, body, root).map(|(m, v)| (m, v.to_owned()))
    }

    #[test]
    fn the_member_that_names_the_resource_is_found_in_any_input() {
        assert_eq!(
            named("DescribeKey", r#"{"KeyId":"alias/orders"}"#),
            Some(("KeyId", "alias/orders".to_owned()))
        );
        // An input that names nothing names nothing.
        assert_eq!(named("ListKeys", "{}"), None);
        assert_eq!(named("CreateKey", r#"{"Description":"x"}"#), None);
    }

    fn put(body: &str) -> Option<(&'static str, String)> {
        let tables = &aws_model_eventbridge::tables::TABLES;
        let shape = tables
            .op(tables.find_op("PutEvents").unwrap())
            .unwrap()
            .input
            .unwrap();
        let mut arena = Arena::new();
        let root = decode(tables, &Patterns::new(tables), shape, body, &mut arena).unwrap();
        resource_of(tables, &arena, body, root).map(|(m, v)| (m, v.to_owned()))
    }

    #[test]
    fn events_put_on_a_bus_are_the_buss() {
        let entry = r#""Source":"a","DetailType":"b","Detail":"{}""#;
        assert_eq!(
            put(&format!(
                r#"{{"Entries":[{{"EventBusName":"larder",{entry}}}]}}"#
            )),
            Some(("EventBusName", "larder".to_owned()))
        );
        // An entry that names no bus is put on the default one.
        assert_eq!(
            put(&format!(r#"{{"Entries":[{{{entry}}}]}}"#)),
            Some(("EventBusName", "default".to_owned()))
        );
        assert_eq!(put(r#"{"Entries":[{"Source":"a"}]}"#), None);
    }
}
