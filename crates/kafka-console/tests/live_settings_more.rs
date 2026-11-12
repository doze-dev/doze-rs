//! The rule the Settings dialog teaches, hostile text on Settings, and a caller without rights: over a real broker in this process.
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
use kafka_console::settings_words::{DYNAMIC, least};
use support::get;
use support_access::{alter, app, router_with, seeded, start};
use support_write::{page, post_form, world};

/// CON-569: the rule the dialog teaches is the one the broker holds: the least value is taken and one less is refused.
#[tokio::test]
async fn the_rule_the_dialog_teaches_is_the_rule_the_broker_holds() {
    let w = seeded("set-rule").await;
    for (key, _, _) in DYNAMIC {
        if let Some(n) = least(key) {
            assert_eq!(
                alter(&w, "", key, &n.to_string(), true).await.0,
                0,
                "{key} takes {n}"
            );
            if key != "log.retention.ms" || n > i64::MIN {
                assert_eq!(
                    alter(&w, "", key, &(n - 1).to_string(), true).await.0,
                    42,
                    "{key} does not take {}",
                    n - 1
                );
            }
        }
    }
    assert_eq!(
        alter(&w, "", "log.retention.bytes", "-5", true).await.0,
        0,
        "retention.bytes takes any number"
    );
    assert_eq!(
        alter(&w, "", "log.segment.bytes", "2147483648", true)
            .await
            .0,
        42,
        "the 32-bit ones stop at 2,147,483,647"
    );
    assert_eq!(
        alter(&w, "", "log.segment.bytes", "2147483647", true)
            .await
            .0,
        0
    );
    w.stop().await;
}

/// CON-570: a hostile key, value or topic name is escaped and cannot start a line of the call.
#[tokio::test]
async fn hostile_text_is_escaped_and_cannot_start_a_line_of_the_call() {
    let w = seeded("set-hostile").await;
    let app = app(&w);
    let (status, doc) = page(
        &app,
        get("/settings/config/edit?key=%3Cscript%3Ealert(1)%3C/script%3E"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(!doc.contains("<script>alert(1)"), "{doc}");
    let (_, dlg) = page(&app, get("/settings/config/edit?key=log.retention.ms")).await;
    assert!(!dlg.contains("<script>alert(1)"), "{dlg}");
    let evil = "5\n  SET evil = 1\r\nSet-Cookie: x=1";
    let f = [
        ("key", "log.retention.ms"),
        ("was", "log.retention.ms"),
        ("scope", "all"),
        ("value", evil),
        ("check", "1"),
    ];
    let (res, text) = support::send(&app, post_form("/settings/config/edit", &f)).await;
    assert!(res.headers().get(axum::http::header::SET_COOKIE).is_none());
    // Kafka's own sentence repeats what was typed, as text; the call drawn beside it is one line for each part.
    let pre: String = text
        .split("<pre")
        .skip(1)
        .map(|p| p.split("</pre>").next().unwrap_or(""))
        .collect();
    assert!(
        !pre.contains("\n  SET evil = 1") && pre.contains("\\n  SET evil"),
        "{pre}"
    );
    let f = [
        ("key", "<img src=x onerror=alert(1)>"),
        ("was", "x"),
        ("scope", "all"),
        ("value", "1"),
    ];
    let (_, doc) = page(&app, post_form("/settings/config/edit", &f)).await;
    assert!(!doc.contains("<img src=x"), "{doc}");
    w.stop().await;
}

/// CON-571: a caller that may not alter configs is refused as a client would be, and the page still draws.
#[tokio::test]
async fn a_caller_without_rights_is_refused_in_kafkas_words_and_the_page_still_draws() {
    use kafka_auth::Principal;
    use kafka_broker::Caller;
    let w = world("set-acl", |b, _| b.acl_default_allow = false).await;
    let nobody = Caller::new(
        Principal::user("nobody"),
        Some("127.0.0.1".parse().unwrap()),
    );
    let denied = router_with(w.facts.clone(), start(false), w.backend.acting_as(nobody));
    let (status, doc) = page(&denied, get("/settings")).await;
    assert_eq!(status, StatusCode::OK, "{doc:.500}");
    let f = [
        ("key", "log.retention.ms"),
        ("was", "log.retention.ms"),
        ("scope", "all"),
        ("value", "5000"),
    ];
    let (status, doc) = page(&denied, post_form("/settings/config/edit", &f)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(doc.contains("CLUSTER_AUTHORIZATION_FAILED (31)"), "{doc}");
    // The console itself, a super user, is not stopped by the same broker.
    let (_, saved) = page(&app(&w), post_form("/settings/config/edit", &f)).await;
    assert!(saved.contains("<b>Saved.</b>"), "{saved}");
    w.stop().await;
}
