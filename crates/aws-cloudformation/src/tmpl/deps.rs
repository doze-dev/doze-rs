//! What refers to what: references are checked against the template's declarations, and the resources are put in an order that
//! creates each after everything it names (`Ref`, `Fn::GetAtt`, `Fn::Sub`, `DependsOn`). A cycle is refused, naming its members.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use aws_wire::ApiError;

use super::eval::intrinsic;
use super::sub::{Piece, parts, pieces};
use super::{PSEUDO, resources};
use crate::errors::{format, template, validation};

/// A name a value refers to: a `Ref` (parameter, pseudo parameter or resource) or a resource whose attribute it reads.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Name {
    Ref(String),
    Att(String),
}

/// Every name `v` refers to. `conds`, when given, picks the live branch of each `Fn::If` so a reference in a branch that is not
/// taken does not count.
pub(crate) fn names(v: &Value, conds: Option<&BTreeMap<String, bool>>, out: &mut BTreeSet<Name>) {
    if let Some((f, arg)) = intrinsic(v) {
        match f {
            "Ref" => {
                out.insert(Name::Ref(arg.as_str().unwrap_or_default().to_owned()));
                return;
            }
            "Fn::GetAtt" => {
                let res = match arg {
                    Value::String(s) => s.split('.').next().unwrap_or_default().to_owned(),
                    Value::Array(a) => {
                        if let Some(attr) = a.get(1) {
                            names(attr, conds, out);
                        }
                        a.first()
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned()
                    }
                    _ => String::new(),
                };
                out.insert(Name::Att(res));
                return;
            }
            "Fn::Sub" => {
                if let Some((text, vars)) = parts(arg) {
                    for p in pieces(text) {
                        if let Piece::Var(name) = p
                            && !vars.is_some_and(|m| m.contains_key(name))
                        {
                            out.insert(match name.split_once('.') {
                                Some((r, _)) if !name.starts_with("AWS::") => {
                                    Name::Att(r.to_owned())
                                }
                                _ => Name::Ref(name.to_owned()),
                            });
                        }
                    }
                    if let Some(vars) = vars {
                        vars.values().for_each(|x| names(x, conds, out));
                    }
                }
                return;
            }
            "Fn::If" => {
                if let (Some(conds), Some([c, yes, no])) =
                    (conds, arg.as_array().map(Vec::as_slice))
                    && let Some(b) = c.as_str().and_then(|c| conds.get(c))
                {
                    names(if *b { yes } else { no }, Some(conds), out);
                    return;
                }
            }
            _ => {}
        }
    }
    match v {
        Value::Object(o) => o.values().for_each(|x| names(x, conds, out)),
        Value::Array(a) => a.iter().for_each(|x| names(x, conds, out)),
        _ => {}
    }
}

/// `DependsOn` as a list.
pub(crate) fn depends_on(r: &Value) -> Vec<String> {
    match r.get("DependsOn") {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

/// Every reference in the template names something it declares.
///
/// # Errors
/// `ValidationError` in AWS's words (recorded 2026-11-12 for an unknown `Ref` in a resource and an unknown `Fn::GetAtt` in an
/// output).
pub(crate) fn check_references(t: &Value) -> Result<(), ApiError> {
    let res = resources(t).cloned().unwrap_or_default();
    let params: BTreeSet<&str> = t
        .get("Parameters")
        .and_then(Value::as_object)
        .map(|m| m.keys().map(String::as_str).collect())
        .unwrap_or_default();
    let known_ref = |n: &str| res.contains_key(n) || params.contains(n) || PSEUDO.contains(&n);
    for (section, items) in [
        ("Resources", res.clone()),
        (
            "Outputs",
            t.get("Outputs")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
        ),
    ] {
        let mut unresolved = Vec::new();
        for (logical, item) in &items {
            let mut found = BTreeSet::new();
            names(item, None, &mut found);
            for n in &found {
                match n {
                    Name::Ref(r) if !known_ref(r) => unresolved.push(r.clone()),
                    Name::Att(r) if !res.contains_key(r) => {
                        return Err(template(format!(
                            "instance of Fn::GetAtt references undefined resource {r}"
                        )));
                    }
                    _ => {}
                }
            }
            if section == "Resources" {
                for d in depends_on(item) {
                    if !res.contains_key(&d) {
                        // (memory)
                        return Err(format(format!(
                            "Unresolved resource dependencies [{d}] in the Resources block of the template"
                        )));
                    }
                }
                let _ = logical;
            }
        }
        if !unresolved.is_empty() {
            unresolved.sort();
            unresolved.dedup();
            return Err(format(format!(
                "Unresolved resource dependencies [{}] in the {section} block of the template",
                unresolved.join(", ")
            )));
        }
    }
    Ok(())
}

/// The resources each live resource waits for (resources named by its references and `DependsOn` that are themselves live).
pub(crate) fn graph(
    t: &Value,
    live: &BTreeSet<String>,
    conds: &BTreeMap<String, bool>,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut out = BTreeMap::new();
    let Some(res) = resources(t) else {
        return out;
    };
    for name in live {
        let Some(r) = res.get(name) else {
            continue;
        };
        let mut found = BTreeSet::new();
        for part in ["Properties", "Metadata", "UpdatePolicy", "CreationPolicy"] {
            if let Some(p) = r.get(part) {
                names(p, Some(conds), &mut found);
            }
        }
        let mut waits: BTreeSet<String> = found
            .into_iter()
            .map(|n| match n {
                Name::Ref(x) | Name::Att(x) => x,
            })
            .filter(|x| live.contains(x) && x != name)
            .collect();
        waits.extend(depends_on(r).into_iter().filter(|d| live.contains(d)));
        out.insert(name.clone(), waits);
    }
    out
}

/// The resources in an order that puts each after what it waits for.
///
/// # Errors
/// `ValidationError` naming the resources of a cycle.
pub(crate) fn order(graph: &BTreeMap<String, BTreeSet<String>>) -> Result<Vec<String>, ApiError> {
    let mut indegree: BTreeMap<&str, usize> = graph.keys().map(|k| (k.as_str(), 0)).collect();
    for (k, waits) in graph {
        indegree.insert(k, waits.len());
    }
    let mut ready: Vec<&str> = indegree
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(k, _)| *k)
        .collect();
    let mut out = Vec::new();
    while let Some(next) = ready.pop() {
        out.push(next.to_owned());
        for (k, waits) in graph {
            if waits.contains(next)
                && let Some(n) = indegree.get_mut(k.as_str())
            {
                *n -= 1;
                if *n == 0 {
                    ready.push(k);
                }
            }
        }
    }
    if out.len() < graph.len() {
        let stuck: Vec<&str> = graph
            .keys()
            .map(String::as_str)
            .filter(|k| !out.iter().any(|o| o == k))
            .collect();
        // (memory)
        return Err(validation(format!(
            "Circular dependency between resources: [{}]",
            stuck.join(", ")
        )));
    }
    Ok(out)
}
