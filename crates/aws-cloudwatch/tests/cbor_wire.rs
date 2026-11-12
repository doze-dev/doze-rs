//! CloudWatch over a real socket in the RPC v2 CBOR protocol, as a current SDK speaks it (and, once, in JSON): the gateway reads the
//! CBOR body, the service answers, and the answer and the errors come back as CBOR with the headers that protocol uses.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use aws_cloudwatch::Cloudwatch;
use aws_gateway::{Auth, Config, Gateway};
use aws_schema::cbor::{map_header, text};
use doze_runtime::{Ctx, SystemClock, Tasks};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const SCOPE: &str = "Credential=test/20130524/us-east-1/monitoring/aws4_request,SignedHeaders=host;x-amz-date,Signature=00";
const PATH: &str = "/service/GraniteServiceVersion20100801/operation/";

fn float(out: &mut Vec<u8>, f: f64) {
    out.push(0xfb);
    out.extend_from_slice(&f.to_be_bytes());
}

/// A CBOR map of text keys to the given already-encoded values.
fn map(pairs: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    map_header(&mut out, pairs.len());
    for (k, v) in pairs {
        text(&mut out, k);
        out.extend_from_slice(v);
    }
    out
}

fn string(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    text(&mut out, s);
    out
}

fn number(f: f64) -> Vec<u8> {
    let mut out = Vec::new();
    float(&mut out, f);
    out
}

fn timestamp(secs: f64) -> Vec<u8> {
    let mut out = vec![0xc1];
    float(&mut out, secs);
    out
}

fn array(items: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![0x80 | u8::try_from(items.len()).unwrap()];
    for i in items {
        out.extend_from_slice(i);
    }
    out
}

/// One request, one response: the status, the lower-cased head, and the body bytes.
async fn send(
    addr: SocketAddr,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> (u16, String, Vec<u8>) {
    let mut head = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\nAuthorization: AWS4-HMAC-SHA256 {SCOPE}\r\nX-Amz-Date: 20130524T000000Z\r\n",
        body.len()
    );
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(head.as_bytes()).await.unwrap();
    stream.write_all(body).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8(raw[..split].to_vec())
        .unwrap()
        .to_ascii_lowercase();
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    (status, head, raw[split + 4..].to_vec())
}

async fn cbor(addr: SocketAddr, op: &str, body: &[u8]) -> (u16, String, Value) {
    let headers = [
        ("smithy-protocol", "rpc-v2-cbor"),
        ("Content-Type", "application/cbor"),
        ("Accept", "application/cbor"),
    ];
    let (status, head, bytes) = send(addr, &format!("{PATH}{op}"), &headers, body).await;
    let json = aws_wire::cbor::to_json(&bytes).unwrap_or_else(|e| panic!("{e}: {bytes:?}"));
    (status, head, serde_json::from_str(&json).unwrap())
}

/// A gateway with CloudWatch behind it on a directory of its own, and the time now in seconds.
async fn start(tag: &str) -> (aws_gateway::Running, SocketAddr, std::path::PathBuf, f64) {
    let dir = std::env::temp_dir().join(format!("doze-aws-cw-wire-{tag}-{}", std::process::id()));
    let ctx = Arc::new(Ctx {
        dir: dir.clone(),
        idle: Duration::from_secs(300),
        tasks: Tasks::new(),
        clock: Arc::new(SystemClock),
        ext: None,
    });
    let mut config = Config::new("127.0.0.1:0".parse().unwrap());
    config.auth = Auth::Trust;
    let running = Gateway::new(config)
        .with(Arc::new(Cloudwatch::new(ctx)))
        .bind()
        .await
        .unwrap();
    let addr = running.local_addr();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    (running, addr, dir, now)
}

