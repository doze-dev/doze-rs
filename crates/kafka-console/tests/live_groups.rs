//! The Groups pages over a real broker in the same process, with real members: classic and KIP-848 groups joined through the
//! group machines, commits and none, a topic recreated under a commit, a commit that expires, a rebalance in progress, every
//! refusal Kafka gives, an ACL-on broker, and group ids that are hostile input.
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
use kafka_wire::messages::{DeleteTopicsRequest, TopicName};
use support::get;
use support_groups::*;
use support_write::{World, page, post_form, world};

/// A broker with `orders` (3 partitions holding 4, 3 and 2 records).
async fn with_orders(name: &str, tweak: impl FnOnce(&mut kafka_broker::Config)) -> World {
    let w = world(name, |b, _| tweak(b)).await;
    w.create_topic("orders", 3, &[]).await;
    produce(&w, "orders", 0, 4).await;
    produce(&w, "orders", 1, 3).await;
    produce(&w, "orders", 2, 2).await;
    w
}

fn live_dir() -> std::path::PathBuf {
    let m = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let t = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || m.parent().unwrap().parent().unwrap().join("target"),
        Into::into,
    );
    t.join("board-diff-live")
}

/// A page, asserted a 200, written for the live board-diff under the board's id.
async fn ok(app: &axum::Router, path: &str, board: Option<&str>) -> String {
    let (status, body) = page(app, get(path)).await;
    assert_eq!(status, StatusCode::OK, "{path}: {body:.600}");
    if let Some(id) = board {
        std::fs::create_dir_all(live_dir()).unwrap();
        std::fs::write(live_dir().join(format!("{id}.html")), &body).unwrap();
    }
    body
}

#[tokio::test]
async fn a_classic_group_with_a_member_and_commits_shows_its_members_lag_and_what_the_protocol_hides()
 {
    let w = with_orders("g-classic", |_| {}).await;
    let member = stable_group(
        &w,
        "search-indexer",
        "roundrobin",
        &[("orders", &[0, 1, 2])],
    )
    .await;
    commit_as_member(&w, "search-indexer", &member, 1, &[(0, 1), (1, 3), (2, 2)]).await;
    let app = w.router();
    let list = ok(&app, "/groups", None).await;
    assert!(
        list.contains(">search-indexer</a>") && list.contains("classic, roundrobin"),
        "{list:.3000}"
    );
    assert!(
        list.contains("generation 1") && list.contains(">3<"),
        "the lag 3 in orders-0: {list:.3000}"
    );
    let detail = ok(&app, "/groups/search-indexer", Some("group-detail")).await;
    assert!(
        detail.contains(&member) && detail.contains("leader"),
        "{detail:.3000}"
    );
    assert!(
        detail.contains("Session timeout")
            && detail.contains("45 s")
            && detail.contains("Rebalance timeout")
            && detail.contains("60 s")
    );
    let members = ok(
        &app,
        "/groups/search-indexer/members",
        Some("group-members"),
    )
    .await;
    assert!(
        members.contains("Who holds which partition") && members.contains("all three"),
        "{members:.3000}"
    );
    let one = ok(
        &app,
        &format!("/groups/search-indexer/members/{member}"),
        Some("member-detail"),
    )
    .await;
    assert!(
        one.contains("Leader") && one.contains("orders-0 to 2"),
        "{one:.3000}"
    );
    let lag = ok(&app, "/groups/search-indexer/lag", Some("group-lag")).await;
    // orders ends at 4, 3, 2; the group committed 1, 3, 2: 3 behind in orders-0, caught up elsewhere.
    assert!(
        lag.contains("3 behind") && lag.contains(">orders-0<") || lag.contains("orders-0"),
        "{lag:.3500}"
    );
    assert!(lag.contains("<span class=\"n\">3</span>"), "{lag:.3500}");
    assert!(!lag.contains(">-"), "a negative number");
    w.stop().await;
}

