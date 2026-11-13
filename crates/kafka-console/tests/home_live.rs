//! Home as the API answers it (audit A13) and what it may and may not dial (audit A2), on a real broker.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod world;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use axum::http::{StatusCode, header};
use kafka_console::model::Verdict;
use kafka_console::probe::Prober;
use support::{api_get, send};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use world::world;

/// A listener that counts the connections made to it, and answers ApiVersions the way a broker does.
async fn counting_kafka() -> (u16, Arc<AtomicUsize>) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let dials = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&dials);
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                return;
            };
            seen.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut head = [0_u8; 4];
                if s.read_exact(&mut head).await.is_err() {
                    return;
                }
                let mut rest = vec![0_u8; i32::from_be_bytes(head) as usize];
                if s.read_exact(&mut rest).await.is_err() {
                    return;
                }
                // Length 6: the correlation id the check sent (7) and error code 0.
                let _ = s.write_all(&[0, 0, 0, 6, 0, 0, 0, 7, 0, 0]).await;
            });
        }
    });
    (port, dials)
}

/// A listener that accepts and never says anything: a black hole that is reachable offline.
async fn black_hole() -> (u16, tokio::task::JoinHandle<()>) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((s, _)) = l.accept().await {
            held.push(s);
        }
    });
    (port, task)
}

/// `GET /api/v1/home` as JSON.
async fn home(app: &axum::Router) -> serde_json::Value {
    let (res, body) = send(app, api_get("/api/v1/home")).await;
    assert_eq!(res.status(), StatusCode::OK, "{body}");
    serde_json::from_str(&body).unwrap()
}

/// `POST /api/v1/probe`, as the Check now button sends it.
fn probe_now() -> axum::http::Request<axum::body::Body> {
    axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/probe")
        .header(header::HOST, support::HOST)
        .header("x-console", "1")
        .header("sec-fetch-site", "same-origin")
        .body(axum::body::Body::empty())
        .unwrap()
}

#[tokio::test]
async fn creating_a_topic_flips_title_sentence_and_cards_in_the_one_fragment_without_a_reload() {
    let w = world("flip", |_, _| {}).await;
    let app = w.router();
    let quiet = home(&app).await;
    assert_eq!(quiet["home"]["state"]["kind"], "quiet", "{quiet}");
    assert_eq!(quiet["words"]["title"], "Your broker is up");
    w.create_topic("orders").await;
    // The next read (a refresh of the page) is the steady Home: no reload of anything but the data.
    let steady = home(&app).await;
    assert_eq!(steady["home"]["state"]["kind"], "steady", "{steady}");
    assert_eq!(steady["words"]["title"], "Home");
    assert_ne!(quiet["words"]["sub"], steady["words"]["sub"]);
    assert!(steady["words"]["attention_headline"].is_null());
    w.stop().await;
}

#[tokio::test]
async fn the_live_fragments_never_dial_and_a_page_load_dials_once_in_thirty_seconds() {
    let (port, dials) = counting_kafka().await;
    let w = world("dials", |_, s| s.advertised_port = Some(port)).await;
    let app = w.router();
    // Loading Home makes the check: one dial of the advertised address.
    home(&app).await;
    assert_eq!(dials.load(Ordering::SeqCst), 1);
    // Sixty-six seconds of Home open with a 2 s refresh is thirty-three reads: the result is reused, nothing dials again.
    for _ in 0..33 {
        let v = home(&app).await;
        assert!(v["home"]["probe"]["rules"].is_array(), "{v}");
    }
    assert_eq!(
        dials.load(Ordering::SeqCst),
        1,
        "a refresh of Home dialled the advertised address again"
    );
    // Check now is a person's: it dials again, whatever the age of the result.
    let (res, _) = send(&app, probe_now()).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(dials.load(Ordering::SeqCst), 2);
    w.stop().await;
}

