use super::*;

#[test]
fn the_request_is_a_framed_api_versions_v0_with_the_client_id() {
    let frame = api_versions_request(7, "console-check");
    let (len, payload) = frame.split_at(4);
    assert_eq!(
        i32::from_be_bytes(len.try_into().unwrap()) as usize,
        payload.len()
    );
    assert_eq!(&payload[..2], &18_i16.to_be_bytes(), "ApiVersions");
    assert_eq!(&payload[2..4], &0_i16.to_be_bytes(), "v0");
    assert_eq!(&payload[4..8], &7_i32.to_be_bytes());
    assert_eq!(&payload[8..10], &13_i16.to_be_bytes());
    assert_eq!(&payload[10..], b"console-check");
}

#[test]
fn an_answer_must_echo_the_correlation_id_and_carry_no_error() {
    let body = |corr: i32, err: i16| {
        [
            corr.to_be_bytes().as_slice(),
            err.to_be_bytes().as_slice(),
            &[0, 0],
        ]
        .concat()
    };
    assert!(is_api_versions_answer(&body(7, 0), 7));
    assert!(!is_api_versions_answer(&body(8, 0), 7));
    assert!(!is_api_versions_answer(&body(7, 35), 7));
    assert!(!is_api_versions_answer(&[0, 0, 0, 7], 7));
    assert!(!is_api_versions_answer(&[], 7));
}

#[test]
fn a_failed_dial_says_why_in_a_few_words() {
    use std::io::{Error, ErrorKind};
    assert_eq!(
        why(&Error::from(ErrorKind::ConnectionRefused)),
        "connection refused"
    );
    assert_eq!(why(&Error::from(ErrorKind::TimedOut)), "timed out");
    assert_eq!(
        why(&Error::other(
            "failed to lookup address information: Name or service not known"
        )),
        "no such host"
    );
    assert_eq!(
        why(&Error::other("nodename nor servname provided")),
        "no such host"
    );
    assert_eq!(
        why(&Error::other("something with a secret path /etc/x")),
        "could not connect"
    );
}

/// A loopback port that refuses a connection right now, from below every system's ephemeral range (Linux starts at 32768,
/// macOS at 49152), so no `bind(":0")` of a test running beside this one can be given it. A port from `bind(":0")`, dropped,
/// was free for exactly that, and one was taken (macOS check, run 43501441380: "connected, but it did not answer
/// ApiVersions"). Refusal is checked by connecting, not by binding: std binds with `SO_REUSEADDR`, which on macOS succeeds
/// beside a process listening on the wildcard address. A socket bound and never listened on would hold the port, but macOS
/// drops the SYN to it (a timeout, not a refusal).
fn closed_port() -> std::net::SocketAddr {
    let start = std::process::id() % 10_000;
    (0..10_000)
        .map(|i| {
            let port = 20_000 + u16::try_from((start + i) % 10_000).unwrap();
            std::net::SocketAddr::from(([127, 0, 0, 1], port))
        })
        .find(|addr| {
            std::net::TcpStream::connect_timeout(addr, Duration::from_millis(200))
                .is_err_and(|e| e.kind() == std::io::ErrorKind::ConnectionRefused)
        })
        .expect("a loopback port that refuses, below the ephemeral range")
}

#[tokio::test]
async fn a_closed_port_is_refused_and_a_silent_one_times_out_and_neither_waits_longer_than_its_deadline()
 {
    let closed = closed_port();
    let d = dial(&closed.to_string(), false, Duration::from_millis(500)).await;
    assert!(!d.answered);
    assert_eq!(d.why, "connection refused");
    // A listener that accepts and says nothing: connected, but ApiVersions is never answered.
    let silent = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = silent.local_addr().unwrap().to_string();
    let keep = tokio::spawn(async move {
        let (socket, _) = silent.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(socket);
    });
    let start = Instant::now();
    let d = dial(&addr, false, Duration::from_millis(300)).await;
    assert!(
        !d.answered && d.why.starts_with("timed out after 300 ms"),
        "{d:?}"
    );
    assert!(start.elapsed() < Duration::from_millis(900));
    // With TLS on only the connection is judged.
    assert!(dial(&addr, true, Duration::from_millis(300)).await.answered);
    keep.abort();
}

#[tokio::test]
async fn something_that_is_not_kafka_is_said_not_to_be() {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap().to_string();
    tokio::spawn(async move {
        let (mut s, _) = l.accept().await.unwrap();
        s.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(1)).await;
    });
    let d = dial(&addr, false, Duration::from_millis(500)).await;
    assert!(!d.answered);
    assert!(d.why.contains("not Kafka"), "{d:?}");
}
