//! The view-models of the Lab's fault rules: what the API sends and takes under `/api/v1/lab/rules`, `/check`, `/stop-all`,
//! `/history/clear` and `/guide`, and the tag a rule leaves on a Live row. Design: `docs/port/aws/lab.md`.
//!
//! Every sentence here is made once from the rule (never a live number in a sentence): a count that changes is its own field.

use serde::{Deserialize, Serialize};

use super::CallRecord;

/// The kind of a rule: what it does to the calls it fits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum LabKind {
    /// The call is served, after a wait.
    Slow,
    /// The call is answered with an error and the service is never asked.
    Fail,
    /// The connection is closed with no answer.
    Drop,
}

/// The error a Fail rule answers with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum LabFailKind {
    /// The service's own throttling error (`ThrottlingException`, `ProvisionedThroughputExceededException`, `SlowDown`...).
    Throttle,
    /// The service is unavailable (`ServiceUnavailable`, 503).
    Unavailable,
    /// The service failed inside (`InternalFailure`, `InternalServerError`, `InternalError`...: a 500).
    Internal,
    /// The caller may not do this (`AccessDeniedException`, `AccessDenied`).
    AccessDenied,
    /// A status and a code the person gives.
    Custom,
}

/// What a rule does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum LabEffect {
    /// Wait `delay_ms` and a random 0 to `jitter_ms`, then serve the call.
    Slow { delay_ms: u32, jitter_ms: u32 },
    /// Answer with an error. `status` and `code` are given for `custom` and ignored for the other kinds.
    Fail {
        error: LabFailKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        status: Option<u16>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        code: Option<String>,
    },
    /// Close the connection with no answer.
    Drop,
}

/// How many of the calls a rule fits it fires on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum LabShare {
    /// Every one.
    Every,
    /// The Nth, the 2Nth... (`n` from 2 to 1,000).
    Nth { n: u32 },
    /// A chance of `percent` in a hundred (1 to 99), drawn from a seeded generator.
    Percent { percent: u8 },
}

/// Which calls a rule fits. Every list that is not empty must fit; an empty one fits anything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabMatcher {
    /// By the name `--services` takes (`sqs`, `stepfunctions`), the name in a credential scope (`states`) or the SDK id (`SFN`,
    /// `DynamoDB Streams`). At most 16. `dynamodb` fits DynamoDB and its Streams, which sign alike.
    #[serde(default)]
    pub services: Vec<String>,
    /// Operations by exact name (`SendMessage`). At most 16.
    #[serde(default)]
    pub operations: Vec<String>,
    /// Regions the call was signed for (`eu-west-1`). At most 8.
    #[serde(default)]
    pub regions: Vec<String>,
    /// Access key ids the call was signed with. At most 8.
    #[serde(default)]
    pub callers: Vec<String>,
    /// Patterns (`*` any run, `?` any one character) for what the call names: a queue URL, a table, an ARN, `bucket/key`; a
    /// pattern fits the whole text or its last name (`orders` fits `http://localhost:4566/000000000000/orders`). A call that
    /// names nothing does not fit a rule that has patterns. At most 16.
    #[serde(default)]
    pub resources: Vec<String>,
}

/// `POST /api/v1/lab/rules/check` and `POST /api/v1/lab/rules`: a rule as it is asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabRuleBody {
    /// At most 80 characters, no control character; empty is named after what the rule does.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub matcher: LabMatcher,
    pub effect: LabEffect,
    /// Every call is `{ kind: "every" }` (which the server also takes when the member is left out).
    #[serde(default = "every")]
    pub share: LabShare,
    /// How long the rule lasts, in seconds: 1 to 86,400.
    pub ends_in_secs: u64,
    /// The rule ends after firing this many times, if that comes first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub max_hits: Option<u32>,
}

const fn every() -> LabShare {
    LabShare::Every
}

/// A thing wrong with a rule that was asked for, in words, and the field it is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabProblem {
    /// `name`, `services`, `operations`, `regions`, `callers`, `resources`, `effect`, `share`, `ends_in_secs`, `max_hits`, or
    /// `rules` (the Lab as a whole: it is full, or off).
    pub field: String,
    pub text: String,
}

/// What a Fail rule answers one service with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabCode {
    /// The service by its title (`SQS`, `DynamoDB`).
    pub service: String,
    pub status: u16,
    /// The code the client reads (`RequestThrottled`).
    pub code: String,
}

