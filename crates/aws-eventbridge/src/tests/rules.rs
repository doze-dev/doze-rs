//! Rules and targets: the recorded answers (2026-11-09) and the rules around them.

use serde_json::json;

use super::World;

const RULE: &str = "arn:aws:events:us-east-1:000000000000:rule/shop/orders";

#[test]
fn a_rule_is_put_described_listed_disabled_and_deleted_as_aws_recorded() {
    let mut w = World::new();
    w.bus("shop");
    let pattern = r#"{"source": ["shop.orders"], "detail-type": ["placed"]}"#;
    let put = w.ok("PutRule", &json!({"Name": "orders", "EventBusName": "shop", "EventPattern": pattern, "Description": "orders"}));
    assert_eq!(put, json!({"RuleArn": RULE}));
    let d = w.ok(
        "DescribeRule",
        &json!({"Name": "orders", "EventBusName": "shop"}),
    );
    assert_eq!(
        d,
        json!({
            "Arn": RULE, "CreatedBy": "000000000000", "Description": "orders", "EventBusName": "shop",
            "EventPattern": pattern, "Name": "orders", "State": "ENABLED",
        })
    );
    let listed = w.ok("ListRules", &json!({"EventBusName": "shop"}));
    assert_eq!(
        listed["Rules"][0],
        json!({
            "Arn": RULE, "Description": "orders", "EventBusName": "shop", "EventPattern": pattern, "Name": "orders", "State": "ENABLED",
        })
    );
    w.ok(
        "DisableRule",
        &json!({"Name": "orders", "EventBusName": "shop"}),
    );
    assert_eq!(
        w.ok(
            "DescribeRule",
            &json!({"Name": "orders", "EventBusName": "shop"})
        )["State"],
        "DISABLED"
    );
    w.ok(
        "EnableRule",
        &json!({"Name": "orders", "EventBusName": "shop"}),
    );
    assert_eq!(
        w.ok(
            "DeleteRule",
            &json!({"Name": "orders", "EventBusName": "shop"})
        ),
        json!({})
    );
    assert_eq!(
        w.err(
            "DescribeRule",
            &json!({"Name": "orders", "EventBusName": "shop"})
        ),
        (
            "ResourceNotFoundException".into(),
            "Rule orders does not exist on EventBus shop.".into()
        )
    );
}

#[test]
fn a_rule_on_the_default_bus_has_no_bus_in_its_arn() {
    let mut w = World::new();
    let arn = w.ok(
        "PutRule",
        &json!({"Name": "r", "EventPattern": "{\"source\":[\"x\"]}"}),
    )["RuleArn"]
        .clone();
    assert_eq!(arn, "arn:aws:events:us-east-1:000000000000:rule/r");
    assert_eq!(
        w.ok("DescribeRule", &json!({"Name": "r"}))["EventBusName"],
        "default"
    );
}

#[test]
fn the_wording_of_the_recorded_rule_refusals() {
    let mut w = World::new();
    w.bus("shop");
    let put = |p: serde_json::Value| {
        let mut r = json!({"Name": "r", "EventBusName": "shop"});
        for (k, v) in p.as_object().unwrap() {
            r[k] = v.clone();
        }
        r
    };
    assert_eq!(
        w.err("PutRule", &put(json!({}))),
        (
            "ValidationException".into(),
            "Parameter(s) EventPattern or ScheduleExpression must be specified.".into()
        )
    );
    assert_eq!(
        w.err("PutRule", &put(json!({"EventPattern": "{not json"}))),
        ("InvalidEventPatternException".into(),
         "Event pattern is not valid. Reason: Unexpected character ('n' (code 110)): was expecting double-quote to start field name\n at [Source: (String)\"{not json\"; line: 1, column: 2]".into())
    );
    assert_eq!(
        w.err("PutRule", &put(json!({"EventPattern": "{\"source\": \"x\"}"}))),
        ("InvalidEventPatternException".into(),
         "Event pattern is not valid. Reason: \"source\" must be an object or an array\n at [Source: (String)\"{\"source\": \"x\"}\"; line: 1, column: 13]".into())
    );
    assert_eq!(
        w.err(
            "PutRule",
            &put(json!({"ScheduleExpression": "rate(5 minutes)"}))
        ),
        (
            "ValidationException".into(),
            "ScheduleExpression is supported only on the default event bus.".into()
        )
    );
    for bad in [
        "rate(1 minutes)",
        "rate(5 minute)",
        "cron(0 0 * * * *)",
        "every day",
    ] {
        assert_eq!(
            w.err("PutRule", &json!({"Name": "r", "ScheduleExpression": bad})),
            (
                "ValidationException".into(),
                "Parameter ScheduleExpression is not valid.".into()
            ),
            "{bad}"
        );
    }
    assert_eq!(
        w.err("PutTargets", &json!({"Rule": "absent-rule", "EventBusName": "shop", "Targets": [{"Id": "t", "Arn": "arn:aws:sqs:us-east-1:000000000000:q"}]})),
        ("ResourceNotFoundException".into(), "Rule absent-rule does not exist on EventBus shop.".into())
    );
}

