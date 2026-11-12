//! What ApiVersions says the broker serves.

use super::*;

#[tokio::test]
async fn api_versions_lists_only_what_the_broker_serves() {
    let (mut client, _task, _stop) = start(Timeouts::default(), 4096);
    client.write_all(&api_versions(7)).await.unwrap();
    let (id, error, keys) = read_answer(&mut client).await;
    assert_eq!((id, error), (7, 0));
    // What the broker has a handler for, by key.
    let served = [
        ApiKey::Produce,
        ApiKey::Fetch,
        ApiKey::ListOffsets,
        ApiKey::Metadata,
        ApiKey::OffsetCommit,
        ApiKey::OffsetFetch,
        ApiKey::FindCoordinator,
        ApiKey::JoinGroup,
        ApiKey::Heartbeat,
        ApiKey::LeaveGroup,
        ApiKey::SyncGroup,
        ApiKey::DescribeGroups,
        ApiKey::ListGroups,
        ApiKey::ApiVersions,
        ApiKey::CreateTopics,
        ApiKey::DeleteTopics,
        ApiKey::DeleteRecords,
        ApiKey::InitProducerId,
        ApiKey::AddPartitionsToTxn,
        ApiKey::AddOffsetsToTxn,
        ApiKey::EndTxn,
        ApiKey::TxnOffsetCommit,
        ApiKey::DescribeAcls,
        ApiKey::CreateAcls,
        ApiKey::DeleteAcls,
        ApiKey::DescribeConfigs,
        ApiKey::AlterConfigs,
        ApiKey::CreatePartitions,
        ApiKey::DeleteGroups,
        ApiKey::IncrementalAlterConfigs,
        ApiKey::OffsetDelete,
        ApiKey::DescribeUserScramCredentials,
        ApiKey::AlterUserScramCredentials,
        ApiKey::DescribeProducers,
        ApiKey::DescribeTransactions,
        ApiKey::ListTransactions,
        ApiKey::ConsumerGroupHeartbeat,
        ApiKey::ConsumerGroupDescribe,
    ];
    assert_eq!(keys, served.map(|k| k as i16));
}
