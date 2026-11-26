//! The view-models of Home, Access, Lab and Settings.

use serde::Serialize;

use super::{CallRecord, RingFill};

/// Who an access key is, as IAM knows it, resolved when asked (nothing is kept on the request path).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct KeyPrincipal {
    /// The access key id, as the Live record has it.
    pub key: String,
    pub kind: PrincipalKind,
    /// The user's name (`kind` `user`); `null` otherwise.
    pub name: Option<String>,
    /// The user's or the account root's ARN; `null` for a session and for the console.
    pub arn: Option<String>,
}

/// What kind of caller a key is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum PrincipalKind {
    /// An access key IAM issued to a user.
    User,
    /// A key IAM did not issue (`test`, a key from the command line): IAM takes it for the account root, allowed everything.
    Root,
    /// A temporary key (`ASIA...`): a role session or a user's session token. IAM can say whose only with the session token,
    /// which the Live record does not keep.
    Session,
    /// This console's own calls.
    Console,
}

/// `GET /api/v1/principals`: every caller in the Live record, named.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PrincipalsResponse {
    pub principals: Vec<KeyPrincipal>,
}

/// A row of IAM's access log that was not allowed: who, what, on what, and IAM's own reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Refusal {
    /// The principal's ARN (a user, an assumed-role session).
    pub principal: String,
    /// The IAM action asked about (`sqs:SendMessage`; S3's `ListObjectsV2` asks for `s3:ListBucket`).
    pub action: String,
    /// The ARN acted on; `*` when the service named none.
    pub resource: String,
    /// `implicitDeny` (no statement allows it) or `explicitDeny` (a statement denies it).
    pub decision: String,
    /// IAM's words for why (`because no identity-based policy allows the sqs:SendMessage action`).
    pub reason: String,
    pub count: u64,
    /// When it was last asked, ISO 8601.
    pub last_seen: String,
}

/// The calls of one caller to one operation that IAM refused or would have refused, from the Live record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DenialGroup {
    /// The access key.
    pub caller: String,
    pub principal: KeyPrincipal,
    /// By its `--services` name.
    pub service: Option<String>,
    pub api: Option<String>,
    pub action: String,
    /// Refused (`--iam enforce`).
    pub denied: u64,
    /// Let through by `--iam soft`.
    pub would_be_denied: u64,
    pub first_ms: u64,
    pub last_ms: u64,
    /// The newest such call: Live opens its drawer.
    pub last_seq: u64,
    /// What the calls acted on, as they named it, at most five.
    pub resources: Vec<String>,
    /// IAM's access log rows for this principal and action (matched by the action's name): why, in IAM's words.
    pub explained: Vec<Refusal>,
    /// No row was named as the operation is, so `explained` holds the principal's refusals of the service's actions (S3's
    /// `ListObjectsV2` is checked as `s3:ListBucket`).
    pub by_service: bool,
}

/// `GET /api/v1/access`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AccessView {
    /// `off`, `soft` or `enforce`.
    pub iam: String,
    /// Newest first.
    pub groups: Vec<DenialGroup>,
    /// Every refusal in IAM's access log, newest first: the log outlives the Live record's 1,024 calls.
    pub refusals: Vec<Refusal>,
    /// Rows IAM's log let go to stay at its size.
    pub evicted: u64,
    /// Whether IAM's access log could be read (IAM is in the build and answered).
    pub log_read: bool,
    pub ring: RingFill,
}

/// `POST /api/v1/access/attach`'s answer: the inline policy was put.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Attached {
    /// `user` or `role`.
    pub kind: String,
    pub name: String,
    pub policy_name: String,
}

/// One client that has called, by its access key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CallerSeen {
    pub principal: KeyPrincipal,
    /// The clients its calls named in `User-Agent` (`boto3 1.34.69`), most recent first.
    pub sdks: Vec<String>,
    pub calls: u64,
    pub last_ms: u64,
}

/// One service apps called, from the Live record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ServiceSeen {
    pub service: String,
    pub title: String,
    pub calls: u64,
    pub last_ms: u64,
}

/// What a signal is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum SignalKind {
    WouldBeDenied,
    Denied,
    Errors,
    NotBuilt,
    DeadLetters,
    Alarm,
    FailedExecutions,
    FailedStack,
    /// Resources a script created that doze-aws keeps and does nothing with (an email subscription).
    NotActive,
}

/// Something worth a look, and where it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Signal {
    pub kind: SignalKind,
    pub count: u64,
    /// One line (`3 calls would be denied`).
    pub title: String,
    /// What it is about (`soft IAM`, a queue's name).
    pub detail: String,
    /// The console address it opens when it is not a resource (`/live?outcome=denied`, `/access`).
    pub href: Option<String>,
    /// The resource it is about, whose page it opens.
    pub resource: Option<super::ResourceLink>,
}

