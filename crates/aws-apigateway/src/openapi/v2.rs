//! An HTTP API from an OpenAPI 3.0 document: `ImportApi` makes one, `ReimportApi` replaces an API's routes, integrations and
//! authorizers. Each operation is a route (`GET /pets/{id}`, `x-amazon-apigateway-any-method` as `ANY`, the path `$default` as the
//! `$default` route) with its own integration from `x-amazon-apigateway-integration`; authorizers come from the security schemes'
//! `x-amazon-apigateway-authorizer` (`jwt`, `request`); `x-amazon-apigateway-cors` is the CORS configuration.

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use super::{body_of, methods, schemes, upper};
use crate::core::Core;
use crate::doc::{Doc, put, text};
use crate::errors::{bad, no_api};
use crate::req::{Req, Res, new_id};

pub(crate) fn import(core: &mut Core, r: &Req) -> Res {
    let spec = body_of(r, false)?;
    let mut req = Req {
        body: json!({"name": spec.pointer("/info/title").and_then(Value::as_str).unwrap_or("imported"), "protocolType": "HTTP"}),
        caller: r.caller.clone(),
        now_ms: r.now_ms,
    };
    if let Some(d) = spec.pointer("/info/description") {
        put(&mut req.body, "description", d.clone());
    }
    if let Some(v) = spec.pointer("/info/version") {
        put(&mut req.body, "version", v.clone());
    }
    let (id, mut doc) = crate::v2::new_api(&req, "HTTP")?;
    let warnings = apply(&mut doc, &spec, r)?;
    let mut shown = doc.get("api").cloned().unwrap_or(Value::Null);
    if !warnings.is_empty() {
        put(&mut shown, "warnings", json!(warnings));
    }
    core.http.insert(id, Doc(doc));
    Ok(shown.into())
}

pub(crate) fn reimport(core: &mut Core, r: &Req) -> Res {
    let spec = body_of(r, false)?;
    let id = r.s("ApiId");
    let doc = core
        .http
        .get_mut(id)
        .map(|d| &mut d.0)
        .ok_or_else(|| no_api(r.account(), id))?;
    for part in ["routes", "integrations", "authorizers"] {
        put(doc, part, json!({}));
    }
    if let (Some(a), Some(t)) = (doc.get_mut("api"), spec.pointer("/info/title")) {
        put(a, "name", t.clone());
    }
    let warnings = apply(doc, &spec, r)?;
    let mut shown = doc.get("api").cloned().unwrap_or(Value::Null);
    if !warnings.is_empty() {
        put(&mut shown, "warnings", json!(warnings));
    }
    Ok(shown.into())
}

pub(crate) fn export(core: &mut Core, r: &Req) -> Res {
    super::export::v2(core, r)
}

fn apply(doc: &mut Value, spec: &Value, r: &Req) -> Result<Vec<String>, ApiError> {
    let mut warnings = Vec::new();
    if let (Some(cors), Some(a)) = (spec.get("x-amazon-apigateway-cors"), doc.get_mut("api")) {
        put(a, "corsConfiguration", cors.clone());
    }
    let authorizers = authorizers(doc, spec, &mut warnings);
    // `basepath` (ignore, prepend, split) matters for custom domain mappings, which serve nothing locally: the paths are taken as written.
    let mut routes = Map::new();
    let mut integrations = Map::new();
    for (path, item) in spec
        .get("paths")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        for (verb, op) in methods(item) {
            let key = if path == "$default" {
                "$default".to_owned()
            } else {
                format!("{verb} {path}")
            };
            let mut route = json!({"routeId": new_id(7), "routeKey": key, "apiKeyRequired": false, "authorizationType": "NONE"});
            if let Some(i) = op.get("x-amazon-apigateway-integration") {
                let iid = new_id(7);
                integrations.insert(iid.clone(), integration(&iid, i));
                put(&mut route, "target", json!(format!("integrations/{iid}")));
            } else {
                warnings.push(format!("Route {key} has no integration"));
            }
            secure(
                &mut route,
                op.get("security").or_else(|| spec.get("security")),
                &authorizers,
                &mut warnings,
            );
            if let Some(id) = op.get("operationId") {
                put(&mut route, "operationName", id.clone());
            }
            let rid = text(&route, "routeId").unwrap_or_default().to_owned();
            routes.insert(rid, route);
        }
    }
    put(doc, "routes", Value::Object(routes));
    put(doc, "integrations", Value::Object(integrations));
    if r.flag("FailOnWarnings") == Some(true) && !warnings.is_empty() {
        return Err(bad(format!(
            "Warnings found during import: {}",
            warnings.join("; ")
        )));
    }
    Ok(warnings)
}

