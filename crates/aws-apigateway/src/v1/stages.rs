//! Deployments and stages. A deployment is a snapshot of the API (`_snapshot`: resources, methods, integrations, authorizers,
//! validators, models, gateway responses and the API's own settings), as on AWS: a change is served once it is deployed and the stage
//! points at the deployment. Stage settings round-trip whole, method settings with AWS's defaults filled in.

use aws_model_api_gateway::Operation;
use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use super::{api, api_mut};
use crate::core::Core;
use crate::doc::{child, copy, items, obj, public, put, text};
use crate::errors::{bad, conflict, invalid_id};
use crate::patch::{Field, Kind, Patch, apply, f, unescape};
use crate::req::{Req, Res, new_id, none};

pub(super) fn dispatch(core: &mut Core, op: Operation, r: &Req) -> Option<Res> {
    Some(match op {
        Operation::CreateDeployment => create_deployment(core, r),
        Operation::GetDeployment => get_deployment(core, r),
        Operation::GetDeployments => api(core, r).map(|d| {
            let list = items(obj(d, "deployments"));
            super::page(list, r).into()
        }),
        Operation::UpdateDeployment => update_deployment(core, r),
        Operation::DeleteDeployment => delete_deployment(core, r),
        Operation::CreateStage => create_stage(core, r),
        Operation::GetStage => {
            api(core, r).and_then(|d| stage(d, r.s("stageName")).map(|s| public(s).into()))
        }
        Operation::GetStages => api(core, r).map(|d| {
            let want = r.opt("deploymentId");
            let list: Vec<Value> = items(obj(d, "stages"))
                .into_iter()
                .filter(|s| want.is_none_or(|w| text(s, "deploymentId") == Some(w)))
                .collect();
            json!({"item": list}).into()
        }),
        Operation::UpdateStage => update_stage(core, r),
        Operation::DeleteStage => delete_stage(core, r),
        Operation::FlushStageCache | Operation::FlushStageAuthorizersCache => {
            api(core, r).and_then(|d| stage(d, r.s("stageName")).map(|_| Default::default()))
        }
        _ => return None,
    })
}

fn stage<'a>(doc: &'a Value, name: &str) -> Result<&'a Value, ApiError> {
    obj(doc, "stages")
        .and_then(|s| s.get(name))
        .ok_or_else(|| invalid_id("stage"))
}

/// What a deployment keeps of the API: everything the execute-api plane reads.
fn snapshot(doc: &Value) -> Value {
    let mut snap = Map::new();
    for part in [
        "api",
        "resources",
        "authorizers",
        "validators",
        "models",
        "responses",
    ] {
        if let Some(v) = doc.get(part) {
            // WHY: a snapshot is a copy by definition.
            snap.insert(part.to_owned(), v.clone());
        }
    }
    Value::Object(snap)
}

/// The recorded refusals of a deployment of an API that cannot serve anything yet.
fn check_deployable(doc: &Value) -> Result<(), ApiError> {
    let methods: Vec<&Value> = obj(doc, "resources")
        .into_iter()
        .flat_map(|rs| rs.values())
        .filter_map(|res| obj(res, "resourceMethods"))
        .flat_map(|ms| ms.values())
        .collect();
    if methods.is_empty() {
        return Err(bad("The REST API doesn't contain any methods"));
    }
    if methods.iter().any(|m| m.get("methodIntegration").is_none()) {
        return Err(bad("No integration defined for method"));
    }
    Ok(())
}

fn valid_stage_name(name: &str) -> Result<(), ApiError> {
    let ok = !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'));
    if ok {
        Ok(())
    } else {
        Err(bad("Stage name only allows a-zA-Z0-9_"))
    }
}

fn create_deployment(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    check_deployable(doc)?;
    let stage_name = r.opt("stageName").filter(|s| !s.is_empty());
    if let Some(name) = stage_name {
        valid_stage_name(name)?;
    }
    let id = new_id(6);
    let mut dep = Map::new();
    dep.insert("id".into(), json!(id));
    copy(&mut dep, &r.body, &["description"]);
    dep.insert("createdDate".into(), json!(r.now_secs()));
    let shown = Value::Object(dep.clone());
    dep.insert("_snapshot".into(), snapshot(doc));
    if let Some(ds) = child(doc, "deployments") {
        ds.insert(id.clone(), Value::Object(dep));
    }
    if let Some(name) = stage_name {
        let existing = obj(doc, "stages").is_some_and(|s| s.contains_key(name));
        if existing {
            if let Some(s) = doc.pointer_mut(&format!("/stages/{name}")) {
                put(s, "deploymentId", json!(id));
                put(s, "lastUpdatedDate", json!(r.now_secs()));
            }
        } else {
            let mut s = new_stage(r, name, &id);
            if let Some(desc) = r.opt("stageDescription") {
                put(&mut s, "description", json!(desc));
            }
            if let Some(stages) = child(doc, "stages") {
                stages.insert(name.to_owned(), s);
            }
        }
    }
    Ok(shown.into())
}

