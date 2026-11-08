//! One file per API: the request in, the response out.

mod create_topics;
mod delete_groups;
mod delete_topics;
mod describe_groups;
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
