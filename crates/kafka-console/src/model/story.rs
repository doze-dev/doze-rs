//! What the Story pages say, as plain data: why a group rebalanced, one generation at a time, and whether it is flapping.
//! Made from `ConsoleView::group_events` and what the group page already knows (`story` in `gather`); no HTML, no broker
//! types.

use super::{GroupHead, GroupPhase, GroupProtocol};

/// Why a generation began, in the console's own type (the broker's `Cause` is mapped to it in `gather::story`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StoryCause {
    MemberJoined,
    MemberLeft,
    SessionExpired,
    TopicChanged,
    ProtocolsChanged,
    LeaderRejoined,
    RebalanceTimeout,
    SubscriptionChanged,
    RightsChanged,
    BecameClassic,
    BecameConsumer,
}

impl StoryCause {
    /// Every cause, in the order the Story page lists them.
    pub const ALL: [Self; 11] = [
        Self::MemberJoined,
        Self::MemberLeft,
        Self::SessionExpired,
        Self::TopicChanged,
        Self::ProtocolsChanged,
        Self::LeaderRejoined,
        Self::RebalanceTimeout,
        Self::SubscriptionChanged,
        Self::RightsChanged,
        Self::BecameClassic,
        Self::BecameConsumer,
    ];
}

/// How a generation is drawn: the colour of its dot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoryTone {
    /// Still going.
    Warn,
    /// A member was lost to a timeout.
    Bad,
    /// Something outside the group moved (a topic).
    Idle,
    Plain,
}

/// The member a generation is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryWho {
    pub member: String,
    pub client: Option<String>,
    /// What the client said when it joined or left, clipped by the broker to 128 bytes.
    pub reason: Option<String>,
    /// The session timeout the member had, where the broker knew it.
    pub session_ms: Option<u64>,
}

/// Partitions by topic: the topic and the partitions of it.
pub type StoryParts = Vec<(String, Vec<i32>)>;

/// What one member held in a generation, by topic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryHold {
    pub member: String,
    pub topics: StoryParts,
    pub total: u32,
}

/// A member that did not join again in time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryDropped {
    pub who: StoryWho,
    pub waited_ms: u64,
}

/// One generation (a classic group) or epoch (a consumer group): why it began and what came of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryGen {
    pub generation: i32,
    pub protocol: GroupProtocol,
    pub cause: StoryCause,
    pub at_ms: i64,
    pub who: Option<StoryWho>,
    /// More members that did the same in the same request.
    pub more: u32,
    pub before: u32,
    pub after: u32,
    /// How long the members were given to join again.
    pub wait_ms: Option<u64>,
    /// From the beginning to the generation formed, and to its assignment handed out.
    pub formed_ms: Option<u64>,
    pub settled_ms: Option<u64>,
    /// The wait has not ended: the generation is still forming.
    pub forming: bool,
    /// The members of the generation formed (the first few) and how many there were.
    pub members: Vec<String>,
    pub members_total: u32,
    pub leader: Option<String>,
    pub holds: Vec<StoryHold>,
    pub holds_total: u32,
    pub dropped: Vec<StoryDropped>,
    /// How long it ran before the next began; `None` for the newest.
    pub lasted_ms: Option<u64>,
    /// What the generation before held, for the member who left: partitions that have to move.
    pub moves: Vec<(String, StoryParts)>,
    pub tone: StoryTone,
}

/// The rebalance as the group's tabs and sentences know it: how many the ring holds and the newest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoryFacts {
    /// Generations begun, or epochs moved, that the ring still holds.
    pub count: usize,
    pub last: Option<StoryLast>,
    /// When each member that joined last did, and the generation or epoch it began (the consumer group's member rows).
    pub joins: Vec<StoryJoin>,
    /// The member whose going began the newest rebalance, while it is forming: it is not in the group any more.
    pub left: Option<StoryLeft>,
}

/// A member that left or timed out and began the rebalance now forming.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryLeft {
    pub member: String,
    /// What it held, by topic (the generation before noted it).
    pub held: StoryParts,
    pub ago_ms: u64,
    pub cause: StoryCause,
}

/// A member that joined: how long ago, and the generation or epoch that began.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryJoin {
    pub member: String,
    pub ago_ms: u64,
    pub generation: i32,
}

/// The newest rebalance, for a sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryLast {
    pub cause: StoryCause,
    pub generation: i32,
    pub who: Option<String>,
    pub ago_ms: u64,
    pub forming: bool,
}

/// What the group is doing now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryNow {
    pub phase: GroupPhase,
    pub protocol: GroupProtocol,
    /// The generation running, or the one forming.
    pub generation: i32,
    pub began_ms: Option<i64>,
    pub cause: Option<StoryCause>,
    pub who: Option<StoryWho>,
    pub waiting_for: Vec<String>,
    pub remaining_ms: Option<u64>,
    pub timeout_ms: Option<u64>,
    /// How long the generation running has run.
    pub held_ms: Option<u64>,
}

/// One line of the flapping rule held against the group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryRule {
    pub ok: bool,
    pub title: String,
    pub detail: String,
    pub value: String,
}

/// The flapping rule, worked out: three or more expired-then-joined cycles in ten minutes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryFlap {
    pub flapping: bool,
    /// Expired-then-joined cycles in the window.
    pub cycles: usize,
    /// How many make flapping.
    pub needs: usize,
    pub window_ms: u64,
    /// Session expiries in the window and in everything the ring holds.
    pub expiries_in_window: usize,
    pub expiries: usize,
    pub rows: Vec<StoryRule>,
}

/// A group that rebalances rarely, for contrast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryCalm {
    pub group: String,
    pub generation: StoryGen,
    pub count: usize,
}

/// A page of a group's story.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoryView {
    pub head: GroupHead,
    pub now: StoryNow,
    /// Newest first.
    pub generations: Vec<StoryGen>,
    /// How many of each cause the ring holds.
    pub causes: Vec<(StoryCause, usize)>,
    pub flap: StoryFlap,
    pub calm: Option<StoryCalm>,
    /// The time of the oldest note the ring holds.
    pub since_ms: Option<i64>,
    /// The ring is at its limit, so older notes were let go.
    pub full: bool,
    pub events_per_group: usize,
    /// The group has a task in memory to ask: false when the broker was restarted and nobody has joined since.
    pub in_memory: bool,
}
