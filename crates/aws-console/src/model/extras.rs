//! The view-models of what a resource page adds to the generic list and detail: a call's preview, tags, what stands in the way
//! of a delete, a list's filters, and how a detail speaks of its resource (badges, verbs, what it is wired to).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ResourceLink;

/// `POST /api/v1/preview`: the call a form would make, asked for before it is made.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PreviewRequest {
    /// The service by its name on the peer bus (`sqs`, `s3`, `stepfunctions`).
    pub service: String,
    /// `CreateQueue`.
    pub operation: String,
    /// The members so far; a JSON object (anything else is read as none).
    #[cfg_attr(test, ts(type = "unknown"))]
    #[serde(default)]
    pub input: Value,
}

/// What a call would be, written as the service's wire shows it and as the AWS CLI and boto3 write it. Nothing is called.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Preview {
    /// The request body of a JSON protocol; the form members of a query protocol, as an object; for a REST protocol, the
    /// members that are not in the address, as an object.
    #[cfg_attr(test, ts(type = "unknown"))]
    pub json: Value,
    pub cli: String,
    pub boto3: String,
    /// Members the operation requires that `input` does not have.
    pub missing: Vec<String>,
}

/// One tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Tag {
    pub key: String,
    pub value: String,
}

/// `GET` and `PUT /api/v1/resources/<service>/<kind>/<name>/tags`: the tags, as the service holds them now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TagsView {
    pub tags: Vec<Tag>,
    /// Whether the console can change them (a kind whose tags it only reads is not).
    pub editable: bool,
}

/// The body of `PUT …/tags`: tags to set (added, or given a new value) and keys to remove.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TagsChange {
    #[serde(default)]
    pub set: Vec<Tag>,
    #[serde(default)]
    pub remove: Vec<String>,
}

/// One thing that depends on the resource, in a list of what a delete meets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BlockerItem {
    pub name: String,
    pub link: Option<ResourceLink>,
}

/// A kind of dependent (attached policies, access keys, targets) and what it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BlockerGroup {
    pub title: String,
    pub items: Vec<BlockerItem>,
    /// The console can clear them (`DELETE …?clear=yes` does) before it deletes.
    pub clears: bool,
    /// The service refuses the delete while they are there. `false` for what is only left behind (a function's event source
    /// mappings): they are listed so the person knows.
    pub blocks: bool,
    /// There are more than `items` shows.
    pub more: bool,
}

/// `GET /api/v1/resources/<service>/<kind>/<name>/blockers`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Blockers {
    pub blockers: Vec<BlockerGroup>,
    /// What a delete leaves behind.
    pub kept: Vec<String>,
    /// What the person types to confirm: the name, or a word for a kind with no name worth typing.
    pub words: String,
}

/// What a filter or selector of a list takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum ParamKind {
    Select,
    Text,
    Chips,
}

/// One choice of a select or a chip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ParamOption {
    pub value: String,
    pub label: String,
    /// How many rows it holds, when that is cheap to say.
    pub count: Option<u32>,
}

/// A filter or selector of a list, `?p.<key>=<value>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ParamSpec {
    pub key: String,
    pub label: String,
    pub input: ParamKind,
    pub options: Vec<ParamOption>,
    /// What it holds now: what the address gave, else its default; `null` for none.
    pub value: Option<String>,
}

/// How a state is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    Good,
    Warn,
    Bad,
    Info,
    Neutral,
}

/// A badge in a detail's header (`Standard`, `Dead-letter queue`, `ACTIVE`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Badge {
    pub text: String,
    pub tone: Tone,
}

impl Badge {
    #[must_use]
    pub fn new(text: impl Into<String>, tone: Tone) -> Self {
        Self {
            text: text.into(),
            tone,
        }
    }
}

/// What a verb asks of the person before it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum Needs {
    Confirm,
    Input,
}

/// A verb in a detail's header (`Purge`, `Send and receive messages`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct VerbSpec {
    pub id: String,
    pub label: String,
    pub danger: bool,
    /// The operation of the resource's pages it makes (`PurgeQueue`); `null` for a verb the page handles itself.
    pub op: Option<String>,
    pub needs: Option<Needs>,
}

/// A piece the resource is wired to, for the strip under the sentence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Wired {
    /// How: the field's or list's own name (`Dead-letter queue`, `Lambda triggers`).
    pub label: String,
    pub to: ResourceLink,
    pub note: Option<String>,
}
