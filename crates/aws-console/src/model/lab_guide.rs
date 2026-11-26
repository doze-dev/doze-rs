//! The Lab's words and the tag a rule leaves on a Live row: what a rule did to a call, the intro, and one lesson for each rehearsal.

use serde::Serialize;

/// What a rule did to a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum LabDid {
    /// The call waited this long before it was served (its time includes it).
    Delayed { ms: u32 },
    /// The call was answered with an error, and the service never saw it.
    Failed,
    /// The connection was closed with no answer (the row's status is 0).
    Dropped,
}

/// Which rule touched a Live row: its number (the link back to it), its name and what it did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabTag {
    pub id: u32,
    pub name: String,
    pub effect: LabDid,
}

/// One step of how the Lab is used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabStep {
    pub title: String,
    pub text: String,
}

/// How the Lab works, in the words a newcomer needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabIntro {
    /// What the Lab is for, in a sentence.
    pub what: String,
    pub steps: Vec<LabStep>,
    /// What it does not do, and how to stop everything.
    pub limits: Vec<String>,
}

/// An SDK setting that decides how a client takes the rehearsal, with the default it ships with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabSetting {
    /// `boto3 / botocore`, `AWS SDK for JavaScript v3`, `AWS SDK for Java 2.x`, `AWS SDK for Go v2`.
    pub sdk: String,
    pub name: String,
    pub default: String,
    pub why: String,
}

/// One rehearsal, taught.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabTeach {
    /// `throttle`, `unavailable`, `internal`, `access-denied`, `slow` or `drop`.
    pub tab: String,
    pub title: String,
    /// What it does here, in a sentence.
    pub lede: String,
    /// What it stands for in production.
    pub simulates: String,
    /// What happens once it starts.
    pub expect: String,
    /// What a well-behaved client does, one point each.
    pub client: Vec<String>,
    pub settings: Vec<LabSetting>,
    /// Where to see it in this console.
    pub watch: String,
}

/// `GET /api/v1/lab/guide`: the words. The recipes that name the person's own queues and tables are made by the page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabGuide {
    pub intro: LabIntro,
    /// One per rehearsal, in the order of the form's tabs.
    pub rehearsals: Vec<LabTeach>,
}