fn integration(id: &str, i: &Value) -> Value {
    let kind = upper(i.get("type")).unwrap_or_else(|| "AWS_PROXY".into());
    let lambda = kind == "AWS_PROXY";
    let mut out = json!({
        "integrationId": id,
        "integrationType": kind,
        "connectionType": upper(i.get("connectionType")).unwrap_or_else(|| "INTERNET".into()),
        "timeoutInMillis": i.get("timeoutInMillis").cloned().unwrap_or(json!(30_000)),
        "payloadFormatVersion": i.get("payloadFormatVersion").cloned().unwrap_or(json!(if lambda { "2.0" } else { "1.0" })),
        "integrationMethod": i.get("httpMethod").cloned().unwrap_or(json!(if lambda { "POST" } else { "ANY" })),
    });
    for (from, to) in [
        ("uri", "integrationUri"),
        ("credentials", "credentialsArn"),
        ("requestParameters", "requestParameters"),
        ("integrationSubtype", "integrationSubtype"),
        ("connectionId", "connectionId"),
        ("tlsConfig", "tlsConfig"),
    ] {
        if let Some(v) = i.get(from) {
            put(&mut out, to, v.clone());
        }
    }
    out
}

fn authorizers(doc: &mut Value, spec: &Value, warnings: &mut Vec<String>) -> Map<String, Value> {
    let mut by_name = Map::new();
    let mut records = Map::new();
    for (name, scheme) in schemes(spec).into_iter().flatten() {
        let Some(a) = scheme.get("x-amazon-apigateway-authorizer") else {
            warnings.push(format!(
                "Security scheme {name} is not an API Gateway authorizer and was ignored"
            ));
            continue;
        };
        let kind = upper(a.get("type")).unwrap_or_else(|| "REQUEST".into());
        let id = new_id(6);
        let sources: Vec<Value> = match a.get("identitySource") {
            Some(Value::String(s)) => s.split(',').map(|x| json!(x.trim())).collect(),
            Some(Value::Array(list)) => list.clone(),
            _ if kind == "JWT" => vec![json!("$request.header.Authorization")],
            _ => Vec::new(),
        };
        let mut rec = json!({"authorizerId": id, "name": name, "authorizerType": kind, "identitySource": sources});
        for k in [
            "jwtConfiguration",
            "authorizerUri",
            "authorizerPayloadFormatVersion",
            "enableSimpleResponses",
            "authorizerResultTtlInSeconds",
            "authorizerCredentialsArn",
        ] {
            if let Some(v) = a.get(k) {
                put(&mut rec, k, v.clone());
            }
        }
        records.insert(id.clone(), rec);
        by_name.insert(
            name.clone(),
            json!({"id": id, "kind": if kind == "JWT" { "JWT" } else { "CUSTOM" }}),
        );
    }
    put(doc, "authorizers", Value::Object(records));
    by_name
}

fn secure(
    route: &mut Value,
    security: Option<&Value>,
    authorizers: &Map<String, Value>,
    warnings: &mut Vec<String>,
) {
    for entry in security
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
    {
        for (name, scopes) in entry {
            match authorizers.get(name) {
                Some(a) => {
                    put(
                        route,
                        "authorizationType",
                        a.get("kind").cloned().unwrap_or(Value::Null),
                    );
                    put(
                        route,
                        "authorizerId",
                        a.get("id").cloned().unwrap_or(Value::Null),
                    );
                    if scopes.as_array().is_some_and(|s| !s.is_empty()) {
                        put(route, "authorizationScopes", scopes.clone());
                    }
                }
                None => warnings.push(format!("Security {name} names no security scheme")),
            }
        }
    }
}
