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

/// Java's `toString` of a collection: `[a, b]`, and of a map `{k=v, l=w}`.
fn java_list(items: impl Iterator<Item = String>) -> String {
    let all: Vec<String> = items.collect();
    format!("[{}]", all.join(", "))
}

fn java_map<V: std::fmt::Display>(m: &BTreeMap<String, V>) -> String {
    let all: Vec<String> = m.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{{{}}}", all.join(", "))
}

fn java_set(s: &BTreeSet<String>) -> String {
    java_list(s.iter().cloned())
}

fn or_null(v: Option<&String>) -> &str {
    v.map_or("null", String::as_str)
}

impl Metadata {
    /// Confluent's `Metadata.toString`, as a refusal quotes a schema.
    #[must_use]
    pub fn java(&self) -> String {
        let tags: BTreeMap<String, String> = self
            .tags
            .iter()
            .map(|(k, v)| (k.clone(), java_set(v)))
            .collect();
        format!(
            "Metadata{{tags={}, properties={}, sensitive={}}}",
            java_map(&tags),
            java_map(&self.properties),
            java_set(&self.sensitive)
        )
    }
}

impl Rule {
    fn java(&self) -> String {
        format!(
            "Rule{{name={}, doc={}, kind={}, mode={}, type='{}', tags='{}', params='{}', expr='{}', onSuccess='{}', onFailure='{}', disabled='{}'}}",
            or_null(self.name.as_ref()),
            or_null(self.doc.as_ref()),
            self.kind.name(),
            self.mode.name(),
            or_null(self.rule_type.as_ref()),
            java_set(&self.tags),
            java_map(&self.params),
            or_null(self.expr.as_ref()),
            or_null(self.on_success.as_ref()),
            or_null(self.on_failure.as_ref()),
            self.disabled
        )
    }
}

impl RuleSet {
    /// Confluent's `RuleSet.toString` (8.x's, with encoding rules and no `enableAt`), as a refusal quotes a schema.
    #[must_use]
    pub fn java(&self) -> String {
        let l = |rs: &[Rule]| java_list(rs.iter().map(Rule::java));
        format!(
            "Rules{{migrationRules={}, domainRules={}, encodingRules={}, enableAt=null}}",
            l(&self.migration),
            l(&self.domain),
            l(&self.encoding)
        )
    }
}

/// How the API writes a contract: Confluent's JSON (`@JsonInclude(NON_EMPTY)`, in its classes' field order).
pub mod json {
    use std::collections::{BTreeMap, BTreeSet};

    use serde::Serialize;

    use super::{Metadata, Rule, RuleSet};

    #[derive(Serialize)]
    struct MetadataOut<'a> {
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        tags: &'a BTreeMap<String, BTreeSet<String>>,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        properties: &'a BTreeMap<String, String>,
        #[serde(skip_serializing_if = "BTreeSet::is_empty")]
        sensitive: &'a BTreeSet<String>,
    }

    #[derive(Serialize)]
    struct RuleOut<'a> {
        #[serde(skip_serializing_if = "Option::is_none")]
        name: &'a Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        doc: &'a Option<String>,
        kind: &'static str,
        mode: &'static str,
        #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
        rule_type: &'a Option<String>,
        #[serde(skip_serializing_if = "BTreeSet::is_empty")]
        tags: &'a BTreeSet<String>,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        params: &'a BTreeMap<String, String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        expr: &'a Option<String>,
        #[serde(rename = "onSuccess", skip_serializing_if = "Option::is_none")]
        on_success: &'a Option<String>,
        #[serde(rename = "onFailure", skip_serializing_if = "Option::is_none")]
        on_failure: &'a Option<String>,
        disabled: bool,
    }

    #[derive(Serialize)]
    struct RuleSetOut<'a> {
        #[serde(rename = "migrationRules", skip_serializing_if = "Vec::is_empty")]
        migration: Vec<RuleOut<'a>>,
        #[serde(rename = "domainRules", skip_serializing_if = "Vec::is_empty")]
        domain: Vec<RuleOut<'a>>,
        #[serde(rename = "encodingRules", skip_serializing_if = "Vec::is_empty")]
        encoding: Vec<RuleOut<'a>>,
    }

    fn rule(r: &Rule) -> RuleOut<'_> {
        RuleOut {
            name: &r.name,
            doc: &r.doc,
            kind: r.kind.name(),
            mode: r.mode.name(),
            rule_type: &r.rule_type,
            tags: &r.tags,
            params: &r.params,
            expr: &r.expr,
            on_success: &r.on_success,
            on_failure: &r.on_failure,
            disabled: r.disabled,
        }
    }

    /// For `#[serde(serialize_with)]` on an `Option<Metadata>` (skipped when `None`).
    ///
    /// # Errors
    /// The serializer's.
    pub fn metadata<S: serde::Serializer>(m: &Option<Metadata>, s: S) -> Result<S::Ok, S::Error> {
        m.as_ref()
            .map(|m| MetadataOut {
                tags: &m.tags,
                properties: &m.properties,
                sensitive: &m.sensitive,
            })
            .serialize(s)
    }

    /// For `#[serde(serialize_with)]` on an `Option<RuleSet>` (skipped when `None`).
    ///
    /// # Errors
    /// The serializer's.
    pub fn rule_set<S: serde::Serializer>(r: &Option<RuleSet>, s: S) -> Result<S::Ok, S::Error> {
        r.as_ref()
            .map(|r| RuleSetOut {
                migration: r.migration.iter().map(rule).collect(),
                domain: r.domain.iter().map(rule).collect(),
                encoding: r.encoding.iter().map(rule).collect(),
            })
            .serialize(s)
    }

    /// A metadata's JSON text.
    #[must_use]
    pub fn metadata_text(m: &Metadata) -> String {
        let mut out = Vec::new();
        let _ = metadata(&Some(m.clone()), &mut serde_json::Serializer::new(&mut out));
        String::from_utf8(out).unwrap_or_default()
    }

    /// A rule set's JSON text.
    #[must_use]
    pub fn rule_set_text(r: &RuleSet) -> String {
        let mut out = Vec::new();
        let _ = rule_set(&Some(r.clone()), &mut serde_json::Serializer::new(&mut out));
        String::from_utf8(out).unwrap_or_default()
    }
}

