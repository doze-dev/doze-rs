//! The view-models of the topic forms: what a person typed, what the broker said of it, and the call that goes with it.

use super::{Span, Tok};

/// A block of code, one entry per line: what `ui::code::block` draws.
pub type Lines = Vec<Vec<(Tok, String)>>;

/// The words of a refused call: Kafka's code and the broker's sentence, then the console's reading of it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Refusal {
    /// `36`.
    pub code: i16,
    /// `TOPIC_ALREADY_EXISTS`.
    pub name: String,
    /// The broker's own sentence for this call (its `error_message`), else Kafka's description of the code.
    pub message: String,
    /// Why, in the console's words, where the table has them.
    pub rule: Option<Vec<Span>>,
    /// What would pass, where the table has it.
    pub would_pass: Option<Vec<Span>>,
}

/// The step of the Create-topic form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, ts(rename = "CreateStep"))]
pub enum Step {
    Name,
    Config,
    Review,
}

/// A starting point for the configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename = "CreateTemplate"))]
pub enum Template {
    #[serde(rename = "event")]
    EventLog,
    #[serde(rename = "compact")]
    Compacted,
    #[serde(rename = "short")]
    ShortLived,
}

/// What a person typed in the Create-topic form, as text (checked, never trusted).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Draft {
    pub name: String,
    pub partitions: String,
    pub template: Template,
    /// `cleanup.policy`, as typed or picked.
    pub cleanup: String,
    /// `retention.ms`, as typed.
    pub retention: String,
}

/// The lines under a field: what Kafka says, the default, the range, when it takes effect, and what is so here.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Teach {
    pub says: String,
    pub default: Vec<Span>,
    pub range: Vec<Span>,
    pub takes_effect: Vec<Span>,
    pub here: Vec<Span>,
}

/// One field's error, as the form draws it.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename = "CreateErrors"))]
pub struct Errors {
    pub name: Option<String>,
    pub partitions: Option<String>,
    pub retention: Option<String>,
}

/// The Create-topic form at one of its steps.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CreateView {
    pub step: Step,
    pub draft: Draft,
    pub errors: Errors,
    /// The teaching for name, partitions, cleanup.policy and retention.ms, if the broker told them to this caller.
    pub teach_name: Teach,
    pub teach_partitions: Teach,
    pub teach_cleanup: Option<Teach>,
    pub teach_retention: Option<Teach>,
    /// The call that will be sent, and the same as a terminal command.
    pub call: Lines,
    pub terminal: Vec<String>,
    /// The step-3 result of "Check only", if it was pressed: nothing is created.
    pub checked: Option<Checked>,
    /// The broker's partition limit, if read.
    pub most: Option<i32>,
    /// The address clients use, for the terminal command.
    pub address: String,
}

/// What a dry run of the create said: every check passed, and nothing was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename = "CreateChecked"))]
pub struct Checked {
    pub partitions: i32,
}

/// A topic that was made.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Created {
    pub name: String,
    pub partitions: i32,
    pub cleanup: String,
    pub retention: String,
    /// Offsets of every partition, `(start, end)`: empty for a new topic, which is what the page says.
    pub empty: bool,
    pub address: String,
}

/// A refused create: the answer, and the form's call.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CreateRefused {
    pub draft: Draft,
    pub refusal: Refusal,
    /// Other refusals CreateTopics can give, each with Kafka's code (the card of the board).
    pub request: Lines,
    pub answer: Lines,
    /// The topic that is in the way, when the name is taken.
    pub existing: Option<String>,
    /// The broker's partition limit, if read.
    pub most: Option<i32>,
}

/// The Produce form.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProduceView {
    pub topic: String,
    pub partitions: i32,
    /// `delete`, `compact`, ...
    pub cleanup: String,
    pub compacts: bool,
    pub partition: String,
    pub key: String,
    pub value: String,
    pub headers: Vec<(String, String)>,
    pub acks: String,
    pub errors: ProduceErrors,
    pub call: Lines,
    /// The topic page's open transaction, if any, for the sentence under acks.
    pub transaction: Option<String>,
}

/// Field errors of the Produce form.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProduceErrors {
    pub partition: Option<String>,
    pub key: Option<String>,
    pub acks: Option<String>,
}

/// A record the broker took.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Produced {
    pub topic: String,
    pub partition: i32,
    pub offset: i64,
    pub timestamp_ms: i64,
    pub acks: i16,
    pub log_append_time_ms: i64,
    pub log_start_offset: i64,
    pub head: TopicHead,
    pub row: RecordRow,
    /// The newest records of the other partitions, where the page read them (the sent record is `row`).
    pub others: Vec<RecordRow>,
    pub visibility: Visibility,
    pub answer: Lines,
}

