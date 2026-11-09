use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use kafka_meta::MetaStore;
use kafka_storage::sim::SimFs;
use kafka_wire::ResponseError;
use kafka_wire::ids::{GroupId, InstanceId, MemberId};
use tokio::sync::oneshot;

use super::*;
use crate::groups::machine::{JoinRequest, LeaveTarget, Protocol};

fn groups() -> Groups {
    let fs = SimFs::new();
    kafka_storage::fs::create_dirs_durably(&fs, Path::new("/data")).unwrap();
    let meta = Arc::new(MetaStore::open(&fs, Path::new("/data")).unwrap());
    Groups::new(
        Settings {
            initial_delay_ms: 0,
            consumer: crate::groups::ConsumerConfig {
                session_ms: 45_000,
                assign_interval_ms: 1_000,
            },
            ids: member_ids,
        },
        meta,
    )
}

fn group(name: &str) -> GroupId {
    GroupId::parse(name).unwrap()
}

/// A JoinGroup for a consumer.
fn join(member: Option<&MemberId>, instance: Option<&str>) -> Req {
    Req::Join(JoinRequest {
        member_id: member.cloned(),
        instance: instance.map(InstanceId::new),
        protocol_type: "consumer".into(),
        protocols: vec![Protocol {
            name: "range".into(),
            metadata: Bytes::new(),
        }],
        client_id: "test".into(),
        session_ms: 10_000,
        rebalance_ms: 30_000,
        issued: false,
        id_first: member.is_none() && instance.is_none(),
    })
}

fn joined(answer: Option<Answer>) -> JoinAnswer {
    match answer {
        Some(Answer::Join(a)) => a,
        other => panic!("expected a join answer, found {other:?}"),
    }
}

/// A group with no members that is in memory: someone joined and left.
async fn emptied(groups: &Groups, name: &GroupId) {
    // Without an id first: the registry would otherwise tell this client to come back with one.
    let mut req = join(None, None);
    if let Req::Join(j) = &mut req {
        j.id_first = false;
    }
    let id = joined(groups.ask(name, req).await).member_id.unwrap();
    let leave = Req::Leave(vec![LeaveTarget {
        member_id: Some(id),
        instance: None,
    }]);
    groups.ask(name, leave).await;
}

#[tokio::test]
async fn a_join_queued_behind_the_deletion_of_its_group_is_put_again_to_a_new_one_and_answered() {
    let groups = groups();
    let g = group("g");
    emptied(&groups, &g).await;
    let sender = groups.open.lock().await.get(&g).unwrap().clone();
    // The Delete is in the group's queue and has not run when the join is put behind it.
    let (reply, _deleted) = oneshot::channel();
    sender
        .send(Command {
            req: Req::Delete,
            reply,
        })
        .await
        .unwrap();
    let mut req = join(None, None);
    if let Req::Join(j) = &mut req {
        j.id_first = false;
    }
    let answer = joined(groups.put(&g, req).await);
    assert_eq!(
        answer.error, None,
        "not lost with the group it was queued for"
    );
    assert!(
        groups.open.lock().await.contains_key(&g),
        "and it made the group again"
    );
}

#[tokio::test]
async fn a_group_with_a_full_queue_is_busy_and_is_not_waited_for_when_deleting() {
    let groups = groups();
    let g = group("busy");
    let mut req = join(None, None);
    if let Req::Join(j) = &mut req {
        j.id_first = false;
    }
    joined(groups.ask(&g, req).await);
    let sender = groups.open.lock().await.get(&g).unwrap().clone();
    // Fill the queue without letting the group's task run.
    let mut replies = Vec::new();
    while let Ok(permit) = sender.try_reserve() {
        let (reply, answer) = oneshot::channel();
        permit.send(Command {
            req: Req::Describe,
            reply,
        });
        replies.push(answer);
    }
    let (outcome, cleaned) = groups.delete_with(&g, || ()).await;
    assert_eq!((outcome, cleaned), (Deleted::NotEmpty, None));
}

#[tokio::test]
async fn deleting_cleans_up_only_when_the_group_is_gone_or_was_never_there() {
    let groups = groups();
    let (with, without, never) = (group("with"), group("without"), group("never"));
    let mut req = join(None, None);
    if let Req::Join(j) = &mut req {
        j.id_first = false;
    }
    joined(groups.ask(&with, req).await);
    emptied(&groups, &without).await;
    let (outcome, cleaned) = groups.delete_with(&with, || "cleaned").await;
    assert_eq!(
        (outcome, cleaned),
        (Deleted::NotEmpty, None),
        "it has a member"
    );
    let (outcome, cleaned) = groups.delete_with(&without, || "cleaned").await;
    assert_eq!((outcome, cleaned), (Deleted::Gone, Some("cleaned")));
    let (outcome, cleaned) = groups.delete_with(&never, || "cleaned").await;
    assert_eq!((outcome, cleaned), (Deleted::NoSuchGroup, Some("cleaned")));
}

