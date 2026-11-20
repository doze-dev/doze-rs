//! The admin APIs of debt K3: partition descriptions, leader elections, reassignments, log directories, config resources,
//! features and delegation tokens. A token's HMAC is its password and is never read; nor is its id, which a login uses as
//! its user name: a token row says only whose token it is and how the request ended.

use kafka_wire::messages::{
    AlterPartitionReassignmentsRequest, AlterPartitionReassignmentsResponse,
    AlterReplicaLogDirsRequest, AlterReplicaLogDirsResponse, CreateDelegationTokenRequest,
    CreateDelegationTokenResponse, DescribeDelegationTokenRequest, DescribeDelegationTokenResponse,
    DescribeTopicPartitionsRequest, DescribeTopicPartitionsResponse, ElectLeadersRequest,
    ElectLeadersResponse, ExpireDelegationTokenRequest, ExpireDelegationTokenResponse,
    ListConfigResourcesRequest, ListConfigResourcesResponse, ListPartitionReassignmentsRequest,
    ListPartitionReassignmentsResponse, RenewDelegationTokenRequest, RenewDelegationTokenResponse,
    UpdateFeaturesRequest, UpdateFeaturesResponse,
};

use super::n;
use crate::recorder::facts::Facts;

pub(super) fn topic_partitions_request(r: &DescribeTopicPartitionsRequest, f: &mut Facts) {
    if r.topics.is_empty() {
        f.about("all topics");
    }
    for t in &r.topics {
        f.topic(&t.name.0, []);
    }
    f.asked = n(r.topics.len());
}

pub(super) fn topic_partitions_response(r: &DescribeTopicPartitionsResponse, f: &mut Facts) {
    f.items = n(r.topics.len());
    for t in &r.topics {
        f.error(t.error_code);
    }
    if r.next_cursor.is_some() {
        f.about("more to come");
    }
}

pub(super) fn elect_request(r: &ElectLeadersRequest, f: &mut Facts) {
    match &r.topic_partitions {
        None => f.about("every partition"),
        Some(topics) => {
            for t in topics {
                f.topic(&t.topic.0, t.partitions.iter().copied());
            }
        }
    }
}

pub(super) fn elect_response(r: &ElectLeadersResponse, f: &mut Facts) {
    f.error(r.error_code);
    for t in &r.replica_election_results {
        for p in &t.partition_result {
            f.result(&t.topic.0, p.partition_id, p.error_code, None);
        }
    }
}

pub(super) fn reassign_request(r: &AlterPartitionReassignmentsRequest, f: &mut Facts) {
    for t in &r.topics {
        f.topic(&t.name.0, t.partitions.iter().map(|p| p.partition_index));
    }
}

pub(super) fn reassign_response(r: &AlterPartitionReassignmentsResponse, f: &mut Facts) {
    f.error(r.error_code);
    f.say(r.error_message.as_deref());
    for t in &r.responses {
        for p in &t.partitions {
            f.result(&t.name.0, p.partition_index, p.error_code, None);
        }
    }
}

pub(super) fn reassignments_request(r: &ListPartitionReassignmentsRequest, f: &mut Facts) {
    match &r.topics {
        None => f.about("every reassignment"),
        Some(topics) => {
            for t in topics {
                f.topic(&t.name.0, t.partition_indexes.iter().copied());
            }
        }
    }
}

pub(super) fn reassignments_response(r: &ListPartitionReassignmentsResponse, f: &mut Facts) {
    f.error(r.error_code);
    f.say(r.error_message.as_deref());
    f.items = n(r.topics.iter().map(|t| t.partitions.len()).sum());
}

pub(super) fn log_dirs_request(r: &AlterReplicaLogDirsRequest, f: &mut Facts) {
    for d in &r.dirs {
        for t in &d.topics {
            f.topic(&t.name.0, t.partitions.iter().copied());
        }
    }
}

pub(super) fn log_dirs_response(r: &AlterReplicaLogDirsResponse, f: &mut Facts) {
    for t in &r.results {
        for p in &t.partitions {
            f.result(&t.topic_name.0, p.partition_index, p.error_code, None);
        }
    }
}

pub(super) fn config_resources_request(r: &ListConfigResourcesRequest, f: &mut Facts) {
    f.asked = n(r.resource_types.len());
}

pub(super) fn config_resources_response(r: &ListConfigResourcesResponse, f: &mut Facts) {
    f.error(r.error_code);
    f.items = n(r.config_resources.len());
}

pub(super) fn features_request(r: &UpdateFeaturesRequest, f: &mut Facts) {
    for u in &r.feature_updates {
        f.about(&format!(
            "{} to {}",
            u.feature.as_str(),
            u.max_version_level
        ));
    }
    if r.validate_only {
        f.about("validate only");
    }
}

pub(super) fn features_response(r: &UpdateFeaturesResponse, f: &mut Facts) {
    f.error(r.error_code);
    f.say(r.error_message.as_deref());
}

pub(super) fn create_token_request(r: &CreateDelegationTokenRequest, f: &mut Facts) {
    match &r.owner_principal_name {
        Some(owner) if !owner.is_empty() => f.about(&format!("a token for {}", owner.as_str())),
        _ => f.about("a token for the caller"),
    }
}

pub(super) fn create_token_response(r: &CreateDelegationTokenResponse, f: &mut Facts) {
    f.error(r.error_code);
}

pub(super) fn renew_token_request(_: &RenewDelegationTokenRequest, f: &mut Facts) {
    f.about("a token's renewal");
}

pub(super) fn renew_token_response(r: &RenewDelegationTokenResponse, f: &mut Facts) {
    f.error(r.error_code);
}

pub(super) fn expire_token_request(_: &ExpireDelegationTokenRequest, f: &mut Facts) {
    f.about("a token's expiry");
}

pub(super) fn expire_token_response(r: &ExpireDelegationTokenResponse, f: &mut Facts) {
    f.error(r.error_code);
}

pub(super) fn describe_tokens_request(r: &DescribeDelegationTokenRequest, f: &mut Facts) {
    match &r.owners {
        None => f.about("every token"),
        Some(owners) => f.asked = n(owners.len()),
    }
}

pub(super) fn describe_tokens_response(r: &DescribeDelegationTokenResponse, f: &mut Facts) {
    f.error(r.error_code);
    f.items = n(r.tokens.len());
}
