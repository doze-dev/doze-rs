//! REST APIs and their resource tree. An API is one document: `api` (what `GetRestApi` shows) and, beside it, `resources` keyed by id
//! with each resource's methods under `resourceMethods`, and the deployments, stages, authorizers and the rest (other modules).

use aws_model_api_gateway::Operation;
use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use super::{api, api_mut, page};
use crate::core::Core;
use crate::doc::{Doc, child, copy, obj, public, put, text};
use crate::errors::{bad, conflict, invalid_id};
use crate::patch::{Kind, apply, f};
use crate::req::{Req, Res, new_id, none};

pub(super) fn dispatch(core: &mut Core, op: Operation, r: &Req) -> Option<Res> {
    Some(match op {
        Operation::CreateRestApi => create(core, r),
        Operation::GetRestApi => api(core, r).map(|d| Value::from(api_view(d)).into()),
        Operation::GetRestApis => Ok(list(core, r).into()),
        Operation::UpdateRestApi => update(core, r),
        Operation::DeleteRestApi => delete(core, r),
        Operation::ImportRestApi => crate::openapi::v1::import(core, r),
        Operation::PutRestApi => crate::openapi::v1::put_rest_api(core, r),
        Operation::GetExport => crate::openapi::v1::export(core, r),
        Operation::CreateResource => create_resource(core, r),
        Operation::GetResource => get_resource(core, r),
        Operation::GetResources => get_resources(core, r),
        Operation::UpdateResource => update_resource(core, r),
        Operation::DeleteResource => delete_resource(core, r),
        _ => return None,
    })
}

/// What `GetRestApi` shows of a stored API.
pub(crate) fn api_view(doc: &Value) -> Map<String, Value> {
    obj(doc, "api").cloned().unwrap_or_default()
}

/// A new, empty REST API document from a `CreateRestApi`-shaped request (also the base of an import).
pub(crate) fn new_api(r: &Req, name: &str) -> (String, Value) {
    let id = new_id(10);
    let root = new_id(10);
    let mut a = Map::new();
    a.insert("id".into(), json!(id));
    a.insert("name".into(), json!(name));
    copy(
        &mut a,
        &r.body,
        &[
            "description",
            "version",
            "binaryMediaTypes",
            "minimumCompressionSize",
            "policy",
            "tags",
        ],
    );
    a.insert("createdDate".into(), json!(r.now_secs()));
    a.insert(
        "apiKeySource".into(),
        json!(r.opt("apiKeySource").unwrap_or("HEADER")),
    );
    let mut endpoint = r
        .value("endpointConfiguration")
        .cloned()
        .unwrap_or_else(|| json!({"types": ["EDGE"]}));
    if endpoint.get("ipAddressType").is_none() {
        put(&mut endpoint, "ipAddressType", json!("ipv4"));
    }
    a.insert("endpointConfiguration".into(), endpoint);
    a.insert(
        "disableExecuteApiEndpoint".into(),
        json!(r.flag("disableExecuteApiEndpoint").unwrap_or(false)),
    );
    a.insert("rootResourceId".into(), json!(root));
    a.insert(
        "securityPolicy".into(),
        json!(r.opt("securityPolicy").unwrap_or("TLS_1_0")),
    );
    a.insert("apiStatus".into(), json!("AVAILABLE"));
    let models = default_models();
    // The account is kept for the execute-api plane, whose requests are unsigned and name none.
    let doc = json!({"api": a, "resources": {root.clone(): {"id": root, "path": "/"}}, "models": models,
        "account": r.account(), "region": r.region()});
    (id, doc)
}

/// The two models every new REST API starts with, `Empty` and `Error` (the texts are `(memory)`).
fn default_models() -> Value {
    let empty = "{\n  \"$schema\": \"http://json-schema.org/draft-04/schema#\",\n  \"title\" : \"Empty Schema\",\n  \"type\" : \"object\"\n}";
    let error = "{\n  \"$schema\" : \"http://json-schema.org/draft-04/schema#\",\n  \"title\" : \"Error Schema\",\n  \"type\" : \"object\",\n  \"properties\" : {\n    \"message\" : { \"type\" : \"string\" }\n  }\n}";
    json!({
        "Empty": super::extras::new_model("Empty", Some("This is a default empty schema model"), Some(empty), "application/json"),
        "Error": super::extras::new_model("Error", Some("This is a default error schema model"), Some(error), "application/json"),
    })
}

fn create(core: &mut Core, r: &Req) -> Res {
    let (id, mut doc) = new_api(r, r.s("name"));
    if let Some(from) = r.opt("cloneFrom") {
        let source = core
            .rest
            .get(from)
            .ok_or_else(|| crate::errors::no_api(r.account(), from))?;
        clone_into(&mut doc, &source.0);
    }
    let shown = api_view(&doc);
    core.rest.insert(id, Doc(doc));
    Ok(Value::Object(shown).into())
}

