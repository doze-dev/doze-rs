#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use serde_json::{Value, json};

use super::{Names, check_target, shape};

fn event() -> Value {
    json!({
        "id": "e1", "source": "shop.orders", "detail-type": "placed",
        "detail": {"customer": "ada", "total": 250, "items": ["book", "pen"], "paid": true, "address": {"city": "Oslo"}}
    })
}

fn names() -> Names<'static> {
    Names {
        rule_arn: "arn:aws:events:us-east-1:1:rule/r",
        rule_name: "r",
        ingestion_time: "2026-11-13T10:00:00Z",
    }
}

fn run(target: &Value) -> Option<String> {
    let e = event();
    shape(target, &e, &e.to_string(), &names())
}

fn transformed(paths: &Value, template: &str) -> String {
    run(&json!({"Id": "t", "Arn": "a", "InputTransformer": {"InputPathsMap": paths, "InputTemplate": template}})).unwrap()
}

#[test]
fn the_recorded_template_comes_out_as_aws_made_it() {
    let paths = json!({"who": "$.detail.customer", "kind": "$.detail-type"});
    assert_eq!(
        transformed(&paths, r#"{"message": "<who> did <kind>"}"#),
        r#"{"message": "ada did placed"}"#
    );
}

#[test]
fn with_no_shaping_the_event_goes_whole() {
    let got = run(&json!({"Id": "t", "Arn": "a"})).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&got).unwrap(), event());
}

#[test]
fn input_is_sent_as_it_was_given() {
    assert_eq!(
        run(&json!({"Id": "t", "Arn": "a", "Input": "{ \"fixed\":  1 }"})).unwrap(),
        "{ \"fixed\":  1 }"
    );
}

#[test]
fn input_path_sends_the_part_it_finds_as_json() {
    let at = |p: &str| run(&json!({"Id": "t", "Arn": "a", "InputPath": p}));
    assert_eq!(at("$.detail.address").unwrap(), r#"{"city":"Oslo"}"#);
    assert_eq!(at("$.detail.customer").unwrap(), "\"ada\"");
    assert_eq!(at("$.detail.items[1]").unwrap(), "\"pen\"");
    assert_eq!(at("$.detail.total").unwrap(), "250");
    assert_eq!(at("$.detail.nothing"), None);
}

#[test]
fn values_that_are_not_text_go_in_as_json() {
    let paths = json!({"n": "$.detail.total", "all": "$.detail.items", "o": "$.detail.address", "p": "$.detail.paid"});
    assert_eq!(
        transformed(
            &paths,
            r#"{"n": <n>, "s": "<n>", "all": <all>, "o": <o>, "p": <p>}"#
        ),
        r#"{"n": 250, "s": "250", "all": ["book","pen"], "o": {"city":"Oslo"}, "p": true}"#
    );
}

#[test]
fn text_is_quoted_outside_a_json_string_and_raw_inside() {
    let paths = json!({"who": "$.detail.customer"});
    assert_eq!(
        transformed(&paths, r#"{"a": <who>, "b": "<who>"}"#),
        r#"{"a": "ada", "b": "ada"}"#
    );
    // A template that is plain text takes the text as it is.
    assert_eq!(transformed(&paths, "hello <who>!"), "hello ada!");
    assert_eq!(transformed(&paths, "\"hello <who>\""), "\"hello ada\"");
}

#[test]
fn a_path_that_finds_nothing_is_empty_in_a_string_and_null_outside_one() {
    let paths = json!({"gone": "$.detail.nothing"});
    assert_eq!(
        transformed(&paths, r#"{"a": <gone>, "b": "<gone>"}"#),
        r#"{"a": null, "b": ""}"#
    );
}

#[test]
fn the_predefined_names_need_no_path() {
    let got = transformed(
        &json!({}),
        r#"{"r": "<aws.events.rule-name>", "arn": "<aws.events.rule-arn>", "t": "<aws.events.event.ingestion-time>", "e": <aws.events.event.json>}"#,
    );
    let v: Value = serde_json::from_str(&got).unwrap();
    assert_eq!(v["r"], "r");
    assert_eq!(v["arn"], "arn:aws:events:us-east-1:1:rule/r");
    assert_eq!(v["t"], "2026-11-13T10:00:00Z");
    assert_eq!(v["e"], event());
}

#[test]
fn angle_brackets_that_are_not_names_stay() {
    let paths = json!({"x": "$.detail.customer"});
    assert_eq!(
        transformed(&paths, "a < b and c > d <x> <not a name> <>"),
        "a < b and c > d ada <not a name> <>"
    );
}

#[test]
fn targets_are_checked_when_they_are_put() {
    let ok = |t: Value| check_target(&t);
    assert!(ok(json!({"Id": "t", "Arn": "a"})).is_ok());
    assert!(ok(json!({"Id": "t", "Arn": "a", "Input": "{}"})).is_ok());
    assert!(ok(json!({"Id": "t", "Arn": "a", "Input": "{not"})).is_err());
    assert!(ok(json!({"Id": "t", "Arn": "a", "InputPath": "$.detail"})).is_ok());
    assert!(ok(json!({"Id": "t", "Arn": "a", "InputPath": "detail"})).is_err());
    assert!(ok(json!({"Id": "t", "Arn": "a", "Input": "{}", "InputPath": "$.a"})).is_err());
    let transformer = |paths: Value, template: &str| json!({"Id": "t", "Arn": "a", "InputTransformer": {"InputPathsMap": paths, "InputTemplate": template}});
    assert!(ok(transformer(json!({"a": "$.x"}), "<a>")).is_ok());
    assert!(ok(transformer(json!({}), "<aws.events.rule-name>")).is_ok());
    let err = ok(transformer(json!({"a": "$.x"}), "<a> <b>")).unwrap_err();
    assert_eq!(
        err,
        "InputTemplate for target t contains invalid placeholder b."
    );
    assert!(ok(transformer(json!({"a": "x"}), "<a>")).is_err());
}
