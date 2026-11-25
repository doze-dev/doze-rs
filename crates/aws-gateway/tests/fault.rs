//! The hook for the Lab's rules, over a real socket with a fake SQS behind it and a scripted [`Faults`]: a request is judged after it
//! is decoded and before the service; a fail answers in the protocol's own shape and the service never sees the request; a slow one
//! waits and is then served; a drop closes the connection with no answer; and what no rule touches is untouched.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aws_gateway::{
    Ask, Config, Done, Failure, Faults, Finished, Gateway, Observer, Tag, Verdict,
};
use aws_model_sqs::{meta::META, tables::TABLES};
use aws_schema::meta::ServiceMeta;
use aws_schema::tables::Tables;
use aws_wire::service::{BoxFuture, Call, Output, Reply, Service};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// SQS's model with nothing behind it that counts the calls it is asked.
#[derive(Debug, Default)]
struct CountingSqs(AtomicUsize);

impl Service for CountingSqs {
    fn meta(&self) -> &'static ServiceMeta {
        &META
    }

    fn tables(&self) -> &'static Tables {
        &TABLES
    }

    fn call(&self, _call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(Output {
                arena: aws_schema::arena::Arena::new(),
                root: aws_schema::arena::NIL,
            })
        })
    }
}

/// A [`Faults`] that answers every request it is asked about with one verdict, and notes what it was shown.
#[derive(Debug)]
struct Script {
    armed: AtomicBool,
    verdict: Mutex<Verdict>,
    judged: AtomicUsize,
    seen: Mutex<Vec<String>>,
}

impl Script {
    fn new(verdict: Verdict) -> Arc<Self> {
        Arc::new(Self {
            armed: AtomicBool::new(true),
            verdict: Mutex::new(verdict),
            judged: AtomicUsize::new(0),
            seen: Mutex::new(Vec::new()),
        })
    }
}

impl Faults for Script {
    fn armed(&self) -> bool {
        self.armed.load(Ordering::Relaxed)
    }

    fn judge(&self, ask: &Ask<'_>) -> Verdict {
        self.judged.fetch_add(1, Ordering::SeqCst);
        self.seen.lock().unwrap().push(format!(
            "{}|{}|{}|{}|{}|{}",
            ask.signing_name, ask.sdk_id, ask.operation, ask.resource, ask.access_key_id, ask.region
        ));
        self.verdict.lock().unwrap().clone()
    }
}

fn tag() -> Tag {
    Tag {
        rule: 7,
        name: Arc::from("throttle orders"),
    }
}

fn throttle() -> Verdict {
    Verdict::Fail(Box::new(Failure {
        shape: Some("RequestThrottled"),
        status: 403,
        code: "RequestThrottled".into(),
        message: "Rate exceeded.".to_owned(),
        tag: tag(),
    }))
}

/// What the observer was told of a request, with the rule that touched it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Told {
    action: String,
    status: u16,
    code: String,
    took: Duration,
    lab: Option<(u32, String, Done)>,
}

#[derive(Debug, Default)]
struct Tape(Mutex<Vec<Told>>);

impl Observer for Tape {
    fn finished(&self, call: &Finished<'_>) {
        self.0.lock().unwrap().push(Told {
            action: call.action.to_owned(),
            status: call.status,
            code: call.code.to_owned(),
            took: call.took,
            lab: call.lab.map(|l| (l.rule, l.name.to_owned(), l.done)),
        });
    }
}

struct Harness {
    running: aws_gateway::Running,
    addr: SocketAddr,
    service: Arc<CountingSqs>,
    tape: Arc<Tape>,
}

async fn start(faults: Option<Arc<Script>>, observed: bool) -> Harness {
    let service = Arc::new(CountingSqs::default());
    let tape = Arc::new(Tape::default());
    let mut config = Config::new("127.0.0.1:0".parse().unwrap());
    if observed {
        config.observer = Some(tape.clone());
    }
    config.faults = faults.map(|f| f as Arc<dyn Faults>);
    let running = Gateway::new(config)
        .with(service.clone())
        .bind()
        .await
        .unwrap();
    let addr = running.local_addr();
    Harness {
        running,
        addr,
        service,
        tape,
    }
}

