//! How the API writes a contract: Confluent's JSON (`@JsonInclude(NON_EMPTY)`, in its classes' field order).
//!
//! And Java's `toString` of a contract, as a refusal quotes a schema.

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
