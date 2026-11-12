//! More of the Topics pages over a real broker (ACLs, hostile records, fragments) in the same process, as the principal `User:__console`: seeded with topics of several
//! shapes (a compacted one, a gzip batch, headers, a value that is not UTF-8, an open transaction, a topic with a 249-byte
//! name), every route requested through `Router::oneshot`, and the answers compared with the boards on structure (the live
//! render of `tools/board-diff.ts`, written to `target/board-diff-live/`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
#[path = "live_topics/world.rs"]
mod world;

use axum::http::StatusCode;
use support::{get, send};
use world::*;

use kafka_auth::Principal;
use kafka_broker::Caller;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::batch::build::Rec;
use kafka_wire::messages::CreateAclsRequest;
use kafka_wire::messages::create_acls_request::AclCreation;

fn creation(topic: &str, principal: &str, op: i8) -> AclCreation {
    let mut c = AclCreation::default();
    c.resource_type = 2;
    c.resource_name = StrBytes::from_string(topic.to_owned());
    c.resource_pattern_type = 3;
    c.principal = StrBytes::from_string(principal.to_owned());
    c.host = StrBytes::from_static_str("*");
    c.operation = op;
    c.permission_type = 3;
    c
}

#[tokio::test]
async fn on_an_acl_broker_a_principal_that_may_describe_but_not_read_is_refused_in_kafkas_words() {
    let w = seeded("acl", |c| c.acl_default_allow = false).await;
    let mut r = CreateAclsRequest::default();
    r.creations = vec![creation("orders", "User:viewer", 8)];
    let Response::CreateAcls(r) = w.ask(3, Request::CreateAcls(r)).await else {
        panic!()
    };
    assert_eq!(r.results[0].error_code, 0);
    let viewer = Caller::new(
        Principal::user("viewer"),
        Some("127.0.0.1".parse().unwrap()),
    );
    let app = w.router_as(viewer);
    let list = page(&app, "/topics", None).await;
    assert!(
        list.contains(">orders</a>") && !list.contains(">gz.events</a>"),
        "only what it may describe: {list:.2500}"
    );
    let t = page(&app, "/topics/orders", None).await;
    assert!(
        t.contains("The broker refused the read: TOPIC_AUTHORIZATION_FAILED (29)."),
        "{t:.3000}"
    );
    assert_eq!(row_count(&t), 0);
    let (res, _) = send(&app, get("/topics/gz.events")).await;
    assert_eq!(
        res.status(),
        StatusCode::NOT_FOUND,
        "a topic it may not describe is not there"
    );
    // Somebody with no right at all sees an empty broker.
    let nobody = Caller::new(
        Principal::user("nobody"),
        Some("127.0.0.1".parse().unwrap()),
    );
    let none = page(&w.router_as(nobody), "/topics", None).await;
    assert!(none.contains("This broker holds no topics"), "{none:.1500}");
    // The console itself, a super user, reads everything.
    let all = page(&w.router(), "/topics/orders", None).await;
    assert!(all.contains("ord-5"));
    w.stop().await;
}

#[tokio::test]
async fn hostile_records_headers_and_names_from_a_real_broker_are_inert_on_every_page() {
    let w = world("evil", |_| {}).await;
    // Every character a topic name may hold, and the longest.
    let name = format!("a.b_c-D9{}", "z".repeat(249 - 8));
    w.create(&name, 1, &[]).await;
    let payloads = [
        "</script><script>alert(1)</script>".to_owned(),
        "\"><img src=x onerror=alert(1)>".to_owned(),
        "'><svg/onload=alert(1)>".to_owned(),
        "bidi \u{202e}txet\u{2066} nul \u{0} del \u{7f} bom \u{feff} ls \u{2028}".to_owned(),
        format!("long {}", "L".repeat(100_000)),
    ];
    let recs: Vec<Rec> = payloads
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut r = rec(i64::try_from(i).unwrap(), Some(p), p.as_bytes(), &[]);
            r.headers = vec![(p.as_bytes().to_vec(), p.as_bytes().to_vec())];
            r
        })
        .collect();
    for r in &recs {
        w.produce(&name, 0, plain(std::slice::from_ref(r)), None)
            .await;
    }
    let app = w.router();
    let mut pages = vec![
        "/topics".to_owned(),
        format!("/topics/{name}"),
        format!("/topics/{name}/messages?partition=0"),
        format!("/topics/{name}/messages?isolation=read_committed"),
        format!("/topics/{name}/partitions"),
        format!("/fragments/topic-messages?topic={name}"),
        format!("/fragments/topic-sentence?topic={name}"),
    ];
    for i in 0..payloads.len() {
        pages.push(format!("/topics/{name}/messages/0/{i}"));
    }
    for path in pages {
        let doc = page(&app, &path, None).await;
        for live in [
            "<script>alert",
            "<img src=x",
            "<svg/onload",
            "</script><script>",
        ] {
            assert!(!doc.contains(live), "{path}: {live:?}");
        }
        assert!(
            !doc.contains('\u{202e}')
                && !doc.contains('\u{2066}')
                && !doc.contains('\u{0}')
                && !doc.contains('\u{feff}'),
            "{path}: a control character came through"
        );
        if !path.starts_with("/fragments") {
            assert_eq!(
                doc.matches("<script").count(),
                1,
                "{path}: only the console's own script"
            );
        }
    }
    // The 100 KB line is cut at 64 KiB and says how big it is.
    let long = page(&app, &format!("/topics/{name}/messages/0/4"), None).await;
    assert!(long.contains("100,005 bytes"));
    assert!(long.contains("Shown up to 64.0 KiB of 97.6 KiB"));
    // A value that was text with a NUL in it is still text, with the NUL written out.
    let nul = page(&app, &format!("/topics/{name}/messages/0/3"), None).await;
    assert!(
        nul.contains("⟨U+0000⟩") && nul.contains("⟨U+202E⟩"),
        "{nul}"
    );
    w.stop().await;
}

#[tokio::test]
async fn a_fragment_is_what_the_page_holds_and_an_unchanged_one_is_a_304() {
    let w = seeded("frag", |_| {}).await;
    let app = w.router();
    let page_html = page(&app, "/topics/orders", None).await;
    assert!(
        page_html
            .contains("data-stream=\"/events?watch=topic-sentence,topic-messages,topic-wire\"")
    );
    let first = send(&app, get("/fragments/topic-messages?topic=orders")).await;
    assert_eq!(first.0.status(), StatusCode::OK);
    let tag = first.0.headers()[axum::http::header::ETAG]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(first.1.contains("ord-5"), "{}", first.1);
    let mut again = get("/fragments/topic-messages?topic=orders");
    again
        .headers_mut()
        .insert(axum::http::header::IF_NONE_MATCH, tag.parse().unwrap());
    let (res, _) = send(&app, again).await;
    assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
    let (res, _) = send(&app, get("/fragments/topic-messages?topic=nope")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    let (res, body) = send(&app, get("/fragments/topics-table")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body.contains("4 of 4 topics"), "{body}");
    w.stop().await;
}
