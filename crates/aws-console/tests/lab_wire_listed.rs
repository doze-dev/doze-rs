//! A Fail rule with "an error I choose": the error is one the operation's model lists, and each protocol words it as the model does (the
//! model's status, the model's code, and for SQS's JSON the Query-compatible code), never as an invented code.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod lab_wire;

use aws_console::lab::listed::{Model, common, find};
use aws_console::lab::{Effect, Fail};
use lab_wire::{rule, send, start};

const SQS: Model = (&aws_model_sqs::meta::META, &aws_model_sqs::tables::TABLES);
const SNS: Model = (&aws_model_sns::meta::META, &aws_model_sns::tables::TABLES);
const S3: Model = (&aws_model_s3::meta::META, &aws_model_s3::tables::TABLES);

/// What a rule that fails with the error `code` of `operation` is, made from the model as the API makes it.
fn chosen(model: Model, operation: &str, code: &str) -> Fail {
    let list = common(&[model], &[operation]);
    let found = find(&list, code).unwrap_or_else(|| panic!("{operation} lists no {code}"));
    Fail::Custom {
        status: found.status,
        code: found.code.into(),
        shape: Some(found.shape),
    }
}

#[tokio::test]
async fn an_sqs_error_the_model_lists_is_answered_with_the_models_status_and_its_query_code() {
    let w = start("listed-sqs").await;
    let fail = chosen(SQS, "SendMessage", "InvalidMessageContents");
    w.table
        .add(rule("sqs", "SendMessage", Effect::Fail(fail)))
        .unwrap();
    let a = send(
        w.addr,
        "sqs",
        ("POST", "/"),
        &[
            ("X-Amz-Target", "AmazonSQS.SendMessage"),
            ("Content-Type", "application/x-amz-json-1.0"),
        ],
        br#"{"QueueUrl":"http://localhost/000000000000/orders","MessageBody":"hi"}"#,
    )
    .await;
    assert_eq!(a.status, Some(400), "{}", a.text());
    let body: serde_json::Value = serde_json::from_slice(&a.body).unwrap();
    assert!(
        body["__type"]
            .as_str()
            .unwrap()
            .ends_with("#InvalidMessageContents"),
        "{body}"
    );
    assert!(
        a.head
            .contains("x-amzn-query-error: invalidmessagecontents;sender"),
        "the query-compatible code of the model: {}",
        a.head
    );
    w.stop().await;
}

#[tokio::test]
async fn an_sns_error_the_model_lists_is_answered_in_query_xml_with_its_query_code() {
    let w = start("listed-sns").await;
    let fail = chosen(SNS, "Publish", "ParameterValueInvalid");
    w.table
        .add(rule("sns", "Publish", Effect::Fail(fail)))
        .unwrap();
    let a = send(
        w.addr,
        "sns",
        ("POST", "/"),
        &[("Content-Type", "application/x-www-form-urlencoded")],
        b"Action=Publish&TopicArn=arn%3Aaws%3Asns%3Aus-east-1%3A000000000000%3At&Message=hi&Version=2010-03-31",
    )
    .await;
    assert_eq!(a.status, Some(400), "{}", a.text());
    let xml = a.text();
    assert!(xml.contains("<Code>ParameterValueInvalid</Code>"), "{xml}");
    assert!(xml.contains("<Type>Sender</Type>"), "{xml}");
    w.stop().await;
}

#[tokio::test]
async fn an_s3_error_the_model_lists_is_answered_in_s3s_xml_with_the_models_404() {
    let w = start("listed-s3").await;
    assert_eq!(
        send(w.addr, "s3", ("PUT", "/orders"), &[], b"").await.status,
        Some(200)
    );
    let fail = chosen(S3, "GetObject", "NoSuchKey");
    w.table
        .add(rule("s3", "GetObject", Effect::Fail(fail)))
        .unwrap();
    // The object is not there either way; the rule is what answers, so the answer carries the rule's mark in its words.
    let a = send(w.addr, "s3", ("GET", "/orders/a.txt"), &[], b"").await;
    assert_eq!(a.status, Some(404), "{}", a.text());
    let xml = a.text();
    assert!(xml.contains("<Code>NoSuchKey</Code>"), "{xml}");
    assert!(xml.contains("made by doze-aws Lab rule"), "{xml}");
    assert!(a.head.contains("server: amazons3"), "{}", a.head);
    w.stop().await;
}