async fn commit_as_member(
    w: &World,
    group: &str,
    member: &str,
    generation: i32,
    offsets: &[(i32, i64)],
) {
    use kafka_wire::messages::offset_commit_request::{
        OffsetCommitRequestPartition, OffsetCommitRequestTopic,
    };
    let Request::OffsetCommit(mut r) =
        kafka_console::backend::requests::admin_commit(group, "orders", offsets)
    else {
        panic!()
    };
    r.generation_id_or_member_epoch = generation;
    r.member_id = kafka_wire::StrBytes::from_string(member.to_owned());
    let _: Option<(OffsetCommitRequestPartition, OffsetCommitRequestTopic)> = None;
    let kafka_wire::api::Response::OffsetCommit(a) = w.ask(8, Request::OffsetCommit(r)).await
    else {
        panic!()
    };
    assert_eq!(a.topics[0].partitions[0].error_code, 0, "{a:?}");
}

#[tokio::test]
async fn a_consumer_group_of_the_new_protocol_is_described_by_its_own_api() {
    let w = with_orders("g-848", |_| {}).await;
    join_848(&w, "payments-workers", "m-one", &["orders"]).await;
    let app = w.router();
    let detail = ok(&app, "/groups/payments-workers", Some("group-detail-848")).await;
    assert!(
        detail.contains("consumer protocol (KIP-848)")
            && detail.contains("Group epoch")
            && detail.contains("m-one"),
        "{detail:.3500}"
    );
    assert!(detail.contains("Members, target and assigned"));
    let list = ok(&app, "/groups", None).await;
    assert!(list.contains("consumer (KIP-848)"), "{list:.3000}");
    w.stop().await;
}

#[tokio::test]
async fn a_group_with_no_commit_shows_no_commits_and_not_the_log_end_as_lag() {
    let w = with_orders("g-nocommit", |_| {}).await;
    stable_group(&w, "fulfilment", "range", &[("orders", &[0, 1, 2])]).await;
    let app = w.router();
    let lag = ok(&app, "/groups/fulfilment/lag", None).await;
    assert!(
        lag.contains("No commits yet") && lag.contains("No commit is not lag"),
        "{lag:.3000}"
    );
    assert!(lag.contains("lag not known"));
    assert!(!lag.contains(">4 behind<"), "no commit is not lag 4");
    let list = ok(&app, "/groups", None).await;
    assert!(list.contains("no commits"), "{list:.3000}");
    w.stop().await;
}

#[tokio::test]
async fn a_commit_beyond_the_end_of_a_recreated_topic_is_past_the_end_never_negative_lag() {
    let w = with_orders("g-recreate", |_| {}).await;
    // The topic is deleted and made again, shorter, while an app keeps its old position and commits it.
    let mut d = DeleteTopicsRequest::default();
    d.topic_names = vec![TopicName(kafka_wire::StrBytes::from_static_str("orders"))];
    d.timeout_ms = 5000;
    w.ask(6, Request::DeleteTopics(d)).await;
    w.create_topic("orders", 3, &[]).await;
    produce(&w, "orders", 0, 1).await;
    commit(&w, "analytics", "orders", &[(0, 4), (1, 3), (2, 2)]).await;
    let app = w.router();
    let lag = ok(&app, "/groups/analytics/lag", Some("lag-ahead")).await;
    assert!(
        lag.contains("3 past the end") && lag.contains("Kafka accepted this commit."),
        "{lag:.3500}"
    );
    assert!(
        lag.contains("analytics committed 4 on orders-0, but the log ends at 1"),
        "{lag:.3000}"
    );
    let list = ok(&app, "/groups", None).await;
    assert!(
        list.contains("Past the end") && list.contains("orders-0 is 3 beyond its log"),
        "{list:.3000}"
    );
    w.stop().await;
}

