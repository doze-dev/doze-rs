//! The view-models of the Lab's forced events (`api/lab_once.rs`): rehearsals that are not rules.

use serde::Serialize;

/// `GET /api/v1/lab/once`: the kinds of forced event this doze-aws can carry out, and what was forced lately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabOnceIndex {
    /// Every kind, in the order of the Lab's cards, with whether it can be carried out in this run.
    pub kinds: Vec<LabOnceKind>,
    /// The last forced events, newest first (at most 200 are kept).
    pub history: Vec<LabOnce>,
}

/// One kind of forced event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabOnceKind {
    /// `rest-service`, `fire-schedule`, `expire-visibility`, `sweep-ttl`, `cold-start`.
    pub id: String,
    pub title: String,
    /// One sentence: what it does.
    pub lede: String,
    /// Whether it can be carried out in this run.
    pub available: bool,
    /// When it cannot, why, in words (the service is not served, the build has no bus).
    pub why_not: Option<String>,
}

/// One forced event, as it was kept: what was asked, and in a sentence what came of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabOnce {
    /// Its number: increasing in this run.
    pub id: u64,
    /// When, in milliseconds since 1970.
    pub at_ms: u64,
    /// The kind's id.
    pub kind: String,
    /// What was done, or why nothing was ("Fired nightly-report now: it sent a message to email-outbox.").
    pub sentence: String,
    /// Whether it ran (a refusal in words is not ok).
    pub ok: bool,
    /// What it acted on (a queue, a schedule as `group/name`, a table, a function, a service).
    pub subject: String,
    /// The region it was for.
    pub region: String,
}

/// `GET /api/v1/lab/once/<kind>/form`: what there is to choose from in a region, read through the bus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabOnceForm {
    pub kind: String,
    pub region: String,
    /// What the choices are, as a person says it: `queue`, `schedule`, `table`, `function`, `service`.
    pub noun: String,
    pub choices: Vec<LabOnceChoice>,
    /// The service the choices are read from is resting, and was left so: `choices` is empty until the form is asked again with
    /// `look=yes` (which starts it, and it rests again when it has been idle).
    pub resting: bool,
    /// A sentence to show beside the choices (a resting service, a list that was cut short).
    pub note: Option<String>,
    /// Why there is nothing to choose ("There is no queue in eu-west-1."), when there is not.
    pub empty: Option<String>,
    /// What forcing the chosen one (else the first) would do, in a sentence.
    pub preview: Option<String>,
    /// An extra choice the kind has, if it has one.
    pub option: Option<LabOnceOption>,
}

/// One thing to choose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabOnceChoice {
    /// What the POST names: a queue's name, a schedule's `group/name`, a table, a function, a service's `--services` name.
    pub value: String,
    pub label: String,
    /// What to know about it before forcing (2 messages in flight; DISABLED; the attribute that holds the expiry).
    pub detail: Option<String>,
    /// For a service: `running` or `resting` in the region.
    pub state: Option<String>,
}

/// A yes-or-no that goes with the choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabOnceOption {
    /// The member of the POST's body (`time_wake`).
    pub id: String,
    pub label: String,
    pub default: bool,
}

/// `POST /api/v1/lab/once/<kind>`: what came of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabOnceDone {
    /// Whether it ran. A refusal in words (nothing to do, a right missing, a resource that is not there) is not ok.
    pub ok: bool,
    /// What was done, or why it was not.
    pub sentence: String,
    pub subject: String,
    /// Where to see the result: an address of the console.
    pub link: Option<String>,
}

/// `GET /api/v1/lab/once/guide`: the lesson of each kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabOnceGuide {
    /// How forced events differ from rules, in a sentence or two.
    pub intro: String,
    /// One lesson for each kind, in the order of the kinds.
    pub kinds: Vec<LabOnceTeach>,
}

/// What one kind teaches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabOnceTeach {
    /// The kind's id.
    pub kind: String,
    pub title: String,
    /// What it stands for in production.
    pub production: String,
    /// What to expect when it is forced here.
    pub expect: String,
    /// What a well-behaved client does about it.
    pub client: String,
    /// The settings that decide it, with the defaults AWS documents.
    pub settings: Vec<LabOnceSetting>,
    /// Where to see it in this console.
    pub see: String,
}

/// A setting that decides how a client meets the event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabOnceSetting {
    pub name: String,
    /// Whose it is: `queue`, `function`, `schedule`...
    pub side: String,
    pub default: String,
    pub why: String,
}