/// Copies the resource tree and the API's other parts of `from` into a new API (`cloneFrom`), keeping the new root's id.
fn clone_into(doc: &mut Value, from: &Value) {
    let root_new = doc
        .pointer("/api/rootResourceId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let root_old = from
        .pointer("/api/rootResourceId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    for part in ["authorizers", "validators", "models", "responses"] {
        if let Some(v) = from.get(part) {
            // WHY: the clone is a new document that must own its copy.
            put(doc, part, v.clone());
        }
    }
    let Some(resources) = obj(from, "resources") else {
        return;
    };
    let text = serde_json::to_string(resources).unwrap_or_default();
    let remapped = text.replace(&format!("\"{root_old}\""), &format!("\"{root_new}\""));
    if let Ok(v) = serde_json::from_str::<Value>(&remapped) {
        put(doc, "resources", v);
    }
}

fn list(core: &Core, r: &Req) -> Value {
    let items = core
        .rest
        .iter()
        .map(|(_, d)| Value::Object(api_view(&d.0)))
        .collect();
    page(items, r)
}

const API_FIELDS: &[crate::patch::Field] = &[
    f("/name", Kind::Str),
    f("/description", Kind::Str),
    f("/version", Kind::Str),
    f("/apiKeySource", Kind::Str),
    f("/policy", Kind::Str),
    f("/binaryMediaTypes", Kind::List),
    f("/minimumCompressionSize", Kind::Int),
    f("/disableExecuteApiEndpoint", Kind::Bool),
    f("/endpointConfiguration/types", Kind::List),
    f("/endpointConfiguration/ipAddressType", Kind::Str),
    f("/endpointConfiguration/vpcEndpointIds", Kind::List),
    f("/securityPolicy", Kind::Str),
];

fn update(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let mut view = doc.get("api").cloned().unwrap_or_else(|| json!({}));
    apply(&mut view, &r.patches(), API_FIELDS)?;
    let shown = view.clone();
    put(doc, "api", view);
    Ok(shown.into())
}

fn delete(core: &mut Core, r: &Req) -> Res {
    api(core, r)?;
    core.rest.remove(r.s("restApiId"));
    let id = r.s("restApiId");
    // A usage plan that named one of the API's stages no longer does.
    let plans: Vec<String> = core.plans.iter().map(|(k, _)| k.to_owned()).collect();
    for p in plans {
        if let Some(Doc(plan)) = core.plans.get_mut(&p)
            && let Some(stages) = plan.get_mut("apiStages").and_then(Value::as_array_mut)
        {
            stages.retain(|s| text(s, "apiId") != Some(id));
        }
    }
    none()
}

/// Whether `part` is a path part API Gateway takes: letters, digits and `._-:`, or one `{variable}` (`{name+}` for a greedy one).
pub(crate) fn valid_part(part: &str) -> bool {
    let plain = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':'))
    };
    match part.strip_prefix('{').and_then(|p| p.strip_suffix('}')) {
        Some(var) => plain(var.strip_suffix('+').unwrap_or(var)),
        None => plain(part),
    }
}

fn create_resource(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let part = r.s("pathPart");
    let parent_id = r.s("parentId");
    let parent = doc
        .pointer(&format!("/resources/{parent_id}"))
        .ok_or_else(|| invalid_id("Resource"))?;
    if !valid_part(part) {
        return Err(bad(
            "Resource's path part only allow a-zA-Z0-9._-: or a valid greedy path variable and curly braces at the beginning and the end.",
        ));
    }
    let parent_path = text(parent, "path").unwrap_or("/").to_owned();
    if text(parent, "pathPart").is_some_and(|p| p.ends_with("+}")) {
        return Err(bad(format!(
            "Cannot create a child of a resource with a greedy path variable: {}",
            text(parent, "pathPart").unwrap_or_default()
        )));
    }
    check_siblings(doc, parent_id, part, None)?;
    let id = new_id(6);
    let path = join(&parent_path, part);
    let resource = json!({"id": id, "parentId": parent_id, "pathPart": part, "path": path});
    if let Some(rs) = child(doc, "resources") {
        // WHY: the stored record and the answer are two values.
        rs.insert(id, resource.clone());
    }
    Ok(resource.into())
}

/// A sibling with the same part conflicts (recorded wording); a second variable part under one parent is refused `(memory)`.
fn check_siblings(
    doc: &Value,
    parent: &str,
    part: &str,
    except: Option<&str>,
) -> Result<(), ApiError> {
    let siblings = obj(doc, "resources")
        .into_iter()
        .flat_map(|m| m.values())
        .filter(|s| text(s, "parentId") == Some(parent) && text(s, "id") != except);
    for s in siblings {
        let theirs = text(s, "pathPart").unwrap_or_default();
        if theirs == part {
            return Err(conflict(format!(
                "Another resource with the same parent already has this name: {part}"
            )));
        }
        if theirs.starts_with('{') && part.starts_with('{') {
            return Err(bad(format!(
                "A sibling ({theirs}) of this resource already has a variable path part -- only one is allowed"
            )));
        }
    }
    Ok(())
}

