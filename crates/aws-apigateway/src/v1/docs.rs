//! Documentation parts and versions of a REST API. A part is kept in the API document (`documentation`, by id) with its location
//! checked as the model documents `DocumentationPartLocation`: which fields each type takes, which it needs (`name` for
//! `AUTHORIZER`, `MODEL`, the parameters, `REQUEST_BODY` and `RESPONSE_HEADER`), and the defaults (`path` `/`, `method` `*`,
//! `statusCode` `*`). One part per location. `properties` must be a JSON object. A version (`docVersions`) is a snapshot of every
//! part, and a stage names the one it publishes (`documentationVersion`), which `GetExport` writes when asked for
//! `extensions=documentation`. `ImportDocumentationParts` reads `x-amazon-apigateway-documentation` from an OpenAPI document.
//! The refusals of a location, of `properties` and of a part or version that exists are recorded (2026-11-19); the rest of the
//! words are `(memory)`.

use aws_model_api_gateway::Operation;
use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use super::{api, api_mut, page};
use crate::core::Core;
use crate::doc::{child, obj, put, text};
use crate::errors::{bad, conflict, not_found};
use crate::patch::{Kind, apply, f};
use crate::req::{Req, Res, new_id, none};

pub(super) fn dispatch(core: &mut Core, op: Operation, r: &Req) -> Option<Res> {
    Some(match op {
        Operation::CreateDocumentationPart => create_part(core, r),
        Operation::GetDocumentationPart => {
            api(core, r).and_then(|d| part(d, r).cloned().map(Into::into))
        }
        Operation::GetDocumentationParts => api(core, r).map(|d| page(list_parts(d, r), r).into()),
        Operation::UpdateDocumentationPart => update_part(core, r),
        Operation::DeleteDocumentationPart => api_mut(core, r).and_then(|d| {
            child(d, "documentation")
                .and_then(|m| m.remove(r.s("documentationPartId")))
                .ok_or_else(no_part)
                .and_then(|_| none())
        }),
        Operation::ImportDocumentationParts => import(core, r),
        Operation::CreateDocumentationVersion => create_version(core, r),
        Operation::GetDocumentationVersion => {
            api(core, r).and_then(|d| version(d, r).map(|v| crate::doc::public(v).into()))
        }
        Operation::GetDocumentationVersions => api(core, r).map(|d| {
            let mut vs = crate::doc::items(obj(d, "docVersions"));
            vs.sort_by(|a, b| text(a, "version").cmp(&text(b, "version")));
            page(vs, r).into()
        }),
        Operation::UpdateDocumentationVersion => update_version(core, r),
        Operation::DeleteDocumentationVersion => api_mut(core, r).and_then(|d| {
            // Recorded 2026-11-19: a version a stage publishes cannot be deleted.
            let name = r.s("documentationVersion");
            let used = obj(d, "stages").is_some_and(|stages| {
                stages
                    .values()
                    .any(|s| s.get("documentationVersion").and_then(Value::as_str) == Some(name))
            });
            if used {
                return Err(bad(
                    "Cannot delete documentation version because there are API Stages associated with it.",
                ));
            }
            child(d, "docVersions")
                .and_then(|m| m.remove(r.s("documentationVersion")))
                .ok_or_else(no_version)
                .and_then(|_| none())
        }),
        _ => return None,
    })
}

fn no_part() -> ApiError {
    not_found("Invalid Documentation part identifier specified")
}

fn no_version() -> ApiError {
    not_found("Invalid Documentation version identifier specified")
}

fn part<'a>(doc: &'a Value, r: &Req) -> Result<&'a Value, ApiError> {
    obj(doc, "documentation")
        .and_then(|m| m.get(r.s("documentationPartId")))
        .ok_or_else(no_part)
}

fn version<'a>(doc: &'a Value, r: &Req) -> Result<&'a Value, ApiError> {
    obj(doc, "docVersions")
        .and_then(|m| m.get(r.s("documentationVersion")))
        .ok_or_else(no_version)
}

