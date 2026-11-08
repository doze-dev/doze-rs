//! Consumer groups: each group is a state machine ([`machine`]) run by a task ([`actor`]); the registry finds it.

mod actor;
mod machine;
mod order;
mod registry;
mod subscription;

pub(crate) use actor::Req;
pub(crate) use machine::State;
pub(crate) use machine::{
    Answer, CommitRequest, Description, HeartbeatRequest, JoinAnswer, JoinRequest, LeaveTarget,
    Protocol, SyncRequest,
};
pub(crate) use order::kafka_order;
pub(crate) use registry::{Deleted, Groups};
