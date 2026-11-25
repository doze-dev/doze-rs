//! The cost of the Lab on the request path, measured: one connection sending `ListQueues` to a real SQS behind the gateway, with
//! no observer and no table (`--no-console`), with the console's recorder only (`--lab=false`), with the table installed and no rule
//! switched on (one relaxed load), with one rule that fits nothing, and with 50 rules that fit nothing; then the table's own
//! `armed` and `judge` over a call. Prints the cost per request as the median and the best of several rounds.
//! Run: `cargo run --release -p aws-console --example lab_bench`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::print_stdout
)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use aws_console::Recorder;
use aws_console::lab::{Effect, Fail, LabTable, Matcher, Share, Spec};
use aws_gateway::{Ask, Config, Faults, Gateway};
use doze_runtime::{Ctx, SystemClock, Tasks};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const ROUNDS: usize = 7;
const REQUESTS: usize = 5_000;

const REQUEST: &str = "POST / HTTP/1.1\r\nHost: bench\r\nContent-Length: 2\r\nContent-Type: application/x-amz-json-1.0\r\nX-Amz-Target: AmazonSQS.ListQueues\r\nX-Amz-Date: 20130524T000000Z\r\nAuthorization: AWS4-HMAC-SHA256 Credential=AKIDBENCH/20130524/us-east-1/sqs/aws4_request,SignedHeaders=host;x-amz-date,Signature=00\r\n\r\n{}";

async fn call(s: &mut TcpStream) {
    s.write_all(REQUEST.as_bytes()).await.unwrap();
    let mut raw = Vec::with_capacity(512);
    let mut buf = [0u8; 1024];
    loop {
        let n = s.read(&mut buf).await.unwrap();
        assert!(n > 0, "closed");
        raw.extend_from_slice(&buf[..n]);
        let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
        let len: usize = head
            .lines()
            .find_map(|l| l.strip_prefix("content-length: "))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0);
        if raw.len() >= end + 4 + len {
            assert!(head.starts_with("http/1.1 200"), "{head}");
            return;
        }
    }
}

fn rule(n: usize) -> Spec {
    let (matcher, said) = Matcher::build(
        &["kms".to_owned()],
        &[format!("Op{n}")],
        &[],
        &[],
        &[],
    );
    assert!(said.is_empty());
    Spec {
        name: String::new(),
        matcher,
        effect: Effect::Fail(Fail::Throttle),
        share: Share::Every,
        ttl: Duration::from_secs(3600),
        max_hits: None,
    }
}

/// Nanoseconds per request: the median and the best of the rounds.
async fn measure(observed: bool, table: Option<Arc<LabTable>>) -> (f64, f64) {
    let dir = std::env::temp_dir().join(format!("doze-aws-lab-bench-{}", std::process::id()));
    let ctx = Arc::new(Ctx {
        dir,
        idle: Duration::from_secs(300),
        tasks: Tasks::new(),
        clock: Arc::new(SystemClock),
        ext: None,
    });
    let mut config = Config::new("127.0.0.1:0".parse().unwrap());
    if observed {
        config.observer = Some(Arc::new(Recorder::new()));
    }
    config.faults = table.map(|t| t as Arc<dyn Faults>);
    let running = Gateway::new(config)
        .with(Arc::new(aws_sqs::Sqs::new(ctx)))
        .bind()
        .await
        .unwrap();
    let mut s = TcpStream::connect(running.local_addr()).await.unwrap();
    for _ in 0..500 {
        call(&mut s).await;
    }
    let mut rounds = Vec::new();
    for _ in 0..ROUNDS {
        let started = Instant::now();
        for _ in 0..REQUESTS {
            call(&mut s).await;
        }
        rounds.push(started.elapsed().as_nanos() as f64 / REQUESTS as f64);
    }
    running.shutdown().await;
    rounds.sort_by(f64::total_cmp);
    (rounds[ROUNDS / 2], rounds[0])
}

fn judging(table: &LabTable) -> (f64, f64) {
    let ask = Ask {
        signing_name: "sqs",
        sdk_id: "SQS",
        operation: "ListQueues",
        resource: "",
        access_key_id: "AKIDBENCH",
        region: "us-east-1",
        user_agent: "Boto3/1.34.69",
    };
    let n: u32 = 2_000_000;
    let started = Instant::now();
    let mut armed = 0u64;
    for _ in 0..n {
        armed += u64::from(std::hint::black_box(table).armed());
    }
    let armed_ns = started.elapsed().as_nanos() as f64 / f64::from(n);
    assert!(armed == 0 || armed == u64::from(n));
    let started = Instant::now();
    for _ in 0..n {
        std::hint::black_box(table.judge(std::hint::black_box(&ask)));
    }
    (armed_ns, started.elapsed().as_nanos() as f64 / f64::from(n))
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let none = LabTable::new();
    let one = LabTable::new();
    one.add(rule(0)).unwrap();
    let fifty = LabTable::new();
    for n in 0..50 {
        fifty.add(rule(n)).unwrap();
    }
    println!("one connection, {REQUESTS} ListQueues a round, {ROUNDS} rounds: nanoseconds a request (median, best)");
    let mut base = None;
    for (what, observed, table) in [
        ("no observer, no table (--no-console)", false, None),
        ("the recorder only (--lab=false)", true, None),
        ("recorder and table, no rule", true, Some(none.clone())),
        ("recorder and table, one rule that fits nothing", true, Some(one.clone())),
        ("recorder and table, 50 rules that fit nothing", true, Some(fifty.clone())),
    ] {
        let (median, best) = measure(observed, table).await;
        let delta = base.map_or(String::new(), |b: f64| format!("  ({:+.0} ns over the recorder only)", median - b));
        if what.starts_with("the recorder only") {
            base = Some(median);
        }
        println!("  {what:<52} {median:>9.0} {best:>9.0}{delta}");
    }
    println!("the table over a call (ns a call: armed, judge):");
    for (what, t) in [("no rule", &none), ("one rule that fits nothing", &one), ("50 rules that fit nothing", &fifty)] {
        let (armed, judge) = judging(t);
        println!("  {what:<28} {armed:>7.1} {judge:>8.1}");
    }
}
