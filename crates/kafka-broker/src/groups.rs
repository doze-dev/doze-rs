//! Consumer groups: each group is a state machine ([`machine`]) run by a task ([`actor`]); the registry finds it.

mod actor;
mod consumer;
mod group;
mod machine;
mod order;
mod registry;
mod retention;
mod seat;
mod subscription;

pub(crate) use actor::Req;
pub(crate) use consumer::assign::Partitions;
pub(crate) use consumer::fetch_refusal;
pub(crate) use consumer::{Config as ConsumerConfig, Heartbeat, HeartbeatAnswer};
pub(crate) use group::Settings;
pub(crate) use machine::State;
pub(crate) use machine::{
    Answer, CommitRequest, Description, HeartbeatRequest, JoinAnswer, JoinRequest, LeaveTarget,
    Protocol, SyncRequest,
};
pub(crate) use order::kafka_order;
pub(crate) use registry::member_ids;
pub(crate) use registry::{Deleted, Groups};
pub(crate) use retention::{Members, View, expired};
