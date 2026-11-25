//! The Lab's rules through the Query path (SNS): the model's own code in Query XML, a drop, a slow call, and each kind of failure worded its own way.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod lab_wire;

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use aws_console::lab::{Effect, Fail};
use lab_wire::{Answer, rule, send, start};

// ---- Query: SNS ---------------------------------------------------------------------------------------------------------------

async fn sns(addr: SocketAddr, form: &str) -> Answer {
    send(
        addr,
        "sns",
        ("POST", "/"),
        &[("Content-Type", "application/x-www-form-urlencoded")],
        form.as_bytes(),
    )
    .await
}

const CREATE_TOPIC: &str = "Action=CreateTopic&Name=order-events&Version=2010-03-31";

async fn topics(addr: SocketAddr) -> String {
    sns(addr, "Action=ListTopics&Version=2010-03-31")
        .await
        .text()
}

#[tokio::test]
async fn a_query_fail_answers_in_xml_with_the_models_code_and_writes_nothing() {
    let w = start("query-fail").await;
    let id = w
        .table
        .add(rule("sns", "CreateTopic", Effect::Fail(Fail::Throttle)))
        .unwrap();
    let a = sns(w.addr, CREATE_TOPIC).await;
    // SNS's own throttle: the model's `Throttled`, 429, as Query XML.
    assert_eq!(a.status, Some(429), "{}", a.text());
    let xml = a.text();
    assert!(xml.contains("<Code>Throttled</Code>"), "{xml}");
    assert!(xml.contains("<ErrorResponse"), "{xml}");
    assert!(xml.contains("<Type>Sender</Type>"), "{xml}");
    assert!(xml.contains("Rate exceeded."), "{xml}");
    assert!(
        !topics(w.addr).await.contains("order-events"),
        "a failed call writes nothing"
    );
    let failed = w.recorder.touched(Some(id), 5).0;
    assert_eq!(failed.len(), 1);
    assert_eq!((failed[0].status, &*failed[0].code), (429, "Throttled"));
    w.stop().await;
}

#[tokio::test]
async fn a_query_drop_and_a_query_slow_call() {
    let w = start("query-drop-slow").await;
    let id = w
        .table
        .add(rule("sns", "CreateTopic", Effect::Drop))
        .unwrap();
    assert_eq!(sns(w.addr, CREATE_TOPIC).await.status, None);
    assert!(!topics(w.addr).await.contains("order-events"));
    assert!(w.table.remove(id));
    let slow = Effect::Slow {
        delay_ms: 250,
        jitter_ms: 0,
    };
    w.table.add(rule("sns", "CreateTopic", slow)).unwrap();
    let started = Instant::now();
    let a = sns(w.addr, CREATE_TOPIC).await;
    assert_eq!(a.status, Some(200), "{}", a.text());
    assert!(started.elapsed() >= Duration::from_millis(250));
    assert!(topics(w.addr).await.contains("order-events"));
    w.stop().await;
}

#[tokio::test]
async fn a_query_service_words_each_kind_of_failure_its_own_way() {
    let w = start("query-kinds").await;
    for (fail, status, code, who) in [
        (Fail::Internal, 500, "InternalError", "Receiver"),
        (Fail::Unavailable, 503, "ServiceUnavailable", "Receiver"),
        (Fail::AccessDenied, 403, "AuthorizationError", "Sender"),
    ] {
        let id = w
            .table
            .add(rule("sns", "ListTopics", Effect::Fail(fail)))
            .unwrap();
        let a = sns(w.addr, "Action=ListTopics&Version=2010-03-31").await;
        assert_eq!(a.status, Some(status), "{code}");
        assert!(
            a.text().contains(&format!("<Code>{code}</Code>")),
            "{}",
            a.text()
        );
        assert!(
            a.text().contains(&format!("<Type>{who}</Type>")),
            "{}",
            a.text()
        );
        w.table.remove(id);
    }
    w.stop().await;
}
