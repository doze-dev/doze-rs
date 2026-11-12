//! What Parameter Store holds: a parameter, and each version it has had.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// The most versions a parameter keeps (AWS: 100; the oldest unlabelled one goes when a 101st is written).
pub(crate) const MAX_VERSIONS: usize = 100;

/// What a value is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Kind {
    String,
    StringList,
    SecureString,
}

impl Kind {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        match text {
            "String" => Some(Self::String),
            "StringList" => Some(Self::StringList),
            "SecureString" => Some(Self::SecureString),
            _ => None,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::String => "String",
            Self::StringList => "StringList",
            Self::SecureString => "SecureString",
        }
    }
}

/// A policy attached to an advanced parameter. Only `Expiration` does anything; the others are kept and returned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Policy {
    pub(crate) text: String,
    pub(crate) kind: String,
}

/// One version of a parameter, with the settings it was written with (AWS reports them per version).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Version {
    pub(crate) number: i64,
    /// The value; for a `SecureString`, sealed.
    pub(crate) value: Vec<u8>,
    pub(crate) kind: Kind,
    pub(crate) key_id: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) allowed_pattern: Option<String>,
    pub(crate) data_type: String,
    pub(crate) advanced: bool,
    pub(crate) policies: Vec<Policy>,
    pub(crate) labels: BTreeSet<String>,
    pub(crate) modified_ms: i64,
    pub(crate) modified_by: String,
}

/// A parameter: the newest version, the older ones, and what belongs to the name and not to a version.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Parameter {
    pub(crate) latest: Version,
    pub(crate) older: Vec<Version>,
    pub(crate) tags: BTreeMap<String, String>,
    /// When the `Expiration` policy of the newest version removes it.
    pub(crate) expires_ms: Option<i64>,
}

impl Parameter {
    /// Every version, oldest first.
    pub(crate) fn versions(&self) -> impl Iterator<Item = &Version> {
        self.older.iter().chain(std::iter::once(&self.latest))
    }

    pub(crate) fn version(&self, number: i64) -> Option<&Version> {
        self.versions().find(|v| v.number == number)
    }

    pub(crate) fn version_mut(&mut self, number: i64) -> Option<&mut Version> {
        if self.latest.number == number {
            return Some(&mut self.latest);
        }
        self.older.iter_mut().find(|v| v.number == number)
    }

    pub(crate) fn labelled(&self, label: &str) -> Option<&Version> {
        self.versions().find(|v| v.labels.contains(label))
    }

    /// Makes `next` the newest version. Past [`MAX_VERSIONS`] the oldest version without a label is dropped; when every one
    /// has a label there is nothing to drop and the caller is told.
    pub(crate) fn push(&mut self, next: Version) -> Result<(), ()> {
        if self.older.len() + 1 >= MAX_VERSIONS {
            let Some(ix) = self.older.iter().position(|v| v.labels.is_empty()) else {
                return Err(());
            };
            self.older.remove(ix);
        }
        let previous = std::mem::replace(&mut self.latest, next);
        self.older.push(previous);
        Ok(())
    }

    /// Whether the `Expiration` policy has run out.
    pub(crate) fn expired(&self, now_ms: i64) -> bool {
        self.expires_ms.is_some_and(|at| at <= now_ms)
    }
}
