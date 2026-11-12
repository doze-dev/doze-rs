//! What goes into the group machine and what comes out of it. No wire types: the handlers translate.

use bytes::Bytes;
use kafka_wire::ResponseError;
use kafka_wire::ids::{InstanceId, MemberId};

/// A moment, in milliseconds on the clock whoever drives the machine keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub(crate) struct Millis(pub u64);

impl Millis {
    /// This moment and `ms` later.
    #[must_use]
    pub(crate) fn plus(self, ms: u64) -> Self {
        Self(self.0.saturating_add(ms))
    }
}

/// Names a request that is waiting for its answer. The machine never holds the channel the answer goes down: the
/// one that drives it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Token(pub u64);

/// A protocol a member offers, with the metadata it goes with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Protocol {
    /// Its name, as the assignor is called.
    pub(crate) name: String,
    /// What the member says about itself under that protocol (for the consumer protocol, its subscription).
    pub(crate) metadata: Bytes,
}

/// A JoinGroup.
#[derive(Debug, Clone)]
pub(crate) struct JoinRequest {
    /// The member id the client has, if it has one.
    pub(crate) member_id: Option<MemberId>,
    /// Its static instance id, if it has one.
    pub(crate) instance: Option<InstanceId>,
    /// `consumer`, for a consumer group.
    pub(crate) protocol_type: String,
    /// What it offers, most preferred first.
    pub(crate) protocols: Vec<Protocol>,
    /// How long the broker waits for a heartbeat.
    pub(crate) session_ms: i32,
    /// How long a rebalance waits for this member to rejoin.
    pub(crate) rebalance_ms: i32,
    /// The client software's name for itself, as DescribeGroups shows it.
    pub(crate) client_id: String,
    /// Where the client connected from (a slash, then the address).
    pub(crate) client_host: String,
    /// The id was issued to this client by the broker and is still good: it is a new member's, not an unknown one's.
    /// Set by whoever keeps the ids issued; the machine only reads it.
    pub(crate) issued: bool,
    /// JoinGroup v4 and later: a client without an id is given one and told to come back with it.
    pub(crate) id_first: bool,
    /// What the caller may DESCRIBE (`None`: everything): a consumer group gives a classic member only the topics of
    /// its subscription that this lets through, and keeps it for the member's life there (SEC-231).
    pub(crate) rights: Option<crate::authorizer::TopicRights>,
    /// JoinGroup v5 and later: why the client says it joins, clipped. Only a note keeps it; nothing decides by it.
    pub(crate) reason: Option<crate::groups::note::Reason>,
}

/// A SyncGroup.
#[derive(Debug, Clone)]
pub(crate) struct SyncRequest {
    /// Who is asking.
    pub(crate) member_id: MemberId,
    /// Its static instance id, if it has one.
    pub(crate) instance: Option<InstanceId>,
    /// The generation it believes it is in.
    pub(crate) generation: i32,
    /// v5: the protocol type it believes the group has.
    pub(crate) protocol_type: Option<String>,
    /// v5: the protocol it believes was chosen.
    pub(crate) protocol: Option<String>,
    /// From the leader: what each member is to do.
    pub(crate) assignments: Vec<(MemberId, Bytes)>,
}

/// A Heartbeat.
#[derive(Debug, Clone)]
pub(crate) struct HeartbeatRequest {
    /// Who is asking.
    pub(crate) member_id: MemberId,
    /// Its static instance id, if it has one.
    pub(crate) instance: Option<InstanceId>,
    /// The generation it believes it is in.
    pub(crate) generation: i32,
    /// What the caller may DESCRIBE (`None`: everything), kept up to date for a classic member of a consumer group.
    pub(crate) rights: Option<crate::authorizer::TopicRights>,
}

/// A member as DescribeGroups shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MemberDescription {
    pub(crate) id: MemberId,
    pub(crate) instance: Option<InstanceId>,
    pub(crate) client_id: String,
    /// Where it connected from (a slash, then the address).
    pub(crate) client_host: String,
    /// Its metadata under the protocol chosen.
    pub(crate) metadata: Bytes,
    /// What the leader assigned it.
    pub(crate) assignment: Bytes,
}

/// A group as DescribeGroups and ListGroups show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Description {
    pub(crate) state: super::State,
    pub(crate) protocol_type: Option<String>,
    pub(crate) protocol: Option<String>,
    pub(crate) members: Vec<MemberDescription>,
    /// The topics its members subscribe to; `None` if that cannot be said (not a consumer group, or a member's
    /// subscription is unreadable).
    pub(crate) subscribed: Option<std::collections::BTreeSet<String>>,
    /// Set if this is a consumer group, which the classic APIs do not describe.
    pub(crate) consumer: Option<crate::groups::consumer::ConsumerView>,
    /// What the console is told besides (feature `inspect`).
    #[cfg(feature = "inspect")]
    pub(crate) inspect: Box<crate::groups::Inspect>,
}

/// One member a LeaveGroup names, by id or by static instance.
#[derive(Debug, Clone)]
pub(crate) struct LeaveTarget {
    /// Its id, if the request gave one.
    pub(crate) member_id: Option<MemberId>,
    /// Its instance id, if the request gave one.
    pub(crate) instance: Option<InstanceId>,
    /// LeaveGroup v5 and later: why the client says it leaves, clipped. Only a note keeps it.
    pub(crate) reason: Option<crate::groups::note::Reason>,
}

/// A member as the leader is told of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JoinMember {
    /// Its id.
    pub(crate) id: MemberId,
    /// Its instance id, if it is static.
    pub(crate) instance: Option<InstanceId>,
    /// Its metadata under the chosen protocol.
    pub(crate) metadata: Bytes,
}