/// A contract read from a request's JSON as Jackson binds it to Confluent's classes: a scalar where a string is wanted is
/// its text, a key it does not know is read past, and each refusal in Jackson's words (400; recorded by
/// tools/registry-diff, `data-contracts/errors`).
pub mod read {
    use std::collections::{BTreeMap, BTreeSet};

    use serde_json::Value;

    use super::{Metadata, Rule, RuleKind, RuleMode, RuleSet};
    use crate::error::RegistryError;

    fn bad(message: String) -> RegistryError {
        RegistryError::http(400, message)
    }

    /// Jackson's refusal of a value that is not the object `class` is read from.
    fn not_object(v: &Value, class: &str) -> RegistryError {
        bad(match v {
            Value::Array(_) => format!(
                "Cannot deserialize value of type `{class}` from Array value (token `JsonToken.START_ARRAY`)"
            ),
            Value::String(s) => format!(
                "Cannot construct instance of `{class}` (although at least one Creator exists): no String-argument constructor/factory method to deserialize from String value ('{s}')"
            ),
            Value::Number(n) => format!(
                "Cannot construct instance of `{class}` (although at least one Creator exists): no int/Int-argument constructor/factory method to deserialize from Number value ({n})"
            ),
            Value::Bool(b) => format!(
                "Cannot construct instance of `{class}` (although at least one Creator exists): no boolean/Boolean-argument constructor/factory method to deserialize from boolean value ({b})"
            ),
            Value::Object(_) | Value::Null => format!(
                "Cannot deserialize value of type `{class}` from Object value (token `JsonToken.START_OBJECT`)"
            ),
        })
    }

    /// A string as Jackson coerces one: a number or boolean is its text, `null` is none.
    fn text(v: &Value) -> Result<Option<String>, RegistryError> {
        match v {
            Value::Null => Ok(None),
            Value::String(s) => Ok(Some(s.clone())),
            Value::Number(n) => Ok(Some(n.to_string())),
            Value::Bool(b) => Ok(Some(b.to_string())),
            Value::Object(_) => Err(bad(
                "Cannot deserialize value of type `java.lang.String` from Object value (token `JsonToken.START_OBJECT`)"
                    .into(),
            )),
            Value::Array(_) => Err(bad(
                "Cannot deserialize value of type `java.lang.String` from Array value (token `JsonToken.START_ARRAY`)"
                    .into(),
            )),
        }
    }

    fn strings(v: &Value, class: &str) -> Result<BTreeSet<String>, RegistryError> {
        match v {
            Value::Null => Ok(BTreeSet::new()),
            Value::Array(items) => Ok(items
                .iter()
                .map(text)
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .flatten()
                .collect()),
            other => Err(not_object(other, class)),
        }
    }

