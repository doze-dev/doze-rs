//! The Lab over a real broker, its rehearsals that happen once: a member's session, a rebalance, Compact now; who may; hostile names.
#![allow(
    clippy::too_many_lines,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    unused_imports
)]

mod support;
mod support_lab;
mod support_log;
mod support_write;

use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use kafka_auth::Principal;
use kafka_broker::{Caller, LabOutcome};
use kafka_console::Recorder;
use kafka_console::gather::lab as glab;
use kafka_console::lab::Lab;
use kafka_console::model::lab::{LabFields, LabKind, LabMade, LabOnceDone};
use kafka_console::model::live::LabDid;
use kafka_protocol::protocol::{Decodable, Encodable};
use kafka_server::fault::FaultTable;
use kafka_wire::StrBytes;
use kafka_wire::api::{Request, Response};
use kafka_wire::messages::join_group_request::JoinGroupRequestProtocol;
use kafka_wire::messages::*;
use kafka_wire::testing::{framed, request_payload};
use support_lab::*;
use support_log::{keyed, records};
use support_write::{World, world};

/// an expired member is gone through the group's own session path (the story says session expired, the next
/// heartbeat is told it is unknown); a forced rebalance makes the members rejoin, and says it was forced.
#[tokio::test]
async fn an_expired_member_and_a_forced_rebalance_go_through_the_groups_own_machine() {
    let l = lab_world("lab-groups").await;
    let a = classic_member(&l.w, "fulfilment").await;
    assert_eq!(heartbeat(&l.w, "fulfilment", &a, 1).await, 0);
    let form = glab::expire_form(&l.w.backend, "").await;
    assert_eq!(form.choices.len(), 1);
    assert!(form.choices[0].label.contains(&a));
    // Rebalance first: the member is told to rejoin at its next heartbeat.
    assert_eq!(
        glab::rebalance(&l.lab, &l.w.backend, "fulfilment").await,
        LabOnceDone::Done
    );
    assert_eq!(heartbeat(&l.w, "fulfilment", &a, 1).await, 27);
    let LabOnceDone::Invalid(again) = glab::rebalance(&l.lab, &l.w.backend, "fulfilment").await
    else {
        panic!("a rebalance already running is said so")
    };
    assert!(again.refused.unwrap().contains("rebalancing already"));
    let events = l.w.backend.group_events("fulfilment").await.unwrap();
    let causes: Vec<_> = events.events.iter().map(|e| format!("{e:?}")).collect();
    assert!(
        causes.iter().any(|c| c.contains("cause: Some(Forced)")),
        "{causes:?}"
    );
    // Another group, running: its member is expired, and the story says the session ran out.
    let b = classic_member(&l.w, "payments").await;
    let target = kafka_console::lab::act::member_value("payments", &b);
    assert_eq!(
        glab::expire(&l.lab, &l.w.backend, &target).await,
        LabOnceDone::Done
    );
    assert_eq!(
        heartbeat(&l.w, "payments", &b, 1).await,
        25,
        "UNKNOWN_MEMBER_ID"
    );
    let events = l.w.backend.group_events("payments").await.unwrap();
    let causes: Vec<_> = events.events.iter().map(|e| format!("{e:?}")).collect();
    assert!(
        causes
            .iter()
            .any(|c| c.contains("cause: Some(SessionExpired)")),
        "{causes:?}"
    );
    // No such group or member.
    let LabOnceDone::Invalid(f) = glab::rebalance(&l.lab, &l.w.backend, "nope").await else {
        panic!()
    };
    assert!(f.refused.unwrap().contains("no such group"));
    let home = glab::home(&l.lab, &l.w.backend).await;
    assert!(home.once.iter().filter(|o| o.ok).count() >= 2 && home.once.iter().any(|o| !o.ok));
    l.w.stop().await;
}

/// a group of the new protocol moves to the next epoch when a rebalance is forced.
#[tokio::test]
async fn a_forced_rebalance_moves_a_consumer_group_to_the_next_epoch() {
    let l = lab_world("lab-848").await;
    let mut hb = ConsumerGroupHeartbeatRequest::default();
    hb.group_id = GroupId(s("payments-workers"));
    hb.member_id = s("m-1");
    hb.member_epoch = 0;
    hb.rebalance_timeout_ms = 30_000;
    hb.subscribed_topic_names = Some(vec![TopicName(s("orders"))]);
    hb.topic_partitions = Some(vec![]);
    let Response::ConsumerGroupHeartbeat(r) = l.w.ask(0, Request::ConsumerGroupHeartbeat(hb)).await
    else {
        panic!()
    };
    assert_eq!(r.error_code, 0, "{r:?}");
    let before =
        l.w.backend
            .group_detail("payments-workers")
            .await
            .unwrap()
            .generation;
    assert_eq!(
        glab::rebalance(&l.lab, &l.w.backend, "payments-workers").await,
        LabOnceDone::Done
    );
    let after =
        l.w.backend
            .group_detail("payments-workers")
            .await
            .unwrap()
            .generation;
    assert_eq!(after, before + 1);
    l.w.stop().await;
}

