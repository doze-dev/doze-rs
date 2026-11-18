//! The Lab over a real broker with traffic: a SLOW rule delays a Fetch, a FAIL rule answers Produce with its code and writes nothing, a
//! DROP closes the connection, an expiry runs a member out through the group's own timer path, a rebalance goes through the group
//! machine, Compact now runs the cleaner's real round; every request a rule touched carries its tag on the wire; a caller without
//! the right is refused; hostile names stay text.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    unused_imports
)]

mod support;
mod support_lab;
mod support_log;
mod support_write;

use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use kafka_auth::Principal;
use kafka_broker::{Caller, LabOutcome};
use kafka_console::Recorder;
use kafka_console::gather::lab as glab;
use kafka_console::lab::Lab;
use kafka_console::model::lab::{LabFields, LabKind, LabMade, LabOnceDone};
use kafka_console::model::live::LabDid;
use kafka_protocol::protocol::{Decodable, Encodable};
use kafka_server::fault::FaultTable;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::join_group_request::JoinGroupRequestProtocol;
use kafka_wire::messages::*;
use kafka_wire::testing::{framed, request_payload};
use support_lab::*;
use support_log::{keyed, records};
use support_write::{World, world};

/// a SLOW rule delays the Fetch it matches by its delay and by nothing else; the wire tags the row with the rule and says how
/// much it added; clearing the rule makes the next Fetch fast and untagged.
#[tokio::test]
async fn a_slow_rule_delays_a_fetch_and_the_wire_says_so() {
    let l = lab_world("lab-slow").await;
    let mut c = Raw::connect(&l.w, "slow-client").await;
    let began = Instant::now();
    c.try_call(ApiKey::Fetch, 12, &fetch_req("orders"))
        .await
        .unwrap();
    let plain = began.elapsed();
    let id = make(
        &l,
        LabKind::Slow,
        &[
            ("api", "fetch"),
            ("topic", "orders"),
            ("delay", "300"),
            ("share", "every"),
            ("ends", "5"),
        ],
    )
    .await;
    let began = Instant::now();
    let a = c
        .try_call(ApiKey::Fetch, 12, &fetch_req("orders"))
        .await
        .unwrap();
    let slow = began.elapsed();
    let r: FetchResponse = body(ApiKey::Fetch, 12, a);
    assert_eq!(r.responses[0].partitions[0].error_code, 0);
    assert!(
        slow >= Duration::from_millis(300),
        "{slow:?} vs plain {plain:?}"
    );
    let hits = wire_hits(&l, id);
    assert_eq!(hits.len(), 1);
    let lab = hits[0].lab.clone().unwrap();
    assert_eq!((lab.rule, lab.what), (id, LabDid::Delayed(300)));
    assert_eq!(hits[0].took_sub.as_deref(), Some("includes +300 ms"));
    let v = glab::rule(&l.lab, Some(&l.rec), id).unwrap();
    assert_eq!((v.rule.hits, v.rule.matched), (1, 1));
    assert!(
        v.rule
            .sentence
            .starts_with("+300 ms on every Fetch request to orders"),
        "{}",
        v.rule.sentence
    );
    assert!(glab::remove(&l.lab, id));
    let began = Instant::now();
    c.try_call(ApiKey::Fetch, 12, &fetch_req("orders"))
        .await
        .unwrap();
    assert!(began.elapsed() < Duration::from_millis(250));
    assert_eq!(wire_hits(&l, id).len(), 1, "the later fetch is not tagged");
    let home = glab::home(&l.lab, &l.w.backend).await;
    assert!(home.active.is_empty() && home.ended.len() == 1 && home.ended[0].ended.is_some());
    l.w.stop().await;
}

/// a FAIL rule answers Produce with its code and the partition's offset unset, and the log is as it was.
#[tokio::test]
async fn a_fail_rule_answers_produce_with_its_code_and_writes_nothing() {
    let l = lab_world("lab-fail").await;
    make(
        &l,
        LabKind::Fail,
        &[
            ("api", "produce"),
            ("topic", "orders"),
            ("share", "every"),
            ("code", "7"),
            ("ends", "5"),
        ],
    )
    .await;
    let mut c = Raw::connect(&l.w, "fail-client").await;
    let a = c
        .try_call(ApiKey::Produce, 9, &produce_req("orders"))
        .await
        .unwrap();
    let r: ProduceResponse = body(ApiKey::Produce, 9, a);
    let p = &r.responses[0].partition_responses[0];
    assert_eq!((p.error_code, p.base_offset), (7, -1));
    assert_eq!(l.w.offset("orders", 0, -1).await, 0, "nothing was appended");
    let hits = wire_hits(&l, 1);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].code, 7);
    assert_eq!(hits[0].lab.as_ref().unwrap().what, LabDid::Failed);
    l.w.stop().await;
}

/// Drop once closes what the app has open at its next request and leaves the rest; a new connection is served.
#[tokio::test]
async fn drop_once_closes_the_connections_that_were_open_and_not_the_next_one() {
    let l = lab_world("lab-drop").await;
    let mut old = Raw::connect(&l.w, "catalog").await;
    let mut other = Raw::connect(&l.w, "payments").await;
    old.try_call(ApiKey::Metadata, 12, &MetadataRequest::default())
        .await
        .unwrap();
    other
        .try_call(ApiKey::Metadata, 12, &MetadataRequest::default())
        .await
        .unwrap();
    let form = glab::drop_form(Some(&l.rec), "catalog", "once");
    assert_eq!(
        form.choices
            .iter()
            .map(|c| c.value.as_str())
            .collect::<Vec<_>>(),
        ["catalog", "payments"]
    );
    assert_eq!(
        glab::drop_connections(&l.lab, Some(&l.rec), "catalog", "once"),
        LabOnceDone::Done
    );
    assert!(
        old.try_call(ApiKey::Metadata, 12, &MetadataRequest::default())
            .await
            .is_none()
    );
    assert!(
        other
            .try_call(ApiKey::Metadata, 12, &MetadataRequest::default())
            .await
            .is_some()
    );
    let mut fresh = Raw::connect(&l.w, "catalog").await;
    assert!(
        fresh
            .try_call(ApiKey::Metadata, 12, &MetadataRequest::default())
            .await
            .is_some()
    );
    // An app with nothing open is told so.
    let LabOnceDone::Invalid(f) = glab::drop_connections(&l.lab, Some(&l.rec), "nobody", "once")
    else {
        panic!()
    };
    assert!(f.error.unwrap().contains("no connection open"));
    l.w.stop().await;
}
