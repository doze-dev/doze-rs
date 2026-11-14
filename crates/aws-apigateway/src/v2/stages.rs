//! Stages and deployments of an HTTP or WebSocket API. An auto-deploying stage serves the live API; another serves the snapshot of
//! its deployment (`_snapshot`: routes, integrations, authorizers, models and the WebSocket responses), as on AWS.

use aws_model_apigatewayv2::Operation;
use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use super::{api, api_mut, members, missing, page};
use crate::core::Core;
use crate::doc::{child, items, obj, public, text};
use crate::errors::{bad, conflict, not_found};
use crate::req::{Req, Res, new_id, none};

pub(super) fn dispatch(core: &mut Core, op: Operation, r: &Req) -> Option<Res> {
    Some(match op {
        Operation::CreateStage => create_stage(core, r),
        Operation::GetStage => {
            api(core, r).and_then(|d| stage(d, r.s("StageName")).map(|s| public(s).into()))
        }
        Operation::GetStages => api(core, r).map(|d| page(items(obj(d, "stages")), r).into()),
        Operation::UpdateStage => update_stage(core, r),
        Operation::DeleteStage => api_mut(core, r).and_then(|d| {
            child(d, "stages")
                .and_then(|s| s.remove(r.s("StageName")))
                .ok_or_else(|| no_stage(r.s("StageName")))?;
            none()
        }),
        Operation::DeleteAccessLogSettings => remove_from_stage(core, r, &["accessLogSettings"]),
        Operation::DeleteRouteSettings => {
            remove_from_stage(core, r, &["routeSettings", r.s("RouteKey")])
        }
        Operation::ResetAuthorizersCache => {
            api(core, r).and_then(|d| stage(d, r.s("StageName")).map(|_| Default::default()))
        }
        Operation::CreateDeployment => create_deployment(core, r),
        Operation::GetDeployment => api(core, r).and_then(|d| {
            obj(d, "deployments")
                .and_then(|m| m.get(r.s("DeploymentId")))
                .map(|x| public(x).into())
                .ok_or_else(|| missing("Deployment", r.s("DeploymentId")))
        }),
        Operation::GetDeployments => {
            api(core, r).map(|d| page(items(obj(d, "deployments")), r).into())
        }
        Operation::UpdateDeployment => api_mut(core, r).and_then(|d| {
            let id = r.s("DeploymentId");
            let dep = child(d, "deployments")
                .and_then(|m| m.get_mut(id))
                .ok_or_else(|| missing("Deployment", id))?;
            if let (Some(o), Some(desc)) = (dep.as_object_mut(), r.value("description")) {
                o.insert("description".into(), desc.clone());
            }
            Ok(public(dep).into())
        }),
        Operation::DeleteDeployment => delete_deployment(core, r),
        _ => return None,
    })
}

fn no_stage(name: &str) -> ApiError {
    not_found(format!("Invalid stage identifier specified {name}"))
}

fn stage<'a>(doc: &'a Value, name: &str) -> Result<&'a Value, ApiError> {
    obj(doc, "stages")
        .and_then(|s| s.get(name))
        .ok_or_else(|| no_stage(name))
}

fn websocket(doc: &Value) -> bool {
    doc.pointer("/api/protocolType").and_then(Value::as_str) == Some("WEBSOCKET")
}

fn create_stage(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let name = r.s("stageName");
    if obj(doc, "stages").is_some_and(|s| s.contains_key(name)) {
        return Err(conflict(format!("Stage already exists: {name}")));
    }
    if let Some(d) = r.opt("deploymentId")
        && doc.pointer(&format!("/deployments/{d}")).is_none()
    {
        return Err(bad(format!("Deployment {d} not found")));
    }
    let route_defaults = if websocket(doc) {
        json!({"dataTraceEnabled": false, "detailedMetricsEnabled": false, "loggingLevel": "OFF"})
    } else {
        json!({"detailedMetricsEnabled": false})
    };
    let mut s = json!({
        "autoDeploy": false, "createdDate": r.now_iso(), "lastUpdatedDate": r.now_iso(),
        "defaultRouteSettings": route_defaults, "routeSettings": {}, "stageVariables": {}, "tags": {},
    })
    .as_object()
    .cloned()
    .unwrap_or_default();
    s.extend(members(r, &[]));
    let s = Value::Object(s);
    if let Some(stages) = child(doc, "stages") {
        // WHY: the record and the answer are two values.
        stages.insert(name.to_owned(), s.clone());
    }
    Ok(s.into())
}