fn join(parent: &str, part: &str) -> String {
    if parent == "/" {
        format!("/{part}")
    } else {
        format!("{parent}/{part}")
    }
}

/// A resource as an answer shows it: its methods named with nothing in them, unless `embed=methods` asks for them whole.
fn resource_view(res: &Value, embed: bool) -> Value {
    let mut v = public(res);
    if let Some(methods) = obj(res, "resourceMethods") {
        let shown: Map<String, Value> = methods
            .iter()
            .map(|(k, m)| (k.clone(), if embed { m.clone() } else { json!({}) }))
            .collect();
        put(&mut v, "resourceMethods", Value::Object(shown));
    }
    v
}

fn embeds(r: &Req) -> bool {
    r.value("embed")
        .and_then(Value::as_array)
        .is_some_and(|e| e.iter().any(|x| x.as_str() == Some("methods")))
}

fn get_resource(core: &Core, r: &Req) -> Res {
    let doc = api(core, r)?;
    let res = doc
        .pointer(&format!("/resources/{}", r.s("resourceId")))
        .ok_or_else(|| invalid_id("Resource"))?;
    Ok(resource_view(res, embeds(r)).into())
}

fn get_resources(core: &Core, r: &Req) -> Res {
    let doc = api(core, r)?;
    let mut items: Vec<&Value> = obj(doc, "resources")
        .map(|m| m.values().collect())
        .unwrap_or_default();
    items.sort_by_key(|v| text(v, "path").unwrap_or_default());
    let embed = embeds(r);
    let items = items.into_iter().map(|v| resource_view(v, embed)).collect();
    Ok(page(items, r).into())
}

const RESOURCE_FIELDS: &[crate::patch::Field] =
    &[f("/pathPart", Kind::Str), f("/parentId", Kind::Str)];

fn update_resource(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let rid = r.s("resourceId");
    let mut res = doc
        .pointer(&format!("/resources/{rid}"))
        .cloned()
        .ok_or_else(|| invalid_id("Resource"))?;
    apply(&mut res, &r.patches(), RESOURCE_FIELDS)?;
    let part = text(&res, "pathPart").unwrap_or_default().to_owned();
    let parent = text(&res, "parentId").unwrap_or_default().to_owned();
    if !valid_part(&part) {
        return Err(bad(
            "Resource's path part only allow a-zA-Z0-9._-: or a valid greedy path variable and curly braces at the beginning and the end.",
        ));
    }
    check_siblings(doc, &parent, &part, Some(rid))?;
    if let Some(rs) = child(doc, "resources") {
        rs.insert(rid.to_owned(), res);
    }
    repath(doc);
    let res = doc
        .pointer(&format!("/resources/{rid}"))
        .map(|v| resource_view(v, false))
        .unwrap_or_default();
    Ok(res.into())
}

/// Recomputes every resource's full path from its parent's (after a move or a rename).
fn repath(doc: &mut Value) {
    let Some(rs) = child(doc, "resources") else {
        return;
    };
    let parents: Vec<(String, Option<String>, String)> = rs
        .values()
        .map(|v| {
            (
                text(v, "id").unwrap_or_default().to_owned(),
                text(v, "parentId").map(str::to_owned),
                text(v, "pathPart").unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let path_of = |id: &str| -> String {
        let mut parts = Vec::new();
        let mut cur = id.to_owned();
        for _ in 0..parents.len() {
            let Some((_, parent, part)) = parents.iter().find(|(i, _, _)| *i == cur) else {
                break;
            };
            let Some(parent) = parent else { break };
            parts.push(part.clone());
            cur.clone_from(parent);
        }
        parts.reverse();
        format!("/{}", parts.join("/"))
    };
    for (id, _, _) in &parents {
        let path = path_of(id);
        if let Some(v) = rs.get_mut(id) {
            put(v, "path", json!(path));
        }
    }
}

fn delete_resource(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let rid = r.s("resourceId");
    let res = doc
        .pointer(&format!("/resources/{rid}"))
        .ok_or_else(|| invalid_id("Resource"))?;
    if text(res, "parentId").is_none() {
        return Err(bad("Cannot delete root resource"));
    }
    let mut gone = vec![rid.to_owned()];
    if let Some(rs) = obj(doc, "resources") {
        let mut i = 0;
        while let Some(cur) = gone.get(i).cloned() {
            gone.extend(
                rs.values()
                    .filter(|v| text(v, "parentId") == Some(cur.as_str()))
                    .filter_map(|v| text(v, "id").map(str::to_owned)),
            );
            i += 1;
        }
    }
    if let Some(rs) = child(doc, "resources") {
        for id in &gone {
            rs.remove(id);
        }
    }
    none()
}
