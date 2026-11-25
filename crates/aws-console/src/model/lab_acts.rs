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
