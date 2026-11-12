//! A template: read from JSON or YAML, checked the way CloudFormation checks one before it starts, and summarised for
//! `ValidateTemplate` and `GetTemplateSummary`.

pub(crate) mod conditions;
pub(crate) mod deps;
pub(crate) mod eval;
pub(crate) mod fns;
pub(crate) mod params;
pub(crate) mod rules;
pub(crate) mod sub;

use serde_json::{Map, Value, json};

use aws_wire::ApiError;

use crate::errors::{format, template};
use crate::j::J;

/// The sections a template may have.
const SECTIONS: &[&str] = &[
    "AWSTemplateFormatVersion",
    "Description",
    "Metadata",
    "Parameters",
    "Rules",
    "Mappings",
    "Conditions",
    "Transform",
    "Resources",
    "Outputs",
    "Hooks",
];

/// The attributes a resource may carry besides its type and properties.
const RESOURCE_KEYS: &[&str] = &[
    "Type",
    "Properties",
    "DependsOn",
    "Condition",
    "Metadata",
    "DeletionPolicy",
    "UpdateReplacePolicy",
    "UpdatePolicy",
    "CreationPolicy",
    "Version",
];

/// The pseudo parameters a `Ref` may name.
pub(crate) const PSEUDO: &[&str] = &[
    "AWS::AccountId",
    "AWS::Region",
    "AWS::StackId",
    "AWS::StackName",
    "AWS::Partition",
    "AWS::URLSuffix",
    "AWS::NotificationARNs",
    "AWS::NoValue",
];

/// The text of a template as JSON. A body that is not JSON is read as YAML (JSON is YAML), and a body neither can read is refused
/// with the YAML reader's position, as AWS words it (recorded 2026-11-12: `{nope` is `YAML not well-formed. (line 1, column 6)`).
///
/// # Errors
/// `ValidationError` naming where the text stops being a template.
pub(crate) fn read(text: &str) -> Result<Value, ApiError> {
    let v = match serde_json::from_str::<Value>(text) {
        Ok(v) => v,
        Err(_) => crate::yaml::parse(text).map_err(|e| {
            format(format!(
                "YAML not well-formed. (line {}, column {})",
                e.line, e.column
            ))
        })?,
    };
    if !v.is_object() {
        // (memory)
        return Err(format("unsupported structure."));
    }
    Ok(v)
}

/// A snippet (from `AWS::Include`): any JSON or YAML value.
///
/// # Errors
/// `ValidationError` where the text stops being YAML.
pub(crate) fn read_snippet(text: &str) -> Result<Value, ApiError> {
    match serde_json::from_str::<Value>(text) {
        Ok(v) => Ok(v),
        Err(_) => crate::yaml::parse(text).map_err(|e| {
            format(format!(
                "YAML not well-formed. (line {}, column {})",
                e.line, e.column
            ))
        }),
    }
}

/// The template's `Transform` section as a list.
pub(crate) fn transforms(t: &Value) -> Vec<String> {
    match t.get("Transform") {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|x| match x {
                Value::String(s) => Some(s.clone()),
                Value::Object(o) => o.get("Name").and_then(Value::as_str).map(str::to_owned),
                _ => None,
            })
            .collect(),
        Some(Value::Object(o)) => o
            .get("Name")
            .and_then(Value::as_str)
            .map(|s| vec![s.to_owned()])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// The `Resources` section.
pub(crate) fn resources(t: &Value) -> Option<&Map<String, Value>> {
    t.get("Resources").and_then(Value::as_object)
}

/// A resource's type.
pub(crate) fn type_of(r: &Value) -> &str {
    r.get("Type").and_then(Value::as_str).unwrap_or_default()
}

/// The structure CloudFormation insists on before anything is resolved: the sections it knows, at least one resource, a type on
/// each, and references to things the template declares.
///
/// # Errors
/// `ValidationError` in AWS's words.
pub(crate) fn check(t: &Value) -> Result<(), ApiError> {
    let Some(top) = t.as_object() else {
        return Err(format("unsupported structure."));
    };
    let unknown: Vec<&str> = top
        .keys()
        .map(String::as_str)
        .filter(|k| !SECTIONS.contains(k))
        .collect();
    if !unknown.is_empty() {
        // (memory)
        return Err(format(format!(
            "Invalid template property or properties [{}]",
            unknown.join(", ")
        )));
    }
    if let Some(v) = top.get("AWSTemplateFormatVersion")
        && v.as_str() != Some("2010-09-09")
    {
        // (memory)
        return Err(format(format!(
            "'{}' is not a valid template version. Valid versions: 2010-09-09",
            v.as_str().map_or_else(|| v.to_string(), str::to_owned)
        )));
    }
    let res = match top.get("Resources") {
        Some(Value::Object(m)) if !m.is_empty() => m,
        _ => return Err(format("At least one Resources member must be defined.")),
    };
    for (name, r) in res {
        let Some(o) = r.as_object() else {
            // (memory)
            return Err(format(format!(
                "[/Resources/{name}] resource definition is malformed"
            )));
        };
        if !o.get("Type").is_some_and(Value::is_string) {
            return Err(format(format!(
                "[/Resources/{name}] Every Resources object must contain a Type member."
            )));
        }
        if let Some(bad) = o.keys().find(|k| !RESOURCE_KEYS.contains(&k.as_str())) {
            // (memory)
            return Err(format(format!(
                "Invalid template resource property '{bad}'"
            )));
        }
        if !valid_logical_id(name) {
            // (memory)
            return Err(format(format!("Resource name {name} is non alphanumeric.")));
        }
    }
    params::check_declarations(t)?;
    deps::check_references(t)?;
    conditions::check_declared(t)?;
    Ok(())
}

/// A logical id: letters and digits only.
pub(crate) fn valid_logical_id(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|c| c.is_ascii_alphanumeric())
}

