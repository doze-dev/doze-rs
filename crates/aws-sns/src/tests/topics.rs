//! Topics: creation and its idempotence, names, kinds, attributes, permissions, tags and lists.

use serde_json::{Value, json};

use super::{World, arn};

#[test]
fn a_topic_is_created_once_and_asking_again_for_the_same_definition_is_the_same_topic() {
    let mut w = World::new();
    let made = w.ok(
        "CreateTopic",
        &json!({"Name": "orders", "Attributes": {"DisplayName": "Orders"}, "Tags": [{"Key": "env", "Value": "dev"}]}),
    );
    assert_eq!(made["TopicArn"], arn("orders"));
    let same = w.ok(
        "CreateTopic",
        &json!({"Name": "orders", "Attributes": {"DisplayName": "Orders"}, "Tags": [{"Key": "env", "Value": "dev"}]}),
    );
    assert_eq!(same, made);
    assert_eq!(w.ok("CreateTopic", &json!({"Name": "orders"})), made);
    // Recorded 2026-11-09.
    assert_eq!(
        w.err(
            "CreateTopic",
            &json!({"Name": "orders", "Attributes": {"DisplayName": "Returns"}})
        ),
        (
            "InvalidParameterException".into(),
            "Invalid parameter: Attributes Reason: Topic already exists with different attributes"
                .into()
        )
    );
    assert_eq!(
        w.refused(
            "CreateTopic",
            &json!({"Name": "orders", "Tags": [{"Key": "env", "Value": "prod"}]})
        ),
        "Invalid parameter: Tags Reason: Topic already exists with different tags"
    );
    let got = w.ok("GetTopicAttributes", &json!({"TopicArn": arn("orders")}));
    assert_eq!(got["Attributes"]["DisplayName"], "Orders");
}

#[test]
fn names_and_the_fifo_rule_are_recorded() {
    let mut w = World::new();
    for name in ["not a topic name!", "", "a.b"] {
        assert_eq!(
            w.refused("CreateTopic", &json!({"Name": name})),
            "Invalid parameter: Topic Name",
            "{name:?}"
        );
    }
    // A `.fifo` name without the attribute, and the attribute without the name: both recorded 2026-11-09.
    assert_eq!(
        w.refused("CreateTopic", &json!({"Name": "q.fifo"})),
        "Invalid parameter: Topic Name"
    );
    assert_eq!(
        w.refused(
            "CreateTopic",
            &json!({"Name": "q", "Attributes": {"FifoTopic": "true"}})
        ),
        "Invalid parameter: Topic Name"
    );
    let long = "a".repeat(257);
    assert!(w.run("CreateTopic", &json!({"Name": long})).is_err());
    w.ok("CreateTopic", &json!({"Name": "a".repeat(256)}));
    let fifo = w.ok(
        "CreateTopic",
        &json!({"Name": "q.fifo", "Attributes": {"FifoTopic": "true", "ContentBasedDeduplication": "true"}}),
    );
    let attrs = w.ok("GetTopicAttributes", &json!({"TopicArn": fifo["TopicArn"]}));
    assert_eq!(attrs["Attributes"]["FifoTopic"], "true");
    assert_eq!(attrs["Attributes"]["ContentBasedDeduplication"], "true");
    // The kind of a topic is fixed when it is made.
    assert_eq!(
        w.refused("SetTopicAttributes", &json!({"TopicArn": fifo["TopicArn"], "AttributeName": "FifoTopic", "AttributeValue": "false"})),
        "Invalid parameter: AttributeName"
    );
}

#[test]
fn a_new_topic_has_the_attributes_and_the_policy_aws_recorded() {
    let mut w = World::new();
    let arn = w.topic("t");
    let got = w.ok("GetTopicAttributes", &json!({"TopicArn": arn}));
    let a = &got["Attributes"];
    assert_eq!(a["DisplayName"], "");
    assert_eq!(
        (
            &a["SubscriptionsConfirmed"],
            &a["SubscriptionsDeleted"],
            &a["SubscriptionsPending"]
        ),
        (&json!("0"), &json!("0"), &json!("0"))
    );
    assert_eq!(a["Owner"], super::ACCOUNT);
    assert_eq!(a["TopicArn"], arn);
    let policy: Value = serde_json::from_str(a["Policy"].as_str().unwrap()).unwrap();
    assert_eq!(policy["Id"], "__default_policy_ID");
    assert_eq!(policy["Statement"][0]["Sid"], "__default_statement_ID");
    assert_eq!(policy["Statement"][0]["Resource"], arn);
    assert_eq!(
        policy["Statement"][0]["Condition"]["StringEquals"]["AWS:SourceOwner"],
        super::ACCOUNT
    );
    let delivery: Value =
        serde_json::from_str(a["EffectiveDeliveryPolicy"].as_str().unwrap()).unwrap();
    assert_eq!(
        delivery["http"]["defaultHealthyRetryPolicy"]["numRetries"],
        3
    );
    assert!(
        a.get("FifoTopic").is_none()
            && a.get("DeliveryPolicy").is_none()
            && a.get("MaximumMessageSize").is_none()
    );
}

