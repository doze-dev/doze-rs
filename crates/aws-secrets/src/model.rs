//! What Secrets Manager holds: a secret, and each version of its value.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

pub(crate) const DAY_MS: i64 = 86_400_000;
pub(crate) const CURRENT: &str = "AWSCURRENT";
pub(crate) const PREVIOUS: &str = "AWSPREVIOUS";
/// A version with no staging label is kept this long, then dropped.
pub(crate) const DEPRECATED_KEPT_MS: i64 = DAY_MS;
/// The most versions a secret keeps (AWS: 100).
pub(crate) const MAX_VERSIONS: usize = 100;

/// One version of a secret's value.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Version {
    /// The value, sealed.
    pub(crate) sealed: Vec<u8>,
    /// Whether it was given as `SecretBinary`.
    pub(crate) binary: bool,
    pub(crate) stages: BTreeSet<String>,
    pub(crate) created_ms: i64,
    /// The start of the day (UTC) the version was last read: AWS keeps the date and not the time.
    pub(crate) accessed_day_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Secret {
    pub(crate) name: String,
    pub(crate) arn: String,
    pub(crate) description: Option<String>,
    pub(crate) kms_key_id: Option<String>,
    pub(crate) kind: Option<String>,
    pub(crate) tags: BTreeMap<String, String>,
    pub(crate) created_ms: i64,
    pub(crate) changed_ms: i64,
    pub(crate) accessed_day_ms: Option<i64>,
    /// When it was marked for deletion, and when it will be removed.
    pub(crate) deleted_ms: Option<i64>,
    pub(crate) purge_ms: Option<i64>,
    pub(crate) versions: BTreeMap<String, Version>,
    pub(crate) policy: Option<String>,
}

impl Secret {
    /// The version that carries `stage`.
    pub(crate) fn staged(&self, stage: &str) -> Option<(&str, &Version)> {
        self.versions
            .iter()
            .find(|(_, v)| v.stages.contains(stage))
            .map(|(id, v)| (id.as_str(), v))
    }

    /// The ids and stages of the versions that have a label.
    pub(crate) fn labelled(&self) -> impl Iterator<Item = (&str, &Version)> {
        self.versions
            .iter()
            .filter(|(_, v)| !v.stages.is_empty())
            .map(|(id, v)| (id.as_str(), v))
    }
}

/// A secret's rotation: kept in a table of its own, so the secret record and the rows already on disk keep their shape.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct Rotation {
    /// The rotation function's ARN.
    pub(crate) function: Option<String>,
    pub(crate) after_days: Option<i64>,
    pub(crate) duration: Option<String>,
    pub(crate) schedule: Option<String>,
    pub(crate) enabled: bool,
    /// When rotation was last configured, the base of `NextRotationDate` before a first rotation.
    pub(crate) configured_ms: Option<i64>,
    pub(crate) last_ms: Option<i64>,
    /// The token of the rotation in progress, whose version carries `AWSPENDING`.
    pub(crate) pending: Option<String>,
}

/// The start of the UTC day `ms` falls in.
pub(crate) const fn day_of(ms: i64) -> i64 {
    ms - ms.rem_euclid(DAY_MS)
}
