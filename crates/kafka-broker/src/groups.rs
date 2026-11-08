//! Consumer groups: each group is a state machine ([`machine`]) run by a task ([`actor`]); the registry finds it.

mod actor;
mod machine;
mod registry;

pub(crate) use actor::Req;
pub(crate) use machine::{
    Answer, CommitRequest, HeartbeatRequest, JoinAnswer, JoinRequest, LeaveTarget, Protocol,
    SyncRequest,
};
pub(crate) use registry::Groups;
