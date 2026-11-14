//! Routes, integrations, authorizers and models of an HTTP or WebSocket API, and a WebSocket API's route and integration responses.
//! Each is a map in the API document keyed by its id; an update merges the members sent.

use aws_model_apigatewayv2::Operation;
use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use super::{api, api_mut, members, missing, page};
use crate::core::Core;
use crate::doc::{child, items, obj, text};
use crate::errors::{bad, conflict};
use crate::req::{Req, Res, new_id, none};

/// A kind of part: where it is kept, its id member and label, and its name in a refusal.
struct Part {
    map: &'static str,
    id: &'static str,
    label: &'static str,
    thing: &'static str,
}

const ROUTE: Part = Part {
    map: "routes",
    id: "routeId",
    label: "RouteId",
    thing: "Route",
};
const INTEGRATION: Part = Part {
    map: "integrations",
    id: "integrationId",
    label: "IntegrationId",
    thing: "Integration",
};
const AUTHORIZER: Part = Part {
    map: "authorizers",
    id: "authorizerId",
    label: "AuthorizerId",
    thing: "Authorizer",
};
const MODEL: Part = Part {
    map: "models",
    id: "modelId",
    label: "ModelId",
    thing: "Model",
};

pub(super) fn dispatch(core: &mut Core, op: Operation, r: &Req) -> Option<Res> {
    Some(match op {
        Operation::CreateRoute => create_route(core, r),
        Operation::GetRoute => get(core, r, &ROUTE),
        Operation::GetRoutes => list(core, r, ROUTE.map),
        Operation::UpdateRoute => update_route(core, r),
        Operation::DeleteRoute => delete(core, r, &ROUTE),
        Operation::DeleteRouteRequestParameter => api_mut(core, r).and_then(|d| {
            let route = child(d, "routes")
                .and_then(|m| m.get_mut(r.s("RouteId")))
                .ok_or_else(|| missing("Route", r.s("RouteId")))?;
            if let Some(p) = route
                .get_mut("requestParameters")
                .and_then(Value::as_object_mut)
            {
                p.remove(r.s("RequestParameterKey"));
            }
            none()
        }),
        Operation::CreateIntegration => create_integration(core, r),
        Operation::GetIntegration => get(core, r, &INTEGRATION),
        Operation::GetIntegrations => list(core, r, INTEGRATION.map),
        Operation::UpdateIntegration => update(core, r, &INTEGRATION),
        Operation::DeleteIntegration => delete_integration(core, r),
        Operation::CreateAuthorizer => create_authorizer(core, r),
        Operation::GetAuthorizer => get(core, r, &AUTHORIZER),
        Operation::GetAuthorizers => list(core, r, AUTHORIZER.map),
        Operation::UpdateAuthorizer => update(core, r, &AUTHORIZER),
        Operation::DeleteAuthorizer => delete_authorizer(core, r),
        Operation::CreateModel => create(core, r, &MODEL, 6, json!({})),
        Operation::GetModel => get(core, r, &MODEL),
        Operation::GetModels => list(core, r, MODEL.map),
        Operation::UpdateModel => update(core, r, &MODEL),
        Operation::DeleteModel => delete(core, r, &MODEL),
        _ => return responses(core, op, r),
    })
}

fn get(core: &Core, r: &Req, p: &Part) -> Res {
    let doc = api(core, r)?;
    let id = r.s(p.label);
    obj(doc, p.map)
        .and_then(|m| m.get(id))
        .cloned()
        .map(Into::into)
        .ok_or_else(|| missing(p.thing, id))
}

fn list(core: &Core, r: &Req, map: &str) -> Res {
    let doc = api(core, r)?;
    Ok(page(items(obj(doc, map)), r).into())
}

/// A new part with the request's members, `extra` defaults beneath them.
fn create(core: &mut Core, r: &Req, p: &Part, id_len: usize, extra: Value) -> Res {
    let doc = api_mut(core, r)?;
    let id = new_id(id_len);
    let mut v = extra.as_object().cloned().unwrap_or_default();
    v.extend(members(r, &[]));
    v.insert(p.id.into(), json!(id));
    let v = Value::Object(v);
    if let Some(m) = child(doc, p.map) {
        // WHY: the record and the answer are two values.
        m.insert(id, v.clone());
    }
    Ok(v.into())
}

fn update(core: &mut Core, r: &Req, p: &Part) -> Res {
    let doc = api_mut(core, r)?;
    let id = r.s(p.label);
    let slot = child(doc, p.map)
        .and_then(|m| m.get_mut(id))
        .ok_or_else(|| missing(p.thing, id))?;
    if let Some(o) = slot.as_object_mut() {
        o.extend(members(r, &[]));
    }
    Ok(slot.clone().into())
}

