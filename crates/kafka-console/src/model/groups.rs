//! The view-models of the Groups pages: the list, one group (members, lag), and the dialogs that change its offsets.
//!
//! Every name here starts with `Group`, `Lag`, `Reset` or `Offset` so that no other family of pages can clash with it.

use super::write::{Lines, Refusal};
use crate::lag::Lag;

/// Which protocol the group speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupProtocol {
    Classic,
    /// KIP-848: the broker assigns.
    Consumer,
}

/// Where a group is in its life, in the console's one word for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupPhase {
    Empty,
    Stable,
    /// `PreparingRebalance` or `CompletingRebalance`.
    Rebalancing,
    /// A consumer group whose targets are yet to be computed.
    Assigning,
    /// A consumer group whose members are not yet where their targets are.
    Reconciling,
}

impl GroupPhase {
    /// Kafka's state word, mapped: unknown words are the nearest honest one.
    #[must_use]
    pub fn of(word: &str) -> Self {
        match word {
            "Stable" => Self::Stable,
            "PreparingRebalance" | "CompletingRebalance" => Self::Rebalancing,
            "Assigning" => Self::Assigning,
            "Reconciling" => Self::Reconciling,
            _ => Self::Empty,
        }
    }

    /// The word the page writes.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Empty => "Empty",
            Self::Stable => "Stable",
            Self::Rebalancing => "Rebalancing",
            Self::Assigning => "Assigning",
            Self::Reconciling => "Reconciling",
        }
    }
}

/// A partition of a topic.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct GroupPart {
    pub topic: String,
    pub partition: i32,
}

/// One member of a group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupMemberRow {
    pub id: String,
    pub instance: Option<String>,
    pub client_id: String,
    pub host: String,
    /// The classic group's leader.
    pub leader: bool,
    /// A classic member's generation, a consumer-protocol member's epoch.
    pub epoch: i32,
    pub session_ms: u64,
    pub rebalance_ms: Option<u64>,
    /// How long ago it was last heard from; `None` where the broker did not say.
    pub heard_ago_ms: Option<u64>,
    /// It has joined again in the rebalance that is running (true when none is).
    pub rejoined: bool,
    /// What it holds now.
    pub holds: Vec<GroupPart>,
    /// What the broker targets for it (a consumer group); equal to `holds` once it has caught up.
    pub target: Vec<GroupPart>,
}

/// A rebalance running now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRebalance {
    pub began_ago_ms: u64,
    pub remaining_ms: u64,
    pub timeout_ms: u64,
    /// The members that have not joined again.
    pub waiting_for: Vec<String>,
}

/// What every page of one group says about it: the title row, the tabs and the switch-group chips.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupHead {
    pub id: String,
    pub protocol: GroupProtocol,
    pub phase: GroupPhase,
    /// The assignor: a classic group's chosen protocol, a consumer group's.
    pub assignor: Option<String>,
    /// A classic generation, a consumer epoch.
    pub generation: i32,
    /// The epoch a consumer group's targets were computed for.
    pub target_epoch: Option<i32>,
    pub leader: Option<String>,
    pub rebalance: Option<GroupRebalance>,
    pub members: Vec<GroupMemberRow>,
    pub protocol_type: Option<String>,
    /// The other groups of the broker, for the switch-group chips (this one is among them, by id).
    pub groups: Vec<String>,
    /// All the lag the group is behind by (the Lag tab's count).
    pub lag_total: i64,
    /// How many partitions it has committed.
    pub commits: usize,
    pub last_commit_ms: Option<i64>,
    pub empty_since_ms: Option<i64>,
    pub now_ms: i64,
    /// What a consumer group's members are given as a session (`--consumer-session-timeout`).
    pub session_ms: Option<u64>,
    /// What the group has said of itself: how many rebalances the ring holds and the newest (the Story tab).
    pub story: super::story::StoryFacts,
}

impl GroupHead {
    /// The group has no member.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

/// How a row of the list reads its lag.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RowLag {
    /// The sum of what the group is behind by.
    pub total: i64,
    /// The same lag in each partition it is behind in, if it is.
    pub each: Option<i64>,
    /// The first partition whose commit is beyond its log, and by how much.
    pub ahead: Option<(String, i64)>,
    /// Topics with a partition never committed.
    pub uncommitted: Vec<String>,
    /// The group has committed anything.
    pub any_commit: bool,
}

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRow {
    pub id: String,
    pub protocol: GroupProtocol,
    pub assignor: Option<String>,
    pub phase: GroupPhase,
    pub generation: i32,
    pub members: usize,
    /// The topics it committed on (or its members hold).
    pub reads: Vec<String>,
    pub lag: RowLag,
    pub last_commit_ms: Option<i64>,
    /// The first moment the sweep takes one of its commits.
    pub expires_ms: Option<i64>,
    /// What the group has said of itself: the newest rebalance (the list says "last rebalance 6 h ago" of a consumer group).
    pub story: super::story::StoryFacts,
}