/// One `SendMessage`; the raw bytes the server sent back (empty when it closed the connection with no answer).
async fn send_raw(addr: SocketAddr, body: &str) -> Vec<u8> {
    let request = format!(
        "POST / HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\nContent-Type: application/x-amz-json-1.0\r\nX-Amz-Target: AmazonSQS.SendMessage\r\nX-Amz-Date: 20130524T000000Z\r\nUser-Agent: Boto3/1.34.69\r\nAuthorization: AWS4-HMAC-SHA256 Credential=AKIDLAB/20130524/eu-west-1/sqs/aws4_request,SignedHeaders=host;x-amz-date,Signature=00\r\n\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    // A closed connection may be a reset: that is no answer either.
    drop(stream.read_to_end(&mut raw).await);
    raw
}

const SEND: &str = r#"{"QueueUrl":"http://localhost:4566/000000000000/orders","MessageBody":"hi"}"#;

fn text(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).to_ascii_lowercase()
}

#[tokio::test]
async fn a_request_is_judged_with_what_the_pipeline_learnt_of_it() {
    let script = Script::new(Verdict::Pass);
    let h = start(Some(script.clone()), true).await;
    let answer = text(&send_raw(h.addr, SEND).await);
    assert!(answer.starts_with("http/1.1 200"), "{answer}");
    assert_eq!(
        *script.seen.lock().unwrap(),
        ["sqs|SQS|SendMessage|http://localhost:4566/000000000000/orders|AKIDLAB|eu-west-1"]
    );
    assert_eq!(h.service.0.load(Ordering::SeqCst), 1);
    let told = h.tape.0.lock().unwrap().clone();
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].lab, None, "a request no rule touched has no tag");
    h.running.shutdown().await;
}

#[tokio::test]
async fn a_fail_answers_in_the_protocols_shape_and_the_service_never_sees_the_request() {
    let script = Script::new(throttle());
    let h = start(Some(script), true).await;
    let answer = text(&send_raw(h.addr, SEND).await);
    // SQS's JSON protocol, with its Query-compatible code in the header SDKs read it from, and the model's status for the error.
    assert!(answer.starts_with("http/1.1 403"), "{answer}");
    assert!(
        answer.contains("x-amzn-query-error: requestthrottled;sender"),
        "{answer}"
    );
    assert!(answer.contains("requestthrottled"), "{answer}");
    assert!(answer.contains("rate exceeded."), "{answer}");
    assert_eq!(
        h.service.0.load(Ordering::SeqCst),
        0,
        "a fault never reaches the service"
    );
    let told = h.tape.0.lock().unwrap().clone();
    assert_eq!(
        (told[0].status, told[0].code.as_str()),
        (403, "RequestThrottled")
    );
    assert_eq!(
        told[0].lab,
        Some((7, "throttle orders".to_owned(), Done::Failed))
    );
    h.running.shutdown().await;
}

#[tokio::test]
async fn a_fail_with_a_code_the_model_does_not_carry_states_its_own_status_and_code() {
    let script = Script::new(Verdict::Fail(Box::new(Failure {
        shape: None,
        status: 503,
        code: "ServiceUnavailable".into(),
        message: "The service is temporarily unavailable.".to_owned(),
        tag: tag(),
    })));
    let h = start(Some(script), true).await;
    let answer = text(&send_raw(h.addr, SEND).await);
    assert!(answer.starts_with("http/1.1 503"), "{answer}");
    assert!(
        answer.contains("x-amzn-query-error: serviceunavailable;receiver"),
        "{answer}"
    );
    assert!(
        answer.contains("com.amazon.coral.service#serviceunavailable"),
        "{answer}"
    );
    assert_eq!(h.service.0.load(Ordering::SeqCst), 0);
    h.running.shutdown().await;
}

