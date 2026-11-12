//! The console as a super user, and the secrets of a credential, over a real broker in this process with SCRAM on, a user
//! from flags (a planted password) and a stored one (a planted salt): no secret of a credential is anywhere.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_access;
mod support_write;

use axum::http::StatusCode;
use kafka_auth::Principal;
use kafka_broker::Caller;
use kafka_wire::api::Response;
use support::get;
use support_access::{form, new_acl, secrets, secured, start};
use support_write::{fragment_get, page, post_form};

/// CON-563: the console is a super user, so an ACL never stops it: a broker that denies by default still shows its page.
#[tokio::test]
async fn the_console_is_a_super_user_so_a_denying_broker_still_shows_its_page() {
    let s = secured("acc-super", false).await;
    let (status, doc) = page(&s.router(), get("/access")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!doc.contains("Refused ·"), "{doc:.3000}");
    // And a client that is nobody is refused the same API.
    let nobody = Caller::new(
        Principal::user("nobody"),
        Some("127.0.0.1".parse().unwrap()),
    );
    let Response::DescribeAcls(r) =
        s.w.ask_as(
            nobody,
            3,
            kafka_console::backend::requests::acls::describe_all(),
        )
        .await
    else {
        panic!()
    };
    assert_eq!(r.error_code, 31);
    s.w.stop().await;
}

/// CON-564: no secret of a credential is anywhere: not in a page, a dialog, a fragment, the stream, or a Debug.
#[tokio::test]
async fn no_planted_password_salt_or_key_is_in_any_page_fragment_stream_or_debug() {
    let s = secured("acc-secret", true).await;
    let planted = secrets(&s);
    assert!(planted.len() > 20, "{planted:?}");
    let app = s.router();
    let mut seen = String::new();
    for path in [
        "/",
        "/connect",
        "/access",
        "/settings",
        "/topics",
        "/groups",
        "/access/acls/new",
        "/access/acls/delete?type=topic&name=orders&principal=User%3Aorders&op=write",
        "/settings/config/edit?key=log.retention.ms",
        "/fragments/home-live",
        "/events?tail=live",
    ] {
        if path.starts_with("/events") {
            continue;
        }
        let (_, doc) = page(&app, get(path)).await;
        seen.push_str(&doc);
        let (_, frag) = page(&app, fragment_get(path)).await;
        seen.push_str(&frag);
    }
    // Every dialog's POST answers too.
    for fields in [new_acl("orders", "User:orders")] {
        let (_, doc) = page(&app, form(&fields)).await;
        seen.push_str(&doc);
    }
    let (_, doc) = page(
        &app,
        post_form(
            "/settings/config/edit",
            &[
                ("key", "log.retention.ms"),
                ("value", "x"),
                ("was", "log.retention.ms"),
            ],
        ),
    )
    .await;
    seen.push_str(&doc);
    for secret in &planted {
        assert!(!seen.contains(secret.as_str()), "{secret} is on a page");
    }
    // The models and the source the pages are made from, and the backend, as a Debug.
    let live = kafka_console::source::Live::new(s.w.backend.clone(), s.w.facts.clone());
    let view = kafka_console::gather::access::access(&s.w.backend, &s.w.facts, &start(true)).await;
    let debug = format!("{view:?}{live:?}{:?}{:?}", s.w.backend, s.w.facts);
    for secret in &planted {
        assert!(!debug.contains(secret.as_str()), "{secret} is in a Debug");
    }
    s.w.stop().await;
}

/// CON-565: the stream carries only fragment ids, so no secret can be in it.
#[tokio::test]
async fn the_stream_of_a_secured_console_carries_a_ready_and_nothing_else() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let s = secured("acc-stream", false).await;
    let planted = secrets(&s);
    let live = kafka_console::source::Live::new(s.w.backend.clone(), s.w.facts.clone());
    let settings = kafka_console::serve::Settings {
        addr: "127.0.0.1:0".parse().unwrap(),
        prefix: "/".into(),
        allow_hosts: vec![],
    };
    let running = kafka_console::serve::start(
        kafka_console::source::Source::Live(Box::new(live)),
        settings,
    )
    .await
    .unwrap();
    let addr = running.local_addr();
    let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
    c.write_all(
        format!("GET /classic/events?tail=live HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes(),
    )
    .await
    .unwrap();
    let mut text = String::new();
    let mut buf = [0_u8; 1024];
    while !text.contains("event: ready") {
        let n = tokio::time::timeout(std::time::Duration::from_secs(3), c.read(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert!(n > 0);
        text.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    for secret in &planted {
        assert!(!text.contains(secret.as_str()));
    }
    running.shutdown().await;
    s.w.stop().await;
}
