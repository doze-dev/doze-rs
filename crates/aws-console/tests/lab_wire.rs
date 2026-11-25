//! The Lab's rules on the wire, through the four protocol paths of the gateway with the real services behind it: a JSON service (SQS),
//! a Query one (SNS), a CBOR one (CloudWatch) and a REST one (S3). For each: a fail rule answers in the protocol's own shape with a
//! code the SDKs retry and the service never sees the call (nothing is written); a drop closes the connection with no answer
//! (nothing is written either); a slow rule serves the call after its wait; Live tags each call a rule touched; and a call no rule
//! fits is untouched.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aws_console::Recorder;
use aws_console::lab::{Effect, Fail, LabTable, Matcher, Share, Spec};
use aws_console::ring::{LabDid, Call};
use aws_gateway::{Auth, Config, Gateway};
use aws_schema::cbor::{map_header, text};
use doze_runtime::{Ctx, SystemClock, Tasks};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// botocore's list of the codes it retries as a throttle (`ThrottledRetryableChecker`), and its transient statuses.
const THROTTLED: &[&str] = &[
    "Throttling",
    "ThrottlingException",
    "ThrottledException",
    "RequestThrottledException",
    "TooManyRequestsException",
    "ProvisionedThroughputExceededException",
    "TransactionInProgressException",
    "RequestLimitExceeded",
    "BandwidthLimitExceeded",
    "LimitExceededException",
    "RequestThrottled",
    "SlowDown",
    "PriorRequestNotComplete",
    "EC2ThrottledException",
];

struct Wire {
    running: aws_gateway::Running,
    addr: SocketAddr,
    table: Arc<LabTable>,
    recorder: Arc<Recorder>,
    dir: PathBuf,
}

impl Wire {
    async fn stop(self) {
        self.running.shutdown().await;
        drop(std::fs::remove_dir_all(&self.dir));
    }
}

async fn start(tag: &str) -> Wire {
    let dir = std::env::temp_dir().join(format!("doze-aws-lab-wire-{tag}-{}", std::process::id()));
    let ctx = |name: &str| {
        Arc::new(Ctx {
            dir: dir.join(name),
            idle: Duration::from_secs(300),
            tasks: Tasks::new(),
            clock: Arc::new(SystemClock),
            ext: None,
        })
    };
    let table = LabTable::new();
    let recorder = Arc::new(Recorder::new());
    let mut config = Config::new("127.0.0.1:0".parse().unwrap());
    config.auth = Auth::Trust;
    config.observer = Some(recorder.clone());
    config.faults = Some(table.clone());
    let running = Gateway::new(config)
        .with(Arc::new(aws_sqs::Sqs::new(ctx("sqs"))))
        .with(Arc::new(aws_sns::Sns::new(ctx("sns"))))
        .with(Arc::new(aws_s3::S3::new(ctx("s3"))))
        .with(Arc::new(aws_cloudwatch::Cloudwatch::new(ctx("cloudwatch"))))
        .bind()
        .await
        .unwrap();
    let addr = running.local_addr();
    Wire {
        running,
        addr,
        table,
        recorder,
        dir,
    }
}

/// What the server sent back; no status at all when it closed the connection with no answer.
struct Answer {
    status: Option<u16>,
    head: String,
    body: Vec<u8>,
}