#[test]
fn targets_are_put_listed_replaced_and_removed_as_aws_recorded() {
    let mut w = World::new();
    w.bus("shop");
    w.rule("shop", "orders", &json!({"source": ["x"]}));
    let q = "arn:aws:sqs:us-east-1:000000000000:q";
    let put = w.ok(
        "PutTargets",
        &json!({"Rule": "orders", "EventBusName": "shop", "Targets": [{"Id": "queue", "Arn": q}]}),
    );
    assert_eq!(put, json!({"FailedEntries": [], "FailedEntryCount": 0}));
    let listed = w.ok(
        "ListTargetsByRule",
        &json!({"Rule": "orders", "EventBusName": "shop"}),
    );
    assert_eq!(listed["Targets"], json!([{"Id": "queue", "Arn": q}]));
    // The same id replaces the target; every parameter block comes back as it was sent.
    let full = json!({"Id": "queue", "Arn": q, "RoleArn": "arn:aws:iam::000000000000:role/r", "Input": "{\"a\":1}",
        "SqsParameters": {"MessageGroupId": "g"}, "RetryPolicy": {"MaximumRetryAttempts": 3, "MaximumEventAgeInSeconds": 120},
        "DeadLetterConfig": {"Arn": "arn:aws:sqs:us-east-1:000000000000:dlq"}});
    w.ok(
        "PutTargets",
        &json!({"Rule": "orders", "EventBusName": "shop", "Targets": [full]}),
    );
    let listed = w.ok(
        "ListTargetsByRule",
        &json!({"Rule": "orders", "EventBusName": "shop"}),
    );
    assert_eq!(listed["Targets"], json!([full]));
    assert_eq!(
        w.ok(
            "ListRuleNamesByTarget",
            &json!({"TargetArn": q, "EventBusName": "shop"})
        )["RuleNames"],
        json!(["orders"])
    );
    // A rule with targets cannot be deleted (recorded).
    assert_eq!(
        w.err(
            "DeleteRule",
            &json!({"Name": "orders", "EventBusName": "shop"})
        ),
        (
            "ValidationException".into(),
            "Rule can't be deleted since it has targets.".into()
        )
    );
    // An id that is not there is not a failure (recorded).
    let removed = w.ok(
        "RemoveTargets",
        &json!({"Rule": "orders", "EventBusName": "shop", "Ids": ["queue", "absent"]}),
    );
    assert_eq!(removed, json!({"FailedEntries": [], "FailedEntryCount": 0}));
    w.ok(
        "DeleteRule",
        &json!({"Name": "orders", "EventBusName": "shop"}),
    );
}

#[test]
fn a_rule_holds_five_targets_and_a_target_with_two_input_settings_fails_alone() {
    let mut w = World::new();
    w.rule("default", "r", &json!({"source": ["x"]}));
    let target =
        |i: usize| json!({"Id": format!("t{i}"), "Arn": "arn:aws:sqs:us-east-1:000000000000:q"});
    let five: Vec<_> = (0..5).map(target).collect();
    w.ok("PutTargets", &json!({"Rule": "r", "Targets": five}));
    assert_eq!(
        w.err("PutTargets", &json!({"Rule": "r", "Targets": [target(5)]}))
            .0,
        "LimitExceededException"
    );
    let both = json!({"Id": "t0", "Arn": "arn:aws:sqs:us-east-1:000000000000:q", "Input": "{}", "InputPath": "$.detail"});
    let answer = w.ok(
        "PutTargets",
        &json!({"Rule": "r", "Targets": [both, target(1)]}),
    );
    assert_eq!(answer["FailedEntryCount"], 1);
    assert_eq!(answer["FailedEntries"][0]["TargetId"], "t0");
}

#[test]
fn put_rule_again_replaces_the_settings_and_keeps_targets_and_tags() {
    let mut w = World::new();
    w.rule("default", "r", &json!({"source": ["x"]}));
    let arn = "arn:aws:events:us-east-1:000000000000:rule/r";
    w.ok(
        "TagResource",
        &json!({"ResourceARN": arn, "Tags": [{"Key": "k", "Value": "v"}]}),
    );
    w.ok("PutTargets", &json!({"Rule": "r", "Targets": [{"Id": "t", "Arn": "arn:aws:sqs:us-east-1:000000000000:q"}]}));
    w.ok(
        "PutRule",
        &json!({"Name": "r", "EventPattern": "{\"source\":[\"y\"]}", "State": "DISABLED"}),
    );
    let d = w.ok("DescribeRule", &json!({"Name": "r"}));
    assert_eq!(d["State"], "DISABLED");
    assert_eq!(d["EventPattern"], "{\"source\":[\"y\"]}");
    assert_eq!(
        w.ok("ListTargetsByRule", &json!({"Rule": "r"}))["Targets"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        w.ok("ListTagsForResource", &json!({"ResourceARN": arn}))["Tags"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn test_event_pattern_needs_a_whole_event() {
    let mut w = World::new();
    let event = json!({"id": "1", "account": "1", "source": "s", "time": "2026-01-01T00:00:00Z", "region": "us-east-1", "resources": [], "detail-type": "d", "detail": {}});
    let ask = |w: &mut World, pattern: &str| {
        w.ok(
            "TestEventPattern",
            &json!({"Event": event.to_string(), "EventPattern": pattern}),
        )["Result"]
            .clone()
    };
    assert_eq!(ask(&mut w, "{\"source\":[\"s\"]}"), true);
    assert_eq!(ask(&mut w, "{\"source\":[\"t\"]}"), false);
    assert_eq!(
        w.err(
            "TestEventPattern",
            &json!({"Event": "{}", "EventPattern": "{\"source\":[\"s\"]}"})
        )
        .0,
        "ValidationException"
    );
    assert_eq!(
        w.err(
            "TestEventPattern",
            &json!({"Event": event.to_string(), "EventPattern": "{\"a\":[]}"})
        )
        .0,
        "InvalidEventPatternException"
    );
}