fn update_stage(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let name = r.s("StageName");
    if let Some(d) = r.opt("deploymentId")
        && doc.pointer(&format!("/deployments/{d}")).is_none()
    {
        return Err(bad(format!("Deployment {d} not found")));
    }
    let s = child(doc, "stages")
        .and_then(|m| m.get_mut(name))
        .ok_or_else(|| no_stage(name))?;
    let Some(o) = s.as_object_mut() else {
        return Err(no_stage(name));
    };
    for (k, v) in members(r, &[]) {
        match (o.get_mut(&k), v) {
            // Route settings and stage variables merge key by key, as AWS's do.
            (Some(Value::Object(have)), Value::Object(more))
                if k == "routeSettings" || k == "stageVariables" =>
            {
                have.extend(more)
            }
            (_, v) => {
                o.insert(k, v);
            }
        }
    }
    o.insert("lastUpdatedDate".into(), json!(r.now_iso()));
    Ok(public(s).into())
}

fn remove_from_stage(core: &mut Core, r: &Req, path: &[&str]) -> Res {
    let doc = api_mut(core, r)?;
    let name = r.s("StageName");
    let mut cur = child(doc, "stages")
        .and_then(|m| m.get_mut(name))
        .ok_or_else(|| no_stage(name))?;
    let Some((last, parents)) = path.split_last() else {
        return none();
    };
    for p in parents {
        cur = cur.get_mut(*p).ok_or_else(|| {
            not_found(format!(
                "Invalid route settings identifier specified {last}"
            ))
        })?;
    }
    cur.as_object_mut()
        .and_then(|o| o.remove(*last))
        .ok_or_else(|| not_found(format!("Invalid identifier specified {last}")))?;
    none()
}

/// What a deployment keeps: everything the execute-api plane reads.
fn snapshot(doc: &Value) -> Value {
    let mut snap = Map::new();
    for part in [
        "routes",
        "integrations",
        "authorizers",
        "models",
        "routeResponses",
        "integrationResponses",
    ] {
        if let Some(v) = doc.get(part) {
            // WHY: a snapshot is a copy by definition.
            snap.insert(part.to_owned(), v.clone());
        }
    }
    Value::Object(snap)
}

fn create_deployment(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let stage_name = r.opt("stageName");
    if let Some(name) = stage_name {
        stage(doc, name)?;
    }
    let id = new_id(6);
    let mut dep = members(r, &["stageName"]);
    dep.insert("deploymentId".into(), json!(id));
    dep.insert("deploymentStatus".into(), json!("DEPLOYED"));
    dep.insert("createdDate".into(), json!(r.now_iso()));
    dep.insert("autoDeployed".into(), json!(false));
    let shown = Value::Object(dep.clone());
    dep.insert("_snapshot".into(), snapshot(doc));
    if let Some(m) = child(doc, "deployments") {
        m.insert(id.clone(), Value::Object(dep));
    }
    if let Some(name) = stage_name
        && let Some(s) = child(doc, "stages")
            .and_then(|m| m.get_mut(name))
            .and_then(Value::as_object_mut)
    {
        s.insert("deploymentId".into(), json!(id));
        s.insert("lastUpdatedDate".into(), json!(r.now_iso()));
    }
    Ok(shown.into())
}

fn delete_deployment(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let id = r.s("DeploymentId");
    if obj(doc, "stages")
        .into_iter()
        .flat_map(|m| m.values())
        .any(|s| text(s, "deploymentId") == Some(id))
    {
        return Err(bad(format!("Deployment {id} is in use by a stage")));
    }
    child(doc, "deployments")
        .and_then(|m| m.remove(id))
        .ok_or_else(|| missing("Deployment", id))?;
    none()
}