impl Answer {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

fn scope(service: &str) -> String {
    format!(
        "AWS4-HMAC-SHA256 Credential=AKIDLAB/20130524/us-east-1/{service}/aws4_request,SignedHeaders=host;x-amz-date,Signature=00"
    )
}

async fn send(
    addr: SocketAddr,
    service: &str,
    (method, path): (&str, &str),
    headers: &[(&str, &str)],
    body: &[u8],
) -> Answer {
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\nAuthorization: {}\r\nX-Amz-Date: 20130524T000000Z\r\nUser-Agent: Boto3/1.34.69\r\n",
        body.len(),
        scope(service)
    );
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(head.as_bytes()).await.unwrap();
    // A body is sent when the server asks for it (`100 Continue`), as the SDKs do: a request answered before its body is read must
    // not have the body waiting in the socket when it is closed.
    let mut raw = Vec::new();
    if !body.is_empty() && headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("expect")) {
        let mut buf = [0u8; 4096];
        while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = stream.read(&mut buf).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            raw.extend_from_slice(&buf[..n]);
        }
        if raw.starts_with(b"HTTP/1.1 100") {
            let end = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
            raw.drain(..end);
            stream.write_all(body).await.unwrap();
        }
    } else {
        stream.write_all(body).await.unwrap();
    }
    // A connection closed by a drop may be reset instead: either way there is no answer.
    let mut rest = Vec::new();
    drop(stream.read_to_end(&mut rest).await);
    raw.extend_from_slice(&rest);
    let Some(split) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
        return Answer {
            status: None,
            head: String::new(),
            body: raw,
        };
    };
    let head = String::from_utf8_lossy(&raw[..split]).to_ascii_lowercase();
    Answer {
        status: head.split(' ').nth(1).and_then(|s| s.parse().ok()),
        head,
        body: raw[split + 4..].to_vec(),
    }
}

fn rule(service: &str, operation: &str, effect: Effect) -> Spec {
    let (matcher, said) = Matcher::build(
        &[service.to_owned()],
        &[operation.to_owned()],
        &[],
        &[],
        &[],
    );
    assert!(said.is_empty());
    Spec {
        name: String::new(),
        matcher,
        effect,
        share: Share::Every,
        ttl: Duration::from_secs(600),
        max_hits: None,
    }
}

fn newest(w: &Wire) -> Call {
    w.recorder.newest(1).calls.pop().unwrap()
}

/// The newest call rule `id` touched.
fn tagged(w: &Wire, id: u32) -> Call {
    w.recorder.touched(Some(id), 1).0.pop().unwrap()
}

fn lab_of(c: &Call) -> Option<(u32, LabDid)> {
    c.extra
        .as_ref()
        .and_then(|e| e.lab.as_ref())
        .map(|l| (l.rule, l.did))
}

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

