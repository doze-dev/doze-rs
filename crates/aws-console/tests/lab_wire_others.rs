//! The Lab's rules through the CBOR path (CloudWatch), the REST XML path (S3) and the REST JSON path (Lambda).

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
use aws_schema::cbor::{map_header, text};
use lab_wire::{Answer, lab_of, rule, send, start, tagged};

// ---- CBOR: CloudWatch -----------------------------------------------------------------------------------------------------------

fn cbor_map(pairs: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    map_header(&mut out, pairs.len());
    for (k, v) in pairs {
        text(&mut out, k);
        out.extend_from_slice(v);
    }
    out
}

fn cbor_text(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    text(&mut out, s);
    out
}

fn cbor_float(f: f64) -> Vec<u8> {
    let mut out = vec![0xfb];
    out.extend_from_slice(&f.to_be_bytes());
    out
}

async fn cloudwatch(addr: SocketAddr, op: &str, body: &[u8]) -> Answer {
    send(
        addr,
        "monitoring",
        (
            "POST",
            &format!("/service/GraniteServiceVersion20100801/operation/{op}"),
        ),
        &[
            ("smithy-protocol", "rpc-v2-cbor"),
            ("Content-Type", "application/cbor"),
            ("Accept", "application/cbor"),
        ],
        body,
    )
    .await
}

fn metric_put() -> Vec<u8> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    let mut stamp = vec![0xc1];
    stamp.extend_from_slice(&cbor_float(now - 30.0));
    let datum = cbor_map(&[
        ("MetricName", cbor_text("depth")),
        ("Timestamp", stamp),
        ("Value", cbor_float(4.5)),
        ("Unit", cbor_text("Count")),
    ]);
    cbor_map(&[
        ("Namespace", cbor_text("W")),
        ("MetricData", vec![0x81].into_iter().chain(datum).collect()),
    ])
}

async fn metrics_listed(addr: SocketAddr) -> usize {
    let a = cloudwatch(
        addr,
        "ListMetrics",
        &cbor_map(&[("Namespace", cbor_text("W"))]),
    )
    .await;
    assert_eq!(a.status, Some(200));
    let json = aws_wire::cbor::to_json(&a.body).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    v["Metrics"].as_array().map_or(0, Vec::len)
}

#[tokio::test]
async fn a_cbor_service_is_throttled_slowed_and_dropped_and_a_fail_writes_nothing() {
    let w = start("cbor").await;
    w.table
        .add(rule(
            "monitoring",
            "PutMetricData",
            Effect::Fail(Fail::Throttle),
        ))
        .unwrap();
    let a = cloudwatch(w.addr, "PutMetricData", &metric_put()).await;
    // CloudWatch's model carries no throttle: the common `ThrottlingException`, 400, as a CBOR map.
    assert_eq!(a.status, Some(400));
    assert!(
        a.head.contains("content-type: application/cbor"),
        "{}",
        a.head
    );
    assert!(
        a.head.contains("smithy-protocol: rpc-v2-cbor"),
        "{}",
        a.head
    );
    let json: serde_json::Value =
        serde_json::from_str(&aws_wire::cbor::to_json(&a.body).unwrap()).unwrap();
    assert_eq!(
        json["__type"],
        "com.amazon.coral.service#ThrottlingException"
    );
    assert!(
        json["message"]
            .as_str()
            .unwrap()
            .starts_with("Rate exceeded.")
    );
    assert_eq!(
        metrics_listed(w.addr).await,
        0,
        "a failed call writes nothing"
    );
    w.table.stop_all();

    w.table
        .add(rule("monitoring", "PutMetricData", Effect::Drop))
        .unwrap();
    assert_eq!(
        cloudwatch(w.addr, "PutMetricData", &metric_put())
            .await
            .status,
        None
    );
    assert_eq!(metrics_listed(w.addr).await, 0);
    w.table.stop_all();

    w.table
        .add(rule(
            "monitoring",
            "PutMetricData",
            Effect::Slow {
                delay_ms: 250,
                jitter_ms: 0,
            },
        ))
        .unwrap();
    let started = Instant::now();
    let a = cloudwatch(w.addr, "PutMetricData", &metric_put()).await;
    assert_eq!(a.status, Some(200));
    assert!(started.elapsed() >= Duration::from_millis(250));
    assert_eq!(metrics_listed(w.addr).await, 1);
    w.stop().await;
}

// ---- REST: S3 -----------------------------------------------------------------------------------------------------------------

async fn s3(addr: SocketAddr, method: &str, path: &str, body: &[u8]) -> Answer {
    let expect: &[(&str, &str)] = if body.is_empty() {
        &[]
    } else {
        &[
            ("Expect", "100-continue"),
            ("x-amz-content-sha256", "UNSIGNED-PAYLOAD"),
        ]
    };
    send(addr, "s3", (method, path), expect, body).await
}

#[tokio::test]
async fn a_rest_service_is_throttled_slowed_and_dropped_and_a_fail_writes_nothing() {
    let w = start("rest").await;
    assert_eq!(s3(w.addr, "PUT", "/orders", b"").await.status, Some(200));
    let listing = || s3(w.addr, "GET", "/orders?list-type=2", b"");

    // S3 words its throttle itself: 503 SlowDown, in its XML error body.
    let id = w
        .table
        .add(rule("s3", "PutObject", Effect::Fail(Fail::Throttle)))
        .unwrap();
    let a = s3(w.addr, "PUT", "/orders/a.txt", b"hello").await;
    assert_eq!(a.status, Some(503), "{}", a.text());
    let xml = a.text();
    assert!(xml.contains("<Code>SlowDown</Code>"), "{xml}");
    assert!(
        xml.contains("<Error>") && xml.contains("<RequestId>"),
        "{xml}"
    );
    assert!(a.head.contains("server: amazons3"), "{}", a.head);
    assert!(
        !listing().await.text().contains("a.txt"),
        "a failed call writes nothing"
    );
    let row = tagged(&w, id);
    assert_eq!((row.status, &*row.code), (503, "SlowDown"));
    assert_eq!(&*row.resource, "orders/a.txt", "the row says what it named");
    assert_eq!(lab_of(&row), Some((id, LabDid::Failed)));
    w.table.stop_all();

    w.table.add(rule("s3", "PutObject", Effect::Drop)).unwrap();
    assert_eq!(
        s3(w.addr, "PUT", "/orders/a.txt", b"hello").await.status,
        None
    );
    assert!(!listing().await.text().contains("a.txt"));
    w.table.stop_all();

    w.table
        .add(rule(
            "s3",
            "PutObject",
            Effect::Slow {
                delay_ms: 250,
                jitter_ms: 0,
            },
        ))
        .unwrap();
    let started = Instant::now();
    let a = s3(w.addr, "PUT", "/orders/a.txt", b"hello").await;
    assert_eq!(a.status, Some(200), "{}", a.text());
    assert!(started.elapsed() >= Duration::from_millis(250));
    assert!(listing().await.text().contains("a.txt"));
    w.table.stop_all();

    // A resource pattern: only the keys it names are touched.
    let mut spec = rule("s3", "GetObject", Effect::Fail(Fail::AccessDenied));
    spec.matcher.resources = vec![aws_console::lab::Pattern::new("*.txt").unwrap()];
    w.table.add(spec).unwrap();
    let a = s3(w.addr, "GET", "/orders/a.txt", b"").await;
    assert_eq!(a.status, Some(403));
    assert!(
        a.text().contains("<Code>AccessDenied</Code>"),
        "{}",
        a.text()
    );
    w.stop().await;
}
