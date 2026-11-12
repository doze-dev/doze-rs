//! Metadata under ACLs, part two: implication, prefixes, the default, hosts, and auto-creation.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

mod support;

use kafka_broker::Caller;
use support::acls::{
    ALL, ALLOW, CLUSTER, CREATE, DENY, DESCRIBE, LITERAL, PREFIXED, READ, TOPIC, WRITE,
    closed_world, creation, grant, on, shown, topics_of, user,
};
use support::{rig_with, topic};

#[tokio::test]
async fn read_write_delete_and_alter_imply_describe_but_a_deny_is_exact() {
    let rig = rig_with(closed_world);
    rig.create(vec![topic("t", 1)], false).await;
    let ok = |r: kafka_wire::messages::MetadataResponse| r.topics[0].error_code;
    let probe = |who: &'static str| {
        let rig = &rig;
        async move { ok(rig.metadata_as(&user(who), 12, Some(&["t"]), false).await) }
    };
    grant(
        &rig,
        vec![
            on("t", "User:reader", READ),
            on("t", "User:writer", WRITE),
            on("t", "User:maker", CREATE),
        ],
    )
    .await;
    assert_eq!(probe("reader").await, 0);
    assert_eq!(probe("writer").await, 0);
    assert_eq!(probe("maker").await, 29, "CREATE implies nothing");
    // DENY READ does not deny DESCRIBE; DENY DESCRIBE does; DENY ALL does.
    grant(
        &rig,
        vec![creation((TOPIC, "t", LITERAL), "User:reader", READ, DENY)],
    )
    .await;
    assert_eq!(probe("reader").await, 0);
    grant(
        &rig,
        vec![creation(
            (TOPIC, "t", LITERAL),
            "User:reader",
            DESCRIBE,
            DENY,
        )],
    )
    .await;
    assert_eq!(probe("reader").await, 29);
    grant(
        &rig,
        vec![creation((TOPIC, "t", LITERAL), "User:writer", ALL, DENY)],
    )
    .await;
    assert_eq!(probe("writer").await, 29);
}

#[tokio::test]
async fn prefixes_wildcards_and_the_any_user_are_honoured() {
    let rig = rig_with(closed_world);
    rig.create(
        vec![
            topic("orders-eu", 1),
            topic("orders-us", 1),
            topic("billing", 1),
        ],
        false,
    )
    .await;
    grant(
        &rig,
        vec![
            creation((TOPIC, "orders-", PREFIXED), "User:alice", DESCRIBE, ALLOW),
            on("billing", "User:*", DESCRIBE),
            creation((TOPIC, "orders-us", LITERAL), "User:alice", DESCRIBE, DENY),
        ],
    )
    .await;
    let r = rig.metadata_as(&user("alice"), 12, None, false).await;
    let mut names: Vec<_> = shown(&r).into_iter().map(|t| t.0).collect();
    names.sort();
    assert_eq!(
        names,
        ["billing", "orders-eu"],
        "the prefix allows, the literal deny wins over it"
    );
    let r = rig.metadata_as(&user("zed"), 12, None, false).await;
    assert_eq!(
        shown(&r).into_iter().map(|t| t.0).collect::<Vec<_>>(),
        ["billing"]
    );
}

#[tokio::test]
async fn the_default_applies_only_to_a_topic_nothing_covers() {
    let rig = rig_with(|c| {
        c.super_users.insert("User:admin".to_owned());
    });
    rig.create(vec![topic("covered", 1), topic("free", 1)], false)
        .await;
    grant(&rig, vec![on("covered", "User:alice", DESCRIBE)]).await;
    // allow.everyone.if.no.acl.found is true here: bob sees `free`, not `covered`.
    let r = rig
        .metadata_as(&user("bob"), 12, Some(&["covered", "free"]), false)
        .await;
    assert_eq!(
        shown(&r),
        [("free".into(), 0, 1), ("covered".into(), 29, 0)]
    );
    // The host is part of it: an ACL for another host covers the topic and so turns the default off.
    let mut elsewhere = on("free", "User:bob", DESCRIBE);
    elsewhere.host = kafka_wire::StrBytes::from_string("9.9.9.9".to_owned());
    grant(&rig, vec![elsewhere]).await;
    let r = rig
        .metadata_as(&user("bob"), 12, Some(&["free"]), false)
        .await;
    assert_eq!(shown(&r), [("free".into(), 29, 0)]);
}