#[tokio::test]
async fn a_json_service_is_throttled_slowed_and_dropped_and_a_fail_writes_nothing() {
    let w = start("json").await;
    let made = sqs(w.addr, "CreateQueue", r#"{"QueueName":"orders"}"#).await;
    assert_eq!(made.status, Some(200), "{}", made.text());
    let url = serde_json::from_slice::<serde_json::Value>(&made.body).unwrap()["QueueUrl"]
        .as_str()
        .unwrap()
        .to_owned();
    let send_body = format!(r#"{{"QueueUrl":"{url}","MessageBody":"hi"}}"#);

    // Fail: SQS's own throttle, in the JSON protocol with its Query-compatible code, 403 as the model says.
    let id = w
        .table
        .add(rule("sqs", "SendMessage", Effect::Fail(Fail::Throttle)))
        .unwrap();
    let a = sqs(w.addr, "SendMessage", &send_body).await;
    assert_eq!(a.status, Some(403), "{}", a.text());
    assert!(a.head.contains("x-amzn-query-error: requestthrottled;sender"), "{}", a.head);
    let body: serde_json::Value = serde_json::from_slice(&a.body).unwrap();
    assert!(body["__type"].as_str().unwrap().ends_with("#RequestThrottled"), "{body}");
    assert!(THROTTLED.contains(&"RequestThrottled"));
    assert_eq!(queue_depth(w.addr, &url).await, "0", "a failed call writes nothing");
    let row = tagged(&w, id);
    assert_eq!((row.status, &*row.code), (403, "RequestThrottled"));
    assert_eq!(lab_of(&row), Some((id, LabDid::Failed)));
    // Other operations are not touched.
    assert_eq!(sqs(w.addr, "ListQueues", "{}").await.status, Some(200));
    assert_eq!(lab_of(&newest(&w)), None);
    assert_eq!(w.table.stop_all(), 1);

    // Drop: no answer, and nothing written.
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
    w.table.stop_all();

    // Slow: served after its wait, and the row says it waited.
    let id = w
        .table
        .add(rule(
            "sqs",
            "SendMessage",
            Effect::Slow {
                delay_ms: 300,
                jitter_ms: 0,
            },
        ))
        .unwrap();
    let started = Instant::now();
    let a = sqs(w.addr, "SendMessage", &send_body).await;
    assert_eq!(a.status, Some(200), "{}", a.text());
    assert!(started.elapsed() >= Duration::from_millis(300));
    assert_eq!(queue_depth(w.addr, &url).await, "1", "a slowed call is served");
    let row = tagged(&w, id);
    assert_eq!(lab_of(&row), Some((id, LabDid::Delayed(300))));
    assert!(row.took_us >= 300_000, "its time includes the wait: {}", row.took_us);
    w.table.stop_all();

    // With a rule for another service armed, and with none, the call is untouched.
    w.table
        .add(rule("kms", "Encrypt", Effect::Fail(Fail::Internal)))
        .unwrap();
    for _ in 0..2 {
        let a = sqs(w.addr, "SendMessage", &send_body).await;
        assert_eq!(a.status, Some(200));
        assert_eq!(lab_of(&newest(&w)), None);
        w.table.stop_all();
    }
    assert_eq!(queue_depth(w.addr, &url).await, "3");
    w.stop().await;
}

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

#[tokio::test]
async fn a_query_service_is_throttled_slowed_and_dropped_and_a_fail_writes_nothing() {
    let w = start("query").await;
    let create = "Action=CreateTopic&Name=order-events&Version=2010-03-31";
    let topics = || sns(w.addr, "Action=ListTopics&Version=2010-03-31");

    let id = w
        .table
        .add(rule("sns", "CreateTopic", Effect::Fail(Fail::Throttle)))
        .unwrap();
    let a = sns(w.addr, create).await;
    // SNS's own throttle: the model's `Throttled`, 429, as Query XML.
    assert_eq!(a.status, Some(429), "{}", a.text());
    let xml = a.text();
    assert!(xml.contains("<Code>Throttled</Code>"), "{xml}");
    assert!(xml.contains("<ErrorResponse"), "{xml}");
    assert!(xml.contains("<Type>Sender</Type>"), "{xml}");
    assert!(xml.contains("Rate exceeded."), "{xml}");
    assert!(!topics().await.text().contains("order-events"), "a failed call writes nothing");
    let failed = w.recorder.touched(Some(id), 5).0;
    assert_eq!(failed.len(), 1);
    assert_eq!((failed[0].status, &*failed[0].code), (429, "Throttled"));
    w.table.stop_all();

    w.table
        .add(rule("sns", "CreateTopic", Effect::Drop))
        .unwrap();
    assert_eq!(sns(w.addr, create).await.status, None);
    assert!(!topics().await.text().contains("order-events"));
    w.table.stop_all();

    w.table
        .add(rule(
            "sns",
            "CreateTopic",
            Effect::Slow {
                delay_ms: 250,
                jitter_ms: 0,
            },
        ))
        .unwrap();
    let started = Instant::now();
    let a = sns(w.addr, create).await;
    assert_eq!(a.status, Some(200), "{}", a.text());
    assert!(started.elapsed() >= Duration::from_millis(250));
    assert!(topics().await.text().contains("order-events"));
    w.table.stop_all();

    // The other kinds of failure: an unavailable service, and an internal error with the model's code.
    w.table
        .add(rule("sns", "ListTopics", Effect::Fail(Fail::Internal)))
        .unwrap();
    let a = topics().await;
    assert_eq!(a.status, Some(500));
    assert!(a.text().contains("<Code>InternalError</Code>"), "{}", a.text());
    assert!(a.text().contains("<Type>Receiver</Type>"));
    w.table.stop_all();
    w.table
        .add(rule("sns", "ListTopics", Effect::Fail(Fail::Unavailable)))
        .unwrap();
    let a = topics().await;
    assert_eq!(a.status, Some(503));
    assert!(a.text().contains("<Code>ServiceUnavailable</Code>"));
    w.table.stop_all();
    w.table
        .add(rule("sns", "ListTopics", Effect::Fail(Fail::AccessDenied)))
        .unwrap();
    let a = topics().await;
    assert_eq!(a.status, Some(403));
    assert!(a.text().contains("<Code>AuthorizationError</Code>"), "{}", a.text());
    w.stop().await;
}

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
    let a = cloudwatch(addr, "ListMetrics", &cbor_map(&[("Namespace", cbor_text("W"))])).await;
    assert_eq!(a.status, Some(200));
    let json = aws_wire::cbor::to_json(&a.body).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    v["Metrics"].as_array().map_or(0, Vec::len)
}

#[tokio::test]
async fn a_cbor_service_is_throttled_slowed_and_dropped_and_a_fail_writes_nothing() {
    let w = start("cbor").await;
    w.table
        .add(rule("monitoring", "PutMetricData", Effect::Fail(Fail::Throttle)))
        .unwrap();
    let a = cloudwatch(w.addr, "PutMetricData", &metric_put()).await;
    // CloudWatch's model carries no throttle: the common `ThrottlingException`, 400, as a CBOR map.
    assert_eq!(a.status, Some(400));
    assert!(a.head.contains("content-type: application/cbor"), "{}", a.head);
    assert!(a.head.contains("smithy-protocol: rpc-v2-cbor"), "{}", a.head);
    let json: serde_json::Value =
        serde_json::from_str(&aws_wire::cbor::to_json(&a.body).unwrap()).unwrap();
    assert_eq!(json["__type"], "com.amazon.coral.service#ThrottlingException");
    assert!(json["message"].as_str().unwrap().starts_with("Rate exceeded."));
    assert_eq!(metrics_listed(w.addr).await, 0, "a failed call writes nothing");
    w.table.stop_all();

    w.table
        .add(rule("monitoring", "PutMetricData", Effect::Drop))
        .unwrap();
    assert_eq!(cloudwatch(w.addr, "PutMetricData", &metric_put()).await.status, None);
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
        &[("Expect", "100-continue"), ("x-amz-content-sha256", "UNSIGNED-PAYLOAD")]
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
    assert!(xml.contains("<Error>") && xml.contains("<RequestId>"), "{xml}");
    assert!(a.head.contains("server: amazons3"), "{}", a.head);
    assert!(!listing().await.text().contains("a.txt"), "a failed call writes nothing");
    let row = tagged(&w, id);
    assert_eq!((row.status, &*row.code), (503, "SlowDown"));
    assert_eq!(&*row.resource, "orders/a.txt", "the row says what it named");
    assert_eq!(lab_of(&row), Some((id, LabDid::Failed)));
    w.table.stop_all();

    w.table
        .add(rule("s3", "PutObject", Effect::Drop))
        .unwrap();
    assert_eq!(s3(w.addr, "PUT", "/orders/a.txt", b"hello").await.status, None);
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
    assert!(a.text().contains("<Code>AccessDenied</Code>"), "{}", a.text());
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
        let retryable = status == 200 || THROTTLED.contains(&code_of(&a).as_str()) || status == 429 || status >= 500;
        assert!(retryable, "{status} {}", code_of(&a));
        if status == 200 || attempts == 3 {
            break a;
        }
    };
    assert_eq!(attempts, 3);
    assert_eq!(answered.status, Some(200));
    assert_eq!(queue_depth(w.addr, &url).await, "1", "only the call that was served wrote");
    let found = w.table.find(id).unwrap();
    assert_eq!(found.rule.hits, 2);
    assert!(found.over.is_some(), "the rule ended by itself when it had fired twice");
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
    assert!(payments.head.contains("x-amzn-query-error: serviceunavailable;receiver"), "{}", payments.head);
    w.stop().await;
}