/// The answer to a JoinGroup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JoinAnswer {
    /// Why not, if not.
    pub(crate) error: Option<ResponseError>,
    /// The generation joined, or -1.
    pub(crate) generation: i32,
    /// The group's protocol type (only on success).
    pub(crate) protocol_type: Option<String>,
    /// The protocol chosen (only on success).
    pub(crate) protocol: Option<String>,
    /// The leader's id (only on success).
    pub(crate) leader: Option<MemberId>,
    /// The id the client is to use: the one it has, or the one it was given.
    pub(crate) member_id: Option<MemberId>,
    /// Every member with its metadata: only the leader is told.
    pub(crate) members: Vec<JoinMember>,
}

impl JoinAnswer {
    pub(crate) fn refused(error: ResponseError, member_id: Option<MemberId>) -> Self {
        Self {
            error: Some(error),
            generation: -1,
            protocol_type: None,
            protocol: None,
            leader: None,
            member_id,
            members: Vec::new(),
        }
    }
}

/// The answer to a SyncGroup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyncAnswer {
    /// Why not, if not.
    pub(crate) error: Option<ResponseError>,
    /// The group's protocol type (only on success).
    pub(crate) protocol_type: Option<String>,
    /// The protocol chosen (only on success).
    pub(crate) protocol: Option<String>,
    /// What this member is to do.
    pub(crate) assignment: Bytes,
}

impl SyncAnswer {
    pub(crate) fn refused(error: ResponseError) -> Self {
        Self {
            error: Some(error),
            protocol_type: None,
            protocol: None,
            assignment: Bytes::new(),
        }
    }
}

/// What became of one member a LeaveGroup named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LeaveResult {
    /// As named.
    pub(crate) member_id: Option<MemberId>,
    /// As named.
    pub(crate) instance: Option<InstanceId>,
    /// Why not, if not.
    pub(crate) error: Option<ResponseError>,
}

/// An answer to a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Answer {
    /// To a JoinGroup.
    Join(JoinAnswer),
    /// To a SyncGroup.
    Sync(SyncAnswer),
    /// To a Heartbeat.
    Heartbeat(Option<ResponseError>),
    /// To a LeaveGroup, one result per member named.
    Leave(Vec<LeaveResult>),
    /// To a commit's fence: why it may not be stored, if it may not.
    Commit(Option<ResponseError>),
    /// To a request to describe the group.
    Describe(Description),
    /// To a request to delete the group: why not, if not.
    Delete(Option<ResponseError>),
    /// To a ConsumerGroupHeartbeat.
    ConsumerHeartbeat(crate::groups::consumer::HeartbeatAnswer),
    /// To a request for what the group has said of itself (the actor answers; no machine does).
    Events(Vec<crate::groups::events::Recorded>),
    /// To a forced step.
    Forced(Forced),
}

/// Which clock a timer is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum TimerKind {
    /// A member's session: it has to be heard from before this runs out.
    Session(MemberId),
    /// A rebalance's deadline for members to rejoin.
    Rebalance,
    /// The initial rebalance delay's current window.
    Hold,
    /// A consumer group member's time to give up what it was told to.
    Revoke(MemberId),
}

/// A timer that has run out. The generation tells a stale one (re-armed since) from the current.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TimerKey {
    /// Which.
    pub(crate) kind: TimerKind,
    /// The arming it belongs to.
    pub(crate) generation: u64,
}

/// What can happen to a group.
#[derive(Debug, Clone)]
pub(crate) enum Event {
    /// A JoinGroup arrived.
    Join(Token, JoinRequest),
    /// A SyncGroup arrived.
    Sync(Token, SyncRequest),
    /// A Heartbeat arrived.
    Heartbeat(Token, HeartbeatRequest),
    /// A LeaveGroup arrived.
    Leave(Token, Vec<LeaveTarget>),
    /// An OffsetCommit wants to know if it may be stored.
    Commit(Token, super::commit::CommitRequest),
    /// Someone wants to see the group.
    Describe(Token),
    /// Someone wants the group gone.
    Delete(Token),
    /// A ConsumerGroupHeartbeat arrived.
    ConsumerHeartbeat(Token, crate::groups::consumer::Heartbeat),
    /// A topic was made, grown or deleted.
    TopicsChanged,
    /// A timer ran out.
    Timer(TimerKey),
    /// The console's Lab forces a step the group would take by itself: see [`Force`].
    Force(Token, Force),
}

/// What the Lab forces on a group. Each is the step the group takes for the same cause by itself, taken now: a member's session
/// timer running out, or a change the members cause (a new epoch, a new generation). Nothing is a transition of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Force {
    /// A member's session runs out.
    Expire(MemberId),
    /// The group starts a rebalance (classic) or moves to a new epoch (the new protocol).
    Rebalance,
}

/// What came of a [`Force`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Forced {
    /// It was taken.
    Done,
    /// There is no such group in memory.
    NoSuchGroup,
    /// The group has no such member.
    NoSuchMember,
    /// The group has nobody to rebalance.
    Empty,
    /// A rebalance is already running.
    AlreadyRebalancing,
}

/// What the machine asks of whoever drives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Effect {
    /// Answer a request.
    Reply(Token, Answer),
    /// Call back with this timer at `at`, replacing any of the same kind.
    Arm {
        /// Which timer and which arming.
        key: TimerKey,
        /// When.
        at: Millis,
    },
    /// Forget the timer of this kind.
    Disarm(TimerKind),
    /// Something the machine says of itself: why a rebalance began, what came of it. Nothing depends on it; whoever
    /// drives the machine may keep it or drop it.
    Note(crate::groups::note::Note),
    /// A request is replaced by a later one (a JoinGroup sent again) and will never be answered: whoever waits for
    /// it can stop.
    Forget(Token),
}
