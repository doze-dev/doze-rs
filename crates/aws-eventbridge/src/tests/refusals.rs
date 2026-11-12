//! `PutEvents` entry checks, the operations that are built and the ones refused by name, and what a request asks the authorizer.

use aws_peers::Need;
use serde_json::{Value, json};

use super::{World, caller};

fn entry(extra: &Value) -> Value {
    let mut e = json!({"Source": "s", "DetailType": "d", "Detail": "{}"});
    for (k, v) in extra.as_object().unwrap() {
        e[k] = v.clone();
    }
    e
}

#[test]
fn an_entry_that_cannot_be_accepted_fails_alone_in_awss_words() {
    let mut w = World::new();
    let put = |w: &mut World, entries: Vec<Value>| w.ok("PutEvents", &json!({"Entries": entries}));
    let r = put(&mut w, vec![json!({"DetailType": "x", "Detail": "{}"})]);
    assert_eq!(r["FailedEntryCount"], 1);
    assert_eq!(
        r["Entries"][0],
        json!({"ErrorCode": "InvalidArgument", "ErrorMessage": "Parameter Source is not valid. Reason: Source is a required argument."})
    );
    let r = put(
        &mut w,
        vec![json!({"Source": "s", "DetailType": "x", "Detail": "not json"})],
    );
    assert_eq!(
        r["Entries"][0],
        json!({"ErrorCode": "MalformedDetail", "ErrorMessage": "Detail is malformed."})
    );
    // Each entry is judged on its own; the good one is still accepted.
    let r = put(
        &mut w,
        vec![entry(&json!({"Source": ""})), entry(&json!({}))],
    );
    assert_eq!(
        (
            r["FailedEntryCount"].clone(),
            r["Entries"][1]["EventId"].is_string()
        ),
        (json!(1), true)
    );
    assert_eq!(
        put(&mut w, vec![entry(&json!({"DetailType": ""}))])["Entries"][0]["ErrorMessage"],
        "Parameter DetailType is not valid. Reason: DetailType is a required argument."
    );
    // The detail must be an object.
    assert_eq!(
        put(&mut w, vec![entry(&json!({"Detail": "[1]"}))])["Entries"][0]["ErrorCode"],
        "MalformedDetail"
    );
}

#[test]
fn an_entry_for_a_bus_that_is_not_there_fails_the_call_and_the_time_may_be_given() {
    let mut w = World::new();
    let (code, message) = w.err(
        "PutEvents",
        &json!({"Entries": [entry(&json!({"EventBusName": "absent"}))]}),
    );
    assert_eq!(
        (code.as_str(), message.as_str()),
        (
            "ResourceNotFoundException",
            "Event bus absent does not exist."
        )
    );
    let rule = w.rule("default", "r", &json!({"source": ["s"]}));
    let q = w.queue("q", &rule);
    w.ok(
        "PutTargets",
        &json!({"Rule": "r", "Targets": [{"Id": "t", "Arn": q}]}),
    );
    w.ok("PutEvents", &json!({"Entries": [entry(&json!({"Time": 1_600_000_000, "Resources": ["arn:aws:x:::a", "arn:aws:x:::b"]}))]}));
    let got = w.bodies("q");
    assert_eq!(got[0]["time"], "2020-09-13T12:26:40Z");
    assert_eq!(
        got[0]["resources"],
        json!(["arn:aws:x:::a", "arn:aws:x:::b"])
    );
}

#[test]
fn a_request_over_the_size_limit_is_refused_whole() {
    let mut w = World::new();
    let big = "x".repeat(140 * 1024);
    let (code, message) = w.err(
        "PutEvents",
        &json!({"Entries": [entry(&json!({"Detail": json!({"a": big}).to_string()})), entry(&json!({"Detail": json!({"a": big}).to_string()}))]}),
    );
    assert_eq!(code, "ValidationException");
    assert!(message.contains("over the limit"), "{message}");
}

