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
use support::api_get;
use support_access::{form, new_acl, secrets, secured, start};
use support_write::{page, post_form};

/// CON-563: the console is a super user, so an ACL never stops it: a broker that denies by default still shows its page.
#[tokio::test]
async fn the_console_is_a_super_user_so_a_denying_broker_still_shows_its_page() {
    let s = secured("acc-super", false).await;
    let (status, doc) = page(&s.router(), api_get("/api/v1/access")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!doc.contains("AUTHORIZATION_FAILED"), "{doc:.3000}");
    assert!(
        doc.contains("User:orders") || doc.contains("acls"),
        "{doc:.3000}"
    );
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

/// CON-564: no secret of a credential is anywhere: not in an answer of the API (reads and writes), or a Debug.
#[tokio::test]
async fn no_planted_password_salt_or_key_is_in_any_page_fragment_stream_or_debug() {
    let s = secured("acc-secret", true).await;
    let planted = secrets(&s);
    assert!(planted.len() > 20, "{planted:?}");
    let app = s.router();
    let mut seen = String::new();
    let more = [
        "/api/v1/access",
        "/api/v1/access/acls/new",
        "/api/v1/access/acls/delete?type=topic&name=orders&principal=User%3Aorders&op=write",
        "/api/v1/settings",
        "/api/v1/settings/edit?key=log.retention.ms",
    ];
    for path in kafka_console::api::GET_PATHS.iter().chain(&more) {
        let (_, doc) = page(&app, api_get(path)).await;
        seen.push_str(&doc);
    }
    // Every dialog's POST answers too.
    for fields in [new_acl("orders", "User:orders")] {
        let (_, doc) = page(&app, form(&fields)).await;
        seen.push_str(&doc);
    }
    let mut edit = post_form(
        "/api/v1/settings/edit",
        &[
            ("key", "log.retention.ms"),
            ("value", "x"),
            ("was", "log.retention.ms"),
        ],
    );
    edit.headers_mut().insert("x-console", "1".parse().unwrap());
    let (_, doc) = page(&app, edit).await;
    seen.push_str(&doc);
    for secret in &planted {
        assert!(!seen.contains(secret.as_str()), "{secret} is in an answer");
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
