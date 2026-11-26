//! The Lab's rules through the JSON path (SQS): a fail answers in the protocol's own shape and writes nothing, a drop closes the connection, a slow call is served after its wait, a call no rule fits is untouched; and what an SDK's retries meet.

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
use aws_console::ring::LabDid;
use lab_wire::{Answer, THROTTLED, Wire, lab_of, newest, rule, send, start, tagged};

// ---- JSON: SQS ----------------------------------------------------------------------------------------------------------------

async fn sqs(addr: SocketAddr, op: &str, body: &str) -> Answer {
    send(
        addr,
        "sqs",
        ("POST", "/"),
        &[
            ("X-Amz-Target", &format!("AmazonSQS.{op}")),
            ("Content-Type", "application/x-amz-json-1.0"),
        ],
        body.as_bytes(),
    )
    .await
}

async fn queue_depth(addr: SocketAddr, url: &str) -> String {
    let a = sqs(
        addr,
        "GetQueueAttributes",
        &format!(r#"{{"QueueUrl":"{url}","AttributeNames":["ApproximateNumberOfMessages"]}}"#),
    )
    .await;
    let v: serde_json::Value = serde_json::from_slice(&a.body).unwrap();
    v["Attributes"]["ApproximateNumberOfMessages"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// A world with the queue `orders`, and the body of a `SendMessage` to it.
async fn orders(tag: &str) -> (Wire, String, String) {
    let w = start(tag).await;
    let made = sqs(w.addr, "CreateQueue", r#"{"QueueName":"orders"}"#).await;
    assert_eq!(made.status, Some(200), "{}", made.text());
    let url = serde_json::from_slice::<serde_json::Value>(&made.body).unwrap()["QueueUrl"]
        .as_str()
        .unwrap()
        .to_owned();
    let body = format!(r#"{{"QueueUrl":"{url}","MessageBody":"hi"}}"#);
    (w, url, body)
}

#[tokio::test]
async fn a_json_fail_answers_in_the_protocols_shape_and_writes_nothing() {
    let (w, url, send_body) = orders("json-fail").await;
    // SQS's own throttle, in the JSON protocol with its Query-compatible code, 403 as the model says.
    let id = w
        .table
        .add(rule("sqs", "SendMessage", Effect::Fail(Fail::Throttle)))
        .unwrap();
    let a = sqs(w.addr, "SendMessage", &send_body).await;
    assert_eq!(a.status, Some(403), "{}", a.text());
    assert!(
        a.head
            .contains("x-amzn-query-error: requestthrottled;sender"),
        "{}",
        a.head
    );
    let body: serde_json::Value = serde_json::from_slice(&a.body).unwrap();
    assert!(
        body["__type"]
            .as_str()
            .unwrap()
            .ends_with("#RequestThrottled"),
        "{body}"
    );
    assert!(THROTTLED.contains(&"RequestThrottled"));
    assert_eq!(
        queue_depth(w.addr, &url).await,
        "0",
        "a failed call writes nothing"
    );
    let row = tagged(&w, id);
    assert_eq!((row.status, &*row.code), (403, "RequestThrottled"));
    assert_eq!(lab_of(&row), Some((id, LabDid::Failed)));
    // Other operations are not touched.
    assert_eq!(sqs(w.addr, "ListQueues", "{}").await.status, Some(200));
    assert_eq!(lab_of(&newest(&w)), None);
    w.stop().await;
}

#[tokio::test]
async fn a_json_drop_closes_the_connection_with_no_answer_and_writes_nothing() {
    let (w, url, send_body) = orders("json-drop").await;
    let id = w
        .table
        .add(rule("sqs", "SendMessage", Effect::Drop))
        .unwrap();
    let a = sqs(w.addr, "SendMessage", &send_body).await;
    assert_eq!(a.status, None, "no answer at all");
    assert_eq!(queue_depth(w.addr, &url).await, "0");
    let row = tagged(&w, id);
    assert_eq!(row.status, 0, "a dropped call has no status");
    assert_eq!(lab_of(&row), Some((id, LabDid::Dropped)));
    w.stop().await;
}

#[tokio::test]
async fn a_json_slow_call_is_served_after_its_wait_and_its_row_says_it_waited() {
    let (w, url, send_body) = orders("json-slow").await;
    let slow = Effect::Slow {
        delay_ms: 300,
        jitter_ms: 0,
    };
    let id = w.table.add(rule("sqs", "SendMessage", slow)).unwrap();
    let started = Instant::now();
    let a = sqs(w.addr, "SendMessage", &send_body).await;
    assert_eq!(a.status, Some(200), "{}", a.text());
    assert!(started.elapsed() >= Duration::from_millis(300));
    assert_eq!(
        queue_depth(w.addr, &url).await,
        "1",
        "a slowed call is served"
    );
    let row = tagged(&w, id);
    assert_eq!(lab_of(&row), Some((id, LabDid::Delayed(300))));
    assert!(
        row.took_us >= 300_000,
        "its time includes the wait: {}",
        row.took_us
    );
    w.stop().await;
}

#[tokio::test]
async fn a_json_call_no_rule_fits_is_untouched_with_other_rules_armed_and_with_none() {
    let (w, url, send_body) = orders("json-pass").await;
    w.table
        .add(rule("kms", "Encrypt", Effect::Fail(Fail::Internal)))
        .unwrap();
    for _ in 0..2 {
        let a = sqs(w.addr, "SendMessage", &send_body).await;
        assert_eq!(a.status, Some(200));
        assert_eq!(lab_of(&newest(&w)), None);
        w.table.stop_all();
    }
    assert_eq!(queue_depth(w.addr, &url).await, "2");
    w.stop().await;
}

// ---- Retries ------------------------------------------------------------------------------------------------------------------

/// What an SDK reads from an answer: the error code.
fn code_of(a: &Answer) -> String {
    if let Some(q) = a
        .head
        .lines()
        .find_map(|l| l.strip_prefix("x-amzn-query-error: "))
    {
        let code = q.split(';').next().unwrap_or(q);
        // The head is lower-cased: find the code's own case in the body.
        let body = a.text();
        return body
            .split(['"', '#'])
            .find(|w| w.eq_ignore_ascii_case(code))
            .unwrap_or(code)
            .to_owned();
    }
    String::new()
}

#[tokio::test]
async fn a_client_that_retries_as_the_sdks_do_gets_through_when_the_rule_has_run_out() {
    let w = start("retry").await;
    let made = sqs(w.addr, "CreateQueue", r#"{"QueueName":"retry"}"#).await;
    let url = serde_json::from_slice::<serde_json::Value>(&made.body).unwrap()["QueueUrl"]
        .as_str()
        .unwrap()
        .to_owned();
    // Two throttled calls, then the rule is spent: the third attempt of an SDK (three in all by default) goes through.
    let mut spec = rule("sqs", "SendMessage", Effect::Fail(Fail::Throttle));
    spec.max_hits = Some(2);
    let id = w.table.add(spec).unwrap();
    let body = format!(r#"{{"QueueUrl":"{url}","MessageBody":"hi"}}"#);
    let mut attempts = 0;
    let answered = loop {
        attempts += 1;
        let a = sqs(w.addr, "SendMessage", &body).await;
        let status = a.status.unwrap();
        let retryable = status == 200
            || THROTTLED.contains(&code_of(&a).as_str())
            || status == 429
            || status >= 500;
        assert!(retryable, "{status} {}", code_of(&a));
        if status == 200 || attempts == 3 {
            break a;
        }
    };
    assert_eq!(attempts, 3);
    assert_eq!(answered.status, Some(200));
    assert_eq!(
        queue_depth(w.addr, &url).await,
        "1",
        "only the call that was served wrote"
    );
    let found = w.table.find(id).unwrap();
    assert_eq!(found.rule.hits, 2);
    assert!(
        found.over.is_some(),
        "the rule ended by itself when it had fired twice"
    );
    // Live holds the three attempts: two tagged, one not.
    let (touched, more) = w.recorder.touched(Some(id), 10);
    assert_eq!((touched.len(), more), (2, false));
    w.stop().await;
}

#[tokio::test]
async fn a_rule_that_names_a_resource_touches_only_what_it_names() {
    let w = start("resource").await;
    for q in ["orders", "payments"] {
        let a = sqs(w.addr, "CreateQueue", &format!(r#"{{"QueueName":"{q}"}}"#)).await;
        assert_eq!(a.status, Some(200));
    }
    let mut spec = rule("sqs", "GetQueueAttributes", Effect::Fail(Fail::Unavailable));
    spec.matcher.resources = vec![aws_console::lab::Pattern::new("pay*").unwrap()];
    w.table.add(spec).unwrap();
    let ask = |q: &str| {
        format!(r#"{{"QueueUrl":"http://127.0.0.1:1/000000000000/{q}","AttributeNames":["All"]}}"#)
    };
    let orders = sqs(w.addr, "GetQueueAttributes", &ask("orders")).await;
    let payments = sqs(w.addr, "GetQueueAttributes", &ask("payments")).await;
    assert_eq!(orders.status, Some(200), "{}", orders.text());
    assert_eq!(payments.status, Some(503), "{}", payments.text());
    assert!(
        payments
            .head
            .contains("x-amzn-query-error: serviceunavailable;receiver"),
        "{}",
        payments.head
    );
    w.stop().await;
}