/// A new stage on deployment `deployment`, from a `CreateStage` or `CreateDeployment` request.
fn new_stage(r: &Req, name: &str, deployment: &str) -> Value {
    let mut s = Map::new();
    s.insert("deploymentId".into(), json!(deployment));
    s.insert("stageName".into(), json!(name));
    copy(
        &mut s,
        &r.body,
        &[
            "description",
            "variables",
            "documentationVersion",
            "canarySettings",
            "tags",
        ],
    );
    let cache = r.flag("cacheClusterEnabled").unwrap_or(false);
    s.insert("cacheClusterEnabled".into(), json!(cache));
    if cache {
        s.insert(
            "cacheClusterSize".into(),
            json!(r.opt("cacheClusterSize").unwrap_or("0.5")),
        );
    }
    s.insert(
        "cacheClusterStatus".into(),
        json!(if cache { "AVAILABLE" } else { "NOT_AVAILABLE" }),
    );
    s.insert("methodSettings".into(), json!({}));
    s.insert(
        "tracingEnabled".into(),
        json!(r.flag("tracingEnabled").unwrap_or(false)),
    );
    s.insert("createdDate".into(), json!(r.now_secs()));
    s.insert("lastUpdatedDate".into(), json!(r.now_secs()));
    Value::Object(s)
}

fn get_deployment(core: &Core, r: &Req) -> Res {
    let doc = api(core, r)?;
    let dep = obj(doc, "deployments")
        .and_then(|d| d.get(r.s("deploymentId")))
        .ok_or_else(|| invalid_id("Deployment"))?;
    let mut shown = public(dep);
    let summary = r
        .value("embed")
        .and_then(Value::as_array)
        .is_some_and(|e| e.iter().any(|x| x.as_str() == Some("apisummary")));
    if summary {
        put(&mut shown, "apiSummary", api_summary(dep.get("_snapshot")));
    }
    Ok(shown.into())
}

/// `{path: {METHOD: {authorizationType, apiKeyRequired}}}` of a deployment.
fn api_summary(snap: Option<&Value>) -> Value {
    let mut out = Map::new();
    for res in snap
        .and_then(|s| obj(s, "resources"))
        .into_iter()
        .flat_map(|m| m.values())
    {
        let Some(methods) = obj(res, "resourceMethods") else {
            continue;
        };
        let entry: Map<String, Value> = methods
            .iter()
            .map(|(k, m)| {
                (
                    k.clone(),
                    json!({
                        "authorizationType": text(m, "authorizationType").unwrap_or("NONE"),
                        "apiKeyRequired": m.get("apiKeyRequired").and_then(Value::as_bool).unwrap_or(false),
                    }),
                )
            })
            .collect();
        out.insert(
            text(res, "path").unwrap_or("/").to_owned(),
            Value::Object(entry),
        );
    }
    Value::Object(out)
}

fn update_deployment(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let dep = doc
        .pointer_mut(&format!("/deployments/{}", r.s("deploymentId")))
        .ok_or_else(|| invalid_id("Deployment"))?;
    apply(dep, &r.patches(), &[f("/description", Kind::Str)])?;
    Ok(public(dep).into())
}

fn delete_deployment(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let id = r.s("deploymentId");
    if obj(doc, "deployments").is_none_or(|d| !d.contains_key(id)) {
        return Err(invalid_id("Deployment"));
    }
    let served = obj(doc, "stages")
        .into_iter()
        .flat_map(|s| s.values())
        .any(|s| text(s, "deploymentId") == Some(id));
    if served {
        return Err(bad(
            "Active stages pointing to this deployment must be moved or deleted",
        ));
    }
    if let Some(ds) = child(doc, "deployments") {
        ds.remove(id);
    }
    none()
}

fn create_stage(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let name = r.s("stageName");
    valid_stage_name(name)?;
    let deployment = r.s("deploymentId");
    if obj(doc, "deployments").is_none_or(|d| !d.contains_key(deployment)) {
        return Err(invalid_id("Deployment"));
    }
    if obj(doc, "stages").is_some_and(|s| s.contains_key(name)) {
        return Err(conflict("Stage already exists"));
    }
    let s = new_stage(r, name, deployment);
    if let Some(stages) = child(doc, "stages") {
        // WHY: the record and the answer are two values.
        stages.insert(name.to_owned(), s.clone());
    }
    Ok(s.into())
}

const STAGE_FIELDS: &[Field] = &[
    f("/description", Kind::Str),
    f("/deploymentId", Kind::Str),
    f("/cacheClusterEnabled", Kind::Bool),
    f("/cacheClusterSize", Kind::Str),
    f("/variables", Kind::Map),
    f("/tracingEnabled", Kind::Bool),
    f("/clientCertificateId", Kind::Str),
    f("/documentationVersion", Kind::Str),
    f("/webAclArn", Kind::Str),
    f("/accessLogSettings/format", Kind::Str),
    f("/accessLogSettings/destinationArn", Kind::Str),
    f("/accessLogSettings", Kind::Str),
    f("/canarySettings/percentTraffic", Kind::Num),
    f("/canarySettings/deploymentId", Kind::Str),
    f("/canarySettings/stageVariableOverrides", Kind::Map),
    f("/canarySettings/useStageCache", Kind::Bool),
    f("/canarySettings", Kind::Str),
];

