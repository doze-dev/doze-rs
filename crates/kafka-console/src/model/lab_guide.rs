//! What the Lab teaches: how it works (for a first visit), what each rehearsal stands for in production, what a well-behaved
//! client does about it and the settings that decide it, and ready-made recipes that fill the New rehearsal dialog with the
//! broker's own topics, apps and groups. The words are `crate::lab::teach`; the recipes `crate::lab::recipes`.

use super::lab::LabFields;

/// `GET /api/v1/lab/guide`.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabGuide {
    pub intro: LabIntro,
    /// One per rehearsal, in the order of the dialog's tabs.
    pub rehearsals: Vec<LabTeach>,
    pub recipes: Vec<LabRecipe>,
}

/// How the Lab works, in the words a newcomer needs.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabIntro {
    /// What the Lab is for, in a sentence.
    pub what: String,
    /// How to use it, in order.
    pub steps: Vec<LabStep>,
    /// What it does not do, and how to stop everything.
    pub limits: Vec<String>,
}

/// One step of the 1-2-3.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabStep {
    pub title: String,
    pub text: String,
}

/// One rehearsal, taught.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabTeach {
    /// The dialog's tab: `slow`, `fail`, `drop`, `expire`, `rebalance`, `compact`.
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
    /// The client (or topic) settings that decide how it goes.
    pub settings: Vec<LabSetting>,
    /// Where to see it in this console.
    pub watch: String,
}

/// A setting that matters, with Kafka's default.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabSetting {
    pub name: String,
    /// `producer`, `consumer`, `client` (both) or `topic`.
    pub side: String,
    pub default: String,
    pub why: String,
}

/// A ready-made rehearsal: it opens the dialog filled in; the person reviews it and starts it.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabRecipe {
    /// Its address (`?recipe=`).
    pub id: String,
    pub tab: String,
    /// What it does, with the broker's names in it.
    pub title: String,
    /// The question it answers about your app.
    pub question: String,
    /// The form, for Slow and Fail.
    pub fields: Option<LabFields>,
    /// The app, member (`group/member`) or group, for Drop, Expire and Rebalance; the topic for Compact.
    pub target: Option<String>,
    /// For a drop: `once`, `1` or `5`.
    pub how: Option<String>,
    /// What the broker lacks for it now (no app connected, no group with members): it cannot be used until it has it.
    pub needs: Option<String>,
}
