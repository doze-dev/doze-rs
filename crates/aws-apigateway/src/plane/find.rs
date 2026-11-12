//! Finding what an execute-api request reaches, inside the actor: the API by id, the stage, and the method (REST) or route (HTTP,
//! WebSocket) by AWS's precedence. REST stages serve their deployment's snapshot; an HTTP API's auto-deploying stage serves the live
//! API, another stage its deployment's snapshot. What is found is copied out (the request is served outside the actor).

use serde_json::{Map, Value};

use super::{Find, Found, Kind, Miss, Route};
use crate::core::Core;
use crate::doc::{obj, text};

impl Core {
    /// What `f` reaches.
    pub(crate) fn find(&self, f: &Find) -> Result<Found, Miss> {
        if let Some(doc) = self.rest.get(&f.api) {
            return find_rest(&doc.0, f);
        }
        if let Some(doc) = self.http.get(&f.api) {
            return find_http(&doc.0, f);
        }
        Err(Miss::NoApi)
    }
}

fn base(
    doc: &Value,
    kind: Kind,
    api: &Value,
    stage: &Value,
    stage_name: &str,
    path: String,
) -> Found {
    Found {
        kind,
        account: text(doc, "account").unwrap_or("000000000000").to_owned(),
        region: text(doc, "region").unwrap_or("us-east-1").to_owned(),
        api: api.clone(),
        stage: stage.clone(),
        stage_name: stage_name.to_owned(),
        path,
        route: None,
        responses: Map::new(),
        models: Map::new(),
    }
}

/// The segments of the request after the API id, decoded.
fn segments(raw: &str) -> Vec<String> {
    raw.split('/')
        .filter(|s| !s.is_empty())
        .map(|s| {
            aws_core::uri::decode_str(s)
                .map(|c| c.into_owned())
                .unwrap_or_else(|| s.to_owned())
        })
        .collect()
}

fn find_rest(doc: &Value, f: &Find) -> Result<Found, Miss> {
    let segs = segments(&f.rest);
    let (stage_name, path) = if f.stage_in_path {
        let Some((first, rest)) = segs.split_first() else {
            return Err(Miss::NoStage(Kind::Rest));
        };
        (first.clone(), rest.to_vec())
    } else {
        return Err(Miss::NoStage(Kind::Rest));
    };
    let stage = obj(doc, "stages")
        .and_then(|s| s.get(&stage_name))
        .ok_or(Miss::NoStage(Kind::Rest))?;
    let deployment = text(stage, "deploymentId").unwrap_or_default();
    let snap = doc
        .pointer(&format!("/deployments/{deployment}/_snapshot"))
        .ok_or(Miss::NoStage(Kind::Rest))?;
    let api = snap.get("api").cloned().unwrap_or(Value::Null);
    let mut found = base(
        doc,
        Kind::Rest,
        &api,
        stage,
        &stage_name,
        format!("/{}", path.join("/")),
    );
    found.responses = obj(snap, "responses").cloned().unwrap_or_default();
    found.models = obj(snap, "models").cloned().unwrap_or_default();
    found.route = super::route::rest(snap, &path, &f.method);
    if let Some(route) = found.route.as_mut() {
        route.authorizer = text(&route.method, "authorizerId")
            .and_then(|id| snap.pointer(&format!("/authorizers/{id}")))
            .cloned();
        route.validator = text(&route.method, "requestValidatorId")
            .and_then(|id| snap.pointer(&format!("/validators/{id}")))
            .cloned();
    }
    Ok(found)
}

fn find_http(doc: &Value, f: &Find) -> Result<Found, Miss> {
    let api = doc.get("api").cloned().unwrap_or(Value::Null);
    let ws = text(&api, "protocolType") == Some("WEBSOCKET");
    let kind = if ws { Kind::WebSocket } else { Kind::Http };
    let segs = segments(&f.rest);
    let stages = obj(doc, "stages");
    let named = segs
        .first()
        .filter(|s| f.stage_in_path && stages.is_some_and(|m| m.contains_key(s.as_str())));
    let (stage_name, path) = match named {
        Some(first) => (first.clone(), segs.get(1..).unwrap_or_default().to_vec()),
        None if !ws => ("$default".to_owned(), segs),
        None => return Err(Miss::NoStage(kind)),
    };
    let stage = stages
        .and_then(|s| s.get(&stage_name))
        .ok_or(Miss::NoStage(kind))?;
    let live = stage.get("autoDeploy").and_then(Value::as_bool) == Some(true);
    let config = if live {
        doc
    } else {
        let deployment = text(stage, "deploymentId").unwrap_or_default();
        doc.pointer(&format!("/deployments/{deployment}/_snapshot"))
            .ok_or(Miss::NoStage(kind))?
    };
    let mut found = base(
        doc,
        kind,
        &api,
        stage,
        &stage_name,
        format!("/{}", path.join("/")),
    );
    found.route = if ws {
        f.route_key
            .as_deref()
            .and_then(|k| super::route::websocket(config, k))
    } else {
        super::route::http(config, &path, &f.method)
    };
    if let Some(route) = found.route.as_mut() {
        route.integration = text(&route.method, "target")
            .and_then(|t| t.strip_prefix("integrations/"))
            .and_then(|id| config.pointer(&format!("/integrations/{id}")))
            .cloned()
            .unwrap_or(Value::Null);
        route.authorizer = text(&route.method, "authorizerId")
            .and_then(|id| config.pointer(&format!("/authorizers/{id}")))
            .cloned();
        route.responses = config
            .pointer(&format!("/routeResponses/{}", route.resource_id))
            .cloned();
        route.integration_responses = text(&route.integration, "integrationId")
            .and_then(|id| config.pointer(&format!("/integrationResponses/{id}")))
            .cloned();
    }
    Ok(found)
}