#[test]
fn attributes_are_set_checked_and_cleared() {
    let mut w = World::new();
    let arn = w.topic("t");
    let set = |w: &mut World, name: &str, value: &str| {
        w.run(
            "SetTopicAttributes",
            &json!({"TopicArn": arn, "AttributeName": name, "AttributeValue": value}),
        )
    };
    set(&mut w, "DisplayName", "Orders").unwrap();
    set(&mut w, "MaximumMessageSize", "2048").unwrap();
    set(&mut w, "KmsMasterKeyId", "alias/aws/sns").unwrap();
    set(
        &mut w,
        "SQSFailureFeedbackRoleArn",
        "arn:aws:iam::000000000000:role/r",
    )
    .unwrap();
    let got = w.ok("GetTopicAttributes", &json!({"TopicArn": arn}))["Attributes"].clone();
    assert_eq!(
        (
            &got["DisplayName"],
            &got["MaximumMessageSize"],
            &got["KmsMasterKeyId"]
        ),
        (&json!("Orders"), &json!("2048"), &json!("alias/aws/sns"))
    );
    // Recorded 2026-11-09.
    assert_eq!(
        set(&mut w, "NoSuchAttribute", "x").unwrap_err().message,
        "Invalid parameter: AttributeName"
    );
    for (name, bad) in [
        ("MaximumMessageSize", "100"),
        ("MaximumMessageSize", "2000000"),
        ("ContentBasedDeduplication", "true"),
        ("Policy", "not json"),
        ("DeliveryPolicy", "[]"),
        ("TracingConfig", "Sometimes"),
    ] {
        assert!(set(&mut w, name, bad).is_err(), "{name}={bad}");
    }
    set(&mut w, "DisplayName", "").unwrap();
    assert_eq!(
        w.ok("GetTopicAttributes", &json!({"TopicArn": arn}))["Attributes"]["DisplayName"],
        ""
    );
    let missing = w.err("SetTopicAttributes", &json!({"TopicArn": super::arn("absent"), "AttributeName": "DisplayName", "AttributeValue": "x"}));
    assert_eq!(
        missing,
        ("NotFoundException".into(), "Topic does not exist".into())
    );
}

#[test]
fn a_large_message_limit_needs_few_subscriptions_of_the_kinds_that_carry_it() {
    let mut w = World::new();
    let arn = w.topic("t");
    w.ok(
        "Subscribe",
        &json!({"TopicArn": arn, "Protocol": "http", "Endpoint": "http://localhost:3000/hook"}),
    );
    let set = json!({"TopicArn": arn, "AttributeName": "MaximumMessageSize", "AttributeValue": "1048576"});
    assert!(
        w.refused("SetTopicAttributes", &set)
            .contains("100 or fewer subscriptions")
    );
}

