//! The view-models of the Resources space (`api/resources.rs`).

use serde::Serialize;

use super::CallRecord;

/// `GET /api/v1/resources`: every service, by group, with its kinds and how many of each there are now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ResourcesIndex {
    pub services: Vec<ResourceService>,
}

/// One service in the index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ResourceService {
    /// Its name in `--services` and in addresses.
    pub name: String,
    pub title: String,
    /// `Messaging and events`.
    pub group: String,
    /// Whether this run serves it; the console does not wake one it does not.
    pub served: bool,
    /// Whether it has a descriptor yet.
    pub described: bool,
    /// Why it lists nothing, when it is not described.
    pub note: Option<String>,
    pub kinds: Vec<KindCount>,
}

/// One kind of resource and how many there are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct KindCount {
    /// `queues`.
    pub id: String,
    /// `Queues`.
    pub title: String,
    /// `queue`.
    pub one: String,
    /// The input members that name one of these in a call (`QueueUrl`).
    pub members: Vec<String>,
    /// How many there are; `null` when not counted (the service is not served, or the count failed: `error` says why).
    pub count: Option<u32>,
    /// The count stopped at the most it reads ([`crate::resources::run::MOST_COUNTED`]): there are at least `count`.
    pub more: bool,
    /// The names counted, for the command palette.
    pub names: Vec<String>,
    pub error: Option<String>,
    /// The ones not counted in `count` (AWS managed keys), when the kind sets some aside.
    pub others: Option<SetAside>,
}

/// Rows a list sets aside by default, and how many there are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SetAside {
    /// `AWS managed keys`.
    pub what: String,
    pub count: u32,
}

/// How a value is shown: the page formats `number`, `bytes`, `time` and `seconds`; the rest is shown as it comes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum ValueFormat {
    Text,
    Mono,
    Number,
    Bytes,
    Time,
    Seconds,
    Json,
}

/// A column of a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ColumnSpec {
    pub title: String,
    pub format: ValueFormat,
}

/// One resource in a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ResourceRow {
    /// Its name in the console's addresses.
    pub name: String,
    /// What it is shown as.
    pub label: String,
    /// What its calls take (a queue's URL, a topic's ARN).
    pub id: String,
    /// One cell per column; `null` where the answer had no value.
    pub cells: Vec<Option<String>>,
    /// The cell of the first column links here, when it does.
    pub link: Option<ResourceLink>,
}

/// A resource of the console, as an address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ResourceLink {
    pub service: String,
    pub kind: String,
    pub name: String,
}

/// A field of a create form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FormInput {
    pub key: String,
    pub label: String,
    pub help: String,
    /// `text`, `number`, `textarea`, `select` or `toggle`.
    pub input: String,
    pub required: bool,
    pub placeholder: String,
    /// What it holds at first.
    pub value: String,
    /// A select's choices.
    pub options: Vec<String>,
}

/// How one is created: the operations called and the form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CreateSpec {
    /// `CreateQueue`.
    pub operations: Vec<String>,
    pub inputs: Vec<FormInput>,
}

/// What deleting one does, said before it is done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DeleteSpec {
    /// `Delete`, `Schedule deletion`.
    pub verb: String,
    /// The operation it calls (`DeleteQueue`).
    pub operation: String,
    pub words: String,
}

/// `GET /api/v1/resources/<service>/<kind>`: one page of a kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ResourceList {
    pub service: String,
    pub service_title: String,
    pub group: String,
    pub kind: String,
    pub title: String,
    pub one: String,
    pub about: String,
    /// The operation that lists them (`ListQueues`).
    pub operation: String,
    pub columns: Vec<ColumnSpec>,
    pub rows: Vec<ResourceRow>,
    /// The token of the next page; `null` on the last.
    pub next: Option<String>,
    /// How one is created, when the console can create one.
    pub create: Option<CreateSpec>,
    pub delete: Option<DeleteSpec>,
    /// Whether a row opens a page of its own (a metric does not: there is nothing more to read of one).
    pub openable: bool,
    /// The rows of this page the list sets aside (`?all=yes` shows them), when the kind sets some aside.
    pub others: Option<SetAside>,
    /// Whether the rows set aside are shown.
    pub all: bool,
    /// Why the console does not create one (a function needs its code), when it does not.
    pub create_elsewhere: Option<String>,
}

/// A field of a detail page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FieldValue {
    pub label: String,
    /// `null` when the answer had none; also when hidden.
    pub value: Option<String>,
    pub format: ValueFormat,
    /// Hidden until the person reveals it.
    pub hidden: bool,
    pub link: Option<ResourceLink>,
}

/// A list that belongs to the resource (a bucket's objects).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SubList {
    pub title: String,
    /// The operation it was read with.
    pub operation: String,
    pub columns: Vec<ColumnSpec>,
    pub rows: Vec<ResourceRow>,
    /// The answer said it holds more than it gave (only its first page is shown).
    pub more: bool,
    /// Why it could not be read.
    pub error: Option<String>,
    /// Adds one from this page, when it can.
    pub add: Option<SubAdd>,
}

/// The create a related list offers (Add subscription on a topic): another kind's form, one input given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SubAdd {
    pub kind: String,
    /// `subscription`.
    pub one: String,
    pub create: CreateSpec,
    /// The input filled with this resource's id, and its value.
    pub key: String,
    pub value: String,
}

/// `GET /api/v1/resources/<service>/<kind>/<name>`: one resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ResourceDetail {
    pub service: String,
    pub service_title: String,
    pub group: String,
    pub kind: String,
    pub kind_title: String,
    pub one: String,
    pub name: String,
    pub id: String,
    /// The operations it was read with, in order.
    pub operations: Vec<String>,
    pub fields: Vec<FieldValue>,
    pub subs: Vec<SubList>,
    /// The last calls in the Live record that acted on it, newest first: the newest of others' and, apart, of the console's own
    /// (caller `console`, which a page hides unless asked).
    pub calls: Vec<CallRecord>,
    /// The button that reveals its hidden value, when it has one.
    pub reveal: Option<String>,
    pub delete: Option<DeleteSpec>,
    /// Its pages of its own, shown as tabs beside its details.
    pub deep: Vec<DeepTab>,
    /// The deep page that shows more of it (`lambda-function`), when it has one.
    pub page: Option<String>,
}

/// A page of one resource that the console's app draws (a queue's messages), and the operations it may make on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DeepTab {
    /// `messages`: the tab in the address and the page the app draws.
    pub id: String,
    pub title: String,
    pub operations: Vec<String>,
}

/// `POST /api/v1/resources/s3/buckets/<name>/upload?key=`: the object put.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Uploaded {
    pub key: String,
    pub size: u64,
    pub etag: Option<String>,
    pub version_id: Option<String>,
    pub operation: String,
}

/// `POST /api/v1/resources/<service>/<kind>`: what was made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Created {
    pub name: String,
    /// The operations called.
    pub operations: Vec<String>,
}

/// `POST /api/v1/resources/<service>/<kind>/<name>/reveal`: the hidden value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Revealed {
    pub value: String,
    pub operation: String,
}

/// `DELETE /api/v1/resources/<service>/<kind>/<name>`: what was done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Deleted {
    pub name: String,
    pub operation: String,
}
