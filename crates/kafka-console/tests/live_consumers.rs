//! The Consumers tab of a topic over a real broker in the same process: groups that commit on the topic (CON-705), the lag
//! arithmetic the Groups pages use, an empty state, an ACL-on broker where the caller sees only what it may describe, and
//! group ids and topic names that are hostile input.
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
use support::get;
use support_groups::*;
use support_write::page;

#[tokio::test]
async fn the_consumers_tab_lists_the_groups_that_committed_on_the_topic_with_their_lag_per_partition()
 {
    let w = with_orders("c-live", |_| {}).await;
    // orders ends at 4, 3, 2.
    let member = stable_group(&w, "search-indexer", "range", &[("orders", &[0, 1, 2])]).await;
    commit_as_member(&w, "search-indexer", &member, 1, &[(0, 1), (1, 3), (2, 2)]).await;
    commit(&w, "analytics", "orders", &[(0, 4)]).await;
    // A group that reads another topic is not listed here.
    w.create_topic("other", 1, &[]).await;
    commit(&w, "elsewhere", "other", &[(0, 0)]).await;
    let app = w.router();
    let tab = ok(&app, "/topics/orders/consumers", None).await;
    assert!(
        tab.contains("href=\"/groups/search-indexer\"")
            && tab.contains("href=\"/groups/analytics\""),
        "{tab:.3000}"
    );
    assert!(
        !tab.contains("elsewhere"),
        "a group that reads another topic is not here"
    );
    assert!(
        tab.contains(">orders-0<") && tab.contains(">orders-2<"),
        "a row per partition of THIS topic"
    );
    assert!(
        tab.contains("3 behind"),
        "search-indexer is 3 records behind in orders-0: {tab:.3500}"
    );
    assert!(
        tab.contains("0 behind"),
        "analytics is caught up on orders-0 only"
    );
    assert!(
        tab.contains("Consumers<span class=\"n\">2</span>"),
        "the tab's count is the number listed"
    );
    let messages = ok(&app, "/topics/orders", None).await;
    assert!(
        messages.contains("href=\"/topics/orders/consumers\""),
        "the Messages tab links the Consumers tab inside the topic, not the Groups space"
    );
    assert!(
        !messages.contains("Consumers<span class=\"n\">2</span></a>")
            || messages.contains("/topics/orders/consumers")
    );
    w.stop().await;
}

#[tokio::test]
async fn a_topic_nobody_has_committed_on_says_so_and_a_missing_topic_is_not_found() {
    let w = with_orders("c-empty", |_| {}).await;
    let app = w.router();
    let tab = ok(&app, "/topics/orders/consumers", None).await;
    assert!(
        tab.contains("No consumer group has committed offsets for"),
        "{tab:.2500}"
    );
    assert!(tab.contains("Consumers<span class=\"n\">0</span>"));
    let (status, body) = page(&app, get("/topics/nope/consumers")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body:.300}");
    w.stop().await;
}

#[tokio::test]
async fn on_a_broker_with_acls_the_tab_lists_only_what_the_caller_may_describe() {
    use kafka_auth::Principal;
    use kafka_broker::Caller;
    use kafka_wire::api::{Request, Response};
    let w = with_orders("c-acl", |b| b.acl_default_allow = false).await;
    commit(&w, "analytics", "orders", &[(0, 1)]).await;
    commit(&w, "secret-group", "orders", &[(0, 1)]).await;
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
    let tab = ok(&app, "/topics/orders/consumers", None).await;
    assert!(tab.contains("href=\"/groups/analytics\""), "{tab:.3000}");
    assert!(
        !tab.contains("secret-group"),
        "a group the caller may not describe is not listed: {tab:.3000}"
    );
    w.stop().await;
}

#[tokio::test]
async fn hostile_group_and_topic_names_are_text_on_the_tab() {
    let w = with_orders("c-hostile", |_| {}).await;
    for id in [
        "<script>alert(1)</script>",
        "x\"onmouseover=\"y",
        "a/b?c%d é 😀",
    ] {
        commit(&w, id, "orders", &[(0, 1)]).await;
    }
    let app = w.router();
    let tab = ok(&app, "/topics/orders/consumers", None).await;
    assert!(!tab.contains("<script>alert"), "{tab:.3000}");
    assert!(!tab.contains("onmouseover=\"y"));
    for link in support::local_links(&tab) {
        assert!(
            !link.contains(' ') && !link.contains('<') && !link.contains('"'),
            "{link}"
        );
    }
    for name in [
        "%3Cscript%3Ealert(1)%3C%2Fscript%3E",
        "a%20b",
        "x%22onmouseover%3D%22y",
    ] {
        let (status, body) = page(&app, get(&format!("/topics/{name}/consumers"))).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(
            !body.contains("<script>alert") && !body.contains("onmouseover=\"y"),
            "{body:.400}"
        );
    }
    w.stop().await;
}
