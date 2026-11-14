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
        ApiKey::DescribeCluster,
        ApiKey::DescribeProducers,
        ApiKey::DescribeTransactions,
        ApiKey::ListTransactions,
        ApiKey::ConsumerGroupHeartbeat,
        ApiKey::ConsumerGroupDescribe,
        ApiKey::GetTelemetrySubscriptions,
        ApiKey::PushTelemetry,
    ];
    assert_eq!(keys, served.map(|k| k as i16));
}

#[tokio::test]
async fn telemetry_is_listed_unless_the_broker_is_told_not_to_serve_it() {
    let keys = |client_telemetry: bool| async move {
        let mut config = BrokerConfig::new("/data".into());
        config.client_telemetry = client_telemetry;
        let (mut client, _task, _stop) = start_on(context_of(Timeouts::default(), config), 4096);
        client.write_all(&api_versions(7)).await.unwrap();
        read_answer(&mut client).await.2
    };
    let telemetry = [
        ApiKey::GetTelemetrySubscriptions as i16,
        ApiKey::PushTelemetry as i16,
    ];
    let (on, off) = (keys(true).await, keys(false).await);
    assert!(telemetry.iter().all(|k| on.contains(k)));
    assert!(telemetry.iter().all(|k| !off.contains(k)));
    // Nothing else changes with the flag.
    assert_eq!(
        on.iter().filter(|k| !telemetry.contains(k)).count(),
        off.len()
    );
}
