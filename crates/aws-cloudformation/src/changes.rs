//! A change set's change records: what an execution would add, modify (in place or by replacement) and remove, worked out by
//! evaluating the new template against the stack's resources as they are. A value that depends on a resource that will change is
//! `Dynamic`, as on AWS.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::engine::Working;
use crate::j::J;
use crate::model::Res;
use crate::tmpl::deps::{Name, names};
use crate::tmpl::eval::Fault;

/// The change records, sorted by logical id.
pub(crate) fn compute(
    w: &Working,
    old: &BTreeMap<String, Res>,
    old_template: &Value,
    changed_params: &BTreeSet<String>,
) -> Vec<Value> {
    let mut out = Vec::new();
    let mut ids: BTreeSet<&String> = w.live.iter().collect();
    ids.extend(old.keys());
    for id in ids {
        let live = w.live.contains(id);
        let before = old
            .get(id)
            .filter(|r| r.physical.is_some() && !r.status.starts_with("DELETE_COMPLETE"));
        match (live, before) {
            (true, None) => out.push(json!({"Type": "Resource", "ResourceChange": {
                "Action": "Add", "LogicalResourceId": id, "ResourceType": w.type_of(id), "Scope": [], "Details": []}})),
            (false, Some(r)) => {
                let policy = crate::tmpl::resources(old_template)
                    .and_then(|res| res.get(id))
                    .and_then(|x| x.get("DeletionPolicy"))
                    .and_then(Value::as_str)
                    .unwrap_or("Delete");
                out.push(json!({"Type": "Resource", "ResourceChange": {
                    "Action": "Remove", "LogicalResourceId": id, "PhysicalResourceId": r.physical, "ResourceType": r.ty,
                    "PolicyAction": if policy == "Retain" { "Retain" } else { "Delete" }, "Scope": [], "Details": []}}));
            }
            (true, Some(r)) => {
                if let Some(c) = modify(w, id, r, changed_params) {
                    out.push(c);
                }
            }
            (false, None) => {}
        }
    }
    out
}

fn modify(w: &Working, id: &str, r: &Res, changed_params: &BTreeSet<String>) -> Option<Value> {
    let ty = w.type_of(id);
    let before = r.props_value();
    let declared = w
        .declared(id)
        .and_then(|d| d.get("Properties"))
        .cloned()
        .unwrap_or(Value::Null);
    let scope = w.scope();
    let mut keys: BTreeSet<String> = before
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    keys.extend(
        declared
            .as_object()
            .map(|o| o.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default(),
    );
    let create_only = crate::res::create_only(&ty);
    let mut details = Vec::new();
    let mut replacement = "False";
    let mut tags_only = true;
    for k in &keys {
        let expr = declared.get(k);
        let (now, dynamic) = match expr.map(|e| scope.eval(e)) {
            None => (None, None),
            Some(Ok(v)) => (v, None),
            Some(Err(Fault::Pending(res))) => (None, Some(res)),
            Some(Err(Fault::Error(_))) => (None, None),
        };
        if dynamic.is_none() && now.as_ref() == before.get(k) {
            continue;
        }
        if k != "Tags" {
            tags_only = false;
        }
        let recreate = if create_only.contains(&k.as_str()) {
            if dynamic.is_some() {
                "Conditionally"
            } else {
                "Always"
            }
        } else {
            "Never"
        };
        replacement = match (replacement, recreate) {
            (_, "Always") | ("True", _) => "True",
            (_, "Conditionally") | ("Conditional", _) => "Conditional",
            _ => "False",
        };
        details.push(detail(
            k,
            recreate,
            expr,
            dynamic.as_deref(),
            changed_params,
        ));
    }
    if details.is_empty() {
        return None;
    }
    let scope_list = if tags_only {
        json!(["Tags"])
    } else {
        json!(["Properties"])
    };
    Some(json!({"Type": "Resource", "ResourceChange": {
        "Action": "Modify", "LogicalResourceId": id, "PhysicalResourceId": r.physical, "ResourceType": ty,
        "Replacement": replacement, "Scope": scope_list, "Details": details}}))
}

/// One property's change: where it came from (the template itself, a parameter, or a resource that changes).
fn detail(
    prop: &str,
    recreate: &str,
    expr: Option<&Value>,
    dynamic: Option<&str>,
    changed_params: &BTreeSet<String>,
) -> Value {
    let attribute = if prop == "Tags" { "Tags" } else { "Properties" };
    let mut target = json!({"Attribute": attribute, "RequiresRecreation": recreate});
    if attribute == "Properties" {
        target.put("Name", prop.into());
    }
    if let Some(res) = dynamic {
        return json!({"Target": target, "Evaluation": "Dynamic", "ChangeSource": "ResourceReference", "CausingEntity": res});
    }
    let mut refs = BTreeSet::new();
    if let Some(e) = expr {
        names(e, None, &mut refs);
    }
    let param = refs.iter().find_map(|n| match n {
        Name::Ref(p) if changed_params.contains(p) => Some(p.clone()),
        _ => None,
    });
    match param {
        Some(p) => {
            json!({"Target": target, "Evaluation": "Static", "ChangeSource": "ParameterReference", "CausingEntity": p})
        }
        None => {
            json!({"Target": target, "Evaluation": "Static", "ChangeSource": "DirectModification"})
        }
    }
}