fn update_stage(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let name = r.s("stageName");
    let mut s = stage(doc, name)?.clone();
    let (settings, plain): (Vec<Patch>, Vec<Patch>) = r.patches().into_iter().partition(|p| {
        !STAGE_FIELDS
            .iter()
            .any(|f| p.path == f.path || p.path.starts_with(&format!("{}/", f.path)))
    });
    apply(&mut s, &plain, STAGE_FIELDS)?;
    for p in &settings {
        method_setting(&mut s, p)?;
    }
    if let Some(d) = text(&s, "deploymentId")
        && obj(doc, "deployments").is_none_or(|ds| !ds.contains_key(d))
    {
        return Err(invalid_id("Deployment"));
    }
    let cache = s.get("cacheClusterEnabled").and_then(Value::as_bool) == Some(true);
    put(
        &mut s,
        "cacheClusterStatus",
        json!(if cache { "AVAILABLE" } else { "NOT_AVAILABLE" }),
    );
    if cache && s.get("cacheClusterSize").is_none() {
        put(&mut s, "cacheClusterSize", json!("0.5"));
    }
    put(&mut s, "lastUpdatedDate", json!(r.now_secs()));
    if let Some(stages) = child(doc, "stages") {
        stages.insert(name.to_owned(), s.clone());
    }
    Ok(public(&s).into())
}

/// The setting names of a method-setting path and where each goes, with its kind.
const SETTINGS: &[(&str, &str, Kind)] = &[
    ("metrics/enabled", "metricsEnabled", Kind::Bool),
    ("logging/loglevel", "loggingLevel", Kind::Str),
    ("logging/dataTrace", "dataTraceEnabled", Kind::Bool),
    ("throttling/burstLimit", "throttlingBurstLimit", Kind::Int),
    ("throttling/rateLimit", "throttlingRateLimit", Kind::Num),
    ("caching/enabled", "cachingEnabled", Kind::Bool),
    ("caching/ttlInSeconds", "cacheTtlInSeconds", Kind::Int),
    ("caching/dataEncrypted", "cacheDataEncrypted", Kind::Bool),
    (
        "caching/requireAuthorizationForCacheControl",
        "requireAuthorizationForCacheControl",
        Kind::Bool,
    ),
    (
        "caching/unauthorizedCacheControlHeaderStrategy",
        "unauthorizedCacheControlHeaderStrategy",
        Kind::Str,
    ),
];

/// A `/{resource path}/{method}/{group}/{setting}` patch (`/*/*/logging/loglevel`), applied to `methodSettings` under the key
/// `{path}/{method}` (`*/*`, `orders/GET`); a new key starts from AWS's defaults `(memory)`.
fn method_setting(stage: &mut Value, p: &Patch) -> Result<(), ApiError> {
    let segs: Vec<&str> = p.path.trim_start_matches('/').split('/').collect();
    let invalid = || crate::patch::invalid_path(p, STAGE_FIELDS);
    let (res, method, rest) = match segs.as_slice() {
        [res, method, rest @ ..] => (*res, *method, rest.join("/")),
        _ => return Err(invalid()),
    };
    let res = unescape(res);
    let key = format!("{}/{method}", res.trim_start_matches('/'));
    let settings = child(stage, "methodSettings").ok_or_else(invalid)?;
    if rest.is_empty() && p.op == "remove" {
        settings.remove(&key);
        return Ok(());
    }
    let (_, member, kind) = SETTINGS
        .iter()
        .find(|(path, _, _)| *path == rest)
        .ok_or_else(invalid)?;
    let entry = settings.entry(key).or_insert_with(defaults);
    let mut one = json!({});
    let field = [f("/v", *kind)];
    let patch = Patch {
        path: "/v".into(),
        ..p.clone()
    };
    apply(&mut one, &[patch], &field)?;
    match one.get("v") {
        Some(v) => put(entry, member, v.clone()),
        None => {
            if let Some(o) = entry.as_object_mut() {
                o.remove(*member);
            }
        }
    }
    Ok(())
}

fn defaults() -> Value {
    json!({
        "metricsEnabled": false,
        "loggingLevel": "OFF",
        "dataTraceEnabled": false,
        "throttlingBurstLimit": 5000,
        "throttlingRateLimit": 10000.0,
        "cachingEnabled": false,
        "cacheTtlInSeconds": 300,
        "cacheDataEncrypted": false,
        "requireAuthorizationForCacheControl": true,
        "unauthorizedCacheControlHeaderStrategy": "SUCCEED_WITH_RESPONSE_HEADER",
    })
}

fn delete_stage(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let name = r.s("stageName");
    stage(doc, name)?;
    if let Some(stages) = child(doc, "stages") {
        stages.remove(name);
    }
    none()
}
