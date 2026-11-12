//! The facts about a request that conditions and policy variables can ask: key by key, none or several values each.

use std::collections::BTreeMap;

/// Context keys, compared without regard to case (IAM keys are case-insensitive).
#[derive(Clone, Debug, Default)]
pub struct Context {
    map: BTreeMap<String, Vec<String>>,
    /// Keys AWS has and this emulator does not supply (a VPC endpoint, an organization id): a condition on one can only be guessed.
    unsupplied: Vec<String>,
}

impl Context {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a single-valued key.
    pub fn set(&mut self, key: &str, value: impl Into<String>) -> &mut Self {
        self.map
            .insert(key.to_ascii_lowercase(), vec![value.into()]);
        self
    }

    /// Sets a multi-valued key.
    pub fn set_many(&mut self, key: &str, values: Vec<String>) -> &mut Self {
        self.map.insert(key.to_ascii_lowercase(), values);
        self
    }

    /// Says a key exists in AWS and is not supplied here.
    pub fn leave_unsupplied(&mut self, key: &str) -> &mut Self {
        self.unsupplied.push(key.to_ascii_lowercase());
        self
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&[String]> {
        self.map.get(&key.to_ascii_lowercase()).map(Vec::as_slice)
    }

    /// Whether `key` is one AWS would have supplied and this emulator did not.
    #[must_use]
    pub fn is_unsupplied(&self, key: &str) -> bool {
        let key = key.to_ascii_lowercase();
        !self.map.contains_key(&key) && self.unsupplied.contains(&key)
    }

    /// Every key and its values, in key order (for the simulator's list of what a policy asked for).
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.map.iter().map(|(k, v)| (k.as_str(), v.as_slice()))
    }
}
