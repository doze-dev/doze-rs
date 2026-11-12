//! More live group tests: an ACL-on broker, and group ids that are hostile input.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_groups;
mod support_write;

use axum::http::StatusCode;
use kafka_wire::api::Request;
use support::get;
use support_groups::*;
use support_write::{page, post_form};

#[tokio::test]
async fn on_a_broker_with_acls_a_caller_without_the_right_is_refused_as_a_client_would_be() {
    use kafka_auth::Principal;
    use kafka_broker::Caller;
    use kafka_wire::api::Response;
    let w = with_orders("g-acl", |b| b.acl_default_allow = false).await;
    commit(&w, "analytics", "orders", &[(0, 1)]).await;
    let acl = |res: (i8, &str), op: i8| {
        let mut a = kafka_wire::messages::create_acls_request::AclCreation::default();
        a.resource_type = res.0;
        a.resource_name = kafka_wire::StrBytes::from_string(res.1.to_owned());
        a.resource_pattern_type = 3;
        a.principal = kafka_wire::StrBytes::from_static_str("User:ANONYMOUS");
        a.host = kafka_wire::StrBytes::from_static_str("*");
        a.operation = op;
        a.permission_type = 3;
        a
    };
    let mut grant = kafka_wire::messages::CreateAclsRequest::default();
    grant.creations = vec![acl((3, "analytics"), 8), acl((2, "orders"), 8)];
    let Response::CreateAcls(r) = w.ask(3, Request::CreateAcls(grant)).await else {
        panic!()
    };
    assert_eq!(r.results[0].error_code, 0);
    let someone = Caller::new(Principal::anonymous(), Some("127.0.0.1".parse().unwrap()));
    let app = support_write::router_of(&w, w.backend.acting_as(someone));
    ok(&app, "/groups/analytics", None).await;
    let (_, reset) = page(
        &app,
        post_form(
            "/groups/analytics/reset",
            &[("topic", "orders"), ("to", "earliest")],
        ),
    )
    .await;
    assert!(
        reset.contains("GROUP_AUTHORIZATION_FAILED (30)")
            && reset.contains("Group authorization failed."),
        "{reset:.3500}"
    );
    let (_, del) = page(&app, post_form("/groups/analytics/delete", &[])).await;
    assert!(
        del.contains("GROUP_AUTHORIZATION_FAILED (30)"),
        "{del:.3000}"
    );
    let (nobody, _) = page(&app, get("/groups/search-indexer")).await;
    assert_eq!(
        nobody,
        StatusCode::NOT_FOUND,
        "a group the caller may not describe is not told apart from no group"
    );
    w.stop().await;
}

#[tokio::test]
async fn a_hostile_group_id_is_escaped_and_encoded_on_every_page() {
    let w = with_orders("g-hostile", |_| {}).await;
    let long = "g".repeat(255);
    for id in [
        "a/b?c%d é 😀",
        "<script>alert(1)</script>",
        "x\"onmouseover=\"y",
        &long,
    ] {
        commit(&w, id, "orders", &[(0, 1)]).await;
        let app = w.router();
        let enc = support_write::enc(id);
        for tail in [
            "",
            "/members",
            "/lag",
            "/lag?expiring=1",
            "/reset",
            "/delete",
            "/offsets/delete",
        ] {
            let body = ok(&app, &format!("/groups/{enc}{tail}"), None).await;
            assert!(!body.contains("<script>alert"), "{id} {tail}: unescaped");
            assert!(
                !body.contains("onmouseover=\"y"),
                "{id} {tail}: attribute break"
            );
            for link in support::local_links(&body) {
                assert!(
                    !link.contains(' ') && !link.contains('<') && !link.contains('"'),
                    "{id} {tail}: {link}"
                );
            }
        }
        let list = ok(&app, "/groups", None).await;
        assert!(!list.contains("<script>alert") && !list.contains("onmouseover=\"y"));
    }
    w.stop().await;
}