#[tokio::test]
async fn the_expiry_date_shown_is_the_moment_the_sweep_takes_the_commit() {
    let w = with_orders("g-expire", |b| {
        b.offsets_retention = std::time::Duration::from_secs(2)
    })
    .await;
    commit(&w, "analytics", "orders", &[(0, 1)]).await;
    let d = w.backend.group_detail("analytics").await.unwrap();
    let at = d.offsets[0].expires_ms.unwrap();
    let app = w.router();
    let page_text = ok(
        &app,
        "/groups/analytics/lag?expiring=1",
        Some("lag-expiring"),
    )
    .await;
    let shown = kafka_console::format::date_time(at);
    assert!(
        page_text.contains(shown.trim_end_matches(" UTC"))
            && page_text.contains("Retention window"),
        "{page_text:.3500}"
    );
    // Before the date a sweep leaves it; after, it takes it.
    w.broker.check_offsets().await;
    assert_eq!(committed(&w, "analytics").await.len(), 1);
    let now = kafka_console::source::now_ms();
    tokio::time::sleep(std::time::Duration::from_millis(
        u64::try_from((at - now).max(0)).unwrap() + 50,
    ))
    .await;
    w.broker.check_offsets().await;
    assert!(committed(&w, "analytics").await.is_empty());
    w.stop().await;
}

