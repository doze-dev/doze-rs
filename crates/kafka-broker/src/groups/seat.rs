//! A classic member as it is handed between a classic group and a consumer group when a group changes kind.

use bytes::Bytes;
use kafka_wire::ids::{InstanceId, MemberId};

use super::machine::Protocol;

/// One classic member.
#[derive(Debug, Clone)]
pub(crate) struct Seat {
    pub(crate) id: MemberId,
    pub(crate) instance: Option<InstanceId>,
    pub(crate) client_id: String,
    /// Where it connected from, as DescribeGroups shows it.
    pub(crate) client_host: String,
    /// What it offered, in its order.
    pub(crate) protocols: Vec<Protocol>,
    /// What it holds, as the bytes a classic member is given.
    pub(crate) assignment: Bytes,
    pub(crate) session_ms: u64,
    pub(crate) rebalance_ms: u64,
    /// What its principal may DESCRIBE (`None`: everything), from its last JoinGroup.
    pub(crate) rights: Option<crate::authorizer::TopicRights>,
}

/// A classic group's members and its generation.
#[derive(Debug, Clone)]
pub(crate) struct Handover {
    pub(crate) generation: i32,
    pub(crate) seats: Vec<Seat>,
}
