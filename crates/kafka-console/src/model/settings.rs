//! The view-models of the Settings page (slice 9h): the five dynamic broker configs and their dialogs, the version
//! profile, the intervals, the weight, the data directory and the flags. Every name starts with `Settings` so that no
//! other family of pages can meet it.

use super::{ConfigSource, Lines, Refusal, Span, StopView};

/// The page's title block: how many of the five are set and how long the broker has run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsHead {
    /// How many of the five dynamic configs are set at run time.
    pub set: usize,
    /// `3 d 4 h`, if the broker said when it started.
    pub up: Option<String>,
    /// `Nov 6 18:02`.
    pub since: Option<String>,
    /// `Kafka 4`.
    pub profile: String,
}

/// One of the five dynamic broker configs as DescribeConfigs says it for this broker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsConfigRow {
    pub name: String,
    /// What it is for, in a few words.
    pub what: String,
    /// The value in force, as the broker prints it.
    pub value: String,
    /// The value in words (`128 MiB`, `no limit`), where there is one.
    pub gloss: Option<String>,
    pub source: ConfigSource,
    /// Where the source comes from: the flag, Kafka's default, or the scope a dynamic value was set for.
    pub source_note: String,
    /// When a change counts, in words.
    pub takes_effect: String,
}

/// The dynamic configs, and the id of the broker they are read for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsConfigs {
    pub rows: Vec<SettingsConfigRow>,
    /// The only broker's id: a value set for "this broker" goes under it.
    pub node_id: i32,
}

/// An API the broker offers: its name, its versions and the family it is listed under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsApi {
    pub name: String,
    /// `v3 to v9`, `v0 only`.
    pub versions: String,
    pub family: &'static str,
}

/// What ApiVersions answers: the Kafka the broker stands for and the APIs a client is offered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsProfile {
    pub major: u8,
    /// Every API offered now, by family and key.
    pub apis: Vec<SettingsApi>,
    /// APIs that SASL being on adds (what a broker with SASL off does not offer): name and versions.
    pub with_sasl: Vec<SettingsApi>,
    /// Whether SASL is on here (so the APIs above are already in the list).
    pub sasl_on: bool,
    /// APIs this version of Kafka has that this broker does not offer now.
    pub not_offered: Vec<String>,
}

/// One interval: what it is, its value, its flag and default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsInterval {
    pub name: &'static str,
    pub what: &'static str,
    pub value: String,
    pub flag: &'static str,
    pub default: String,
    pub note: String,
}

/// How often the broker looks at things on its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsIntervals {
    pub rows: Vec<SettingsInterval>,
    /// How often open transactions are looked at (no flag sets it).
    pub transactions: String,
    pub retention_every: String,
    pub cleaner_every: String,
    pub cleaner_on: bool,
}

/// What this instance costs the machine, as the process can read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsWeight {
    pub open_partitions: usize,
    /// The most partitions kept open at once (the flag, lowered to what the file-descriptor limit leaves).
    pub max_open: usize,
    /// `--max-open-partitions` as given, where the flags are known.
    pub flag_max_open: Option<String>,
    /// Descriptors open in the process (partitions, segments, indexes and sockets).
    pub open_files: Option<usize>,
    /// Resident memory in bytes (Linux only).
    pub resident: Option<u64>,
    pub threads: Option<u32>,
    /// The size of the running program, in bytes.
    pub binary: Option<u64>,
    pub topics: usize,
    pub partitions: usize,
}

/// The data directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsData {
    pub dir: String,
    /// Bytes of every file in it; `cut` is true when the walk stopped early and the size is at least this.
    pub size: Option<(u64, bool)>,
    pub files: u64,
    pub topics: usize,
    pub partitions: usize,
    pub last_stop: StopView,
    /// `--data-dir`, where the flags are known: whether it was given.
    pub flag_set: bool,
}

/// One flag: its value, its default, whether the operator gave it, and what it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsFlag {
    /// `--segment-bytes`.
    pub name: String,
    /// The group the table lists it under.
    pub group: String,
    /// The value in force, as text. A flag that carries a secret says how many were given and never what.
    pub value: String,
    pub default: String,
    /// The operator gave the flag (on the command line or in the environment).
    pub set: bool,
    /// The first sentence of the flag's own help.
    pub what: String,
    /// The dynamic broker config the flag is the start value of, if it is one of the five.
    pub dynamic: Option<String>,
}

impl SettingsFlag {
    /// The operator gave the flag and its value is not the default: what the first table lists.
    #[must_use]
    pub fn differs(&self) -> bool {
        self.set && self.value != self.default
    }
}

/// The page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsView {
    pub head: SettingsHead,
    pub configs: SettingsConfigs,
    pub profile: SettingsProfile,
    pub intervals: SettingsIntervals,
    pub weight: SettingsWeight,
    pub data: SettingsData,
    pub flags: Vec<SettingsFlag>,
}

/// Which brokers a dynamic value is set for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsScope {
    #[default]
    All,
    This,
}

impl SettingsScope {
    /// The word a form carries.
    #[must_use]
    pub const fn form(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::This => "this",
        }
    }

    /// The scope a form's word names (anything else is all brokers).
    #[must_use]
    pub fn from_form(text: &str) -> Self {
        if text == "this" {
            Self::This
        } else {
            Self::All
        }
    }
}

/// One line of teaching: what it says, and the dim sentence after it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SettingsLine {
    pub text: Vec<Span>,
    pub dim: String,
}

/// The teaching under the new-value field, line by line.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SettingsTeach {
    pub now: SettingsLine,
    pub allowed: SettingsLine,
    pub takes_effect: SettingsLine,
    pub who_sees: SettingsLine,
}

/// What a dry run (`validate_only`) said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsChecked {
    pub before: String,
    pub after: String,
}

/// The edit dialog: one of the five, what was typed, and what is known about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsEditView {
    pub key: String,
    pub what: String,
    /// The five keys, for the select.
    pub keys: Vec<String>,
    pub scope: SettingsScope,
    pub value: String,
    /// The unit under the field (`ms = 3 d`).
    pub unit: Option<String>,
    pub node_id: i32,
    pub teach: SettingsTeach,
    pub call: Lines,
    pub error: Option<String>,
    pub checked: Option<SettingsChecked>,
}

/// A saved change, and the page around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsSaved {
    pub key: String,
    pub scope: SettingsScope,
    pub value: String,
    pub gloss: Option<String>,
    pub rows: Vec<SettingsConfigRow>,
    pub set: usize,
    pub who_sees: Vec<Span>,
    /// The call, and `-> 200, no error`.
    pub call: Lines,
    pub effect: String,
}

/// A refused change: Kafka's words, and what would pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsRefused {
    pub key: String,
    pub refusal: Refusal,
    /// The rule that was broken, in the console's words.
    pub rule: Vec<Span>,
    /// What would pass, in words.
    pub would_pass: Vec<Span>,
    /// The flag that sets it at start, where there is one.
    pub flag: Option<String>,
    pub call: Lines,
    /// The other refusals a change meets, with Kafka's text.
    pub others: Vec<(String, String, String)>,
    /// The configs this broker holds and does not change while it runs.
    pub fixed: Vec<String>,
    pub validate_only: bool,
}

/// What a save, or a check, came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsResult {
    Saved(Box<SettingsSaved>),
    Refused(Box<SettingsRefused>),
    Checked(Box<SettingsEditView>),
    Invalid(Box<SettingsEditView>),
    /// The select moved to another config: the dialog for it, nothing asked of the broker.
    Switched(Box<SettingsEditView>),
}
