//! One file per API: the request in, the response out.

mod add_offsets_to_txn;
mod add_partitions_to_txn;
mod consumer_group_describe;
mod consumer_group_heartbeat;
mod create_topics;
mod delete_groups;
mod delete_topics;
mod describe_groups;
mod describe_producers;
mod describe_transactions;
mod end_txn;
mod fetch;
mod find_coordinator;
mod heartbeat;
mod init_producer_id;
mod join_group;
mod leave_group;
mod list_groups;
mod list_offsets;
mod metadata;
mod offset_commit;
mod offset_delete;
mod offset_fetch;
mod produce;
mod sync_group;
mod txn_offset_commit;

/// Who is asking: the name the client gave itself, and where it connected from (a slash, then the address, as Kafka shows
/// it).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Client<'a> {
    pub(crate) id: &'a str,
    pub(crate) host: &'a str,
}
