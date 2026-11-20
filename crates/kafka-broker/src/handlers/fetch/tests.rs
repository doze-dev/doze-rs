use std::time::Duration;

use kafka_storage::sim::SimFs;
use kafka_wire::StrBytes;
use kafka_wire::messages::FetchRequest;
use kafka_wire::messages::fetch_request::{FetchPartition, FetchTopic};

use super::wait::Woke;
use super::*;
use crate::Config;

#[tokio::test(start_paused = true)]
async fn a_change_to_the_topics_wakes_a_waiting_fetch_once_and_not_again_until_the_next_change() {
    let fs = SimFs::new();
    let broker = Broker::open(fs, Config::new("/data".into())).unwrap();
    let mut req = FetchRequest::default();
    let mut topic = FetchTopic::default();
    topic.topic = kafka_wire::messages::TopicName(StrBytes::from_static_str("nothing-here"));
    topic.partitions.push(FetchPartition::default());
    req.topics.push(topic);

    let (_wanted, mut watching) = broker
        .resolve_and_watch(12, &crate::Caller::anonymous(None), &req)
        .await;
    broker.topics_changed.send_modify(|n| *n += 1);
    assert_eq!(watching.changed().await, Woke::Topics);
    // What the fetch does on that wake: look again and watch again. The change it just saw must not be seen a second
    // time, or the fetch would loop at full speed until its wait ran out.
    let (_wanted, mut watching) = broker
        .resolve_and_watch(12, &crate::Caller::anonymous(None), &req)
        .await;
    let again = tokio::time::timeout(Duration::from_millis(50), watching.changed()).await;
    assert!(
        again.is_err(),
        "nothing changed since: the wait is still a wait"
    );
    broker.topics_changed.send_modify(|n| *n += 1);
    assert_eq!(watching.changed().await, Woke::Topics);
}

/// Who a Fetch comes from (KIP-903): the top-level replica id up to v14, the `ReplicaState` tagged field's from v15, where
/// the old field is not on the wire. A follower needs CLUSTER_ACTION and is exempt from quotas, so reading the wrong one
/// would let a v15 follower be read as a consumer, or the other way round.
#[test]
fn the_replica_is_read_from_its_state_from_v15() {
    let mut req = FetchRequest::default();
    req.replica_id = kafka_wire::messages::BrokerId(3);
    assert_eq!(replica_of(14, &req), 3);
    assert_eq!(replica_of(15, &req), -1, "v15 has no top-level replica id");
    req.replica_state.replica_id = kafka_wire::messages::BrokerId(5);
    assert_eq!(replica_of(15, &req), 5);
    assert_eq!(replica_of(18, &req), 5);
}
