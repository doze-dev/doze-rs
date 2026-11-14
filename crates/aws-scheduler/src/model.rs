//! What EventBridge Scheduler keeps. Every record is plain data written as postcard behind a version byte; the target is kept as the
//! JSON the caller sent, so nothing a client put in is lost on the way back.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The group every account has, which cannot be deleted.
pub(crate) const DEFAULT_GROUP: &str = "default";

/// A schedule group. Keyed `<account>/<name>`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Group {
    pub(crate) account: String,
    pub(crate) name: String,
    pub(crate) arn: String,
    pub(crate) tags: BTreeMap<String, String>,
    pub(crate) created_ms: i64,
    pub(crate) modified_ms: i64,
}

/// A schedule. Keyed `<account>/<group>/<name>`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Schedule {
    pub(crate) account: String,
    pub(crate) group: String,
    pub(crate) name: String,
    pub(crate) arn: String,
    pub(crate) expression: String,
    /// `ScheduleExpressionTimezone` as given; `None` is UTC.
    pub(crate) timezone: Option<String>,
    pub(crate) start_ms: Option<i64>,
    pub(crate) end_ms: Option<i64>,
    pub(crate) description: Option<String>,
    pub(crate) enabled: bool,
    pub(crate) kms_key: Option<String>,
    /// The whole `Target` as JSON.
    pub(crate) target: String,
    /// `FlexibleTimeWindow` as JSON.
    pub(crate) window: String,
    /// `ActionAfterCompletion` is `DELETE`.
    pub(crate) delete_after: bool,
    pub(crate) created_ms: i64,
    pub(crate) modified_ms: i64,
    /// A rate counts from here when there is no `StartDate`: when the schedule was made or last updated.
    pub(crate) anchor_ms: i64,
    /// When it next fires; `None` while disabled or when it never will again.
    pub(crate) next_ms: Option<i64>,
}

/// A key made of account and names; none of them contains a slash.
pub(crate) fn key(account: &str, parts: &[&str]) -> String {
    let mut k = account.to_owned();
    for p in parts {
        k.push('/');
        k.push_str(p);
    }
    k
}

pub(crate) fn group_arn(region: &str, account: &str, name: &str) -> String {
    format!("arn:aws:scheduler:{region}:{account}:schedule-group/{name}")
}

pub(crate) fn schedule_arn(region: &str, account: &str, group: &str, name: &str) -> String {
    format!("arn:aws:scheduler:{region}:{account}:schedule/{group}/{name}")
}
