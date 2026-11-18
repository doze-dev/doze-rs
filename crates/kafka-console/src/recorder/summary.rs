//! `Summarize`: what the recorder keeps of each API. Two typed matches over the generated enums with no wildcard: an API
//! added to the broker's table without a summary here does not compile.
//!
//! `of_request` runs before the handler (cheap, bounded: names, ids, counts from batch headers), `of_response` after it (result
//! codes and counts). Neither copies record bytes, and neither reads a secret: the SASL tokens, the SCRAM salts and salted
//! passwords, config values and record content are never touched, so they cannot be kept. One module per family.

use kafka_wire::api::{Request, Response};

use super::facts::Facts;

mod configs;
mod fetch;
mod groups;
mod offsets;
mod produce;
mod security;
mod topics;
mod txn;

/// What the request says.
pub fn of_request(request: &Request, f: &mut Facts) {
    match request {
        Request::Produce(r) => produce::request(r, f),
        Request::Fetch(r) => fetch::request(r, f),
        Request::ListOffsets(r) => offsets::list_request(r, f),
        Request::Metadata(r) => topics::metadata_request(r, f),
        Request::OffsetCommit(r) => offsets::commit_request(r, f),
        Request::OffsetFetch(r) => offsets::fetch_request(r, f),
        Request::FindCoordinator(r) => groups::find_request(r, f),
        Request::JoinGroup(r) => groups::join_request(r, f),
        Request::Heartbeat(r) => groups::heartbeat_request(r, f),
        Request::LeaveGroup(r) => groups::leave_request(r, f),
        Request::SyncGroup(r) => groups::sync_request(r, f),
        Request::DescribeGroups(r) => groups::describe_request(r, f),
        Request::ListGroups(r) => groups::list_request(r, f),
        Request::SaslHandshake(r) => security::handshake_request(r, f),
        Request::ApiVersions(r) => security::versions_request(r, f),
        Request::CreateTopics(r) => topics::create_request(r, f),
        Request::DeleteTopics(r) => topics::delete_request(r, f),
        Request::DeleteRecords(r) => topics::delete_records_request(r, f),
        Request::InitProducerId(r) => txn::init_request(r, f),
        Request::AddPartitionsToTxn(r) => txn::add_partitions_request(r, f),
        Request::AddOffsetsToTxn(r) => txn::add_offsets_request(r, f),
        Request::EndTxn(r) => txn::end_request(r, f),
        Request::TxnOffsetCommit(r) => txn::commit_request(r, f),
        Request::DescribeAcls(r) => security::describe_acls_request(r, f),
        Request::CreateAcls(r) => security::create_acls_request(r, f),
        Request::DeleteAcls(r) => security::delete_acls_request(r, f),
        Request::DescribeConfigs(r) => configs::describe_request(r, f),
        Request::AlterConfigs(r) => configs::alter_request(r, f),
        Request::SaslAuthenticate(r) => security::authenticate_request(r, f),
        Request::CreatePartitions(r) => topics::partitions_request(r, f),
        Request::DeleteGroups(r) => groups::delete_request(r, f),
        Request::IncrementalAlterConfigs(r) => configs::incremental_request(r, f),
        Request::OffsetDelete(r) => offsets::delete_request(r, f),
        Request::DescribeCluster(r) => topics::cluster_request(r, f),
        Request::DescribeProducers(r) => txn::describe_producers_request(r, f),
        Request::DescribeTransactions(r) => txn::describe_request(r, f),
        Request::ListTransactions(r) => txn::list_request(r, f),
        Request::ConsumerGroupHeartbeat(r) => groups::consumer_heartbeat_request(r, f),
        Request::ConsumerGroupDescribe(r) => groups::consumer_describe_request(r, f),
        Request::DescribeUserScramCredentials(r) => security::describe_scram_request(r, f),
        Request::AlterUserScramCredentials(r) => security::alter_scram_request(r, f),
        Request::GetTelemetrySubscriptions(r) => security::telemetry_get_request(r, f),
        Request::PushTelemetry(r) => security::telemetry_push_request(r, f),
    }
}

/// What an ApiVersions request says: the software that sent it (kept as one line, clipped).
pub fn of_api_versions(r: &kafka_wire::messages::ApiVersionsRequest, f: &mut Facts) {
    security::versions_request(r, f);
}

/// What the answer says.
pub fn of_response(response: &Response, f: &mut Facts) {
    match response {
        Response::Produce(r) => produce::response(r, f),
        Response::Fetch(r) => fetch::response(r, f),
        Response::ListOffsets(r) => offsets::list_response(r, f),
        Response::Metadata(r) => topics::metadata_response(r, f),
        Response::OffsetCommit(r) => offsets::commit_response(r, f),
        Response::OffsetFetch(r) => offsets::fetch_response(r, f),
        Response::FindCoordinator(r) => groups::find_response(r, f),
        Response::JoinGroup(r) => groups::join_response(r, f),
        Response::Heartbeat(r) => groups::heartbeat_response(r, f),
        Response::LeaveGroup(r) => groups::leave_response(r, f),
        Response::SyncGroup(r) => groups::sync_response(r, f),
        Response::DescribeGroups(r) => groups::describe_response(r, f),
        Response::ListGroups(r) => groups::list_response(r, f),
        Response::SaslHandshake(r) => security::handshake_response(r, f),
        Response::ApiVersions(r) => security::versions_response(r, f),
        Response::CreateTopics(r) => topics::create_response(r, f),
        Response::DeleteTopics(r) => topics::delete_response(r, f),
        Response::DeleteRecords(r) => topics::delete_records_response(r, f),
        Response::InitProducerId(r) => txn::init_response(r, f),
        Response::AddPartitionsToTxn(r) => txn::add_partitions_response(r, f),
        Response::AddOffsetsToTxn(r) => txn::add_offsets_response(r, f),
        Response::EndTxn(r) => txn::end_response(r, f),
        Response::TxnOffsetCommit(r) => txn::commit_response(r, f),
        Response::DescribeAcls(r) => security::describe_acls_response(r, f),
        Response::CreateAcls(r) => security::create_acls_response(r, f),
        Response::DeleteAcls(r) => security::delete_acls_response(r, f),
        Response::DescribeConfigs(r) => configs::describe_response(r, f),
        Response::AlterConfigs(r) => configs::alter_response(r, f),
        Response::SaslAuthenticate(r) => security::authenticate_response(r, f),
        Response::CreatePartitions(r) => topics::partitions_response(r, f),
        Response::DeleteGroups(r) => groups::delete_response(r, f),
        Response::IncrementalAlterConfigs(r) => configs::incremental_response(r, f),
        Response::OffsetDelete(r) => offsets::delete_response(r, f),
        Response::DescribeCluster(r) => topics::cluster_response(r, f),
        Response::DescribeProducers(r) => txn::describe_producers_response(r, f),
        Response::DescribeTransactions(r) => txn::describe_response(r, f),
        Response::ListTransactions(r) => txn::list_response(r, f),
        Response::ConsumerGroupHeartbeat(r) => groups::consumer_heartbeat_response(r, f),
        Response::ConsumerGroupDescribe(r) => groups::consumer_describe_response(r, f),
        Response::DescribeUserScramCredentials(r) => security::describe_scram_response(r, f),
        Response::AlterUserScramCredentials(r) => security::alter_scram_response(r, f),
        Response::GetTelemetrySubscriptions(r) => security::telemetry_get_response(r, f),
        Response::PushTelemetry(r) => security::telemetry_push_response(r, f),
    }
}

/// The count of a length, saturated.
pub(super) fn n(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests;