/// Compact now runs the cleaner's round on a due partition, shows the planner's verdict first, and says no when nothing is due
/// or the topic does not compact.
#[tokio::test]
async fn compact_now_runs_the_real_round_and_refuses_with_a_sentence_otherwise() {
    let l = lab_world("lab-compact").await;
    l.w.create_topic(
        "inventory.levels",
        1,
        &[("cleanup.policy", "compact"), ("segment.bytes", "1024")],
    )
    .await;
    let none = glab::compact_form(&l.w.backend, true, "inventory.levels", 0)
        .await
        .unwrap();
    assert!(none.due.is_none() && none.blocked.is_some());
    let err = glab::compact(&l.lab, &l.w.backend, "inventory.levels", 0)
        .await
        .unwrap_err();
    let why = err.refused.unwrap();
    assert!(why.contains("nothing"), "{why}");
    l.w.fill("inventory.levels", 120, 5).await;
    let due = glab::compact_form(&l.w.backend, true, "inventory.levels", 0)
        .await
        .unwrap();
    assert!(
        due.due.as_deref().unwrap().contains("dirty against"),
        "{due:?}"
    );
    let _ = keyed(&[]);
    glab::compact(&l.lab, &l.w.backend, "inventory.levels", 0)
        .await
        .unwrap();
    let after = glab::compact_form(&l.w.backend, true, "inventory.levels", 0)
        .await
        .unwrap();
    assert!(after.due.is_none(), "the round left nothing due: {after:?}");
    let home = glab::home(&l.lab, &l.w.backend).await;
    assert_eq!(home.compact_target.as_deref(), Some("inventory.levels"));
    assert!(
        home.once
            .iter()
            .any(|o| o.ok && o.sentence.contains("Ran the cleaner"))
    );
    // A topic that does not compact.
    let plain = glab::compact_form(&l.w.backend, true, "orders", 0)
        .await
        .unwrap();
    assert!(plain.blocked.unwrap().contains("does not compact"));
    assert!(
        glab::compact_form(&l.w.backend, true, "missing", 0)
            .await
            .is_none()
    );
    l.w.stop().await;
}

/// the console may only do what the broker lets it: a caller without the right is refused, as a client would be.
#[tokio::test]
async fn a_caller_without_the_right_is_refused_by_the_broker() {
    let l = lab_world("lab-acl").await;
    let a = classic_member(&l.w, "g").await;
    let nobody =
        l.w.backend
            .acting_as(Caller::new(Principal::user("nobody"), None));
    // The default allows everyone here, so deny by an ACL-free broker with default deny is the other world; with default allow the
    // member is expired for any caller, and a missing group is still said.
    assert_eq!(nobody.lab_expire("g", &a).await, LabOutcome::Done);
    assert_eq!(
        nobody.lab_rebalance("absent").await,
        LabOutcome::NoSuchGroup
    );
    l.w.stop().await;
    let table = FaultTable::new();
    let t = Arc::clone(&table);
    let w = world("lab-acl-deny", move |bc, sc| {
        bc.acl_default_allow = false;
        sc.fault = Some(t);
    })
    .await;
    let nobody = w
        .backend
        .acting_as(Caller::new(Principal::user("nobody"), None));
    assert_eq!(nobody.lab_expire("g", "m").await, LabOutcome::Denied);
    assert_eq!(nobody.lab_rebalance("g").await, LabOutcome::Denied);
    assert_eq!(
        nobody.lab_clean("orders", 0).await,
        kafka_broker::CleanOutcome::Denied
    );
    assert_eq!(
        w.backend.lab_rebalance("g").await,
        LabOutcome::NoSuchGroup,
        "the console is a super user"
    );
    w.stop().await;
}

/// names that are hostile are text in every field a view-model carries, and a rule that cannot be made says why.
#[tokio::test]
async fn hostile_topic_and_client_patterns_are_kept_as_text_and_bad_fields_are_said() {
    let l = lab_world("lab-hostile").await;
    let hostile = "<img src=x onerror=alert(1)>";
    let id = make(
        &l,
        LabKind::Slow,
        &[
            ("api", "produce"),
            ("topic", hostile),
            ("client", "<script>"),
            ("delay", "10"),
            ("share", "every"),
            ("ends", "1"),
        ],
    )
    .await;
    let v = glab::rule(&l.lab, Some(&l.rec), id).unwrap();
    assert!(v.rule.name.contains(hostile) && v.rule.sentence.contains(hostile));
    let LabMade::Invalid(f) = glab::create(
        &l.lab,
        &l.w.backend,
        Some(&l.rec),
        LabKind::Slow,
        fields(&[
            ("api", "delete-everything"),
            ("delay", "0"),
            ("share", "x"),
            ("ends", "99"),
        ]),
    )
    .await
    else {
        panic!()
    };
    let bad: Vec<&str> = f.errors.iter().map(|(k, _)| k.as_str()).collect();
    for want in ["api", "delay", "share", "ends"] {
        assert!(bad.contains(&want), "{bad:?}");
    }
    // The 51st rule is refused with the limit in words.
    for n in 0..49 {
        make(
            &l,
            LabKind::Slow,
            &[
                ("api", "produce"),
                ("topic", &format!("t{n}")),
                ("delay", "10"),
                ("share", "every"),
                ("ends", "1"),
            ],
        )
        .await;
    }
    let LabMade::Invalid(f) = glab::create(
        &l.lab,
        &l.w.backend,
        Some(&l.rec),
        LabKind::Slow,
        fields(&[
            ("api", "fetch"),
            ("topic", "last"),
            ("delay", "10"),
            ("share", "every"),
            ("ends", "1"),
        ]),
    )
    .await
    else {
        panic!()
    };
    assert!(f.refused.unwrap().contains("50 rules"));
    l.w.stop().await;
}