/// The sentence and facts under a topic's name.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename = "ProducedHead"))]
pub struct TopicHead {
    pub name: String,
    pub cleanup: String,
    pub partitions: usize,
    pub retention_ms: Option<i64>,
    pub records: i64,
    /// Open transactions on the topic: `(transactional id, partitions)`.
    pub transactions: Vec<(String, Vec<i32>)>,
    /// `(partition, records held back)` where a read_committed consumer is behind the high watermark.
    pub held_back: Vec<(i32, i64)>,
}

/// The row of the record in the table under the banner.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[cfg_attr(test, ts(rename = "ProducedRow"))]
pub struct RecordRow {
    pub partition: i32,
    pub offset: i64,
    pub timestamp_ms: i64,
    pub key: String,
    pub value: String,
    pub size: u64,
    pub committed: bool,
}

/// Whether the record is visible to a `read_committed` consumer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Visibility {
    pub last_stable: i64,
    pub high_watermark: i64,
    pub transaction_on_partition: bool,
}

/// A refused produce: the form as sent and the answer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProduceRefused {
    pub view: ProduceView,
    pub refusal: Refusal,
    pub partition: i32,
    pub base_offset: i64,
    pub answer: Lines,
    /// Up to three of the broker's per-record messages.
    pub record_errors: Vec<String>,
}

/// The Add-partitions dialog.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AddView {
    pub topic: String,
    pub have: i32,
    pub count: String,
    pub most: Option<i32>,
    pub error: Option<String>,
    pub refusal: Option<Refusal>,
    pub call: Lines,
    pub answer: Option<Lines>,
}

/// The Delete-records dialog.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DeleteRecordsView {
    pub topic: String,
    pub partition: i32,
    pub log_start: i64,
    pub high_watermark: i64,
    pub offset: String,
    pub error: Option<String>,
    /// Groups with a committed offset on the partition: `(group, offset)`.
    pub groups: Vec<(String, i64)>,
    pub call: Lines,
}

/// A delete-records that was refused, on a page of its own.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DeleteRecordsRefused {
    pub topic: String,
    pub cleanup: String,
    pub partition: i32,
    pub log_start: i64,
    pub refusal: Refusal,
    pub answer: Lines,
}

/// Records were deleted: where the log starts now.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RecordsDeleted {
    pub topic: String,
    pub partition: i32,
    pub low_watermark: i64,
}

/// What is using a topic, as far as the protocol says.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct InUse {
    /// `(group, state, members)` for groups with committed offsets on the topic.
    pub groups: Vec<(String, String, usize)>,
    /// Transactional ids with an open transaction on the topic.
    pub transactions: Vec<String>,
}

/// The Delete-topic dialog.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DeleteView {
    pub topic: String,
    pub partitions: usize,
    pub records: i64,
    pub in_use: InUse,
    pub typed: String,
    pub error: Option<String>,
    pub call: Lines,
}

/// A topic that is gone, and what is left (the list the board shows).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Deleted {
    /// The milliseconds since the epoch the page is made at (the ages in the list are counted from it).
    pub now_ms: i64,
    pub name: String,
    pub records: i64,
    pub partitions: usize,
    pub in_use: InUse,
    pub remaining: Vec<Remaining>,
    pub topics: usize,
    pub partitions_left: usize,
}

/// One topic of the list after a delete.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Remaining {
    pub name: String,
    pub partitions: usize,
    pub cleanup: String,
    pub records: i64,
    pub last_write_ms: Option<i64>,
}

/// What a create came to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum CreateResult {
    Created(Created),
    /// "Check only": the broker ran every check and made nothing.
    Checked(Checked),
    Refused(Box<CreateRefused>),
    /// The form could not be sent (a number that is not one): the step it belongs to, with the error.
    Invalid(Box<CreateView>),
}

/// What the Produce dialog posted, as text (checked before anything is sent).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProduceInput {
    /// A partition number, or empty to leave it to the key.
    pub partition: String,
    pub key: String,
    pub value: String,
    /// `(name, value)` rows; a row with no name is ignored.
    pub headers: Vec<(String, String)>,
    /// `-1`, `1` or `0`; empty is `-1`.
    pub acks: String,
}

/// What a produce came to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ProduceResult {
    Sent(Box<Produced>),
    Refused(Box<ProduceRefused>),
    /// The form could not be sent: it again, with the errors.
    Invalid(Box<ProduceView>),
    /// There is no such topic.
    NoTopic,
}

impl Step {
    /// The word the form carries for this step.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Config => "config",
            Self::Review => "review",
        }
    }

    /// The step a word names; anything else is the first.
    #[must_use]
    pub fn from_id(s: &str) -> Self {
        match s {
            "config" => Self::Config,
            "review" => Self::Review,
            _ => Self::Name,
        }
    }
}

impl Template {
    /// The word the form carries for this starting point.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::EventLog => "event",
            Self::Compacted => "compact",
            Self::ShortLived => "short",
        }
    }

    /// The starting point a word names; anything else is the event log.
    #[must_use]
    pub fn from_id(s: &str) -> Self {
        match s {
            "compact" => Self::Compacted,
            "short" => Self::ShortLived,
            _ => Self::EventLog,
        }
    }
}