#[tokio::test]
async fn an_acl_for_a_host_applies_to_that_address_only() {
    let rig = rig_with(closed_world);
    rig.create(vec![topic("t", 1)], false).await;
    let mut a = on("t", "User:alice", DESCRIBE);
    a.host = kafka_wire::StrBytes::from_string("10.0.0.1".to_owned());
    grant(&rig, vec![a]).await;
    let from = |ip: &str| {
        Caller::new(
            kafka_auth::Principal::user("alice"),
            Some(ip.parse().unwrap()),
        )
    };
    assert_eq!(
        rig.metadata_as(&from("10.0.0.1"), 12, Some(&["t"]), false)
            .await
            .topics[0]
            .error_code,
        0
    );
    assert_eq!(
        rig.metadata_as(&from("10.0.0.2"), 12, Some(&["t"]), false)
            .await
            .topics[0]
            .error_code,
        29
    );
    assert_eq!(
        rig.metadata_as(&Caller::anonymous(None), 12, Some(&["t"]), false)
            .await
            .topics[0]
            .error_code,
        29
    );
}

#[tokio::test]
async fn a_missing_topic_is_made_only_for_a_caller_that_may_create_it() {
    let rig = rig_with(closed_world);
    grant(
        &rig,
        vec![
            on("a", "User:maker", DESCRIBE),
            on("a", "User:maker", CREATE),
            on("b", "User:maker", DESCRIBE),
            on("c", "User:cluster", DESCRIBE),
            on("d", "User:cluster", DESCRIBE),
            creation(
                (CLUSTER, "kafka-cluster", LITERAL),
                "User:cluster",
                CREATE,
                ALLOW,
            ),
            on("e", "User:nodescribe", CREATE),
        ],
    )
    .await;
    // `a`: describe and create on the topic; `b`: describe only. The one that may not create is 29, listed after.
    let r = rig
        .metadata_as(&user("maker"), 12, Some(&["b", "a"]), true)
        .await;
    assert_eq!(
        shown(&r),
        [("a".into(), 3, 0), ("b".into(), 29, 0)],
        "the first answer says it is not there yet"
    );
    assert_eq!(topics_of(&rig).await, ["a"], "only a was made");
    // CREATE on the cluster covers every topic it may describe.
    let r = rig
        .metadata_as(&user("cluster"), 12, Some(&["c", "d"]), true)
        .await;
    assert_eq!(shown(&r), [("c".into(), 3, 0), ("d".into(), 3, 0)]);
    assert_eq!(topics_of(&rig).await, ["a", "c", "d"]);
    // CREATE without DESCRIBE: describe-denied wins, nothing is made.
    let r = rig
        .metadata_as(&user("nodescribe"), 12, Some(&["e"]), true)
        .await;
    assert_eq!(shown(&r), [("e".into(), 29, 0)]);
    assert!(!topics_of(&rig).await.contains(&"e".to_owned()));
    // A caller that did not allow auto-creation makes nothing and is not asked for CREATE.
    let r = rig
        .metadata_as(&user("maker"), 12, Some(&["b"]), false)
        .await;
    assert_eq!(shown(&r), [("b".into(), 3, 0)]);
    // The super user creates any.
    rig.metadata_as(&user("admin"), 12, Some(&["z"]), true)
        .await;
    assert!(topics_of(&rig).await.contains(&"z".to_owned()));
}