/// `POST /api/v1/lab/rules/check`: what the rule would be, without making it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabCheck {
    /// The rule can be made as it is.
    pub ok: bool,
    /// `Throttle 1 in 4 SQS SendMessage calls to orders for 10 minutes`; empty when the rule is too wrong to say.
    pub sentence: String,
    pub problems: Vec<LabProblem>,
    /// For a Fail rule: the code each service the rule names answers with (every service, when it names none).
    pub answers: Vec<LabCode>,
}

/// `POST /api/v1/lab/rules`: the rule that was made, or why not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum LabMade {
    Made { rule: Box<LabRule> },
    Refused { check: LabCheck },
}

/// Why a rule is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum LabEnd {
    /// Its time ran out.
    Expired,
    /// It had fired as often as it was allowed.
    Spent,
    /// It was removed, or Stop all ended it.
    Cleared,
    /// A rule with the same matcher and the same kind of effect took its place.
    Replaced,
}

/// The first call a rule fired on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabFirst {
    pub at_ms: i64,
    /// The access key id, clipped.
    pub caller: String,
    /// The service by its title.
    pub service: String,
    pub operation: String,
    /// What the call named, clipped; `null` when it named nothing.
    pub resource: Option<String>,
    /// The client that sent it, from its `User-Agent` (`boto3 1.34.69`); `null` when it named none.
    pub sdk: Option<String>,
}

/// A rule, in force or over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabRule {
    /// Its number: unique for the life of the process, never reused. The `lab` of a Live row names it.
    pub id: u32,
    pub name: String,
    pub kind: LabKind,
    /// What it does, in a sentence made once from the rule.
    pub sentence: String,
    /// A rule switched off does nothing and still runs out.
    pub enabled: bool,
    /// Calls that fitted it.
    pub matched: u64,
    /// Calls it fired on (a call another rule won is not counted).
    pub hits: u64,
    pub started_ms: i64,
    /// When it ends (in force) or ended (over).
    pub ends_ms: i64,
    pub last_hit_ms: Option<i64>,
    pub matcher: LabMatcher,
    pub effect: LabEffect,
    pub share: LabShare,
    pub max_hits: Option<u32>,
    /// What the rule touches, as words for a table: `SQS SendMessage`.
    pub what: String,
    /// What it touches the call on, as words: `orders`, `every resource`.
    pub target: String,
    /// How often, as words: `1 in 4`, `every call`, `30 %`.
    pub share_words: String,
    /// What a Fail rule answers with, per service; empty for the other kinds.
    pub answers: Vec<LabCode>,
    pub first: Option<LabFirst>,
    /// The access key ids it fired on, at most eight, in the order they came.
    pub callers: Vec<String>,
    /// Hits by callers past the eight that are kept.
    pub more_callers: u32,
    /// Set once it is over.
    pub ended: Option<LabEnd>,
}

/// What bounds the Lab, for the form to say and the person to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabLimits {
    pub most_rules: u32,
    pub most_ended: u32,
    pub shortest_secs: u32,
    pub longest_secs: u32,
    pub longest_delay_ms: u32,
    pub least_nth: u32,
    pub most_nth: u32,
    pub least_percent: u32,
    pub most_percent: u32,
    pub most_services: u32,
    pub most_operations: u32,
    pub most_regions: u32,
    pub most_callers: u32,
    pub most_resources: u32,
    pub longest_name: u32,
    pub longest_pattern: u32,
    pub longest_code: u32,
    /// The access key ids a rule remembers having fired on.
    pub callers_kept: u32,
}

/// `GET /api/v1/lab/rules`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabRules {
    /// Rules can be made: `--lab` is on. Off, the lists are empty and nothing is judged.
    pub enabled: bool,
    /// Why not, in a sentence, when it is off.
    pub off_why: Option<String>,
    pub now_ms: i64,
    /// Oldest first.
    pub in_force: Vec<LabRule>,
    /// Over, newest first, at most `limits.most_ended`.
    pub ended: Vec<LabRule>,
    pub limits: LabLimits,
}

/// `GET /api/v1/lab/rules/{id}`: the rule and the calls it touched that Live still holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabRuleView {
    pub rule: LabRule,
    pub now_ms: i64,
    /// Newest first, the same rows as Live.
    pub calls: Vec<CallRecord>,
    /// More calls than these were touched and are still held.
    pub more: bool,
}
