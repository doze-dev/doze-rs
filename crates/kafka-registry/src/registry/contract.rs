//! Data contracts: the metadata (tags, properties, sensitive names) and the rule set (migration, domain and encoding
//! rules) a schema is registered with, as Confluent keeps them (`Metadata`, `RuleSet`, `Rule` of its client): sorted as
//! its Java collections sort them, merged as its registry merges a configuration's defaults and overrides with what a
//! registration gives or inherits, and checked as `RuleSet.validate` checks them. They are part of the schema: the same
//! text with other metadata is another schema, with its own id.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// The property Confluent sets to a version's number when a registration asks for a version.
pub const CONFLUENT_VERSION: &str = "confluent:version";

/// A schema's metadata. Confluent's are sorted maps and sets, so two orderings of one are one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metadata {
    pub tags: BTreeMap<String, BTreeSet<String>>,
    pub properties: BTreeMap<String, String>,
    pub sensitive: BTreeSet<String>,
}

/// What a rule does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleKind {
    Transform,
    Condition,
}

impl RuleKind {
    /// The kind Confluent names, upper case only.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "TRANSFORM" => Some(Self::Transform),
            "CONDITION" => Some(Self::Condition),
            _ => None,
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Transform => "TRANSFORM",
            Self::Condition => "CONDITION",
        }
    }
}

/// When a rule applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuleMode {
    Upgrade,
    Downgrade,
    UpDown,
    Write,
    Read,
    WriteRead,
}

impl RuleMode {
    /// The mode Confluent names, upper case only.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "UPGRADE" => Self::Upgrade,
            "DOWNGRADE" => Self::Downgrade,
            "UPDOWN" => Self::UpDown,
            "WRITE" => Self::Write,
            "READ" => Self::Read,
            "WRITEREAD" => Self::WriteRead,
            _ => return None,
        })
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Upgrade => "UPGRADE",
            Self::Downgrade => "DOWNGRADE",
            Self::UpDown => "UPDOWN",
            Self::Write => "WRITE",
            Self::Read => "READ",
            Self::WriteRead => "WRITEREAD",
        }
    }

    fn migration(self) -> bool {
        matches!(self, Self::Upgrade | Self::Downgrade | Self::UpDown)
    }
}

/// A rule, with Confluent's defaults (`TRANSFORM`, `WRITEREAD`) for a kind or mode not given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub name: Option<String>,
    pub doc: Option<String>,
    pub kind: RuleKind,
    pub mode: RuleMode,
    pub rule_type: Option<String>,
    pub tags: BTreeSet<String>,
    pub params: BTreeMap<String, String>,
    pub expr: Option<String>,
    pub on_success: Option<String>,
    pub on_failure: Option<String>,
    pub disabled: bool,
}

/// A schema's rules.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleSet {
    pub migration: Vec<Rule>,
    pub domain: Vec<Rule>,
    pub encoding: Vec<Rule>,
}

/// A schema's contract: its metadata and its rule set, each given or not.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contract {
    pub metadata: Option<Metadata>,
    pub rule_set: Option<RuleSet>,
}

impl Contract {
    /// Whether neither is given.
    #[must_use]
    pub fn is_none(&self) -> bool {
        self.metadata.is_none() && self.rule_set.is_none()
    }

    /// The value of a property of the metadata.
    #[must_use]
    pub fn property(&self, key: &str) -> Option<&str> {
        self.metadata
            .as_ref()?
            .properties
            .get(key)
            .map(String::as_str)
    }
}

fn merge_map<V: Clone>(
    old: &BTreeMap<String, V>,
    new: &BTreeMap<String, V>,
) -> BTreeMap<String, V> {
    let mut m = old.clone();
    m.extend(new.iter().map(|(k, v)| (k.clone(), v.clone())));
    m
}

/// Confluent's `Metadata.mergeMetadata`: each part of `new` laid over `old`'s (a key of `new` replaces `old`'s).
#[must_use]
pub(super) fn merge_metadata(old: Option<&Metadata>, new: Option<&Metadata>) -> Option<Metadata> {
    match (old, new) {
        (None, n) => n.cloned(),
        (o, None) => o.cloned(),
        (Some(o), Some(n)) => Some(Metadata {
            tags: merge_map(&o.tags, &n.tags),
            properties: merge_map(&o.properties, &n.properties),
            sensitive: o.sensitive.union(&n.sensitive).cloned().collect(),
        }),
    }
}

/// Confluent's `RuleSet.mergeRuleSets`: of each list, `old`'s rules not named in `new`'s, then `new`'s.
#[must_use]
pub(super) fn merge_rule_sets(old: Option<&RuleSet>, new: Option<&RuleSet>) -> Option<RuleSet> {
    fn rules(old: &[Rule], new: &[Rule]) -> Vec<Rule> {
        if old.is_empty() {
            return new.to_vec();
        }
        if new.is_empty() {
            return old.to_vec();
        }
        let mut out: Vec<Rule> = old
            .iter()
            .filter(|r| !new.iter().any(|n| n.name == r.name))
            .cloned()
            .collect();
        out.extend(new.iter().cloned());
        out
    }
    match (old, new) {
        (None, n) => n.cloned(),
        (o, None) => o.cloned(),
        (Some(o), Some(n)) => Some(RuleSet {
            migration: rules(&o.migration, &n.migration),
            domain: rules(&o.domain, &n.domain),
            encoding: rules(&o.encoding, &n.encoding),
        }),
    }
}

