use std::path::Path;

use kafka_storage::fs::Fs;
use kafka_storage::sim::SimFs;
use kafka_wire::batch::build::BatchBuilder;
use kafka_wire::messages::create_topics_request::CreatableTopic;
use kafka_wire::messages::{CreateTopicsRequest, DeleteTopicsRequest};

use super::*;
use crate::Config;

fn name(s: &str) -> kafka_wire::messages::TopicName {
    kafka_wire::messages::TopicName(StrBytes::from_string(s.to_owned()))
}

async fn broker_with(topic: &str) -> Broker<SimFs> {
    let fs = SimFs::new();
    fs.create_dir(Path::new("/")).ok();
    let broker = Broker::open(fs, Config::new("/data".into())).unwrap();
    let mut t = CreatableTopic::default();
    t.name = name(topic);
    t.num_partitions = 1;
    t.replication_factor = 1;
    let mut req = CreateTopicsRequest::default();
    req.topics.push(t);
    let made = broker.create_topics(&Caller::anonymous(None), &req).await;
    assert_eq!(made.topics[0].error_code, 0);
    broker
}

async fn delete(broker: &Broker<SimFs>, topic: &str) {
    let mut req = DeleteTopicsRequest::default();
    req.topic_names.push(name(topic));
    let gone = broker.delete_topics(&Caller::anonymous(None), &req).await;
    assert_eq!(gone.responses[0].error_code, 0);
}

fn partition_data(index: i32) -> PartitionProduceData {
    let mut p = PartitionProduceData::default();
    p.index = index;
    p.records = Some(BatchBuilder::new().records(1).build());
    p
}

/// The interleaving the audit's probe hit with four producers and a DeleteTopics: the Produce looked its topic up, then
/// the topic was deleted (metadata, then the partitions, then the files), then the Produce went on to the partition.
/// Kafka answers a partition it no longer hosts with UNKNOWN_TOPIC_OR_PARTITION (`ReplicaManager.getPartitionOrException`
/// once the metadata has dropped the topic), not KAFKA_STORAGE_ERROR, which says a disk failed.
#[tokio::test]
async fn a_produce_that_looked_its_topic_up_before_a_delete_is_told_the_topic_is_unknown() {
    for (opened_first, acks) in [(false, -1_i16), (true, -1), (false, 1), (true, 1)] {
        let broker = broker_with("t").await;
        let topic = broker
            .meta
            .topic(&TopicName::parse("t").unwrap())
            .unwrap();
        let txn = Txn {
            id: None,
            held: None,
        };
        if opened_first {
            // The partition open and written to, so that the delete has a writer to close.
            let stage = broker
                .start(9, acks == -1, &txn, Some(&topic), &partition_data(0))
                .await;
            assert_eq!(finish(stage, acks == -1, |id| broker.deleted(id)).await.error_code, 0);
        }
        delete(&broker, "t").await;
        let stage = broker
            .start(9, acks == -1, &txn, Some(&topic), &partition_data(0))
            .await;
        let answer = finish(stage, acks == -1, |id| broker.deleted(id)).await;
        assert_eq!(
            answer.error_code,
            ResponseError::UnknownTopicOrPartition.code(),
            "opened first {opened_first}, acks {acks}"
        );
    }
}
