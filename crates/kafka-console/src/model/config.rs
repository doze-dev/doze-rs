//! The view-models of the Config pages (slice 9d): a topic's configs, the dialogs that change them, and the answers.
//! Every name starts with `Config` so that no other family of pages can meet it.

use super::{Cleanup, Lines, Refusal, Span};

/// What a change does to a config: IncrementalAlterConfigs numbers them 0 to 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigOp {
    Set,
    Delete,
    Append,
    Subtract,
}

impl ConfigOp {
    /// The number on the wire.
    #[must_use]
    pub const fn wire(self) -> i8 {
        match self {
            Self::Set => 0,
            Self::Delete => 1,
            Self::Append => 2,
            Self::Subtract => 3,
        }
    }

    /// Kafka's word for it, as the boards draw the call.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Set => "SET",
            Self::Delete => "DELETE",
            Self::Append => "APPEND",
            Self::Subtract => "SUBTRACT",
        }
    }

    /// The value a form carries for it.
    #[must_use]
    pub const fn form(self) -> &'static str {
        match self {
            Self::Set => "set",
            Self::Delete => "delete",
            Self::Append => "append",
            Self::Subtract => "subtract",
        }
    }

    /// The operation a form's value names; anything else is a `SET`.
    #[must_use]
    pub fn from_form(text: &str) -> Self {
        match text {
            "delete" => Self::Delete,
            "append" => Self::Append,
            "subtract" => Self::Subtract,
            _ => Self::Set,
        }
    }
}

/// Where a config's value comes from, as DescribeConfigs numbers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    /// 1: set on the topic.
    DynamicTopic,
    /// 2: set for this broker.
    DynamicBroker,
    /// 3: set for all brokers.
    DynamicDefault,
    /// 4: a start flag that is not Kafka's default.
    StaticFlag,
    /// 5: Kafka's default.
    Default,
    /// Any other number.
    Other(i8),
}

impl ConfigSource {
    /// From the number on the wire.
    #[must_use]
    pub const fn from_wire(n: i8) -> Self {
        match n {
            1 => Self::DynamicTopic,
            2 => Self::DynamicBroker,
            3 => Self::DynamicDefault,
            4 => Self::StaticFlag,
            5 => Self::Default,
            other => Self::Other(other),
        }
    }

    /// The tag on a row.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::DynamicTopic => "Dynamic topic",
            Self::DynamicBroker => "Dynamic broker",
            Self::DynamicDefault => "Dynamic broker, all",
            Self::StaticFlag => "Static flag",
            Self::Default => "Default",
            Self::Other(_) => "Other",
        }
    }
}

/// A place a config's value could come from: the broker config behind it, its value and what kind of place it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigSynonym {
    pub name: String,
    pub value: String,
    pub source: ConfigSource,
}

/// One config of a topic as DescribeConfigs lists it, with Kafka's text and the rule for its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigRow {
    pub name: String,
    pub value: String,
    pub source: ConfigSource,
    /// The type as DescribeConfigs numbers it (1 boolean, 2 string, 3 int, 5 long, 6 double, 7 list).
    pub kind: i8,
    /// Kafka's documentation string (it may carry tags, which a page never renders as markup).
    pub doc: String,
    /// What a value may be, in words (from the broker's table), if it said.
    pub allowed: String,
    /// Every place the value could come from, the one that applies first first.
    pub synonyms: Vec<ConfigSynonym>,
}

impl ConfigRow {
    /// The config is a list, which APPEND and SUBTRACT work on.
    #[must_use]
    pub fn is_list(&self) -> bool {
        self.kind == 7
    }

    /// The topic sets it.
    #[must_use]
    pub fn on_topic(&self) -> bool {
        self.source == ConfigSource::DynamicTopic
    }

    /// Not Kafka's default: set on the topic or by a flag.
    #[must_use]
    pub fn changed(&self) -> bool {
        self.source != ConfigSource::Default
    }

    /// The value the topic would follow if it set none: the first place that is not the topic.
    #[must_use]
    pub fn fallback(&self) -> Option<&ConfigSynonym> {
        self.synonyms
            .iter()
            .find(|s| s.source != ConfigSource::DynamicTopic)
    }

    /// The default Kafka gives it, where the broker named it.
    #[must_use]
    pub fn kafka_default(&self) -> Option<&ConfigSynonym> {
        self.synonyms
            .iter()
            .find(|s| s.source == ConfigSource::Default)
    }
}