/// Which location fields a type takes: path, method, status code, name (and whether the name is needed).
fn takes(kind: &str) -> Option<(bool, bool, bool, Option<bool>)> {
    Some(match kind {
        "API" => (false, false, false, None),
        "AUTHORIZER" | "MODEL" => (false, false, false, Some(true)),
        "RESOURCE" => (true, false, false, None),
        "METHOD" => (true, true, false, None),
        "PATH_PARAMETER" | "QUERY_PARAMETER" | "REQUEST_HEADER" | "REQUEST_BODY" => {
            (true, true, false, Some(true))
        }
        "RESPONSE" | "RESPONSE_BODY" => (true, true, true, None),
        "RESPONSE_HEADER" => (true, true, true, Some(true)),
        _ => return None,
    })
}

/// A location as the type takes it, with its defaults filled in; a field the type does not take, or a missing name, is refused.
pub(crate) fn location(loc: Option<&Value>) -> Result<Value, ApiError> {
    let loc = loc.ok_or_else(|| bad("Documentation part location must be specified"))?;
    let kind = text(loc, "type").unwrap_or_default();
    let (path, method, status, name) =
        takes(kind).ok_or_else(|| bad(format!("Invalid documentation part type: {kind}")))?;
    let refused = || bad(rule(kind, path, method, status, name.is_some()));
    let mut out = Map::new();
    out.insert("type".into(), json!(kind));
    for (field, ok, default) in [
        ("path", path, "/"),
        ("method", method, "*"),
        ("statusCode", status, "*"),
    ] {
        match (text(loc, field), ok) {
            (Some(_), false) => return Err(refused()),
            (v, true) => {
                out.insert(field.into(), json!(v.unwrap_or(default)));
            }
            (None, false) => {}
        }
    }
    match (text(loc, "name"), name) {
        (Some(_), None) | (None, Some(true)) => Err(refused()),
        (n, _) => {
            if let Some(n) = n {
                out.insert("name".into(), json!(n));
            }
            Ok(Value::Object(out))
        }
    }
}

/// The refusal of a location its type does not take: one sentence per type, whatever was wrong. Recorded 2026-11-19 for `API`
/// ("cannot have path, method, statusCode or name defined") and `MODEL` ("must have a name and cannot have path, method or
/// statusCode defined"); the other types' sentences are built the same way `(memory)`.
fn rule(kind: &str, path: bool, method: bool, status: bool, named: bool) -> String {
    let mut cannot: Vec<&str> = [("path", path), ("method", method), ("statusCode", status)]
        .into_iter()
        .filter(|(_, ok)| !ok)
        .map(|(f, _)| f)
        .collect();
    if !named {
        cannot.push("name");
    }
    let list = match cannot.split_last() {
        None => String::new(),
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
    };
    let body = match (named, list.is_empty()) {
        (true, true) => "must have a name".to_owned(),
        (true, false) => format!("must have a name and cannot have {list} defined"),
        (false, _) => format!("cannot have {list} defined"),
    };
    format!("[Location type: {kind} {body}.]")
}

/// `properties` must be a JSON object, kept as the text sent.
fn properties(p: Option<&str>) -> Result<String, ApiError> {
    let p = p.ok_or_else(|| bad("Documentation part properties must be specified"))?;
    match serde_json::from_str::<Value>(p) {
        Ok(Value::Object(_)) => Ok(p.to_owned()),
        // Recorded 2026-11-19: also for JSON that is not an object (`[1]`).
        _ => Err(bad("Documentation part properties must be a valid JSON.")),
    }
}

/// How a conflict names a location: `type 'METHOD', path '/pets', method 'GET'` (recorded 2026-11-19; the `statusCode` and `name`
/// labels follow the same form `(memory)`).
fn described(loc: &Value) -> String {
    ["type", "path", "method", "statusCode", "name"]
        .into_iter()
        .filter_map(|f| text(loc, f).map(|v| format!("{f} '{v}'")))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The id of the part at `loc`, if there is one.
fn at_location<'a>(doc: &'a Value, loc: &Value) -> Option<&'a str> {
    obj(doc, "documentation")?
        .iter()
        .find(|(_, p)| p.get("location") == Some(loc))
        .map(|(id, _)| id.as_str())
}

