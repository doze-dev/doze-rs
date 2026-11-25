//! The hook for the Lab's rules, slow and drop: a slow request waits and is then served, a stop does not wait for it, a client that gives up is never served, and a drop closes the connection with no answer.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod fault;

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use aws_gateway::{Done, Verdict};
use fault::{SEND, Script, send_raw, start, tag, text, throttle};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

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
    assert!(
        answer.contains("validation") || answer.contains("missing"),
        "{answer}"
    );
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

#[tokio::test]
async fn a_client_that_gives_up_during_the_wait_is_never_served() {
    let script = Script::new(Verdict::Slow {
        delay: Duration::from_millis(700),
        tag: tag(),
    });
    let h = start(Some(script.clone()), true).await;
    let request = format!(
        "POST / HTTP/1.1\r\nHost: {}\r\nContent-Length: {}\r\nContent-Type: application/x-amz-json-1.0\r\nX-Amz-Target: AmazonSQS.SendMessage\r\nX-Amz-Date: 20130524T000000Z\r\nAuthorization: AWS4-HMAC-SHA256 Credential=AKIDLAB/20130524/eu-west-1/sqs/aws4_request,SignedHeaders=host;x-amz-date,Signature=00\r\n\r\n{SEND}",
        h.addr,
        SEND.len()
    );
    let mut stream = TcpStream::connect(h.addr).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    while script.judged.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // The client's timeout ran out: it hangs up while the call waits.
    drop(stream);
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(
        h.service.0.load(Ordering::SeqCst),
        0,
        "the call that was given up on is not served: here the service never did the work"
    );
    h.running.shutdown().await;
}