#[tokio::test]
async fn a_dead_advertised_address_does_not_slow_a_fragment() {
    let (port, hole) = black_hole().await;
    let w = world("dead", |_, s| s.advertised_port = Some(port)).await;
    let app = w.router();
    let started = Instant::now();
    // The check against it gives up at its deadline: Home is never held longer.
    home(&app).await;
    assert!(
        started.elapsed() < kafka_console::probe::DEADLINE + Duration::from_millis(400),
        "{:?}",
        started.elapsed()
    );
    hole.abort();
    w.stop().await;
}

#[tokio::test]
async fn a_result_is_reused_until_it_is_old_and_callers_at_once_share_one_set_of_dials() {
    let (port, dials) = counting_kafka().await;
    let w = world("single", |_, s| s.advertised_port = Some(port)).await;
    let p = Arc::new(Prober::with_fresh(
        w.facts.clone(),
        w.backend.clone(),
        Duration::from_millis(300),
    ));
    // Twenty callers at once: one check.
    let mut calls = Vec::new();
    for _ in 0..20 {
        let p = Arc::clone(&p);
        calls.push(tokio::spawn(async move { p.cached().await }));
    }
    for c in calls {
        assert_eq!(c.await.unwrap().verdict, Verdict::Reachable);
    }
    assert_eq!(dials.load(Ordering::SeqCst), 1, "not one check for all");
    // Inside the time it is reused, after it a new check is made.
    p.cached().await;
    assert_eq!(dials.load(Ordering::SeqCst), 1);
    tokio::time::sleep(Duration::from_millis(350)).await;
    p.cached().await;
    assert_eq!(dials.load(Ordering::SeqCst), 2);
    // "Check again" at once from many hands is one check too.
    let mut calls = Vec::new();
    for _ in 0..20 {
        let p = Arc::clone(&p);
        calls.push(tokio::spawn(async move { p.run().await }));
    }
    for c in calls {
        c.await.unwrap();
    }
    assert_eq!(dials.load(Ordering::SeqCst), 3);
    w.stop().await;
}

#[tokio::test]
async fn a_probe_against_a_black_hole_finishes_in_its_deadline_and_a_result_that_claims_nothing_is_not_reused()
 {
    use kafka_console::probe::{ADVERTISED_DEADLINE, DEADLINE, LISTENER_DEADLINE};
    // The documented numbers (CON-11): a dial of the listener 700 ms, of the advertised address 1 s, the whole 1.5 s.
    assert_eq!(LISTENER_DEADLINE, Duration::from_millis(700));
    assert_eq!(ADVERTISED_DEADLINE, Duration::from_millis(1000));
    assert_eq!(DEADLINE, Duration::from_millis(1500));
    assert!(LISTENER_DEADLINE <= DEADLINE && ADVERTISED_DEADLINE <= DEADLINE);
    // The advertised address accepts and says nothing.
    let (port, hole) = black_hole().await;
    let w = world("hole", |_, s| s.advertised_port = Some(port)).await;
    let p = Prober::new(w.facts.clone(), w.backend.clone());
    let started = Instant::now();
    let v = p.run().await;
    let took = started.elapsed();
    assert!(
        took >= ADVERTISED_DEADLINE - Duration::from_millis(100)
            && took < ADVERTISED_DEADLINE + Duration::from_millis(400),
        "{took:?}"
    );
    assert_eq!(v.verdict, Verdict::NotReachable, "{v:?}");
    assert!(
        !v.rules[2].ok && format!("{:?}", v.rules[2].detail).contains("timed out after"),
        "{v:?}"
    );
    hole.abort();
    w.stop().await;
    // The listener itself is the black hole: rule 1 fails inside 700 ms, not after the whole check.
    let (port, hole) = black_hole().await;
    let mut w = world("hole2", |_, _| {}).await;
    w.facts.listen = format!("127.0.0.1:{port}").parse().unwrap();
    let p = Prober::new(w.facts.clone(), w.backend.clone());
    let started = Instant::now();
    let v = p.run().await;
    assert!(
        started.elapsed() < LISTENER_DEADLINE + Duration::from_millis(400),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(v.verdict, Verdict::NotReachable, "{v:?}");
    assert!(!v.rules[0].ok, "{v:?}");
    hole.abort();
    w.stop().await;
}
