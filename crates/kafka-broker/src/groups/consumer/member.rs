//! One member of a consumer group.

use kafka_wire::ids::{InstanceId, MemberId};

use super::Topics;
use super::assign::{Assignor, Partitions};
use super::pattern::Pattern;
use crate::authorizer::{TopicRights, permits};
use crate::groups::machine::Protocol;

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
            revoke_gen: 0,
        }
    }

    /// Whether the member wants this topic.
    pub(super) fn subscribes(&self, topic: &str) -> bool {
        self.subscribed.iter().any(|t| t == topic) || self.pattern_takes(topic)
    }

    /// Whether the member's pattern matches the topic and the member may DESCRIBE it (Kafka's
    /// `filterTopicDescribeAuthorizedTopics`: a name subscription was checked when it was made).
    pub(super) fn pattern_takes(&self, topic: &str) -> bool {
        self.pattern
            .as_ref()
            .is_some_and(|p| p.matches(topic) && permits(self.rights.as_ref(), topic))
    }

    /// Takes the rights of the heartbeat just heard. `true` if they change which topics the pattern takes.
    pub(super) fn take_rights(&mut self, new: Option<TopicRights>, topics: &Topics) -> bool {
        if self.rights == new {
            return false;
        }
        let old = std::mem::replace(&mut self.rights, new);
        let Some(pattern) = &self.pattern else {
            return false;
        };
        topics.all().any(|t| {
            pattern.matches(&t.name)
                && permits(old.as_ref(), &t.name) != permits(self.rights.as_ref(), &t.name)
        })
    }
}
