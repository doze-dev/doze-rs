//! The version profile of the Settings board: the 38 APIs a Kafka 4 broker offers with SASL off.

use kafka_wire::messages::ApiKey;

use crate::model::{SettingsApi, SettingsProfile};
use crate::settings_words::{family_of, versions};

const APIS: [(ApiKey, i16, i16); 38] = [
    (ApiKey::Produce, 3, 9),
    (ApiKey::Fetch, 4, 13),
    (ApiKey::ListOffsets, 1, 7),
    (ApiKey::DeleteRecords, 0, 2),
    (ApiKey::ApiVersions, 0, 4),
    (ApiKey::Metadata, 0, 12),
    (ApiKey::FindCoordinator, 0, 4),
    (ApiKey::CreateTopics, 2, 7),
    (ApiKey::DeleteTopics, 1, 6),
    (ApiKey::CreatePartitions, 0, 3),
    (ApiKey::DescribeConfigs, 1, 4),
    (ApiKey::AlterConfigs, 0, 2),
    (ApiKey::IncrementalAlterConfigs, 0, 1),
    (ApiKey::JoinGroup, 0, 9),
    (ApiKey::SyncGroup, 0, 5),
    (ApiKey::Heartbeat, 0, 4),
    (ApiKey::LeaveGroup, 0, 5),
    (ApiKey::DescribeGroups, 0, 5),
    (ApiKey::ListGroups, 0, 5),
    (ApiKey::DeleteGroups, 0, 2),
    (ApiKey::ConsumerGroupHeartbeat, 0, 1),
    (ApiKey::ConsumerGroupDescribe, 0, 1),
    (ApiKey::OffsetCommit, 2, 9),
    (ApiKey::OffsetFetch, 1, 9),
    (ApiKey::OffsetDelete, 0, 0),
    (ApiKey::InitProducerId, 0, 4),
    (ApiKey::AddPartitionsToTxn, 0, 3),
    (ApiKey::AddOffsetsToTxn, 0, 3),
    (ApiKey::EndTxn, 0, 3),
    (ApiKey::TxnOffsetCommit, 0, 3),
    (ApiKey::DescribeProducers, 0, 0),
    (ApiKey::DescribeTransactions, 0, 0),
    (ApiKey::ListTransactions, 0, 1),
    (ApiKey::DescribeAcls, 1, 3),
    (ApiKey::CreateAcls, 1, 3),
    (ApiKey::DeleteAcls, 1, 3),
    (ApiKey::DescribeUserScramCredentials, 0, 0),
    (ApiKey::AlterUserScramCredentials, 0, 0),
];

pub(super) fn profile() -> SettingsProfile {
    SettingsProfile {
        major: 4,
        apis: APIS
            .iter()
            .map(|(k, lo, hi)| SettingsApi {
                name: format!("{k:?}"),
                versions: versions(*lo, *hi),
                family: family_of(*k).map_or("Other", |f| f.1),
            })
            .collect(),
        with_sasl: vec![
            SettingsApi {
                name: "SaslHandshake".into(),
                versions: "v1 only".into(),
                family: "Security: ACLs and users",
            },
            SettingsApi {
                name: "SaslAuthenticate".into(),
                versions: "v0 to v2".into(),
                family: "Security: ACLs and users",
            },
        ],
        sasl_on: false,
        not_offered: vec![],
    }
}
