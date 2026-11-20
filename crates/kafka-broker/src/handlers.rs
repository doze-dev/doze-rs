//! One file per API: the request in, the response out.

mod add_offsets_to_txn;
mod add_partitions_to_txn;
mod alter_client_quotas;
pub(crate) mod alter_configs;
mod alter_partition_reassignments;
mod alter_replica_log_dirs;
mod alter_user_scram_credentials;
mod consumer_group_describe;
mod consumer_group_heartbeat;
mod create_acls;
mod create_partitions;
mod create_topics;
mod delegation_tokens;
mod delete_acls;
mod delete_groups;
mod delete_records;
mod delete_topics;
mod describe_acls;
mod describe_client_quotas;
mod describe_cluster;
mod describe_configs;
mod describe_groups;
mod describe_log_dirs;
mod describe_producers;
mod describe_topic_partitions;
mod describe_transactions;
mod describe_user_scram_credentials;
mod elect_leaders;
mod end_txn;
pub(crate) mod fetch;
mod find_coordinator;
mod get_telemetry_subscriptions;
mod group_guard;
mod heartbeat;
mod init_producer_id;
mod join_group;
mod leave_group;
mod list_config_resources;
mod list_groups;
mod list_offsets;
mod metadata;
mod offset_commit;
mod offset_delete;
mod offset_fetch;
mod offset_for_leader_epoch;
mod produce;
mod push_telemetry;
mod share_acknowledge;
mod share_acks;
mod share_fetch;
mod share_group_describe;
mod share_group_heartbeat;
mod share_group_offsets;
mod streams_group_describe;
mod streams_group_heartbeat;
mod sync_group;
mod txn_offset_commit;
mod update_features;

pub(crate) use streams_group_heartbeat::Failures as StreamsTopicFailures;

/// Who is asking: the name the client gave itself, and where it connected from (a slash, then the address, as Kafka shows
/// it).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Client<'a> {
    pub(crate) id: &'a str,
    pub(crate) host: &'a str,
}
