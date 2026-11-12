//! The parts of CreatePartitions that a request cannot reach by itself: a topic that goes while its partitions are being
//! made, and a change that must wait for the one before it.

use std::path::Path;
use std::time::Duration;

use kafka_storage::sim::SimFs;
use kafka_wire::ids::TopicName;
use kafka_wire::messages::TopicName as WireName;

use super::*;
use crate::Config;

const DATA: &str = "/data";

/// A broker whose topic `t` has one partition: in the metadata and on the disk.
fn broker_with_t() -> (Broker<SimFs>, TopicName) {
    let broker = Broker::open(SimFs::new(), Config::new(DATA.into())).unwrap();
    let name = TopicName::parse("t").unwrap();
    let one = PartitionCount::new(1).unwrap();
    broker
        .meta
        .create_topic(&name, one, Default::default(), &Default::default())
        .unwrap();
    topic_dirs::create_partition_dirs(&broker.fs, Path::new(DATA), &name, one).unwrap();
    (broker, name)
}

fn dirs(broker: &Broker<SimFs>) -> Vec<String> {
    let mut names = broker.fs.names(Path::new(DATA));
    names.retain(|n| !n.starts_with("meta.redb"));
    names.sort();
    names
}

fn ask(count: i32) -> CreatePartitionsRequest {
    let mut t = CreatePartitionsTopic::default();
    t.name = WireName(StrBytes::from_static_str("t"));
    t.count = count;
    t.assignments = None;
    let mut req = CreatePartitionsRequest::default();
    req.topics.push(t);
    req
}

#[test]
fn a_topic_that_is_gone_by_the_time_its_count_is_recorded_leaves_no_directories_behind() {
    let (broker, name) = broker_with_t();
    let stale = broker.meta.topic(&name).unwrap();
    broker.meta.delete_topic(&name).unwrap();
    let before = dirs(&broker);
    let five = PartitionCount::new(5).unwrap();
    let got = broker.add_partitions(&stale, five);
    assert_eq!(got.unwrap_err(), (E::UnknownTopicOrPartition, None));
    assert_eq!(dirs(&broker), before, "the four it made were taken back");
}

#[tokio::test(start_paused = true)]
async fn a_change_waits_while_another_topic_change_is_being_made_and_then_goes_through() {
    let (broker, name) = broker_with_t();
    let held = broker.topic_admin.lock().await;
    let waiting = tokio::time::timeout(
        Duration::from_millis(50),
        broker.create_partitions(&crate::Caller::anonymous(None), &ask(3)),
    )
    .await;
    assert!(
        waiting.is_err(),
        "it must not run beside a delete or a create"
    );
    assert_eq!(broker.meta.topic(&name).unwrap().partitions.get(), 1);
    drop(held);
    let Outcome::Respond(Response::CreatePartitions(resp)) = broker
        .create_partitions(&crate::Caller::anonymous(None), &ask(3))
        .await
    else {
        panic!("not CreatePartitions")
    };
    assert_eq!(resp.results[0].error_code, 0);
    assert_eq!(broker.meta.topic(&name).unwrap().partitions.get(), 3);
}
