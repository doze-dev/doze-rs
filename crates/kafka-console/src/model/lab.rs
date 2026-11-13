//! What the Lab pages say: the rules in force and the ones that ended, the rehearsals that happen once, the forms that make them.
//! Plain data; `crate::lab` makes it from the fault table and the broker, the pages write it. Names are prefixed `Lab` so that
//! no other family's type can clash with them.

use super::live::WireRow;

/// The kind of a rule.
#[derive(Debug, Clone, serde::Serialize, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LabKind {
    /// Delay the answer.
    Slow,
    /// Answer with an error.
    Fail,
    /// Close the connection.
    Drop,
}

/// Why a rule is over.
#[derive(Debug, Clone, serde::Serialize, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LabEnd {
    /// Its time ran out.
    Expired,
    /// It had fired as often as it was allowed.
    Spent,
    /// It was cleared.
    Cleared,
    /// A rule for the same requests took its place.
    Replaced,
}

/// The first request a rule fired on.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabFirst {
    pub at_ms: i64,
    pub client: String,
    pub api: String,
    pub version: i16,
    pub target: Option<String>,
}

/// A rule, in force or over.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabRule {
    pub id: u32,
    pub name: String,
    pub kind: LabKind,
    /// What it does, in a sentence made once from the rule (never a live number in it).
    pub sentence: String,
    pub enabled: bool,
    pub matched: u64,
    pub hits: u64,
    pub started_ms: i64,
    /// When it ends (in force) or ended (over).
    pub ends_ms: i64,
    pub last_hit_ms: Option<i64>,
    /// The delay it adds, for a slow rule.
    pub delay_ms: Option<u32>,
    /// The error it answers with, for a fail rule: code and Kafka's name.
    pub code: Option<(i16, String)>,
    /// The API (or APIs) and the target, as words, for the tables.
    pub api: String,
    pub target: String,
    /// How often, as words: `1 in 4`.
    pub share: String,
    pub first: Option<LabFirst>,
    pub clients: Vec<String>,
    pub more_clients: u32,
    /// Set once it is over.
    pub ended: Option<LabEnd>,
}

/// What a rehearsal that happens once is.
#[derive(Debug, Clone, serde::Serialize, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum LabOnceKind {
    /// Close an app's connections.
    Drop,
    /// Run a member's session out.
    Expire,
    /// Force a rebalance.
    Rebalance,
    /// Run the cleaner on a partition.
    Clean,
}

impl LabOnceKind {
    /// The word for it in a list.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Drop => "drop",
            Self::Expire => "expire",
            Self::Rebalance => "rebalance",
            Self::Clean => "clean",
        }
    }
}

/// A rehearsal that happened once, as the Ended list keeps it.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabOnce {
    pub id: u32,
    pub at_ms: i64,
    pub kind: LabOnceKind,
    /// What was done, in a sentence made once.
    pub sentence: String,
    /// Whether it did what it was asked.
    pub ok: bool,
}

/// The Lab page.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabHome {
    /// `--lab=false`: the feature is not installed.
    pub off: bool,
    pub now_ms: i64,
    pub active: Vec<LabRule>,
    /// Over in the last hour, newest first.
    pub ended: Vec<LabRule>,
    pub once: Vec<LabOnce>,
    /// How many rules there may be.
    pub most_rules: usize,
    /// A compacted topic the cleaner card can open the dialog for, if there is one.
    pub compact_target: Option<String>,
    /// Every compacted topic, for the cleaner's dialog.
    pub compacted: Vec<String>,
}

impl LabHome {}

/// The page of one rule.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabRuleView {
    pub rule: LabRule,
    pub now_ms: i64,
    /// The newest requests it touched, newest first (from the wire).
    #[serde(skip)]
    #[cfg_attr(test, ts(skip))]
    pub hits: Vec<WireRow>,
    /// The wire holds fewer than all of them.
    pub more_hits: bool,
}

/// The fields of the Slow and Fail forms, as text: what was typed or chosen.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct LabFields {
    /// One API or several, by name, separated by commas (`produce`, `Produce,Fetch`).
    pub api: String,
    /// One topic or several (each exact or a glob), separated by commas; empty is every topic.
    pub topic: String,
    /// Partitions of those topics, separated by commas; empty is every partition.
    pub partitions: String,
    /// The group of a group API (exact or a glob); empty is any.
    pub group: String,
    pub client: String,
    pub delay: String,
    pub jitter: String,
    pub share: String,
    pub ends: String,
    pub hits: String,
    pub code: String,
}

/// A choice in a list of a form.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabChoice {
    pub value: String,
    pub label: String,
    pub desc: Option<String>,
}

/// What a rule would do, worked out from the form without making it.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabPreview {
    /// The call, as lines of the rule it makes.
    pub rule: Vec<(String, String)>,
    pub sentence: String,
    /// What the wire would show for a touched request, as words.
    pub wire: String,
}

/// A form of a rule (Slow or Fail).
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabForm {
    pub kind: LabKind,
    pub fields: LabFields,
    /// Field and the sentence that says what to fix.
    pub errors: Vec<(String, String)>,
    pub topics: Vec<LabChoice>,
    pub clients: Vec<String>,
    /// The groups there are, for the group field.
    pub groups: Vec<String>,
    /// The APIs a rule of this kind may name, with what each is.
    pub apis: Vec<LabChoice>,
    /// The errors a Fail rule may answer with (code, Kafka's name, what a client does).
    pub codes: Vec<LabChoice>,
    /// How many of the requests: every, 1 in N, a random per cent.
    pub shares: Vec<LabChoice>,
    /// When the rule ends, in minutes.
    pub ends: Vec<LabChoice>,
    /// A sentence about the chosen topic, when the broker has it.
    pub topic_note: Option<String>,
    pub preview: Option<LabPreview>,
    /// The Lab has no room or the engine said no: its words.
    pub refused: Option<String>,
}

/// The result of asking for a rule.
#[derive(Debug, Clone, PartialEq)]
pub enum LabMade {
    /// The rule is in force: its number.
    Made(u32),
    /// A field is wrong, or the engine refused.
    Invalid(Box<LabForm>),
}

/// A dialog of a rehearsal that happens once.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabOnceForm {
    pub kind: LabOnceKind,
    pub target: String,
    pub how: String,
    pub choices: Vec<LabChoice>,
    pub error: Option<String>,
    pub refused: Option<String>,
    /// A sentence about what the choice is, from the broker.
    pub note: Option<String>,
    /// What there is to choose from is nothing: the sentence that says so.
    pub nothing: Option<String>,
    /// The call, as words, for the box under the form.
    pub call: String,
    /// What follows, from the broker's own facts.
    pub follows: Option<String>,
}

/// The result of a rehearsal that happens once.
#[derive(Debug, Clone, PartialEq)]
pub enum LabOnceDone {
    Done,
    Invalid(Box<LabOnceForm>),
}

/// The Compact now dialog of a partition.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabCompact {
    pub topic: String,
    pub partition: i32,
    /// The planner finds a round due, with its words (`61 % dirty against 50 %`).
    pub due: Option<String>,
    /// What a round would rewrite, in the words of the Log page.
    pub rewrite: Vec<super::Span>,
    /// Where the checkpoint is and where the round would move it.
    pub checkpoint: (i64, i64),
    /// Why nothing can be done, in a sentence.
    pub blocked: Option<String>,
    pub refused: Option<String>,
    pub off: bool,
}
