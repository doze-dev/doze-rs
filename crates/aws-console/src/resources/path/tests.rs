#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;

use serde_json::json;

use super::{Vars, fill, get, shown, tail, text};
use crate::resources::Format;

struct Map(HashMap<&'static str, &'static str>);

impl Vars for Map {
    fn var(&self, name: &str) -> Option<String> {
        self.0.get(name).map(|v| (*v).to_owned())
    }
}

#[test]
fn a_path_reads_members_list_items_and_json_held_in_text() {
    let v = json!({
        "Attributes": {"RedrivePolicy": "{\"deadLetterTargetArn\":\"arn:aws:sqs:us-east-1:000000000000:dlq\",\"maxReceiveCount\":5}"},
        "Table": {"KeySchema": [{"AttributeName": "pk"}, {"AttributeName": "sk"}]},
    });
    assert_eq!(
        text(&v, "Attributes.RedrivePolicy.deadLetterTargetArn").as_deref(),
        Some("arn:aws:sqs:us-east-1:000000000000:dlq")
    );
    assert_eq!(
        text(&v, "Attributes.RedrivePolicy.maxReceiveCount").as_deref(),
        Some("5")
    );
    assert_eq!(
        text(&v, "Table.KeySchema.1.AttributeName").as_deref(),
        Some("sk")
    );
    assert_eq!(text(&v, "Table.KeySchema.2.AttributeName"), None);
    assert_eq!(text(&v, "Nope.Deeper"), None);
    assert_eq!(get(&v, ""), Some(v.clone()));
}

#[test]
fn the_name_is_read_out_of_urls_and_arns_as_live_reads_it() {
    for (value, name) in [
        ("http://127.0.0.1:4566/000000000000/orders", "orders"),
        (
            "http://127.0.0.1:4566/000000000000/orders.fifo?x=1",
            "orders.fifo",
        ),
        ("arn:aws:sns:us-east-1:000000000000:orders", "orders"),
        (
            "arn:aws:lambda:us-east-1:000000000000:function:resize",
            "resize",
        ),
        (
            "arn:aws:lambda:us-east-1:000000000000:function:resize:3",
            "resize",
        ),
        (
            "arn:aws:dynamodb:us-east-1:000000000000:table/orders",
            "orders",
        ),
        (
            "arn:aws:kms:us-east-1:000000000000:key/1234-abcd",
            "1234-abcd",
        ),
        ("arn:aws:s3:::bucket", "bucket"),
        ("arn:short", "arn:short"),
        ("alias/aws/sqs", "alias/aws/sqs"),
        ("plain", "plain"),
    ] {
        assert_eq!(tail(value), name, "{value}");
    }
}

#[test]
fn placeholders_are_filled_typed_and_left_out_when_empty() {
    let vars = Map(HashMap::from([
        ("id", "http://x/000/q"),
        ("form:Name", "orders"),
        ("form:pk", "id"),
        ("form:sk", ""),
        ("form:vt", "45"),
        ("form:fifo", "true"),
        ("account", "000000000000"),
    ]));
    let body = json!({
        "QueueUrl": "{id}",
        "TableName": "{form:Name}",
        "Arn": "arn:aws:sns:us-east-1:{account}:{form:Name}",
        "Attributes": {"VisibilityTimeout": "{form:vt}", "DelaySeconds": "{form:delay}"},
        "KeySchema": [
            {"AttributeName": "{form:pk}", "KeyType": "HASH"},
            {"AttributeName": "{form:sk}", "KeyType": "RANGE"},
        ],
        "Limit": "{num:vt}",
        "Fifo": "{bool:fifo}",
        "Gone": "{form:missing}",
    });
    assert_eq!(
        fill(&body, &vars),
        json!({
            "QueueUrl": "http://x/000/q",
            "TableName": "orders",
            "Arn": "arn:aws:sns:us-east-1:000000000000:orders",
            "Attributes": {"VisibilityTimeout": "45"},
            "KeySchema": [{"AttributeName": "id", "KeyType": "HASH"}],
            "Limit": 45,
            "Fifo": true,
        })
    );
}

#[test]
fn a_value_is_shown_as_its_format_asks() {
    let v = json!({"Attributes": {"FifoQueue": "true", "Policy": "{\"Version\":\"2012-10-17\"}"}, "Arn": "arn:aws:sqs:r:a:q"});
    assert_eq!(
        shown(&v, "Attributes.FifoQueue", Format::Flag("FIFO", "Standard")).as_deref(),
        Some("FIFO")
    );
    assert_eq!(
        shown(&v, "Attributes.Nope", Format::Flag("FIFO", "Standard")).as_deref(),
        Some("Standard")
    );
    assert_eq!(shown(&v, "Arn", Format::Tail).as_deref(), Some("q"));
    assert_eq!(
        shown(&v, "Attributes.Policy", Format::Json).as_deref(),
        Some("{\n  \"Version\": \"2012-10-17\"\n}")
    );
    assert_eq!(shown(&v, "Nope", Format::Text), None);
}