/// The capabilities a template's resources need: `CAPABILITY_NAMED_IAM` when an IAM resource is given a name, else
/// `CAPABILITY_IAM` when there is one at all.
pub(crate) fn capabilities(t: &Value) -> Option<(&'static str, Vec<String>)> {
    let res = resources(t)?;
    let mut types: Vec<String> = Vec::new();
    let mut named = false;
    for r in res.values() {
        let ty = type_of(r);
        if ty.starts_with("AWS::IAM::") {
            if !types.iter().any(|x| x == ty) {
                types.push(ty.to_owned());
            }
            let p = r.get("Properties");
            named |= [
                "RoleName",
                "UserName",
                "GroupName",
                "ManagedPolicyName",
                "InstanceProfileName",
            ]
            .iter()
            .any(|k| p.and_then(|p| p.get(*k)).is_some());
        }
    }
    if types.is_empty() {
        return None;
    }
    Some((
        if named {
            "CAPABILITY_NAMED_IAM"
        } else {
            "CAPABILITY_IAM"
        },
        types,
    ))
}

/// `ValidateTemplate`'s answer.
pub(crate) fn validate_answer(t: &Value) -> Value {
    let mut out = Map::new();
    if let Some(d) = t.get("Description").and_then(Value::as_str) {
        out.insert("Description".into(), d.into());
    }
    let decls = params::declarations(t).unwrap_or_default();
    out.insert(
        "Parameters".into(),
        Value::Array(
            decls
                .iter()
                .map(|d| {
                    let mut p = json!({"ParameterKey": d.key, "NoEcho": d.no_echo});
                    if let Some(def) = &d.default {
                        p.put("DefaultValue", def.clone().into());
                    }
                    if let Some(desc) = &d.description {
                        p.put("Description", desc.clone().into());
                    }
                    p
                })
                .collect(),
        ),
    );
    add_capabilities(t, &mut out);
    let declared = transforms(t);
    if !declared.is_empty() {
        out.insert("DeclaredTransforms".into(), json!(declared));
    }
    Value::Object(out)
}

fn add_capabilities(t: &Value, out: &mut Map<String, Value>) {
    if let Some((cap, types)) = capabilities(t) {
        out.insert("Capabilities".into(), json!([cap]));
        out.insert(
            "CapabilitiesReason".into(),
            format!(
                "The following resource(s) require capabilities: [{}]",
                types.join(", ")
            )
            .into(),
        );
    }
}

/// `GetTemplateSummary`'s answer for a template (`original` keeps the declared transforms; `t` is the template after them).
pub(crate) fn summary_answer(original: &Value, t: &Value) -> Value {
    let mut out = Map::new();
    if let Some(d) = t.get("Description").and_then(Value::as_str) {
        out.insert("Description".into(), d.into());
    }
    let decls = params::declarations(t).unwrap_or_default();
    out.insert(
        "Parameters".into(),
        Value::Array(decls.iter().map(params::Decl::summary).collect()),
    );
    add_capabilities(t, &mut out);
    let mut types: Vec<&str> = resources(t)
        .map(|r| r.values().map(type_of).collect())
        .unwrap_or_default();
    types.sort_unstable();
    types.dedup();
    out.insert("ResourceTypes".into(), json!(types));
    out.insert("Version".into(), "2010-09-09".into());
    if let Some(m) = t.get("Metadata") {
        out.insert("Metadata".into(), m.to_string().into());
    }
    out.insert("DeclaredTransforms".into(), json!(transforms(original)));
    let mut ids: Vec<Value> = Vec::new();
    for ty in &types {
        let logical: Vec<&String> = resources(t)
            .map(|r| {
                r.iter()
                    .filter(|(_, v)| type_of(v) == *ty)
                    .map(|(k, _)| k)
                    .collect()
            })
            .unwrap_or_default();
        if let Some(keys) = crate::res::identifiers(ty) {
            ids.push(json!({"ResourceType": ty, "LogicalResourceIds": logical, "ResourceIdentifiers": keys}));
        }
    }
    if !ids.is_empty() {
        out.insert("ResourceIdentifierSummaries".into(), Value::Array(ids));
    }
    Value::Object(out)
}

/// `Template error: <what>` for a function used with the wrong arguments.
pub(crate) fn bad_fn(name: &str) -> ApiError {
    template(format!("Fn::{name} is used incorrectly")) // (memory)
}