    fn string_map(v: &Value) -> Result<BTreeMap<String, String>, RegistryError> {
        match v {
            Value::Null => Ok(BTreeMap::new()),
            Value::Object(m) => {
                let mut out = BTreeMap::new();
                for (k, v) in m {
                    if let Some(t) = text(v)? {
                        out.insert(k.clone(), t);
                    }
                }
                Ok(out)
            }
            other => Err(not_object(other, "LinkedHashMap")),
        }
    }

    /// `metadata`: `None` for `null`.
    ///
    /// # Errors
    /// Jackson's 400 for what is not one.
    // fuzz:parser registry_api
    pub fn metadata(v: &Value) -> Result<Option<Metadata>, RegistryError> {
        let m = match v {
            Value::Null => return Ok(None),
            Value::Object(m) => m,
            other => return Err(not_object(other, "Metadata")),
        };
        let mut out = Metadata::default();
        if let Some(t) = m.get("tags") {
            match t {
                Value::Null => {}
                Value::Object(tags) => {
                    for (k, v) in tags {
                        out.tags.insert(k.clone(), strings(v, "HashSet")?);
                    }
                }
                other => return Err(not_object(other, "LinkedHashMap")),
            }
        }
        if let Some(p) = m.get("properties") {
            out.properties = string_map(p)?;
        }
        if let Some(s) = m.get("sensitive") {
            out.sensitive = strings(s, "HashSet")?;
        }
        Ok(Some(out))
    }

