//! Consumer groups: each group is a state machine ([`machine`]) run by a task ([`actor`]); the registry finds it.

mod actor;
mod consumer;
pub(crate) mod events;
#[cfg(feature = "inspect")]
mod expiry;
mod group;
#[cfg(feature = "inspect")]
mod inspect;
mod machine;
pub(crate) mod note;
mod order;
mod registry;
mod retention;
mod seat;
mod subscription;

pub(crate) use actor::Req;
pub(crate) use consumer::assign::Partitions;
pub(crate) use consumer::fetch_refusal;
pub(crate) use consumer::{Config as ConsumerConfig, Heartbeat, HeartbeatAnswer};
#[cfg(feature = "inspect")]
pub(crate) use expiry::first_expiry;
pub(crate) use group::Settings;
#[cfg(feature = "inspect")]
pub(crate) use inspect::{Inspect, MemberInspect};
pub(crate) use machine::Force;
pub(crate) use machine::State;
pub(crate) use machine::{
    Answer, CommitRequest, Description, HeartbeatRequest, JoinAnswer, JoinRequest, LeaveTarget,
    Protocol, SyncRequest,
};
pub(crate) use machine::Forced;
pub use note::Cause;
pub(crate) use note::Reason;
pub(crate) use order::{kafka_order, offsets_partition};
pub(crate) use registry::member_ids;
pub(crate) use registry::{Deleted, Groups};
pub(crate) use retention::{Members, View, expired, members_of};
