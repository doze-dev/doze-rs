//! A request queued behind the deletion of its group is put again, and the group it finds is the one there is now.

use super::*;
use crate::groups::consumer::Heartbeat;
use crate::groups::machine::{HeartbeatRequest, SyncRequest};

/// A group that is in memory with a Delete ahead of whatever is put next.
async fn with_a_deletion_queued(groups: &Groups, g: &GroupId) {
    emptied(groups, g).await;
    let sender = groups.open.lock().await.get(g).unwrap().clone();
    let (reply, _deleted) = oneshot::channel();
    sender
        .send(Command {
            req: Req::Delete,
            reply,
        })
        .await
        .unwrap();
}

fn consumer_heartbeat(epoch: i32) -> Heartbeat {
    Heartbeat {
        group: "g".into(),
        member_id: MemberId::new("m"),
        version: 1,
        epoch,
        instance: None,
        rack: None,
        rebalance_ms: 60_000,
        subscribed: (epoch == 0).then(|| vec!["t".to_owned()]),
        regex: None,
        assignor: None,
        owned: (epoch == 0).then(Vec::new),
        client_id: "consumer".into(),
        client_host: "/127.0.0.1".into(),
        rights: None,
    }
}

#[tokio::test]
async fn a_consumer_joining_behind_the_deletion_of_its_group_makes_the_group_again() {
    let groups = groups();
    let g = group("g");
    with_a_deletion_queued(&groups, &g).await;
    let answer = groups
        .put(&g, Req::ConsumerHeartbeat(consumer_heartbeat(0)))
        .await;
    let Some(Answer::ConsumerHeartbeat(a)) = answer else {
        panic!("not answered: {answer:?}")
    };
    assert!(a.error.is_none(), "{:?}", a.error);
    assert!(groups.open.lock().await.contains_key(&g));
}

#[tokio::test]
async fn the_other_requests_behind_a_deletion_are_answered_as_by_a_group_with_no_members() {
    let groups = groups();
    let g = group("g");
    with_a_deletion_queued(&groups, &g).await;
    let sync = Req::Sync(SyncRequest {
        member_id: MemberId::new("m").unwrap(),
        instance: None,
        generation: 1,
        protocol_type: None,
        protocol: None,
        assignments: vec![],
    });
    let Some(Answer::Sync(s)) = groups.put(&g, sync).await else {
        panic!("a sync behind a deletion is dropped")
    };
    assert_eq!(s.error, Some(ResponseError::UnknownMemberId));
    // And nothing was made for a request that cannot begin a group.
    assert!(!groups.open.lock().await.contains_key(&g));
    let beat = Req::Heartbeat(HeartbeatRequest {
        member_id: MemberId::new("m").unwrap(),
        instance: None,
        generation: 1,
    });
    assert!(matches!(
        groups.put(&g, beat).await,
        Some(Answer::Heartbeat(Some(ResponseError::UnknownMemberId)))
    ));
    let beat = Req::ConsumerHeartbeat(consumer_heartbeat(3));
    let Some(Answer::ConsumerHeartbeat(a)) = groups.put(&g, beat).await else {
        panic!("a consumer heartbeat behind a deletion is dropped")
    };
    assert!(a.error.is_some());
}