/// The title row of the Config pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigHead {
    pub name: String,
    pub cleanup: Cleanup,
}

/// How often the broker looks at logs: what "the next look" is in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigPace {
    pub cleaner_secs: u64,
    pub cleaner_on: bool,
    pub retention_secs: u64,
}

/// Which configs the page lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConfigShow {
    #[default]
    All,
    Changed,
    Compaction,
    Segments,
}

/// What the page was asked to show: a filter, the chip, the rows opened, and the replication group.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConfigAsk {
    pub q: String,
    pub show: ConfigShow,
    /// Rows whose "Where from" or "Kafka's text" is open (by key).
    pub open: Vec<String>,
    pub replication: bool,
}

/// The Config page of a topic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPage {
    pub head: ConfigHead,
    pub rows: Vec<ConfigRow>,
    pub pace: Option<ConfigPace>,
    pub ask: ConfigAsk,
}

/// What a person typed in an edit dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigInput {
    pub key: String,
    pub op: ConfigOp,
    pub value: String,
    /// The maximum lag, in the dialog that edits both (the minimum is `value`).
    pub other: String,
}

/// The shapes of edit dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigForm {
    Single,
    List,
    Lags,
}

/// The five lines under a field, as the config boards word them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigTeach {
    pub what: Vec<Span>,
    pub now: Vec<Span>,
    pub allowed: Vec<Span>,
    pub takes_effect: Vec<Span>,
    pub here: Vec<Span>,
}

/// What a dry run (`validate_only`) said: Kafka accepted it, and what the config would be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigChecked {
    /// `(key, value now, value after)`.
    pub after: Vec<(String, String, String)>,
}

/// An edit dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigEditView {
    pub topic: String,
    pub form: ConfigForm,
    pub row: ConfigRow,
    /// The maximum lag's row, in the dialog that edits both.
    pub other: Option<ConfigRow>,
    pub input: ConfigInput,
    pub pace: Option<ConfigPace>,
    pub teach: ConfigTeach,
    pub call: Lines,
    pub error: Option<String>,
    pub checked: Option<ConfigChecked>,
}

/// A value and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigValue {
    pub value: String,
    pub source: ConfigSource,
}

/// One config a save changed: what it was and what it is now, read from the broker on each side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigChange {
    pub name: String,
    pub op: ConfigOp,
    pub before: Option<ConfigValue>,
    pub now: Option<ConfigValue>,
}

/// A save the broker took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigSaved {
    pub head: ConfigHead,
    pub changes: Vec<ConfigChange>,
    /// How many configs the topic has, to say how many are unchanged.
    pub total: usize,
    pub pace: Option<ConfigPace>,
}

/// What kind of refusal it was, for the layout that explains it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigRefusal {
    /// The two lags together.
    Lags,
    /// A value Kafka would not take, or anything else.
    Value,
}

/// A request that would pass, found by asking the broker (`validate_only`), with the words that show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigPass {
    /// The call's lines, as the page draws them (`key  OP  value`).
    pub lines: Vec<String>,
    /// What the config would be, for a list.
    pub result: Option<String>,
    /// The `(key, value)` pairs, to fill the dialog again.
    pub pairs: Vec<(String, String)>,
}

/// A refused save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigRefused {
    pub head: ConfigHead,
    pub kind: ConfigRefusal,
    pub input: ConfigInput,
    pub refusal: Refusal,
    pub request: Lines,
    /// Requests the broker accepted when asked (`validate_only`), if any were found.
    pub pass: Vec<ConfigPass>,
    /// What Kafka says to other values: `(what was sent, what Kafka said)`, each really asked.
    pub others: Vec<(String, String)>,
}

/// The reset dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigResetView {
    pub topic: String,
    pub row: ConfigRow,
    /// What the config would be after: the place that takes over.
    pub after: Option<ConfigValue>,
    pub pace: Option<ConfigPace>,
    pub call: Lines,
    pub refusal: Option<Refusal>,
}

/// What a save came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigResult {
    Saved(Box<ConfigSaved>),
    /// Kafka would accept it (`validate_only`); nothing was changed.
    Checked(Box<ConfigEditView>),
    Refused(Box<ConfigRefused>),
    /// The form is not complete: the dialog again, with the error.
    Invalid(Box<ConfigEditView>),
    NoTopic,
    NoKey,
}

/// What a reset came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigResetResult {
    Done,
    Refused(Box<ConfigResetView>),
    NoTopic,
    NoKey,
}
