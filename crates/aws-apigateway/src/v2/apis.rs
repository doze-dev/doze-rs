//! HTTP and WebSocket APIs themselves: create (with the quick create of `target`), read, update, delete, CORS, tags, import and
//! export, and the domain-name operations (shared with v1).

use aws_model_apigatewayv2::Operation;
use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use super::{api, api_mut, members, page};
use crate::core::Core;
use crate::doc::{Doc, put, text};
use crate::errors::bad;
use crate::req::{Req, Res, new_id, none};

pub(super) fn dispatch(core: &mut Core, op: Operation, r: &Req) -> Option<Res> {
    Some(match op {
        Operation::CreateApi => create(core, r),
        Operation::GetApi => api(core, r).map(|d| view(d).into()),
        Operation::GetApis => {
            Ok(page(core.http.iter().map(|(_, d)| view(&d.0)).collect(), r).into())
        }
        Operation::UpdateApi => update(core, r),
        Operation::DeleteApi => api(core, r).map(|_| ()).map(|()| {
            core.http.remove(r.s("ApiId"));
            Default::default()
        }),
        Operation::DeleteCorsConfiguration => api_mut(core, r).and_then(|d| {
            if let Some(a) = d.get_mut("api").and_then(Value::as_object_mut) {
                a.remove("corsConfiguration");
            }
            none()
        }),
        Operation::ImportApi => crate::openapi::v2::import(core, r),
        Operation::ReimportApi => crate::openapi::v2::reimport(core, r),
        Operation::ExportApi => crate::openapi::v2::export(core, r),
        Operation::GetTags => {
            crate::tags::get(core, r.s("ResourceArn")).map(|t| json!({"tags": t}).into())
        }
        Operation::TagResource => {
            crate::tags::tag(core, r.s("ResourceArn"), r.value("tags")).and_then(|()| none())
        }
        Operation::UntagResource => {
            crate::tags::untag(core, r.s("ResourceArn"), r.value("TagKeys")).and_then(|()| none())
        }
        Operation::CreateDomainName
        | Operation::GetDomainName
        | Operation::GetDomainNames
        | Operation::UpdateDomainName
        | Operation::DeleteDomainName
        | Operation::CreateApiMapping
        | Operation::GetApiMapping
        | Operation::GetApiMappings
        | Operation::UpdateApiMapping
        | Operation::DeleteApiMapping => crate::domains::v2(core, op, r),
        _ => return None,
    })
}

/// What `GetApi` shows.
pub(crate) fn view(doc: &Value) -> Value {
    doc.get("api").cloned().unwrap_or(Value::Null)
}

/// The address an API answers at locally (`ws://` for a WebSocket API).
fn endpoint(r: &Req, id: &str, websocket: bool) -> String {
    let base = if websocket {
        r.caller.endpoint.replacen("http://", "ws://", 1)
    } else {
        r.caller.endpoint.clone()
    };
    format!("{base}/_aws/execute-api/{id}")
}

/// A new API document from a `CreateApi`- or `ImportApi`-shaped request.
pub(crate) fn new_api(r: &Req, protocol: &str) -> Result<(String, Value), ApiError> {
    let websocket = protocol == "WEBSOCKET";
    if websocket && r.opt("routeSelectionExpression").is_none_or(str::is_empty) {
        return Err(bad(
            "RouteSelectionExpression is required for a WEBSOCKET API",
        ));
    }
    let id = new_id(10);
    let mut a = members(r, &["target", "routeKey", "credentialsArn"]);
    a.insert("apiId".into(), json!(id));
    a.insert("apiEndpoint".into(), json!(endpoint(r, &id, websocket)));
    a.insert("createdDate".into(), json!(r.now_iso()));
    a.entry("apiKeySelectionExpression")
        .or_insert_with(|| json!("$request.header.x-api-key"));
    a.entry("disableExecuteApiEndpoint")
        .or_insert_with(|| json!(false));
    a.entry("ipAddressType").or_insert_with(|| json!("ipv4"));
    a.entry("routeSelectionExpression")
        .or_insert_with(|| json!("$request.method $request.path"));
    a.insert("protocolType".into(), json!(protocol));
    let doc = json!({"api": a, "account": r.account(), "region": r.region()});
    Ok((id, doc))
}

fn create(core: &mut Core, r: &Req) -> Res {
    let (id, mut doc) = new_api(r, r.s("protocolType"))?;
    if let Some(target) = r.opt("target") {
        quick_create(&mut doc, r, target)?;
    }
    let shown = view(&doc);
    core.http.insert(id, Doc(doc));
    Ok(shown.into())
}

/// `CreateApi` with `target`: an integration to it, a route to that (`routeKey`, or `$default`), and an auto-deploying `$default` stage.
fn quick_create(doc: &mut Value, r: &Req, target: &str) -> Result<(), ApiError> {
    let integration = new_id(7);
    let route = new_id(7);
    let lambda = target.starts_with("arn:");
    let mut i = Map::new();
    i.insert("integrationId".into(), json!(integration));
    i.insert("connectionType".into(), json!("INTERNET"));
    i.insert(
        "integrationType".into(),
        json!(if lambda { "AWS_PROXY" } else { "HTTP_PROXY" }),
    );
    i.insert("integrationUri".into(), json!(target));
    i.insert(
        "integrationMethod".into(),
        json!(if lambda { "POST" } else { "ANY" }),
    );
    i.insert(
        "payloadFormatVersion".into(),
        json!(if lambda { "2.0" } else { "1.0" }),
    );
    i.insert("timeoutInMillis".into(), json!(30_000));
    i.insert("apiGatewayManaged".into(), json!(true));
    if let Some(c) = r.opt("credentialsArn") {
        i.insert("credentialsArn".into(), json!(c));
    }
    let key = r.opt("routeKey").unwrap_or("$default");
    let rt = json!({"routeId": route, "routeKey": key, "target": format!("integrations/{integration}"),
        "apiKeyRequired": false, "authorizationType": "NONE", "apiGatewayManaged": true});
    let stage = json!({"stageName": "$default", "autoDeploy": true, "createdDate": r.now_iso(), "lastUpdatedDate": r.now_iso(),
        "defaultRouteSettings": {"detailedMetricsEnabled": false}, "routeSettings": {}, "stageVariables": {}, "tags": {},
        "apiGatewayManaged": true});
    put(doc, "integrations", json!({integration: i}));
    put(doc, "routes", json!({route: rt}));
    put(doc, "stages", json!({"$default": stage}));
    Ok(())
}

fn update(core: &mut Core, r: &Req) -> Res {
    let doc = api_mut(core, r)?;
    let changes = members(r, &["target", "routeKey", "credentialsArn"]);
    let a = doc
        .get_mut("api")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| bad("the API is damaged"))?;
    if text(&Value::Object(changes.clone()), "protocolType").is_some() {
        return Err(bad("The protocol of an API cannot be changed"));
    }
    a.extend(changes);
    Ok(Value::Object(a.clone()).into())
}