#[tokio::test]
async fn a_rebalance_in_progress_says_who_it_waits_for() {
    let w = with_orders("g-rebalance", |_| {}).await;
    let a = stable_group(&w, "fulfilment", "range", &[("orders", &[0, 1, 2])]).await;
    commit(&w, "analytics", "orders", &[(0, 1)]).await;
    stable_group(
        &w,
        "search-indexer",
        "roundrobin",
        &[("orders", &[0, 1, 2])],
    )
    .await;
    // A second member joins: the group must rebalance, and the first has to join again. Its join waits for the group.
    let j2 = tokio::spawn({
        let broker = std::sync::Arc::clone(&w.broker);
        async move { join_in_background(&broker, "fulfilment", "range", &["orders"]).await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let app = w.router();
    let detail = ok(&app, "/groups/fulfilment", Some("group-rebalancing")).await;
    assert!(
        detail.contains("Rebalancing")
            && detail.contains("has not joined again")
            && detail.contains("Not rejoined"),
        "{detail:.4000}"
    );
    assert!(detail.contains(&a) || detail.contains("Waiting for"));
    let lag = ok(&app, "/groups/fulfilment/lag", Some("lag-no-commits")).await;
    assert!(lag.contains("No commits yet"), "{lag:.2000}");
    let list = ok(&app, "/groups", Some("groups-list")).await;
    assert!(list.contains("fulfilment is rebalancing."), "{list:.3000}");
    j2.abort();
    w.stop().await;
}

async fn join_in_background(
    broker: &std::sync::Arc<kafka_broker::Broker<kafka_storage::fs::RealFs>>,
    group: &str,
    assignor: &str,
    topics: &[&str],
) {
    use kafka_auth::Principal;
    use kafka_broker::{Caller, Listener};
    let caller = Caller::new(Principal::console(), Some("127.0.0.1".parse().unwrap()));
    let listener = Listener {
        advertised_host: "127.0.0.1".into(),
        advertised_port: 1,
        client_host: "/127.0.0.1".into(),
    };
    let request = Request::JoinGroup(join_request(group, "", assignor, topics));
    let head = kafka_wire::header::RequestHead {
        key: request.api_key(),
        version: 3,
        correlation_id: 1,
        client_id: None,
    };
    let _ = broker.handle(&listener, &caller, &head, request).await;
}

#[tokio::test]
async fn a_reset_of_an_empty_group_moves_its_offsets_and_a_group_with_a_member_is_refused_in_kafkas_words()
 {
    let w = with_orders("g-reset", |_| {}).await;
    commit(&w, "analytics", "orders", &[(0, 0), (1, 0), (2, 0)]).await;
    let app = w.router();
    let preview = ok(
        &app,
        "/groups/analytics/reset?topic=orders&to=latest",
        Some("group-reset-offsets"),
    )
    .await;
    assert!(
        preview.contains("Committed now")
            && preview.contains("Reset 3 offsets")
            && preview.contains("OffsetCommit"),
        "{preview:.4000}"
    );
    let (status, _) = page(
        &app,
        post_form(
            "/groups/analytics/reset",
            &[("topic", "orders"), ("to", "latest")],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let mut now = committed(&w, "analytics").await;
    now.sort();
    assert_eq!(
        now,
        vec![
            ("orders".to_owned(), 0, 4),
            ("orders".to_owned(), 1, 3),
            ("orders".to_owned(), 2, 2)
        ]
    );
    let (_, shifted) = page(
        &app,
        post_form(
            "/groups/analytics/reset",
            &[("topic", "orders"), ("to", "shift"), ("value", "-1")],
        ),
    )
    .await;
    assert!(shifted.is_empty(), "a redirect has no body");
    assert_eq!(
        committed(&w, "analytics")
            .await
            .iter()
            .map(|c| c.2)
            .sum::<i64>(),
        6
    );
    let (bad, body) = page(
        &app,
        post_form(
            "/groups/analytics/reset",
            &[("topic", "orders"), ("to", "offset"), ("value", "x")],
        ),
    )
    .await;
    assert_eq!(bad, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body.contains("must be a whole number"));
    // A group with a member refuses a commit with no generation.
    let member = stable_group(&w, "search-indexer", "range", &[("orders", &[0, 1, 2])]).await;
    commit_as_member(&w, "search-indexer", &member, 1, &[(0, 1)]).await;
    let (status, refused) = page(
        &app,
        post_form(
            "/groups/search-indexer/reset",
            &[("topic", "orders"), ("to", "earliest")],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        refused.contains("UNKNOWN_MEMBER_ID (25)")
            && refused.contains("The coordinator is not aware of this member.")
            && refused.contains("What would pass"),
        "{refused:.4000}"
    );
    std::fs::write(live_dir().join("group-reset-refused.html"), &refused).ok();
    w.stop().await;
}

#[tokio::test]
async fn deleting_a_group_is_refused_with_members_and_done_when_empty() {
    let w = with_orders("g-delete", |_| {}).await;
    stable_group(&w, "search-indexer", "range", &[("orders", &[0, 1, 2])]).await;
    commit(&w, "analytics", "orders", &[(0, 1)]).await;
    let app = w.router();
    let dialog = ok(&app, "/groups/search-indexer/delete", Some("group-delete")).await;
    assert!(
        dialog.contains("This group is not empty."),
        "{dialog:.3000}"
    );
    let (status, refused) = page(&app, post_form("/groups/search-indexer/delete", &[])).await;
    assert_eq!(status, StatusCode::OK);
    std::fs::create_dir_all(live_dir()).unwrap();
    std::fs::write(live_dir().join("group-delete-refused.html"), &refused).unwrap();
    assert!(
        refused.contains("NON_EMPTY_GROUP (68)") && refused.contains("The group is not empty."),
        "{refused:.3500}"
    );
    let (status, _) = page(&app, post_form("/groups/analytics/delete", &[])).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (gone, _) = page(&app, get("/groups/analytics")).await;
    assert_eq!(gone, StatusCode::NOT_FOUND);
    w.stop().await;
}

#[tokio::test]
async fn deleting_offsets_forgets_them_and_a_subscribed_topic_is_refused() {
    let w = with_orders("g-offdel", |_| {}).await;
    commit(&w, "analytics", "orders", &[(0, 1), (1, 1)]).await;
    stable_group(&w, "search-indexer", "range", &[("orders", &[0, 1, 2])]).await;
    let app = w.router();
    ok(
        &app,
        "/groups/analytics/offsets/delete",
        Some("group-offset-delete"),
    )
    .await;
    let (status, _) = page(
        &app,
        post_form("/groups/analytics/offsets/delete", &[("topic", "orders")]),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(committed(&w, "analytics").await.is_empty());
    commit_to_member_group(&w).await;
    let (status, refused) = page(
        &app,
        post_form(
            "/groups/search-indexer/offsets/delete",
            &[("topic", "orders")],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        refused.contains("GROUP_SUBSCRIBED_TO_TOPIC (86)")
            && refused.contains("Deleting offsets of a topic is forbidden"),
        "{refused:.3500}"
    );
    w.stop().await;
}

async fn commit_to_member_group(w: &World) {
    // The member commits at its generation, so the group has something to forget.
    let g = w.backend.group_detail("search-indexer").await.unwrap();
    let id = g.members[0].id.clone();
    commit_as_member(w, "search-indexer", &id, g.generation, &[(0, 1)]).await;
}

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
        let enc = enc.replace("%2F", "%2F");
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