/// Which state chip of the list is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupFilter {
    All,
    Stable,
    Rebalancing,
    Empty,
}

impl GroupFilter {
    /// From `?state=`.
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "stable" => Self::Stable,
            "rebalancing" => Self::Rebalancing,
            "empty" => Self::Empty,
            _ => Self::All,
        }
    }

    /// The value in the address.
    #[must_use]
    pub const fn value(self) -> &'static str {
        match self {
            Self::All => "",
            Self::Stable => "stable",
            Self::Rebalancing => "rebalancing",
            Self::Empty => "empty",
        }
    }

    /// Whether a group in `phase` is shown.
    #[must_use]
    pub fn takes(self, phase: GroupPhase) -> bool {
        match self {
            Self::All => true,
            Self::Stable => phase == GroupPhase::Stable,
            Self::Rebalancing => !matches!(phase, GroupPhase::Stable | GroupPhase::Empty),
            Self::Empty => phase == GroupPhase::Empty,
        }
    }
}

/// The states of the list that are on: a group shows when it is in ANY of them; none on means every group.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GroupStates(Vec<GroupFilter>);

impl GroupStates {
    /// From every `state=` of an address (repeated: `state=stable&state=empty`); a word that is no state is left out.
    #[must_use]
    pub fn parse(fields: &[(String, String)]) -> Self {
        let mut v: Vec<GroupFilter> = Vec::new();
        for (k, val) in fields {
            let s = GroupFilter::parse(val.trim());
            if k == "state" && s != GroupFilter::All && !v.contains(&s) {
                v.push(s);
            }
        }
        v.sort_by_key(|s| *s as u8);
        Self(v)
    }

    /// No state chosen (every group shows).
    #[must_use]
    pub fn is_all(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether this state is on.
    #[must_use]
    pub fn has(&self, s: GroupFilter) -> bool {
        self.0.contains(&s)
    }

    /// The values to put in an address, one per state on.
    #[must_use]
    pub fn values(&self) -> Vec<&'static str> {
        self.0.iter().map(|s| s.value()).collect()
    }

    /// Whether a group in `phase` shows.
    #[must_use]
    pub fn takes(&self, phase: GroupPhase) -> bool {
        self.0.is_empty() || self.0.iter().any(|s| s.takes(phase))
    }

    /// The address's query part for a name filter and these states.
    #[must_use]
    pub fn query(&self, filter: &str) -> String {
        let mut fields: Vec<(&str, &str)> = vec![("q", filter)];
        fields.extend(self.values().into_iter().map(|v| ("state", v)));
        crate::form::encode_query(&fields)
    }
}

/// A group that is rebalancing, for the sentence at the top of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupAttention {
    pub id: String,
    pub waiting_for: Vec<String>,
    pub remaining_ms: u64,
    pub timeout_ms: u64,
    /// Why it began and how long ago, if the group has said (the Story).
    pub last: Option<super::story::StoryLast>,
}

/// The Groups list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupsView {
    /// After the filters, by id.
    pub rows: Vec<GroupRow>,
    /// All the groups the broker lists.
    pub total: usize,
    /// How many are in each state: all, stable, rebalancing, empty.
    pub counts: [usize; 4],
    pub filter: String,
    pub state: GroupStates,
    /// How many topics the broker has (the other tab).
    pub topics: usize,
    pub attention: Option<GroupAttention>,
    pub now_ms: i64,
    /// More groups matched than the page shows.
    pub truncated: bool,
}

/// The states of the Lag page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LagState {
    /// `group-lag`.
    Normal,
    /// `lag-no-commits`: a partition was never committed.
    NoCommits,
    /// `lag-ahead`: a commit is beyond its log.
    Ahead,
    /// `lag-expiring`: asked for with `?expiring=1`.
    Expiring,
}

/// One partition of the Lag table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LagRow {
    pub topic: String,
    pub partition: i32,
    pub end: Option<i64>,
    pub committed: Option<i64>,
    pub lag: Lag,
    /// Per cent of the bar, 0 to 100.
    pub bar: u8,
    pub last_commit_ms: Option<i64>,
    /// The first moment the sweep takes this commit.
    pub expires_ms: Option<i64>,
}