    fn rule(v: &Value) -> Result<Rule, RegistryError> {
        let m = match v {
            Value::Object(m) => m,
            other => return Err(not_object(other, "Rule")),
        };
        let field = |k: &str| m.get(k).map_or(Ok(None), text);
        let kind = match field("kind")? {
            None => RuleKind::Transform,
            Some(k) => RuleKind::parse(&k).ok_or_else(|| {
                bad(format!(
                    "Cannot deserialize value of type `RuleKind` from String \"{k}\": not one of the values accepted for Enum class: [CONDITION, TRANSFORM]"
                ))
            })?,
        };
        let mode = match field("mode")? {
            None => RuleMode::WriteRead,
            Some(k) => RuleMode::parse(&k).ok_or_else(|| {
                bad(format!(
                    "Cannot deserialize value of type `RuleMode` from String \"{k}\": not one of the values accepted for Enum class: [DOWNGRADE, READ, UPGRADE, UPDOWN, WRITE, WRITEREAD]"
                ))
            })?,
        };
        let disabled = match m.get("disabled") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(b)) => *b,
            Some(Value::String(s)) if s == "true" || s == "false" => s == "true",
            Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
            Some(_) => {
                return Err(bad(
                    "Cannot deserialize value of type `boolean` from Object value (token `JsonToken.START_OBJECT`)"
                        .into(),
                ));
            }
        };
        Ok(Rule {
            name: field("name")?,
            doc: field("doc")?,
            kind,
            mode,
            rule_type: field("type")?,
            tags: m
                .get("tags")
                .map_or(Ok(BTreeSet::new()), |t| strings(t, "HashSet"))?,
            params: m.get("params").map_or(Ok(BTreeMap::new()), string_map)?,
            expr: field("expr")?,
            on_success: field("onSuccess")?,
            on_failure: field("onFailure")?,
            disabled,
        })
    }

    fn rules(v: Option<&Value>) -> Result<Vec<Rule>, RegistryError> {
        match v {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(Value::Array(items)) => items.iter().map(rule).collect(),
            // Jackson's own name for a List<Rule> (sic).
            Some(other) => Err(not_object(other, "Rule>")),
        }
    }

    /// `ruleSet`: `None` for `null`.
    ///
    /// # Errors
    /// Jackson's 400 for what is not one.
    // fuzz:parser registry_api
    pub fn rule_set(v: &Value) -> Result<Option<RuleSet>, RegistryError> {
        let m = match v {
            Value::Null => return Ok(None),
            Value::Object(m) => m,
            other => return Err(not_object(other, "RuleSet")),
        };
        Ok(Some(RuleSet {
            migration: rules(m.get("migrationRules"))?,
            domain: rules(m.get("domainRules"))?,
            encoding: rules(m.get("encodingRules"))?,
        }))
    }

    /// A stored setting's metadata, read back (it was checked when it was set).
    #[must_use]
    pub fn stored_metadata(text: Option<&str>) -> Option<Metadata> {
        let v: Value = serde_json::from_str(text?).ok()?;
        metadata(&v).ok().flatten()
    }

    /// A stored setting's rule set, read back.
    #[must_use]
    pub fn stored_rule_set(text: Option<&str>) -> Option<RuleSet> {
        let v: Value = serde_json::from_str(text?).ok()?;
        rule_set(&v).ok().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(name: &str, mode: RuleMode) -> Rule {
        Rule {
            name: Some(name.into()),
            doc: None,
            kind: RuleKind::Condition,
            mode,
            rule_type: Some("CEL".into()),
            tags: BTreeSet::new(),
            params: BTreeMap::new(),
            expr: Some("true".into()),
            on_success: None,
            on_failure: None,
            disabled: false,
        }
    }

    fn props(kv: &[(&str, &str)]) -> Metadata {
        Metadata {
            properties: kv.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect(),
            ..Metadata::default()
        }
    }

    // Recorded from cp-schema-registry 7.7.1 and 8.3.2 (tools/registry-diff, data-contracts/config-defaults-and-overrides).
    #[test]
    fn defaults_lie_under_and_overrides_over_what_a_registration_gives() {
        let cfg = ContractConfig {
            default_metadata: Some(props(&[("team", "x"), ("owner", "dflt")])),
            override_metadata: Some(Metadata {
                sensitive: ["k".to_owned()].into(),
                ..props(&[("owner", "forced")])
            }),
            ..ContractConfig::default()
        };
        let own = Contract {
            metadata: Some(props(&[("owner", "mine"), ("other", "y")])),
            rule_set: None,
        };
        let got = cfg.apply(&own, None, None, false);
        assert_eq!(
            json::metadata_text(got.metadata.as_ref().unwrap()),
            r#"{"properties":{"other":"y","owner":"forced","team":"x"},"sensitive":["k"]}"#
        );
        // Nothing of its own: the previous version's.
        let prev = Contract {
            metadata: Some(props(&[("p", "1")])),
            rule_set: None,
        };
        let got = ContractConfig::default().apply(&Contract::default(), Some(&prev), Some(4), true);
        assert_eq!(
            got.metadata.unwrap().properties,
            BTreeMap::from([
                ("confluent:version".into(), "4".into()),
                ("p".into(), "1".into())
            ])
        );
    }

    #[test]
    fn a_rule_of_the_same_name_replaces_the_older_one() {
        let old = RuleSet {
            domain: vec![rule("d1", RuleMode::Write), rule("shared", RuleMode::Write)],
            ..RuleSet::default()
        };
        let new = RuleSet {
            domain: vec![rule("shared", RuleMode::Read)],
            ..RuleSet::default()
        };
        let m = merge_rule_sets(Some(&old), Some(&new)).unwrap();
        let names: Vec<_> = m
            .domain
            .iter()
            .map(|r| (r.name.clone().unwrap(), r.mode))
            .collect();
        assert_eq!(
            names,
            [
                ("d1".into(), RuleMode::Write),
                ("shared".into(), RuleMode::Read)
            ]
        );
    }

    #[test]
    fn rule_sets_are_checked_as_confluent_checks_them() {
        let bad = |r: RuleSet| r.validate().unwrap_err();
        let one = |r: Rule| RuleSet {
            domain: vec![r],
            ..RuleSet::default()
        };
        assert_eq!(
            bad(RuleSet {
                domain: vec![rule("a", RuleMode::Write), rule("a", RuleMode::Write)],
                ..RuleSet::default()
            }),
            "Found rule with duplicate name 'a'"
        );
        assert_eq!(bad(one(rule("", RuleMode::Write))), "Empty rule name");
        assert_eq!(
            bad(one(rule("1a", RuleMode::Write))),
            "Illegal initial character in rule name: 1a"
        );
        assert_eq!(
            bad(one(rule("a.b", RuleMode::Write))),
            "Illegal character in rule name: a.b"
        );
        assert_eq!(
            bad(one(rule(&"x".repeat(65), RuleMode::Write))),
            "Rule name too long"
        );
        assert!(
            one(rule(&"x".repeat(64), RuleMode::Write))
                .validate()
                .is_ok()
        );
        assert_eq!(
            bad(one(Rule {
                rule_type: None,
                ..rule("t", RuleMode::Write)
            })),
            "Missing rule type"
        );
        assert_eq!(
            bad(one(Rule {
                on_failure: Some("ERROR,NONE".into()),
                ..rule("m", RuleMode::Write)
            })),
            "Multiple actions only valid with WRITEREAD and UPDOWN"
        );
        assert_eq!(
            bad(one(rule("u", RuleMode::Upgrade))),
            "Domain rules can only be WRITE, READ, WRITEREAD"
        );
        assert_eq!(
            bad(RuleSet {
                migration: vec![rule("w", RuleMode::Write)],
                ..RuleSet::default()
            }),
            "Migration rules can only be UPGRADE, DOWNGRADE, UPDOWN"
        );
    }
}
