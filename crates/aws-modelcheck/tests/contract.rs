//! The decoder's contract in one table: what each body must do against the generated SQS and DynamoDB tables.
//! Ported from spike S1b's `pk-bench/tests/decode.rs`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]

use aws_model_dynamodb::keys::{AttributeValue as A, PutItemInput as P};
use aws_model_sqs::keys::SendMessageRequest as S;
use aws_schema::arena::Arena;
use aws_schema::json::{Violation, decode};
use aws_schema::patterns::Patterns;
use aws_schema::view::{Shape, View};

/// The kind of a violation, without the member or position it carries.
fn kind(v: &Violation) -> &'static str {
    match v {
        Violation::Syntax { .. } => "Syntax",
        Violation::TooDeep => "TooDeep",
        Violation::Type { .. } => "Type",
        Violation::Missing { .. } => "Missing",
        Violation::Length { .. } => "Length",
        Violation::Range { .. } => "Range",
        Violation::Pattern { .. } => "Pattern",
        Violation::Enum { .. } => "Enum",
        Violation::Union { .. } => "Union",
        _ => "Table",
    }
}

#[test]
fn sqs_send_message_bodies() {
    let t = &aws_model_sqs::tables::TABLES;
    let pats = Patterns::new(t);
    let mut ar = Arena::new();
    let cases: &[(&str, &str, Option<&str>)] = &[
        (
            "valid",
            r#"{"QueueUrl":"u","MessageBody":"hi","DelaySeconds":3}"#,
            None,
        ),
        (
            "required member missing",
            r#"{"MessageBody":"hi"}"#,
            Some("Missing"),
        ),
        (
            "integer where a string belongs",
            r#"{"QueueUrl":1,"MessageBody":"x"}"#,
            Some("Type"),
        ),
        (
            "string where an integer belongs",
            r#"{"QueueUrl":"u","MessageBody":"x","DelaySeconds":"3"}"#,
            Some("Type"),
        ),
        (
            "unknown member is ignored",
            r#"{"QueueUrl":"u","MessageBody":"x","Nope":[1,{"a":null}]}"#,
            None,
        ),
        (
            "null is absent",
            r#"{"QueueUrl":"u","MessageBody":"x","DelaySeconds":null}"#,
            None,
        ),
        (
            "escaped string",
            r#"{"QueueUrl":"ué\n","MessageBody":"x"}"#,
            None,
        ),
        (
            "truncated body",
            r#"{"QueueUrl":"u","MessageBody":"#,
            Some("Syntax"),
        ),
        (
            "trailing bytes",
            r#"{"QueueUrl":"u","MessageBody":"x"} x"#,
            Some("Syntax"),
        ),
    ];
    for (what, body, want) in cases {
        ar.clear();
        let got = decode(t, &pats, S::INDEX, body, &mut ar);
        assert_eq!(got.as_ref().err().map(kind), *want, "{what}: {got:?}");
    }

    // The values come back through the typed view: escapes decoded, everything else borrowed from the body.
    ar.clear();
    let body = r#"{"QueueUrl":"aé","MessageBody":"plain","DelaySeconds":7}"#;
    let root = decode(t, &pats, S::INDEX, body, &mut ar).unwrap();
    let v = View::<S>::root(&ar, body, root).unwrap();
    assert_eq!(v.get(S::QUEUE_URL), Some("aé"));
    assert_eq!(v.get(S::MESSAGE_BODY), Some("plain"));
    assert_eq!(v.get(S::DELAY_SECONDS), Some(7));
}

#[test]
fn dynamodb_nesting_unions_and_depth() {
    let t = &aws_model_dynamodb::tables::TABLES;
    let pats = Patterns::new(t);
    let mut ar = Arena::new();
    let item = r#"{"TableName":"t","Item":{"a":{"S":"x"},"b":{"L":[{"N":"1"},{"M":{"c":{"BOOL":true}}}]}}}"#;
    let root = decode(t, &pats, P::INDEX, item, &mut ar).unwrap();
    let v = View::<P>::root(&ar, item, root).unwrap();
    let a = v.get(P::ITEM).unwrap().find(|(k, _)| *k == "a").unwrap().1;
    assert_eq!(a.get(A::S), Some("x"));

    // A union holds exactly one member.
    ar.clear();
    let two = r#"{"TableName":"t","Item":{"a":{"S":"x","N":"1"}}}"#;
    assert!(matches!(
        decode(t, &pats, P::INDEX, two, &mut ar),
        Err(Violation::Union { .. })
    ));
    // A hostile depth is refused, not recursed into.
    ar.clear();
    let deep = format!(
        "{{\"TableName\":\"t\",\"Item\":{{\"a\":{}}}}}",
        "{\"L\":[".repeat(120) + &"]}".repeat(120)
    );
    assert!(matches!(
        decode(t, &pats, P::INDEX, &deep, &mut ar),
        Err(Violation::TooDeep)
    ));
}

#[test]
fn timestamps_are_checked_for_shape() {
    let cases = [
        ("1700000000", true),
        ("1700000000.25", true),
        ("2026-11-09T10:00:00Z", true),
        ("2026-11-09T10:00:00.5+05:30", true),
        ("Sun, 06 Nov 1994 08:49:37 GMT", true),
        ("?", false),
        ("2026-13-09T10:00:00Z", false),
        ("yesterday", false),
    ];
    for (text, ok) in cases {
        assert_eq!(aws_schema::time::valid(text), ok, "{text}");
    }
}