fn delete(core: &mut Core, r: &Req, p: &Part) -> Res {
    let doc = api_mut(core, r)?;
    let id = r.s(p.label);
    child(doc, p.map)
        .and_then(|m| m.remove(id))
        .ok_or_else(|| missing(p.thing, id))?;
    if p.map == "routes"
        && let Some(rr) = child(doc, "routeResponses")
    {
        rr.remove(id);
    }
    none()
}

const METHODS: &[&str] = &[
    "GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS", "ANY",
];

/// The recorded refusal of a malformed HTTP route key: `$default` or `<METHOD> /<path>`.
fn check_key(doc: &Value, key: &str) -> Result<(), ApiError> {
    if text(doc.get("api").unwrap_or(&Value::Null), "protocolType") == Some("WEBSOCKET")
        || key == "$default"
    {
        return Ok(());
    }
    let ok = key.split_once(' ').is_some_and(|(m, p)| {
        METHODS.contains(&m)
            && p.starts_with('/')
            && p.split('/')
                .filter(|s| !s.is_empty())
                .enumerate()
                .all(|(i, s)| {
                    let n = p.split('/').filter(|s| !s.is_empty()).count();
                    !s.ends_with("+}") || i + 1 == n
                })
    });
    if ok {
        Ok(())
    } else {
        Err(bad(
            "The provided route key is not formatted properly for HTTP protocol. Format should be \"[HTTP METHOD] /[RESOURCE PATH]\" or \"$default\"",
        ))
    }
}

/// A route's target must be an integration of the API (recorded wording), and its authorizer must exist `(memory)`.
fn check_route(doc: &Value, route: &Map<String, Value>) -> Result<(), ApiError> {
    if let Some(t) = route.get("target").and_then(Value::as_str) {
        let id = t.strip_prefix("integrations/").unwrap_or(t);
        if doc.pointer(&format!("/integrations/{id}")).is_none() {
            return Err(bad(format!(
                "Unable to find route target of type integrations with ID {id}"
            )));
        }
    }
    let kind = route
        .get("authorizationType")
        .and_then(Value::as_str)
        .unwrap_or("NONE");
    if matches!(kind, "CUSTOM" | "JWT") {
        let id = route
            .get("authorizerId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if doc.pointer(&format!("/authorizers/{id}")).is_none() {
            return Err(bad(format!("Unable to find authorizer with id {id}")));
        }
    }
    Ok(())
}

fn create_route(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let key = r.s("routeKey");
    check_key(doc, key)?;
    if obj(doc, "routes")
        .into_iter()
        .flat_map(|m| m.values())
        .any(|x| text(x, "routeKey") == Some(key))
    {
        return Err(conflict(format!(
            "Route with key {key} already exists for this API"
        )));
    }
    let mut route = json!({"apiKeyRequired": false, "authorizationType": "NONE"})
        .as_object()
        .cloned()
        .unwrap_or_default();
    route.extend(members(r, &[]));
    check_route(doc, &route)?;
    let id = new_id(7);
    route.insert("routeId".into(), json!(id));
    let route = Value::Object(route);
    if let Some(m) = child(doc, "routes") {
        m.insert(id, route.clone());
    }
    Ok(route.into())
}

fn update_route(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let id = r.s("RouteId");
    let mut route = obj(doc, "routes")
        .and_then(|m| m.get(id))
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| missing("Route", id))?;
    route.extend(members(r, &[]));
    if let Some(k) = route.get("routeKey").and_then(Value::as_str) {
        check_key(doc, k)?;
    }
    check_route(doc, &route)?;
    let route = Value::Object(route);
    if let Some(m) = child(doc, "routes") {
        m.insert(id.to_owned(), route.clone());
    }
    Ok(route.into())
}

fn create_integration(core: &mut Core, r: &Req) -> Res {
    let websocket = api(core, r)
        .is_ok_and(|d| d.pointer("/api/protocolType").and_then(Value::as_str) == Some("WEBSOCKET"));
    let kind = r.s("integrationType");
    if !websocket && matches!(kind, "AWS" | "HTTP" | "MOCK") {
        return Err(bad(format!(
            "Integration type {kind} is not supported for HTTP APIs"
        )));
    }
    let mut defaults = json!({"connectionType": "INTERNET", "timeoutInMillis": if websocket { 29_000 } else { 30_000 }});
    if kind == "AWS_PROXY" && r.opt("integrationSubtype").is_none() {
        crate::doc::put(&mut defaults, "integrationMethod", json!("POST"));
        if !websocket {
            crate::doc::put(&mut defaults, "payloadFormatVersion", json!("2.0"));
        }
    }
    if kind == "HTTP_PROXY" && !websocket {
        crate::doc::put(&mut defaults, "payloadFormatVersion", json!("1.0"));
    }
    if websocket && kind != "AWS_PROXY" && kind != "HTTP_PROXY" {
        crate::doc::put(&mut defaults, "passthroughBehavior", json!("WHEN_NO_MATCH"));
    }
    create(core, r, &INTEGRATION, 7, defaults)
}