#[tokio::test]
async fn a_cbor_client_puts_data_and_reads_statistics() {
    let (running, addr, dir, now) = start("data").await;
    let datum = map(&[
        ("MetricName", string("depth")),
        ("Timestamp", timestamp(now - 30.0)),
        ("Value", number(4.5)),
        ("Unit", string("Count")),
    ]);
    let put = map(&[("Namespace", string("W")), ("MetricData", array(&[datum]))]);
    let (status, head, answer) = cbor(addr, "PutMetricData", &put).await;
    assert_eq!(status, 200, "{answer}");
    assert!(head.contains("content-type: application/cbor"), "{head}");
    assert!(head.contains("smithy-protocol: rpc-v2-cbor"), "{head}");
    assert_eq!(answer, json!({}));

    let stats = map(&[
        ("Namespace", string("W")),
        ("MetricName", string("depth")),
        ("StartTime", timestamp(now - 600.0)),
        ("EndTime", timestamp(now + 600.0)),
        ("Period", vec![0x18, 60]),
        ("Statistics", array(&[string("Sum"), string("Average")])),
    ]);
    let (status, _, answer) = cbor(addr, "GetMetricStatistics", &stats).await;
    assert_eq!(status, 200, "{answer}");
    assert_eq!(answer["Label"], "depth");
    assert_eq!(answer["Datapoints"][0]["Sum"], 4.5);
    assert_eq!(answer["Datapoints"][0]["Unit"], "Count");
    // The same service answers a JSON 1.0 client on the same port.
    let (status, head, bytes) = send(
        addr,
        "/",
        &[
            ("X-Amz-Target", "GraniteServiceVersion20100801.ListMetrics"),
            ("Content-Type", "application/x-amz-json-1.0"),
        ],
        br#"{"Namespace":"W"}"#,
    )
    .await;
    assert_eq!(status, 200);
    assert!(head.contains("application/x-amz-json-1.0"), "{head}");
    let listed: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(listed["Metrics"][0]["MetricName"], "depth");
    running.shutdown().await;
    drop(std::fs::remove_dir_all(&dir));
}

#[tokio::test]
async fn cbor_errors_are_cbor_maps_with_the_query_code_beside_them() {
    let (running, addr, dir, now) = start("errors").await;
    // A refusal of the service is a CBOR map, with the code the Query protocol would use beside it.
    let bad = map(&[
        ("Namespace", string("W")),
        ("MetricName", string("depth")),
        ("StartTime", timestamp(now - 600.0)),
        ("EndTime", timestamp(now)),
        ("Period", vec![0x18, 45]),
        ("Statistics", array(&[string("Sum")])),
    ]);
    let (status, head, answer) = cbor(addr, "GetMetricStatistics", &bad).await;
    assert_eq!(status, 400);
    assert!(
        head.contains("x-amzn-query-error: invalidparametervalue;sender"),
        "{head}"
    );
    assert_eq!(
        answer["__type"],
        "com.amazonaws.cloudwatch#InvalidParameterValueException"
    );

    // A check of the model fails the same way; an operation not built is refused by name before its body is read.
    let (status, _, answer) = cbor(
        addr,
        "PutMetricAlarm",
        &map(&[
            ("AlarmName", string("a")),
            ("ComparisonOperator", string("Nope")),
        ]),
    )
    .await;
    assert_eq!(status, 400, "{answer}");
    assert_eq!(
        answer["__type"],
        "com.amazonaws.cloudwatch#ValidationException"
    );
    let (status, head, _) = cbor(addr, "PutMetricStream", &[0xff]).await;
    assert_eq!(status, 501);
    assert!(head.contains("x-doze-not-built: 1"), "{head}");
    let (status, _, _) = cbor(addr, "NoSuchOperation", &map(&[])).await;
    assert_eq!(status, 404);
    running.shutdown().await;
    drop(std::fs::remove_dir_all(&dir));
}

#[tokio::test]
async fn a_gzip_body_is_inflated_before_it_is_read() {
    let (running, addr, dir, _) = start("gzip").await;
    // A gzip body, as botocore sends PutMetricData past 10 KB (the signature covers the compressed bytes).
    use std::io::Write;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(br#"{"Namespace":"W","MetricData":[{"MetricName":"zipped","Value":1}]}"#)
        .unwrap();
    let zipped = gz.finish().unwrap();
    let (status, _, _) = send(
        addr,
        "/",
        &[
            (
                "X-Amz-Target",
                "GraniteServiceVersion20100801.PutMetricData",
            ),
            ("Content-Type", "application/x-amz-json-1.0"),
            ("Content-Encoding", "gzip"),
        ],
        &zipped,
    )
    .await;
    assert_eq!(status, 200);
    let (status, _, _) = send(
        addr,
        "/",
        &[
            (
                "X-Amz-Target",
                "GraniteServiceVersion20100801.PutMetricData",
            ),
            ("Content-Encoding", "gzip"),
        ],
        b"not gzip",
    )
    .await;
    assert_eq!(status, 400);
    let (_, _, bytes) = send(
        addr,
        "/",
        &[
            ("X-Amz-Target", "GraniteServiceVersion20100801.ListMetrics"),
            ("Content-Type", "application/x-amz-json-1.0"),
        ],
        br#"{"MetricName":"zipped"}"#,
    )
    .await;
    let listed: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(listed["Metrics"][0]["MetricName"], "zipped");

    running.shutdown().await;
    drop(std::fs::remove_dir_all(&dir));
}
