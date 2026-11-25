//! The Lab's rules through the REST JSON path (Lambda): the model's own throttle with its type in the header SDKs read it from, and a drop.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod lab_wire;

use std::net::SocketAddr;

use aws_console::lab::{Effect, Fail};
use aws_console::ring::LabDid;
use lab_wire::{Answer, THROTTLED, lab_of, rule, send, start, tagged};

// ---- REST JSON: Lambda ------------------------------------------------------------------------------------------------------

async fn lambda(addr: SocketAddr, method: &str, path: &str) -> Answer {
    send(addr, "lambda", (method, path), &[], b"").await
}

#[tokio::test]
async fn a_rest_json_service_words_a_failure_with_its_error_type_header_and_its_own_status() {
    let w = start("restjson").await;
    let listed = lambda(w.addr, "GET", "/2015-03-31/functions").await;
    assert_eq!(listed.status, Some(200), "{}", listed.text());
    let id = w
        .table
        .add(rule(
            "lambda",
            "ListFunctions",
            Effect::Fail(Fail::Throttle),
        ))
        .unwrap();
    // Lambda's own throttle: the model's `TooManyRequestsException`, 429, with its type in the header SDKs read it from.
    let a = lambda(w.addr, "GET", "/2015-03-31/functions").await;
    assert_eq!(a.status, Some(429), "{}", a.text());
    assert!(
        a.head
            .contains("x-amzn-errortype: toomanyrequestsexception"),
        "{}",
        a.head
    );
    let body: serde_json::Value = serde_json::from_slice(&a.body).unwrap();
    assert!(
        body["Message"]
            .as_str()
            .unwrap()
            .starts_with("Rate exceeded."),
        "{body}"
    );
    assert!(THROTTLED.contains(&"TooManyRequestsException"));
    assert_eq!(lab_of(&tagged(&w, id)), Some((id, LabDid::Failed)));
    w.table.remove(id);
    // Dropped, it is no answer; served again once the rule is gone.
    let id = w
        .table
        .add(rule("lambda", "ListFunctions", Effect::Drop))
        .unwrap();
    assert_eq!(
        lambda(w.addr, "GET", "/2015-03-31/functions").await.status,
        None
    );
    w.table.remove(id);
    assert_eq!(
        lambda(w.addr, "GET", "/2015-03-31/functions").await.status,
        Some(200)
    );
    w.stop().await;
}