#[tokio::test]
async fn a_slow_request_waits_and_is_then_served_and_the_observer_is_told() {
    let script = Script::new(Verdict::Slow {
        delay: Duration::from_millis(250),
        tag: tag(),
    });
    let h = start(Some(script), true).await;
    let started = Instant::now();
    let answer = text(&send_raw(h.addr, SEND).await);
    assert!(answer.starts_with("http/1.1 200"), "{answer}");
    assert!(started.elapsed() >= Duration::from_millis(250));
    assert_eq!(h.service.0.load(Ordering::SeqCst), 1, "it is served");
    let told = h.tape.0.lock().unwrap().clone();
    assert!(told[0].took >= Duration::from_millis(250), "{told:?}");
    assert_eq!(
        told[0].lab,
        Some((
            7,
            "throttle orders".to_owned(),
            Done::Delayed(Duration::from_millis(250))
        ))
    );
    h.running.shutdown().await;
}

#[tokio::test]
async fn a_drop_closes_the_connection_with_no_answer_and_the_service_never_sees_it() {
    let script = Script::new(Verdict::Drop { tag: tag() });
    let h = start(Some(script), true).await;
    let raw = send_raw(h.addr, SEND).await;
    assert!(raw.is_empty(), "no answer, not even an error: {raw:?}");
    assert_eq!(h.service.0.load(Ordering::SeqCst), 0);
    let told = h.tape.0.lock().unwrap().clone();
    assert_eq!(told.len(), 1, "the observer is told of a dropped call too");
    assert_eq!(told[0].status, 0, "a dropped call has no status");
    assert_eq!(
        told[0].lab,
        Some((7, "throttle orders".to_owned(), Done::Dropped))
    );
    h.running.shutdown().await;
}

#[tokio::test]
async fn the_next_connection_is_served_when_the_rule_is_gone() {
    let script = Script::new(Verdict::Drop { tag: tag() });
    let h = start(Some(script.clone()), true).await;
    assert!(send_raw(h.addr, SEND).await.is_empty());
    *script.verdict.lock().unwrap() = Verdict::Pass;
    let answer = text(&send_raw(h.addr, SEND).await);
    assert!(answer.starts_with("http/1.1 200"), "{answer}");
    h.running.shutdown().await;
}

#[tokio::test]
async fn a_table_with_nothing_armed_is_never_asked() {
    let script = Script::new(throttle());
    script.armed.store(false, Ordering::Relaxed);
    let h = start(Some(script.clone()), true).await;
    let answer = text(&send_raw(h.addr, SEND).await);
    assert!(answer.starts_with("http/1.1 200"), "{answer}");
    assert_eq!(
        script.judged.load(Ordering::SeqCst),
        0,
        "with nothing armed the request is not even described to the rules"
    );
    h.running.shutdown().await;
}

#[tokio::test]
async fn the_rules_judge_a_request_with_no_observer_too() {
    let script = Script::new(throttle());
    let h = start(Some(script), false).await;
    let answer = text(&send_raw(h.addr, SEND).await);
    assert!(answer.starts_with("http/1.1 403"), "{answer}");
    assert_eq!(h.service.0.load(Ordering::SeqCst), 0);
    h.running.shutdown().await;
}

#[tokio::test]
async fn a_request_the_pipeline_refused_before_it_was_decoded_is_not_judged() {
    let script = Script::new(throttle());
    let h = start(Some(script.clone()), true).await;
    // `SendMessage` needs its `QueueUrl` and `MessageBody`: an invalid input is the service's validation error, as AWS answers it.
    let answer = text(&send_raw(h.addr, "{}").await);
    assert!(answer.starts_with("http/1.1 400"), "{answer}");
    assert!(answer.contains("validation") || answer.contains("missing"), "{answer}");
    assert_eq!(script.judged.load(Ordering::SeqCst), 0);
    h.running.shutdown().await;
}

#[tokio::test]
async fn stopping_the_server_does_not_wait_for_a_slow_request() {
    let script = Script::new(Verdict::Slow {
        delay: Duration::from_secs(60),
        tag: tag(),
    });
    let h = start(Some(script.clone()), true).await;
    let addr = h.addr;
    let waiting = tokio::spawn(async move { send_raw(addr, SEND).await });
    while script.judged.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let started = Instant::now();
    h.running.shutdown().await;
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "a stop does not wait for the delay"
    );
    assert!(waiting.await.unwrap().is_empty());
}