#[test]
fn a_delivery_policy_changes_the_effective_one() {
    let mut w = World::new();
    let arn = w.topic("t");
    w.ok(
        "SetTopicAttributes",
        &json!({"TopicArn": arn, "AttributeName": "DeliveryPolicy",
        "AttributeValue": r#"{"http":{"defaultHealthyRetryPolicy":{"numRetries":9}}}"#}),
    );
    let got = w.ok("GetTopicAttributes", &json!({"TopicArn": arn}))["Attributes"].clone();
    let eff: Value =
        serde_json::from_str(got["EffectiveDeliveryPolicy"].as_str().unwrap()).unwrap();
    assert_eq!(eff["http"]["defaultHealthyRetryPolicy"]["numRetries"], 9);
    assert_eq!(
        eff["http"]["defaultHealthyRetryPolicy"]["backoffFunction"],
        "linear"
    );
    assert!(
        got["DeliveryPolicy"]
            .as_str()
            .unwrap()
            .contains("numRetries")
    );
}

#[test]
fn deleting_a_topic_twice_is_fine_and_takes_its_subscriptions() {
    let mut w = World::new();
    let arn = w.topic("t");
    w.ok(
        "Subscribe",
        &json!({"TopicArn": arn, "Protocol": "email", "Endpoint": "a@example.com"}),
    );
    // Recorded 2026-11-09: deleting what is not there answers 200.
    w.ok("DeleteTopic", &json!({"TopicArn": arn}));
    w.ok("DeleteTopic", &json!({"TopicArn": arn}));
    assert_eq!(w.core.subs.len(), 0);
    let recreated = w.topic("t");
    let counts = w.ok("GetTopicAttributes", &json!({"TopicArn": recreated}));
    assert_eq!(counts["Attributes"]["SubscriptionsPending"], "0");
}

#[test]
fn permissions_add_and_remove_statements_of_the_policy() {
    let mut w = World::new();
    let arn = w.topic("t");
    w.ok("AddPermission", &json!({"TopicArn": arn, "Label": "friends", "AWSAccountId": ["111122223333", "444455556666"], "ActionName": ["Publish", "Subscribe"]}));
    let policy = |w: &mut World| -> Value {
        let got = w.ok("GetTopicAttributes", &json!({"TopicArn": arn}));
        serde_json::from_str(got["Attributes"]["Policy"].as_str().unwrap()).unwrap()
    };
    let p = policy(&mut w);
    let added = &p["Statement"][1];
    assert_eq!(added["Sid"], "friends");
    assert_eq!(
        added["Principal"]["AWS"],
        json!([
            "arn:aws:iam::111122223333:root",
            "arn:aws:iam::444455556666:root"
        ])
    );
    assert_eq!(added["Action"], json!(["SNS:Publish", "SNS:Subscribe"]));
    assert_eq!(p["Statement"][0]["Sid"], "__default_statement_ID");
    for (request, why) in [
        (
            json!({"TopicArn": arn, "Label": "friends", "AWSAccountId": ["111122223333"], "ActionName": ["Publish"]}),
            "Statement already exists",
        ),
        (
            json!({"TopicArn": arn, "Label": "x", "AWSAccountId": ["nope"], "ActionName": ["Publish"]}),
            "Account ID",
        ),
        (
            json!({"TopicArn": arn, "Label": "x", "AWSAccountId": ["111122223333"], "ActionName": ["Fly"]}),
            "action out of service scope",
        ),
    ] {
        assert!(w.refused("AddPermission", &request).contains(why), "{why}");
    }
    w.ok(
        "RemovePermission",
        &json!({"TopicArn": arn, "Label": "friends"}),
    );
    assert_eq!(policy(&mut w)["Statement"].as_array().unwrap().len(), 1);
    assert!(
        w.refused(
            "RemovePermission",
            &json!({"TopicArn": arn, "Label": "friends"})
        )
        .contains("does not exist")
    );
}

#[test]
fn tags_are_added_replaced_listed_and_removed() {
    let mut w = World::new();
    let arn = w.topic("t");
    w.ok("TagResource", &json!({"ResourceArn": arn, "Tags": [{"Key": "a", "Value": "1"}, {"Key": "b", "Value": "2"}]}));
    w.ok(
        "TagResource",
        &json!({"ResourceArn": arn, "Tags": [{"Key": "a", "Value": "9"}]}),
    );
    let tags = w.ok("ListTagsForResource", &json!({"ResourceArn": arn}));
    assert_eq!(
        tags["Tags"],
        json!([{"Key": "a", "Value": "9"}, {"Key": "b", "Value": "2"}])
    );
    w.ok(
        "UntagResource",
        &json!({"ResourceArn": arn, "TagKeys": ["a", "zzz"]}),
    );
    assert_eq!(
        w.ok("ListTagsForResource", &json!({"ResourceArn": arn}))["Tags"],
        json!([{"Key": "b", "Value": "2"}])
    );
    assert_eq!(
        w.err(
            "ListTagsForResource",
            &json!({"ResourceArn": super::arn("absent")})
        ),
        (
            "ResourceNotFoundException".into(),
            "Resource does not exist".into()
        )
    );
    let many: Vec<Value> = (0..51)
        .map(|i| json!({"Key": format!("k{i}"), "Value": "v"}))
        .collect();
    assert_eq!(
        w.err("TagResource", &json!({"ResourceArn": arn, "Tags": many}))
            .0,
        "TagLimitExceededException"
    );
    assert!(
        w.run(
            "TagResource",
            &json!({"ResourceArn": arn, "Tags": [{"Key": "aws:x", "Value": "v"}]})
        )
        .is_err()
    );
}

#[test]
fn topics_are_listed_a_hundred_to_a_page() {
    let mut w = World::new();
    for i in 0..105 {
        w.topic(&format!("t{i:03}"));
    }
    let first = w.ok("ListTopics", &json!({}));
    assert_eq!(first["Topics"].as_array().unwrap().len(), 100);
    let next = first["NextToken"].as_str().unwrap();
    let second = w.ok("ListTopics", &json!({"NextToken": next}));
    assert_eq!(second["Topics"].as_array().unwrap().len(), 5);
    assert!(second.get("NextToken").is_none());
}

#[test]
fn a_data_protection_policy_is_kept_and_returned() {
    let mut w = World::new();
    let arn = w.topic("t");
    assert!(
        w.ok("GetDataProtectionPolicy", &json!({"ResourceArn": arn}))
            .get("DataProtectionPolicy")
            .is_none()
    );
    let policy = r#"{"Name":"p","Version":"2021-06-01","Statement":[]}"#;
    w.ok(
        "PutDataProtectionPolicy",
        &json!({"ResourceArn": arn, "DataProtectionPolicy": policy}),
    );
    assert_eq!(
        w.ok("GetDataProtectionPolicy", &json!({"ResourceArn": arn}))["DataProtectionPolicy"],
        policy
    );
    assert!(
        w.run(
            "PutDataProtectionPolicy",
            &json!({"ResourceArn": arn, "DataProtectionPolicy": "nope"})
        )
        .is_err()
    );
}