#[tokio::test(start_paused = true)]
async fn only_so_many_ids_are_outstanding_and_room_is_made_by_time() {
    let mut groups = groups();
    groups.most_issued = 3;
    let g = group("g");
    for _ in 0..3 {
        let told = joined(groups.ask(&g, join(None, None)).await);
        assert_eq!(told.error, Some(ResponseError::MemberIdRequired));
    }
    let full = joined(groups.ask(&g, join(None, None)).await);
    assert_eq!(full.error, Some(ResponseError::CoordinatorNotAvailable));
    tokio::time::sleep(Duration::from_secs(301)).await;
    let again = joined(groups.ask(&g, join(None, None)).await);
    assert_eq!(
        again.error,
        Some(ResponseError::MemberIdRequired),
        "the old ones have passed"
    );
}

#[tokio::test(start_paused = true)]
async fn ids_past_their_time_are_cleared_from_the_table_now_and_then() {
    let groups = groups();
    let g = group("g");
    for _ in 0..2 {
        groups.ask(&g, join(None, None)).await;
    }
    assert_eq!(groups.issued().ids.len(), 2);
    tokio::time::sleep(Duration::from_secs(301)).await;
    groups.ask(&g, join(None, None)).await;
    assert_eq!(
        groups.issued().ids.len(),
        1,
        "the two old ones are gone, not only unusable"
    );
}

#[tokio::test]
async fn an_issued_id_with_an_instance_id_is_refused_and_makes_no_group() {
    let groups = groups();
    let g = group("g");
    let told = joined(groups.ask(&g, join(None, None)).await);
    let id = told.member_id.unwrap();
    // Ids are issued to clients with no instance; one sent back with an instance is not a flow the broker has.
    let refused = joined(groups.ask(&g, join(Some(&id), Some("pod"))).await);
    assert_eq!(refused.error, Some(ResponseError::UnknownMemberId));
    assert!(
        groups.list().await.is_empty(),
        "no group was made to say so"
    );
}

#[tokio::test]
async fn a_join_put_to_a_group_whose_task_has_already_gone_makes_the_group_again() {
    let groups = groups();
    let g = group("g");
    emptied(&groups, &g).await;
    let sender = groups.open.lock().await.get(&g).unwrap().clone();
    // The group is deleted behind the registry's back, so that its table still holds a sender nobody listens to.
    let (reply, deleted) = oneshot::channel();
    sender
        .send(Command {
            req: Req::Delete,
            reply,
        })
        .await
        .unwrap();
    assert!(matches!(deleted.await, Ok(Answer::Delete(None))));
    while !sender.is_closed() {
        tokio::task::yield_now().await;
    }
    let mut req = join(None, None);
    if let Req::Join(j) = &mut req {
        j.id_first = false;
    }
    assert_eq!(joined(groups.put(&g, req).await).error, None);
}

#[tokio::test]
async fn a_join_that_is_refused_is_told_why_even_where_the_group_is_not_there() {
    use crate::groups::consumer::Heartbeat;
    let groups = groups();
    let join = |assignor: Option<&str>, regex: Option<&str>, member: Option<&str>| Heartbeat {
        group: "pg".into(),
        member_id: member.and_then(MemberId::new),
        version: 1,
        epoch: 0,
        instance: None,
        rack: None,
        rebalance_ms: 60_000,
        subscribed: Some(vec!["t".into()]),
        regex: regex.map(str::to_owned),
        assignor: assignor.map(str::to_owned),
        owned: Some(vec![]),
        client_id: "c".into(),
    };
    let ask = |h: Heartbeat| {
        let groups = &groups;
        async move {
            match groups.ask(&group("pg"), Req::ConsumerHeartbeat(h)).await {
                Some(Answer::ConsumerHeartbeat(a)) => a.error.map(|(e, _)| e),
                other => panic!("{other:?}"),
            }
        }
    };
    assert_eq!(
        ask(join(Some("bogus"), None, Some("m"))).await,
        Some(ResponseError::UnsupportedAssignor)
    );
    assert_eq!(
        ask(join(None, Some("("), Some("m"))).await,
        Some(ResponseError::InvalidRegularExpression)
    );
    assert_eq!(
        ask(join(None, None, None)).await,
        Some(ResponseError::InvalidRequest)
    );
    assert!(
        groups.list().await.is_empty(),
        "and no group was made by any of them"
    );
}

mod queued;