/// `GET /api/v1/home`: what the Live record says about the apps that call. No call is made to answer it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HomeView {
    /// A call from an app (not this console, not a service on the bus) is in the Live record.
    pub connected: bool,
    /// Every caller, most recent first.
    pub callers: Vec<CallerSeen>,
    /// The services apps called, most recent first.
    pub services: Vec<ServiceSeen>,
    /// Every service any call reached (an app, this console, another service on the bus), most recent first: what keeps a
    /// service awake.
    pub activity: Vec<ServiceSeen>,
    /// Seconds without a request before a service lets go of its memory and files (`--idle-secs`): a service whose newest call
    /// is older than this is asleep.
    pub idle_secs: u64,
    /// The newest calls from apps, newest first, at most five.
    pub recent: Vec<CallRecord>,
    /// From the Live record: refusals, error bursts, operations not built.
    pub signals: Vec<Signal>,
    pub ring: RingFill,
}

/// `GET /api/v1/home/checks`: signals read from the services themselves, through the bus as the console.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HomeChecks {
    /// The services looked at: those apps called in this run, so a check never wakes a service that sleeps.
    pub checked: Vec<String>,
    pub signals: Vec<Signal>,
    /// A check that could not be made, in a sentence each.
    pub failed: Vec<String>,
}

/// One setting of the run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct FlagRow {
    pub name: String,
    /// `null` for a secret.
    pub value: Option<String>,
    pub source: String,
    pub default: bool,
}

/// What one service keeps on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DataUse {
    pub name: String,
    pub title: String,
    pub bytes: u64,
}

/// `GET /api/v1/settings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SettingsView {
    pub version: String,
    pub flags: Vec<FlagRow>,
    pub data_dir: String,
    /// The data directory's size; `null` when it could not be read.
    pub data_bytes: Option<u64>,
    /// By service, largest first.
    pub data: Vec<DataUse>,
    /// Why the size could not be read.
    pub data_error: Option<String>,
    pub ring: RingFill,
    /// The most the Live record can hold, in bytes, when every text of every call is as long as it may be.
    pub ring_most_bytes: u64,
    /// Whether this console can reset the data (the binary handed it the data engine).
    pub reset: bool,
}

/// One service's part of a reset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ResetRow {
    pub service: String,
    pub items: u64,
    pub bytes: u64,
    pub freed: u64,
}

/// `POST /api/v1/settings/reset`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ResetDone {
    /// Nothing was removed: this is what would be.
    pub dry_run: bool,
    pub services: Vec<ResetRow>,
}

/// One API on the bus, with the operations it answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabTarget {
    /// Its name on the bus (its signing name: `sqs`, `states`).
    pub target: String,
    pub sdk_id: String,
    /// The service by its `--services` name, and its title.
    pub service: String,
    pub title: String,
    pub served: bool,
    /// The operations built, by name.
    pub operations: Vec<String>,
}

/// `GET /api/v1/lab`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabIndex {
    pub targets: Vec<LabTarget>,
}

/// One input member of an operation, from the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabMember {
    /// The JSON key the bus reads (the member's name).
    pub name: String,
    /// `string`, `integer`, `float`, `boolean`, `timestamp`, `blob`, `list`, `map`, `structure`, `union`.
    pub kind: String,
    pub required: bool,
    /// The values the model allows, when it lists them.
    pub values: Vec<String>,
}

/// `GET /api/v1/lab/<target>/<operation>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabOperation {
    pub target: String,
    pub operation: String,
    pub members: Vec<LabMember>,
    /// A starting input: every required member, with an empty value of its kind (pretty JSON).
    pub example: String,
    /// The most bytes of text the operation's `Body` may be when it carries an object's body (S3 `PutObject`); `null` when it carries none.
    pub body_max: Option<u32>,
    /// Why the Lab cannot send the operation, when it cannot (S3 `UploadPart`); `null` when it can.
    pub cannot: Option<String>,
}

/// An error answer, as the service gave it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabError {
    pub status: u16,
    pub code: String,
    pub message: String,
}

/// `POST /api/v1/lab/<target>/<operation>`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabAnswer {
    /// The output, pretty JSON; `null` for an error.
    pub output: Option<String>,
    pub error: Option<LabError>,
    /// The call's number in the Live record, when it was recorded.
    pub seq: Option<u64>,
    pub took_us: u64,
}

/// One policy a user or a role holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HeldPolicy {
    pub name: String,
    /// Put on the user or role itself (it can be edited here); else a managed policy attached to it.
    pub inline: bool,
    /// A managed policy's ARN.
    pub arn: Option<String>,
    /// The policy document as JSON text; `null` when it could not be read.
    pub document: Option<String>,
}

/// `GET /api/v1/access/policies`: what a user or a role holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct HeldPolicies {
    pub policies: Vec<HeldPolicy>,
    /// The groups a user is in, whose policies are not read.
    pub groups: Vec<String>,
}
