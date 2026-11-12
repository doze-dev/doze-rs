//! What a request's expressions share: the names and values they may refer to, and a record of which they did.

use std::collections::BTreeSet;

use crate::Invalid;
use crate::ast::{NameMap, ValueMap};

/// Which expression is being read; it words the errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `ConditionExpression`
    Condition,
    /// `FilterExpression`
    Filter,
    /// `KeyConditionExpression`
    KeyCondition,
    /// `ProjectionExpression`
    Projection,
    /// `UpdateExpression`
    Update,
}

impl Kind {
    /// The request member's name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Condition => "ConditionExpression",
            Self::Filter => "FilterExpression",
            Self::KeyCondition => "KeyConditionExpression",
            Self::Projection => "ProjectionExpression",
            Self::Update => "UpdateExpression",
        }
    }

    /// `Invalid UpdateExpression: <what>`. AWS puts a count in front of the update expression's refusals (recorded) and not in
    /// front of the key condition's (recorded); the other three are `(memory)` and follow the key condition.
    #[must_use]
    pub fn invalid(self, what: &str) -> Invalid {
        let lead = if self == Self::Update {
            "1 validation error detected: "
        } else {
            ""
        };
        Invalid(format!("{lead}Invalid {}: {what}", self.label()))
    }
}

/// The names and values of one request, and which of them the expressions used.
#[derive(Debug)]
pub struct Scope<'a> {
    names: &'a NameMap,
    values: &'a ValueMap,
    used_names: BTreeSet<String>,
    used_values: BTreeSet<String>,
}

impl<'a> Scope<'a> {
    /// A scope over the request's `ExpressionAttributeNames` and `ExpressionAttributeValues`.
    #[must_use]
    pub fn new(names: &'a NameMap, values: &'a ValueMap) -> Self {
        Self {
            names,
            values,
            used_names: BTreeSet::new(),
            used_values: BTreeSet::new(),
        }
    }

    pub(crate) fn name(&mut self, reference: &str) -> Option<String> {
        let found = self.names.get(reference)?;
        self.used_names.insert(reference.to_owned());
        Some(found.clone())
    }

    pub(crate) fn value(&mut self, reference: &str) -> Option<crate::Value> {
        let found = self.values.get(reference)?;
        self.used_values.insert(reference.to_owned());
        Some(found.clone())
    }

    /// Whether the request gave any names.
    #[must_use]
    pub fn has_names(&self) -> bool {
        !self.names.is_empty()
    }

    /// Whether the request gave any values.
    #[must_use]
    pub fn has_values(&self) -> bool {
        !self.values.is_empty()
    }

    /// Checks that every name and value the request supplied was used by some expression.
    ///
    /// # Errors
    /// AWS's refusal naming the unused keys (the values one is recorded; the names one follows it).
    pub fn finish(&self) -> Result<(), Invalid> {
        let unused = |all: Vec<&String>, used: &BTreeSet<String>| -> Vec<String> {
            all.into_iter()
                .filter(|k| !used.contains(*k))
                .cloned()
                .collect()
        };
        let names = unused(self.names.keys().collect(), &self.used_names);
        if !names.is_empty() {
            return Err(Invalid(format!(
                "1 validation error detected: Value provided in ExpressionAttributeNames unused in expressions: keys: {{{}}}",
                names.join(", ")
            )));
        }
        let values = unused(self.values.keys().collect(), &self.used_values);
        if !values.is_empty() {
            return Err(Invalid(format!(
                "1 validation error detected: Value provided in ExpressionAttributeValues unused in expressions: keys: {{{}}}",
                values.join(", ")
            )));
        }
        Ok(())
    }
}
