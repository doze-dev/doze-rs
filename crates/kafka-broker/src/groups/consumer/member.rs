//! One member of a consumer group.

use kafka_wire::ids::{InstanceId, MemberId};

use super::assign::{Assignor, Partitions};
use super::pattern::Pattern;
use crate::authorizer::{TopicRights, permits};
use crate::groups::machine::{Millis, Protocol};

/// What a member that arrived by the classic JoinGroup keeps of it: it is still told what to do by the classic APIs.
#[derive(Debug, Clone)]
pub(crate) struct ClassicSeat {
    /// As it offered them, in its order.
    pub(crate) protocols: Vec<Protocol>,
    /// The one whose subscription was read.
    pub(crate) protocol: String,
    /// The version that subscription is written at, and the assignments it is given.
    pub(crate) version: i16,
    /// The session it asked for.
    pub(crate) session_ms: u64,
}

pub(super) struct Member {
    pub(super) id: MemberId,
    pub(super) instance: Option<InstanceId>,
    pub(super) rack: Option<String>,
    pub(super) client_id: String,
    pub(super) client_host: String,
    /// The assignor it asked for.
    pub(super) assignor: Option<Assignor>,
    pub(super) subscribed: Vec<String>,
    pub(super) pattern: Option<Pattern>,
    /// What its last heartbeat could DESCRIBE: a pattern matches only those topics. `None`: all of them.
    pub(super) rights: Option<TopicRights>,
    /// The group epoch it has caught up to.
    pub(super) epoch: i32,
    /// Its epoch before that: a heartbeat still carrying it is a retry, not a fenced member.
    pub(super) prev_epoch: i32,
    /// What the assignor wants it to own.
    pub(super) target: Partitions,
    /// What it may consume now.
    pub(super) assigned: Partitions,
    /// What it has been told to give up and has not said it has.
    pub(super) revoking: Partitions,
    /// The assignment last sent to it; `None` until one has been.
    pub(super) sent: Option<Partitions>,
    pub(super) rebalance_ms: i32,
    /// A static member that left (epoch -2): its place is held until its session runs out.
    pub(super) left: bool,
    pub(super) classic: Option<ClassicSeat>,
    /// The arming of its session timer that counts.
    pub(super) session_gen: u64,
    /// When its session timer was last started, on the group's clock; only the console reads it.
    #[cfg_attr(not(feature = "inspect"), allow(dead_code))]
    pub(super) heard: Millis,
    /// The arming of its revocation timer that counts (0: none).
    pub(super) revoke_gen: u64,
}

impl Member {
    pub(super) fn new(id: MemberId) -> Self {
        Self {
            id,
            instance: None,
            rack: None,
            client_id: String::new(),
            client_host: String::new(),
            assignor: None,
            subscribed: Vec::new(),
            pattern: None,
            rights: None,
            epoch: 0,
            prev_epoch: 0,
            target: Partitions::new(),
            assigned: Partitions::new(),
            revoking: Partitions::new(),
            sent: None,
            rebalance_ms: -1,
            left: false,
            classic: None,
            session_gen: 0,
            heard: Millis(0),
            revoke_gen: 0,
        }
    }

    /// Whether the member's pattern matches the topic and the member may DESCRIBE it (Kafka's
    /// `filterTopicDescribeAuthorizedTopics`).
    pub(super) fn pattern_takes(&self, topic: &str) -> bool {
        self.pattern
            .as_ref()
            .is_some_and(|p| p.matches(topic) && permits(self.rights.as_ref(), topic))
    }

    /// The names it subscribed to (or inherited) that its own principal may DESCRIBE: the ones that are shown, counted and
    /// assigned from. Kafka checks a name once, when it is sent; here it is held to the right at every heartbeat, as a
    /// match is (SEC-230, SEC-232).
    pub(super) fn visible_names(&self) -> impl Iterator<Item = &String> {
        self.subscribed
            .iter()
            .filter(|t| permits(self.rights.as_ref(), t))
    }
}
