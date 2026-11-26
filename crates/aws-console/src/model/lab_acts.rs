//! What the Lab's buttons take and answer: Stop all, forgetting the history, and switching a rule.

use serde::{Deserialize, Serialize};

/// `POST /api/v1/lab/stop-all`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabStopped {
    /// How many rules were in force and are ended.
    pub stopped: u32,
}

/// `POST /api/v1/lab/history/clear`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabCleared {
    /// How many ended rules were forgotten.
    pub cleared: u32,
}

/// `POST /api/v1/lab/rules/{id}/switch`: switch a rule on or off. With no body, or no `enabled`, the rule is turned the other way.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabSwitchBody {
    #[serde(default)]
    #[cfg_attr(test, ts(optional))]
    pub enabled: Option<bool>,
}

/// One error AWS's model lists for the operations a rule fits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabListedError {
    /// The code a client reads (`QueueDoesNotExist`).
    pub code: String,
    /// The HTTP status the model gives it.
    pub status: u16,
    /// The service failed (a server fault, the 5xx class) rather than the caller.
    pub server: bool,
}

/// `GET /api/v1/lab/errors?services=sqs&operations=SendMessage,ReceiveMessage`: the errors a Fail rule may answer with, for the calls
/// those services and operations fit (the errors every one of the operations lists).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LabErrors {
    /// In the model's order; empty when there is nothing to choose from (then `why` says why).
    pub errors: Vec<LabListedError>,
    /// Why the list is empty, in words.
    pub why: Option<String>,
}
