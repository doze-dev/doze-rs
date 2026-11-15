//! What Secrets Manager holds: a secret, and each version of its value.

use std::collections::BTreeMap;

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
    pub(crate) stages: Stages,
    pub(crate) created_ms: i64,
    /// The start of the day (UTC) the version was last read: AWS keeps the date and not the time.
    pub(crate) accessed_day_ms: Option<i64>,
}

/// A version's staging labels, in the order they were attached: AWS lists them so, a label moved onto a version after the ones
/// it had (`["PENDING", "AWSCURRENT"]`, recorded 2026-11-15). Stored as a sequence, as the sorted set before it was, so what is
/// on disk reads back unchanged.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct Stages(Vec<String>);

impl Stages {
    pub(crate) fn of(stage: &str) -> Self {
        Self(vec![stage.to_owned()])
    }
    pub(crate) fn contains(&self, stage: &str) -> bool {
        self.0.iter().any(|s| s == stage)
    }
    /// Attaches `stage` last, unless it is already there.
    pub(crate) fn insert(&mut self, stage: String) {
        if !self.contains(&stage) {
            self.0.push(stage);
        }
    }
    pub(crate) fn remove(&mut self, stage: &str) {
        self.0.retain(|s| s != stage);
    }
    pub(crate) fn retain(&mut self, keep: impl FnMut(&String) -> bool) {
        self.0.retain(keep);
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub(crate) fn iter(&self) -> std::slice::Iter<'_, String> {
        self.0.iter()
    }
}

impl FromIterator<String> for Stages {
    fn from_iter<I: IntoIterator<Item = String>>(items: I) -> Self {
        let mut out = Self::default();
        for s in items {
            out.insert(s);
        }
        out
    }
}

impl<'a> IntoIterator for &'a Stages {
    type Item = &'a String;
    type IntoIter = std::slice::Iter<'a, String>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
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