#[test]
fn the_built_operations_and_the_refused_ones() {
    let w = World::new();
    for op in [
        "PutEvents",
        "PutRule",
        "DeleteRule",
        "DescribeRule",
        "ListRules",
        "EnableRule",
        "DisableRule",
        "PutTargets",
        "RemoveTargets",
        "ListTargetsByRule",
        "ListRuleNamesByTarget",
        "CreateEventBus",
        "UpdateEventBus",
        "DeleteEventBus",
        "DescribeEventBus",
        "ListEventBuses",
        "TestEventPattern",
        "TagResource",
        "UntagResource",
        "ListTagsForResource",
        "PutPermission",
        "RemovePermission",
        "CreateArchive",
        "DescribeArchive",
        "ListArchives",
        "UpdateArchive",
        "DeleteArchive",
        "StartReplay",
        "DescribeReplay",
        "ListReplays",
        "CreateConnection",
        "UpdateConnection",
        "DeauthorizeConnection",
        "DeleteConnection",
        "DescribeConnection",
        "ListConnections",
        "CreateApiDestination",
        "UpdateApiDestination",
        "DeleteApiDestination",
        "DescribeApiDestination",
        "ListApiDestinations",
    ] {
        assert!(w.served(op), "{op} is built");
    }
    for op in [
        "ActivateEventSource",
        "DeactivateEventSource",
        "DescribeEventSource",
        "ListEventSources",
        "CreatePartnerEventSource",
        "DeletePartnerEventSource",
        "DescribePartnerEventSource",
        "ListPartnerEventSources",
        "ListPartnerEventSourceAccounts",
        "PutPartnerEvents",
        "CreateEndpoint",
        "DeleteEndpoint",
        "DescribeEndpoint",
        "ListEndpoints",
        "UpdateEndpoint",
        "CancelReplay",
    ] {
        assert!(!w.served(op), "{op} is refused by name");
    }
}

fn asked(w: &World, op: &str, request: &Value) -> Vec<Need> {
    let ix = w.h.op(op);
    let call = w.h.call(ix, request).unwrap();
    w.core.needs(&call)
}

#[test]
fn what_each_request_asks_the_authorizer() {
    let mut w = World::new();
    w.bus("shop");
    w.ok("PutPermission", &json!({"EventBusName": "shop", "StatementId": "s", "Action": "events:PutEvents", "Principal": "111122223333"}));
    let needs = asked(
        &w,
        "PutEvents",
        &json!({"Entries": [entry(&json!({"EventBusName": "shop"})), entry(&json!({"EventBusName": "shop"})), entry(&json!({}))]}),
    );
    assert_eq!(needs.len(), 2, "one for each distinct bus");
    let shop = needs
        .iter()
        .find(|n| n.resource.ends_with("/shop"))
        .unwrap();
    assert_eq!(shop.action, "events:PutEvents");
    assert!(
        shop.policy.as_deref().unwrap().contains("\"Sid\":\"s\""),
        "the bus's policy comes with it"
    );
    let rule = asked(
        &w,
        "PutTargets",
        &json!({"Rule": "r", "EventBusName": "shop", "Targets": [{"Id": "t", "Arn": "arn:aws:sqs:us-east-1:000000000000:q"}]}),
    );
    assert_eq!(rule[0].action, "events:PutTargets");
    assert_eq!(
        rule[0].resource,
        "arn:aws:events:us-east-1:000000000000:rule/shop/r"
    );
    let default_rule = asked(&w, "DescribeRule", &json!({"Name": "r"}));
    assert_eq!(
        default_rule[0].resource,
        "arn:aws:events:us-east-1:000000000000:rule/r"
    );
    let tagged = asked(
        &w,
        "PutRule",
        &json!({"Name": "r", "EventPattern": "{\"a\":[1]}", "Tags": [{"Key": "k", "Value": "v"}]}),
    );
    assert_eq!(
        tagged.iter().map(|n| n.action.as_str()).collect::<Vec<_>>(),
        ["events:PutRule", "events:TagResource"]
    );
    assert_eq!(asked(&w, "ListRules", &json!({}))[0].resource, "*");
}

#[test]
fn a_request_the_authorizer_refuses_does_nothing() {
    use aws_peers::Authorizer;
    use aws_wire::ApiError;
    use aws_wire::service::{BoxFuture, Caller};

    struct Deny;
    impl Authorizer for Deny {
        fn authorize<'a>(
            &'a self,
            _: &'a Caller,
            needs: Vec<Need>,
        ) -> BoxFuture<'a, Result<(), ApiError>> {
            Box::pin(async move {
                Err(ApiError::new(
                    400,
                    "AccessDeniedException",
                    format!("not allowed: {}", needs[0].action),
                ))
            })
        }
    }
    let mut w = World::new();
    w.peers.set_authorizer(std::sync::Arc::new(Deny));
    let refused = w
        .handled("CreateEventBus", &json!({"Name": "shop"}))
        .unwrap_err();
    assert_eq!(
        (refused.code.as_ref(), refused.message.as_str()),
        (
            "AccessDeniedException",
            "not allowed: events:CreateEventBus"
        )
    );
    assert!(w.core.buses.get("000000000000/shop").is_none());
    let _ = caller();
}