/// A topic's block of the Lag table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LagTopic {
    pub name: String,
    pub partitions: usize,
    pub exists: bool,
    pub compacted: bool,
    pub retention_ms: Option<i64>,
    /// The members that hold its partitions, by id (one name when one member holds all).
    pub holders: Vec<String>,
    pub rows: Vec<LagRow>,
    pub behind: i64,
}

/// The Lag tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupLagView {
    pub head: GroupHead,
    pub state: LagState,
    pub topics: Vec<LagTopic>,
    /// `--offsets-retention`.
    pub retention_ms: i64,
    /// A line to show above the page after a change (`Reset 3 offsets.`).
    pub flash: Option<String>,
}

/// What a reset moves the offsets to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetMode {
    Earliest,
    Latest,
    /// One offset for every partition.
    Offset(i64),
    /// The first offset at or after this time, in milliseconds.
    Time(i64),
    /// The committed offset plus this many (negative: back).
    Shift(i64),
    /// The committed offset, kept within the log.
    Current,
}

/// One partition of the preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResetRow {
    pub partition: i32,
    pub committed: Option<i64>,
    /// Where it would be; `None` where the mode cannot say (a time with no record at or after it is the log end).
    pub after: Option<i64>,
    pub lag_after: Lag,
}

/// The reset dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResetView {
    pub head: GroupHead,
    /// The topics the group has committed on.
    pub topics: Vec<String>,
    pub topic: String,
    /// The radio chosen: `earliest`, `latest`, `offset`, `time`, `shift`, `current`.
    pub to: String,
    /// What was typed for the modes that take a value.
    pub value: String,
    pub rows: Vec<ResetRow>,
    pub call: Lines,
    /// Why the choice cannot be previewed (a value that is not a number).
    pub error: Option<String>,
}

/// The delete-group dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupDeleteView {
    pub head: GroupHead,
    /// The topics it has committed on, and the partitions of each: what goes with the group.
    pub offsets: Vec<(String, Vec<i32>)>,
    pub call: Lines,
}

/// The delete-offsets dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OffsetDeleteView {
    pub head: GroupHead,
    /// Each topic with the partitions committed on it.
    pub topics: Vec<(String, Vec<i32>)>,
    pub chosen: Vec<String>,
    pub call: Lines,
}

/// A word of a sentence of the Groups pages: plain, monospace, bold, or a link to a topic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupSpan {
    Text(String),
    Mono(String),
    Strong(String),
    /// A topic's name, linked to its page.
    Topic(String),
}

impl GroupSpan {
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text(s.into())
    }

    pub fn mono(s: impl Into<String>) -> Self {
        Self::Mono(s.into())
    }

    pub fn strong(s: impl Into<String>) -> Self {
        Self::Strong(s.into())
    }

    pub fn topic(s: impl Into<String>) -> Self {
        Self::Topic(s.into())
    }
}

/// One line of "What would pass".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusalStep {
    /// This is what is in the way now.
    pub blocking: bool,
    pub title: String,
    pub sub: String,
    /// The figure at the right (`1 member now`).
    pub value: String,
}

/// A refused write of the Groups pages: Kafka's words and the console's reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRefused {
    pub head: GroupHead,
    /// Which write: `Reset offsets` or `Delete group`.
    pub what: String,
    pub refusal: Refusal,
    /// "The rule", in the console's words.
    pub rule: Vec<GroupSpan>,
    pub steps: Vec<RefusalStep>,
    /// The call that was refused, with its answer.
    pub call: Lines,
    /// What the page says it left alone (`18,240 behind on clickstream`).
    pub unchanged: String,
}

#[cfg(test)]
mod states_tests {
    use super::*;

    fn s(q: &str) -> GroupStates {
        GroupStates::parse(&crate::form::parse(q))
    }

    #[test]
    fn states_are_a_repeated_field_and_a_group_shows_in_any_of_them() {
        let w = s("state=empty&state=stable&state=stable&state=nonsense&state=");
        assert_eq!(w.values(), ["stable", "empty"]);
        assert!(w.has(GroupFilter::Stable) && !w.has(GroupFilter::Rebalancing));
        assert!(
            w.takes(GroupPhase::Stable)
                && w.takes(GroupPhase::Empty)
                && !w.takes(GroupPhase::Rebalancing)
        );
        assert!(s("").is_all() && s("").takes(GroupPhase::Rebalancing));
        assert_eq!(w.query("pay"), "?q=pay&state=stable&state=empty");
        assert_eq!(s("state=all").values(), Vec::<&str>::new());
    }
}