fn create_part(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let loc = location(r.value("location"))?;
    let props = properties(r.opt("properties"))?;
    if at_location(doc, &loc).is_some() {
        return Err(conflict(format!(
            "Documentation part already exists for the specified location: {}.",
            described(&loc)
        )));
    }
    let id = new_id(6);
    let p = json!({"id": id, "location": loc, "properties": props});
    if let Some(m) = child(doc, "documentation") {
        m.insert(id, p.clone());
    }
    Ok(p.into())
}

fn list_parts(doc: &Value, r: &Req) -> Vec<Value> {
    let documented = |p: &Value| {
        text(p, "properties")
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .and_then(|v| v.as_object().map(|o| !o.is_empty()))
            .unwrap_or(false)
    };
    let mut out: Vec<Value> = crate::doc::items(obj(doc, "documentation"))
        .into_iter()
        .filter(|p| {
            r.opt("type")
                .is_none_or(|t| p.pointer("/location/type").and_then(Value::as_str) == Some(t))
        })
        .filter(|p| {
            r.opt("nameQuery")
                .is_none_or(|n| p.pointer("/location/name").and_then(Value::as_str) == Some(n))
        })
        .filter(|p| {
            r.opt("path")
                .is_none_or(|x| p.pointer("/location/path").and_then(Value::as_str) == Some(x))
        })
        .filter(|p| match r.opt("locationStatus") {
            Some("DOCUMENTED") => documented(p),
            Some("UNDOCUMENTED") => !documented(p),
            _ => true,
        })
        .collect();
    out.sort_by(|a, b| text(a, "id").cmp(&text(b, "id")));
    out
}

fn update_part(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let slot = child(doc, "documentation")
        .and_then(|m| m.get_mut(r.s("documentationPartId")))
        .ok_or_else(no_part)?;
    let mut v = slot.clone();
    apply(&mut v, &r.patches(), &[f("/properties", Kind::Str)])?;
    properties(text(&v, "properties"))?;
    *slot = v.clone();
    Ok(v.into())
}

fn create_version(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let name = r.s("documentationVersion");
    if name.is_empty() {
        return Err(bad("Documentation version must be specified"));
    }
    if obj(doc, "docVersions").is_some_and(|m| m.contains_key(name)) {
        // Recorded 2026-11-19.
        return Err(conflict(format!(
            "Documentation version {name} already exists."
        )));
    }
    if let Some(stage) = r.opt("stageName").filter(|s| !s.is_empty()) {
        let slot = doc
            .pointer_mut(&format!("/stages/{stage}"))
            .ok_or_else(|| crate::errors::invalid_id("stage"))?;
        put(slot, "documentationVersion", json!(name));
    }
    let parts = crate::doc::items(obj(doc, "documentation"));
    let mut v = json!({"version": name, "createdDate": r.now_secs()});
    if let Some(d) = r.opt("description") {
        put(&mut v, "description", json!(d));
    }
    let shown = v.clone();
    put(&mut v, "_parts", Value::Array(parts));
    if let Some(m) = child(doc, "docVersions") {
        m.insert(name.to_owned(), v);
    }
    Ok(shown.into())
}

fn update_version(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let slot = child(doc, "docVersions")
        .and_then(|m| m.get_mut(r.s("documentationVersion")))
        .ok_or_else(no_version)?;
    let mut v = slot.clone();
    apply(&mut v, &r.patches(), &[f("/description", Kind::Str)])?;
    *slot = v;
    Ok(crate::doc::public(slot).into())
}

