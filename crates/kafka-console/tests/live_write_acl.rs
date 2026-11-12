//! A GET never writes; and the pages when the caller is not a super user (what a client would be told on a broker with ACLs).
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;
mod support_write;

use axum::http::StatusCode;
use kafka_auth::Principal;
use kafka_broker::Caller;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::ApiKey;
use support::get;
use support_write::{World, page, post_form, router_of, world};

async fn produce_many(w: &World, topic: &str, n: usize) {
    let app = w.router();
    for i in 0..n {
        let value = format!("v{i}");
        let (status, doc) = page(
            &app,
            post_form(
                &format!("/topics/{topic}/produce"),
                &[
                    ("partition", "0"),
                    ("key", "k"),
                    ("value", &value),
                    ("acks", "1"),
                ],
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(doc.contains(&format!("at offset {i}.")), "{doc:.300}");
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_get_never_asks_the_broker_to_change_anything() {
    let w = world("spy", |_, _| {}).await;
    w.create_topic("orders", 3, &[]).await;
    produce_many(&w, "orders", 2).await;
    let (spy, log) = w.backend.recording();
    let app = router_of(&w, spy);
    let before = (
        w.topics().await,
        w.offset("orders", 0, -1).await,
        w.offset("orders", 0, -2).await,
    );
    let mut asked = 0;
    for route in kafka_console::routes::ROUTES
        .iter()
        .filter(|r| r.method == kafka_console::routes::Method::Get)
    {
        let path = route
            .path
            .replace("{topic}", "orders")
            .replace("{id}", "home-sentence")
            .replace("{name}", "x")
            .replace("{group}", "g")
            .replace("{member}", "m");
        if path == "/events" {
            continue;
        }
        for query in [
            "",
            "?step=review&name=x&partitions=2",
            "?partition=0&offset=1&count=9&key=k&value=v&confirm=orders",
        ] {
            let (status, _) = page(&app, get(&format!("{path}{query}"))).await;
            assert!(status.as_u16() < 500, "{path}{query} answered {status}");
            asked += 1;
        }
    }
    assert!(asked > 30, "{asked}");
    let reads = [
        ApiKey::Metadata,
        ApiKey::ListOffsets,
        ApiKey::DescribeConfigs,
        ApiKey::ListGroups,
        ApiKey::DescribeGroups,
        ApiKey::ConsumerGroupDescribe,
        ApiKey::OffsetFetch,
        ApiKey::ListTransactions,
        ApiKey::DescribeTransactions,
        ApiKey::ApiVersions,
        ApiKey::Fetch,
        ApiKey::DescribeProducers,
        // Slice 9h: the Access page reads the ACLs and the names of the users.
        ApiKey::DescribeAcls,
        ApiKey::DescribeUserScramCredentials,
    ];
    let calls = log.lock().unwrap().clone();
    assert!(
        !calls.is_empty(),
        "the pages asked the broker for what they show"
    );
    for call in &calls {
        assert!(
            reads.contains(call),
            "a GET called {call:?}, which is not a read"
        );
    }
    assert_eq!(
        (
            w.topics().await,
            w.offset("orders", 0, -1).await,
            w.offset("orders", 0, -2).await
        ),
        before,
        "and nothing changed"
    );
    // The same pages, then a POST: the spy sees the write.
    let (status, _) = page(
        &app,
        post_form("/topics/orders/produce", &[("value", "x"), ("acks", "1")]),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(log.lock().unwrap().contains(&ApiKey::Produce));
    w.stop().await;
}

fn acl(
    topic_or_cluster: (i8, &str),
    op: i8,
) -> kafka_wire::messages::create_acls_request::AclCreation {
    let mut a = kafka_wire::messages::create_acls_request::AclCreation::default();
    a.resource_type = topic_or_cluster.0;
    a.resource_name = StrBytes::from_string(topic_or_cluster.1.to_owned());
    a.resource_pattern_type = 3;
    a.principal = StrBytes::from_static_str("User:ANONYMOUS");
    a.host = StrBytes::from_static_str("*");
    a.operation = op;
    a.permission_type = 3;
    a
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn on_a_broker_with_acls_a_caller_that_may_only_describe_is_refused_as_a_client_would_be() {
    let w = world("acl", |b, _| b.acl_default_allow = false).await;
    w.create_topic("orders", 2, &[]).await;
    produce_many(&w, "orders", 3).await;
    // Someone who may DESCRIBE the topic and nothing else.
    let mut grant = kafka_wire::messages::CreateAclsRequest::default();
    grant.creations = vec![acl((2, "orders"), 8)];
    let Response::CreateAcls(r) = w.ask(3, Request::CreateAcls(grant)).await else {
        panic!()
    };
    assert_eq!(r.results[0].error_code, 0, "{r:?}");
    let someone = Caller::new(Principal::anonymous(), Some("127.0.0.1".parse().unwrap()));
    let app = router_of(&w, w.backend.acting_as(someone));
    let denied = "Refused · TOPIC_AUTHORIZATION_FAILED (29)";
    let (_, create) = page(
        &app,
        post_form(
            "/topics/new",
            &[
                ("name", "fresh"),
                ("partitions", "1"),
                ("cleanup.policy", "delete"),
                ("retention.ms", "1000"),
            ],
        ),
    )
    .await;
    assert!(
        create.contains(denied)
            && create.contains("Authorization failed.")
            && create.contains("User:__console"),
        "{create:.1500}"
    );
    let (_, produce) = page(
        &app,
        post_form(
            "/topics/orders/produce",
            &[
                ("partition", "0"),
                ("key", "k"),
                ("value", "v"),
                ("acks", "1"),
            ],
        ),
    )
    .await;
    assert!(
        produce.contains(denied) && produce.contains("Topic authorization failed."),
        "{produce:.1800}"
    );
    let (_, add) = page(
        &app,
        post_form("/topics/orders/partitions/add", &[("count", "5")]),
    )
    .await;
    assert!(add.contains(denied), "{add:.1500}");
    let (_, records) = page(
        &app,
        post_form(
            "/topics/orders/delete-records",
            &[("partition", "0"), ("offset", "1")],
        ),
    )
    .await;
    assert!(
        records.contains(denied) && records.contains("Topic authorization failed."),
        "{records:.1500}"
    );
    let (_, delete) = page(
        &app,
        post_form("/topics/orders/delete", &[("confirm", "orders")]),
    )
    .await;
    assert!(delete.contains(denied), "{delete:.1800}");
    // Nothing changed, and a topic the caller may not even describe is, to it, not there.
    assert_eq!(w.topics().await, [("orders".to_owned(), 2)]);
    assert_eq!(w.offset("orders", 0, -1).await, 3);
    w.create_topic("secret", 1, &[]).await;
    let (status, hidden) = page(&app, get("/topics/secret/delete")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(hidden.contains("There is no topic called"));
    w.stop().await;
}