fn delete_integration(core: &mut Core, r: &Req) -> Res {
    let doc = api(core, r)?;
    let id = r.s("IntegrationId");
    let target = format!("integrations/{id}");
    if let Some(route) = obj(doc, "routes")
        .into_iter()
        .flat_map(|m| m.values())
        .find(|x| text(x, "target") == Some(&target))
    {
        return Err(conflict(format!(
            "Cannot delete Integration {id}: it is the target of route {}",
            text(route, "routeKey").unwrap_or_default()
        )));
    }
    delete(core, r, &INTEGRATION)?;
    if let Ok(d) = api_mut(core, r)
        && let Some(ir) = child(d, "integrationResponses")
    {
        ir.remove(id);
    }
    none()
}

fn create_authorizer(core: &mut Core, r: &Req) -> Res {
    let doc = api(core, r)?;
    let name = r.s("name");
    if obj(doc, "authorizers")
        .into_iter()
        .flat_map(|m| m.values())
        .any(|a| text(a, "name") == Some(name))
    {
        return Err(conflict(format!("Authorizer name already exists: {name}")));
    }
    if r.s("authorizerType") == "JWT" && r.value("jwtConfiguration").is_none() {
        return Err(bad("JwtConfiguration is required for authorizer type JWT"));
    }
    create(core, r, &AUTHORIZER, 6, json!({}))
}

fn delete_authorizer(core: &mut Core, r: &Req) -> Res {
    let doc = api(core, r)?;
    let id = r.s("AuthorizerId");
    if obj(doc, "routes")
        .into_iter()
        .flat_map(|m| m.values())
        .any(|x| text(x, "authorizerId") == Some(id))
    {
        return Err(conflict(format!(
            "Cannot delete Authorizer {id}: a route uses it"
        )));
    }
    delete(core, r, &AUTHORIZER)
}

/// Route responses (under `routeResponses.<route>`) and integration responses (under `integrationResponses.<integration>`).
fn responses(core: &mut Core, op: Operation, r: &Req) -> Option<Res> {
    let (map, owner_label, owner_thing, id, label) = match op {
        Operation::CreateRouteResponse
        | Operation::GetRouteResponse
        | Operation::GetRouteResponses
        | Operation::UpdateRouteResponse
        | Operation::DeleteRouteResponse => (
            "routeResponses",
            "RouteId",
            "routes",
            "routeResponseId",
            "RouteResponseId",
        ),
        Operation::CreateIntegrationResponse
        | Operation::GetIntegrationResponse
        | Operation::GetIntegrationResponses
        | Operation::UpdateIntegrationResponse
        | Operation::DeleteIntegrationResponse => (
            "integrationResponses",
            "IntegrationId",
            "integrations",
            "integrationResponseId",
            "IntegrationResponseId",
        ),
        _ => return None,
    };
    let name = format!("{op:?}");
    Some(api_mut(core, r).and_then(|doc| {
        let owner = r.s(owner_label);
        if doc.pointer(&format!("/{owner_thing}/{owner}")).is_none() {
            return Err(missing(owner_label.trim_end_matches("Id"), owner));
        }
        let all = child(doc, map).ok_or_else(|| bad("the API is damaged"))?;
        let mine = all.entry(owner.to_owned()).or_insert_with(|| json!({}));
        let mine = mine
            .as_object_mut()
            .ok_or_else(|| bad("the API is damaged"))?;
        let rid = r.s(label);
        if name.starts_with("Create") {
            let new = new_id(6);
            let mut v = members(r, &[]);
            v.insert(id.into(), json!(new));
            mine.insert(new, Value::Object(v.clone()));
            Ok(Value::Object(v).into())
        } else if name.ends_with("Responses") {
            Ok(page(mine.values().cloned().collect(), r).into())
        } else if name.starts_with("Get") {
            mine.get(rid)
                .cloned()
                .map(Into::into)
                .ok_or_else(|| missing("Response", rid))
        } else if name.starts_with("Update") {
            let slot = mine
                .get_mut(rid)
                .and_then(Value::as_object_mut)
                .ok_or_else(|| missing("Response", rid))?;
            slot.extend(members(r, &[]));
            Ok(Value::Object(slot.clone()).into())
        } else {
            mine.remove(rid).ok_or_else(|| missing("Response", rid))?;
            none()
        }
    }))
}