/// `ImportDocumentationParts`: the parts of an OpenAPI document's `x-amazon-apigateway-documentation`. `overwrite` replaces every
/// part; `merge` (the default) replaces the parts at the locations named and keeps the rest.
fn import(core: &mut Core, r: &Req) -> Res {
    let spec = crate::openapi::body_of(r, true)?;
    let listed = spec
        .pointer("/x-amazon-apigateway-documentation/documentationParts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut warnings = Vec::new();
    let mut parts = Vec::new();
    for (i, p) in listed.iter().enumerate() {
        let props = p.get("properties").map(|v| {
            if v.is_string() {
                crate::doc::plain(v)
            } else {
                v.to_string()
            }
        });
        match location(p.get("location")).and_then(|l| Ok((l, properties(props.as_deref())?))) {
            Ok(x) => parts.push(x),
            Err(e) => warnings.push(format!("Documentation part {i}: {}", e.message)),
        }
    }
    if r.flag("failOnWarnings") == Some(true) && !warnings.is_empty() {
        return Err(bad(format!(
            "Documentation parts were not imported: {}",
            warnings.join("; ")
        )));
    }
    let doc = api_mut(core, r)?;
    if r.opt("mode") == Some("overwrite")
        && let Some(m) = child(doc, "documentation")
    {
        m.clear();
    }
    let mut ids = Vec::new();
    for (loc, props) in parts {
        let id = at_location(doc, &loc).map_or_else(|| new_id(6), str::to_owned);
        if let Some(m) = child(doc, "documentation") {
            m.insert(
                id.clone(),
                json!({"id": id, "location": loc, "properties": props}),
            );
        }
        ids.push(json!(id));
    }
    let mut out = json!({"ids": ids});
    if !warnings.is_empty() {
        put(&mut out, "warnings", json!(warnings));
    }
    Ok(out.into())
}

/// What `GetExport` writes for `extensions=documentation`: the stage's documentation version as AWS's extension, or nothing when
/// the stage publishes none.
pub(crate) fn export(doc: &Value, stage: &str) -> Option<Value> {
    let name = doc
        .pointer(&format!("/stages/{stage}/documentationVersion"))?
        .as_str()?;
    let v = doc.pointer(&format!("/docVersions/{name}"))?;
    let parts: Vec<Value> = v
        .get("_parts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|p| {
            let props = text(p, "properties")
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
                .unwrap_or_else(|| json!({}));
            json!({"location": p.get("location"), "properties": props})
        })
        .collect();
    Some(json!({"version": name, "createdDate": v.get("createdDate"), "documentationParts": parts}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_location_takes_what_its_type_takes() {
        let l = location(Some(&json!({"type": "METHOD", "path": "/pets"}))).unwrap_or_default();
        assert_eq!(l, json!({"type": "METHOD", "path": "/pets", "method": "*"}));
        assert!(location(Some(&json!({"type": "API", "path": "/"}))).is_err());
        assert!(location(Some(&json!({"type": "MODEL"}))).is_err());
        assert!(location(Some(&json!({"type": "RESOURCE", "name": "x"}))).is_err());
        let h =
            location(Some(&json!({"type": "RESPONSE_HEADER", "name": "X-A"}))).unwrap_or_default();
        assert_eq!(
            h,
            json!({"type": "RESPONSE_HEADER", "path": "/", "method": "*", "statusCode": "*", "name": "X-A"})
        );
        assert!(location(Some(&json!({"type": "NOPE"}))).is_err());
    }

    #[test]
    fn a_refused_location_is_worded_as_aws_words_it() {
        // Recorded 2026-11-19.
        let api = location(Some(&json!({"type": "API", "path": "/pets"}))).unwrap_err();
        assert_eq!(
            api.message,
            "[Location type: API cannot have path, method, statusCode or name defined.]"
        );
        let model = location(Some(&json!({"type": "MODEL"}))).unwrap_err();
        assert_eq!(
            model.message,
            "[Location type: MODEL must have a name and cannot have path, method or statusCode defined.]"
        );
        let at = location(Some(
            &json!({"type": "METHOD", "path": "/pets", "method": "GET"}),
        ))
        .unwrap_or_default();
        assert_eq!(described(&at), "type 'METHOD', path '/pets', method 'GET'");
        assert_eq!(
            properties(Some("[1]")).unwrap_err().message,
            "Documentation part properties must be a valid JSON."
        );
    }
}