/// A configuration's defaults and overrides of a contract.
#[derive(Debug, Clone, Default)]
pub struct ContractConfig {
    pub default_metadata: Option<Metadata>,
    pub override_metadata: Option<Metadata>,
    pub default_rule_set: Option<RuleSet>,
    pub override_rule_set: Option<RuleSet>,
}

impl ContractConfig {
    /// Whether any default or override is set (Confluent's `hasDefaultsOrOverrides`).
    #[must_use]
    pub fn any(&self) -> bool {
        self.default_metadata.is_some()
            || self.override_metadata.is_some()
            || self.default_rule_set.is_some()
            || self.override_rule_set.is_some()
    }

    /// Confluent's `maybeSetMetadataRuleSet`: the registration's own metadata and rule set (else the previous version's),
    /// over the defaults, under the overrides. With `version` (the version a registration will take), `confluent:version`
    /// is set to it when the registration `asked` for a version or the merged metadata already names one.
    #[must_use]
    pub fn apply(
        &self,
        own: &Contract,
        previous: Option<&Contract>,
        version: Option<u32>,
        asked: bool,
    ) -> Contract {
        let specific = own
            .metadata
            .as_ref()
            .or_else(|| previous.and_then(|p| p.metadata.as_ref()));
        let mut metadata = merge_metadata(
            merge_metadata(self.default_metadata.as_ref(), specific).as_ref(),
            self.override_metadata.as_ref(),
        );
        let specific = own
            .rule_set
            .as_ref()
            .or_else(|| previous.and_then(|p| p.rule_set.as_ref()));
        let rule_set = merge_rule_sets(
            merge_rule_sets(self.default_rule_set.as_ref(), specific).as_ref(),
            self.override_rule_set.as_ref(),
        );
        let named = metadata
            .as_ref()
            .is_some_and(|m| m.properties.contains_key(CONFLUENT_VERSION));
        if let Some(v) = version.filter(|_| asked || named) {
            let m = metadata.get_or_insert_with(Metadata::default);
            m.properties
                .insert(CONFLUENT_VERSION.to_owned(), v.to_string());
        }
        Contract { metadata, rule_set }
    }
}

impl Rule {
    /// Confluent's `Rule.validate`.
    fn validate(&self) -> Result<(), String> {
        let Some(name) = &self.name else {
            return Err("Missing rule name".into());
        };
        let chars: Vec<char> = name.chars().collect();
        // Java counts UTF-16 units.
        let len = name.encode_utf16().count();
        if len == 0 {
            return Err("Empty rule name".into());
        }
        if len > 64 {
            return Err("Rule name too long".into());
        }
        let (&first, rest) = chars.split_first().ok_or("Empty rule name")?;
        if !(first.is_alphabetic() || first == '_') {
            return Err(format!("Illegal initial character in rule name: {name}"));
        }
        if rest
            .iter()
            .any(|&c| !(c.is_alphanumeric() || c == '_' || c == '-'))
        {
            return Err(format!("Illegal character in rule name: {name}"));
        }
        if self.rule_type.is_none() {
            return Err("Missing rule type".into());
        }
        for action in [&self.on_success, &self.on_failure].into_iter().flatten() {
            if !matches!(self.mode, RuleMode::WriteRead | RuleMode::UpDown) && action.contains(',')
            {
                return Err("Multiple actions only valid with WRITEREAD and UPDOWN".into());
            }
        }
        Ok(())
    }
}

impl RuleSet {
    /// Confluent's `RuleSet.validate` (8.x, with encoding rules): names unique across the lists, each rule valid, each
    /// list's modes its own.
    ///
    /// # Errors
    /// Confluent's sentence for the first rule that is not.
    pub fn validate(&self) -> Result<(), String> {
        let mut names: Vec<&Option<String>> = Vec::new();
        let lists: [(&[Rule], bool, &str); 3] = [
            (
                &self.migration,
                true,
                "Migration rules can only be UPGRADE, DOWNGRADE, UPDOWN",
            ),
            (
                &self.domain,
                false,
                "Domain rules can only be WRITE, READ, WRITEREAD",
            ),
            (
                &self.encoding,
                false,
                "Encoding rules can only be WRITE, READ, WRITEREAD",
            ),
        ];
        for (rules, migration, wrong) in lists {
            for r in rules {
                if names.contains(&&r.name) {
                    let n = r.name.as_deref().unwrap_or("null");
                    return Err(format!("Found rule with duplicate name '{n}'"));
                }
                names.push(&r.name);
                r.validate()?;
                if r.mode.migration() != migration {
                    return Err((*wrong).to_owned());
                }
            }
        }
        Ok(())
    }
}

pub mod json;
pub mod read;

#[cfg(test)]
mod tests;
