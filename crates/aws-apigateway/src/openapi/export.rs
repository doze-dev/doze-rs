//! Export: a REST stage as OpenAPI 3.0 (`oas30`) or Swagger 2.0 (`swagger`), JSON or YAML, with API Gateway's extensions when asked
//! (`extensions=integrations,authorizers,apigateway`); an HTTP API as OpenAPI 3.0 (`OAS30`). The document is what an import of it
//! would read back. Field order and the `description` texts are `(memory)`.

use serde_json::{Map, Value, json};

use crate::core::Core;
use crate::doc::{obj, text};
use crate::errors::{bad, invalid_id, no_api};
use crate::req::{Req, Res};

fn lower(v: Option<&Value>) -> Option<Value> {
    v.and_then(Value::as_str)
        .map(|s| json!(s.to_ascii_lowercase()))
}

fn verb_key(m: &str) -> String {
    if m == "ANY" {
        "x-amazon-apigateway-any-method".into()
    } else {
        m.to_ascii_lowercase()
    }
}

/// A REST integration as `x-amazon-apigateway-integration`.
fn integration_ext(i: &Value) -> Value {
    let mut e = Map::new();
    for (k, v) in i.as_object().into_iter().flatten() {
        match k.as_str() {
            "type" | "passthroughBehavior" | "contentHandling" | "connectionType" => {
                e.insert(k.clone(), lower(Some(v)).unwrap_or(Value::Null));
            }
            "integrationResponses" => {
                let rs: Map<String, Value> = v
                    .as_object()
                    .into_iter()
                    .flatten()
                    .map(|(_, r)| {
                        let pattern = text(r, "selectionPattern").unwrap_or("default").to_owned();
                        let mut r = r.as_object().cloned().unwrap_or_default();
                        r.remove("selectionPattern");
                        (pattern, Value::Object(r))
                    })
                    .collect();
                e.insert("responses".into(), Value::Object(rs));
            }
            "cacheNamespace" | "cacheKeyParameters" => {}
            _ => {
                e.insert(k.clone(), v.clone());
            }
        }
    }
    Value::Object(e)
}

fn operation(m: &Value, ext: bool, oas3: bool, authorizers: &Map<String, Value>) -> Value {
    let mut op = Map::new();
    if let Some(n) = m.get("operationName") {
        op.insert("operationId".into(), n.clone());
    }
    let params: Vec<Value> = obj(m, "requestParameters")
        .into_iter()
        .flatten()
        .filter_map(|(k, req)| {
            let rest = k.strip_prefix("method.request.")?;
            let (place, name) = rest.split_once('.')?;
            let place = if place == "querystring" {
                "query"
            } else {
                place
            };
            let mut p =
                json!({"name": name, "in": place, "required": req.as_bool().unwrap_or(false)});
            crate::doc::put(
                &mut p,
                if oas3 { "schema" } else { "type" },
                if oas3 {
                    json!({"type": "string"})
                } else {
                    json!("string")
                },
            );
            Some(p)
        })
        .collect();
    if !params.is_empty() {
        op.insert("parameters".into(), Value::Array(params));
    }
    let mut responses = Map::new();
    for (code, r) in obj(m, "methodResponses").into_iter().flatten() {
        let mut out = json!({"description": format!("{code} response")});
        if let Some(model) = r
            .pointer("/responseModels/application~1json")
            .and_then(Value::as_str)
        {
            let reference = if oas3 {
                format!("#/components/schemas/{model}")
            } else {
                format!("#/definitions/{model}")
            };
            if oas3 {
                crate::doc::put(
                    &mut out,
                    "content",
                    json!({"application/json": {"schema": {"$ref": reference}}}),
                );
            } else {
                crate::doc::put(&mut out, "schema", json!({"$ref": reference}));
            }
        }
        responses.insert(code.clone(), out);
    }
    op.insert("responses".into(), Value::Object(responses));
    let kind = text(m, "authorizationType").unwrap_or("NONE");
    let mut security = Vec::new();
    if let Some(name) = text(m, "authorizerId")
        .and_then(|id| authorizers.get(id))
        .and_then(|a| text(a, "name"))
    {
        security.push(json!({name: []}));
    } else if kind == "AWS_IAM" {
        security.push(json!({"sigv4": []}));
    }
    if m.get("apiKeyRequired").and_then(Value::as_bool) == Some(true) {
        security.push(json!({"api_key": []}));
    }
    if !security.is_empty() {
        op.insert("security".into(), Value::Array(security));
    }
    if ext && let Some(i) = m.get("methodIntegration") {
        op.insert("x-amazon-apigateway-integration".into(), integration_ext(i));
    }
    Value::Object(op)
}

fn schemes(authorizers: &Map<String, Value>, ext: bool) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert(
        "api_key".into(),
        json!({"type": "apiKey", "name": "x-api-key", "in": "header"}),
    );
    out.insert("sigv4".into(), json!({"type": "apiKey", "name": "Authorization", "in": "header", "x-amazon-apigateway-authtype": "awsSigv4"}));
    for a in authorizers.values() {
        let mut s = json!({"type": "apiKey", "name": "Authorization", "in": "header", "x-amazon-apigateway-authtype": "custom"});
        if ext {
            let mut auth = a.as_object().cloned().unwrap_or_default();
            auth.remove("id");
            auth.remove("name");
            if let Some(t) = lower(auth.get("type")) {
                auth.insert("type".into(), t);
            }
            crate::doc::put(
                &mut s,
                "x-amazon-apigateway-authorizer",
                Value::Object(auth),
            );
        }
        out.insert(text(a, "name").unwrap_or_default().to_owned(), s);
    }
    out
}

/// `GetExport`.
pub(crate) fn v1(core: &mut Core, r: &Req) -> Res {
    let id = r.s("restApiId");
    let doc = core
        .rest
        .get(id)
        .map(|d| &d.0)
        .ok_or_else(|| no_api(r.account(), id))?;
    let stage = r.s("stageName");
    let deployment = doc
        .pointer(&format!("/stages/{stage}/deploymentId"))
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_id("stage"))?;
    let snap = doc
        .pointer(&format!("/deployments/{deployment}/_snapshot"))
        .ok_or_else(|| invalid_id("Deployment"))?;
    let oas3 = match r.s("exportType") {
        "oas30" => true,
        "swagger" => false,
        other => {
            return Err(bad(format!(
                "Unsupported export type: {other}. Valid values are [oas30, swagger]"
            )));
        }
    };
    let wanted = r
        .value("parameters")
        .and_then(|p| p.get("extensions"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let ext = wanted.contains("integrations") || wanted.contains("apigateway");
    let api = snap.get("api").cloned().unwrap_or(Value::Null);
    let authorizers = obj(snap, "authorizers").cloned().unwrap_or_default();
    let mut paths = Map::new();
    for res in obj(snap, "resources").into_iter().flatten().map(|(_, v)| v) {
        let Some(methods) = obj(res, "resourceMethods") else {
            continue;
        };
        let item: Map<String, Value> = methods
            .iter()
            .map(|(verb, m)| (verb_key(verb), operation(m, ext, oas3, &authorizers)))
            .collect();
        paths.insert(
            text(res, "path").unwrap_or("/").to_owned(),
            Value::Object(item),
        );
    }
    let host = format!("{id}.execute-api.{}.amazonaws.com", r.region());
    let info = json!({"title": text(&api, "name"), "version": text(&api, "version").map_or_else(|| r.now_iso(), str::to_owned)});
    let schemas: Map<String, Value> = obj(snap, "models")
        .into_iter()
        .flatten()
        .filter_map(|(n, m)| {
            Some((
                n.clone(),
                serde_json::from_str::<Value>(text(m, "schema")?).ok()?,
            ))
        })
        .collect();
    let mut out = if oas3 {
        json!({"openapi": "3.0.1", "info": info, "servers": [{"url": format!("https://{host}/{{basePath}}"), "variables": {"basePath": {"default": stage}}}],
            "paths": paths, "components": {"schemas": schemas, "securitySchemes": schemes(&authorizers, ext)}})
    } else {
        json!({"swagger": "2.0", "info": info, "host": host, "basePath": format!("/{stage}"), "schemes": ["https"],
            "paths": paths, "securityDefinitions": schemes(&authorizers, ext), "definitions": schemas})
    };
    if ext && let Some(t) = api.get("binaryMediaTypes") {
        crate::doc::put(
            &mut out,
            "x-amazon-apigateway-binary-media-types",
            t.clone(),
        );
    }
    let yaml = r.opt("accepts").is_some_and(|a| a.contains("yaml"));
    Ok(payload(
        &out,
        yaml,
        &format!("{}_{stage}", if oas3 { "oas30" } else { "swagger" }),
    )
    .into())
}

fn payload(doc: &Value, yaml: bool, name: &str) -> Value {
    let (text, ct, suffix) = if yaml {
        (super::to_yaml(doc), "application/yaml", "yaml")
    } else {
        (
            serde_json::to_string_pretty(doc).unwrap_or_default(),
            "application/json",
            "json",
        )
    };
    json!({
        "contentType": ct,
        "contentDisposition": format!("attachment; filename=\"{name}.{suffix}\""),
        "body": aws_core::base64::encode(text.as_bytes()),
    })
}

/// `ExportApi` (v2).
pub(crate) fn v2(core: &mut Core, r: &Req) -> Res {
    let id = r.s("ApiId");
    let doc = core
        .http
        .get(id)
        .map(|d| &d.0)
        .ok_or_else(|| no_api(r.account(), id))?;
    if r.s("Specification") != "OAS30" {
        return Err(bad("Unsupported specification. Valid values are [OAS30]"));
    }
    let config = match r.opt("StageName") {
        Some(s) => {
            let dep = doc
                .pointer(&format!("/stages/{s}/deploymentId"))
                .and_then(Value::as_str);
            dep.and_then(|d| doc.pointer(&format!("/deployments/{d}/_snapshot")))
                .unwrap_or(doc)
        }
        None => doc,
    };
    let ext = r.flag("IncludeExtensions") != Some(false);
    let api = doc.get("api").cloned().unwrap_or(Value::Null);
    let mut paths: Map<String, Value> = Map::new();
    for route in obj(config, "routes").into_iter().flatten().map(|(_, v)| v) {
        let key = text(route, "routeKey").unwrap_or_default();
        let (verb, path) = key.split_once(' ').unwrap_or(("ANY", key));
        let mut op = json!({"responses": {"default": {"description": format!("Default response for {key}")}}});
        if ext
            && let Some(i) = text(route, "target")
                .and_then(|t| t.strip_prefix("integrations/"))
                .and_then(|i| config.pointer(&format!("/integrations/{i}")))
        {
            let mut e = json!({"payloadFormatVersion": i.get("payloadFormatVersion"), "type": lower(i.get("integrationType")),
                "httpMethod": i.get("integrationMethod"), "uri": i.get("integrationUri"), "connectionType": i.get("connectionType"),
                "timeoutInMillis": i.get("timeoutInMillis")});
            if let Some(o) = e.as_object_mut() {
                o.retain(|_, v| !v.is_null());
            }
            crate::doc::put(&mut op, "x-amazon-apigateway-integration", e);
        }
        if let Some(name) = text(route, "authorizerId")
            .and_then(|a| config.pointer(&format!("/authorizers/{a}")))
            .and_then(|a| text(a, "name"))
        {
            crate::doc::put(
                &mut op,
                "security",
                json!([{name: route.get("authorizationScopes").cloned().unwrap_or(json!([]))}]),
            );
        }
        let entry = paths.entry(path.to_owned()).or_insert_with(|| json!({}));
        crate::doc::put(entry, &verb_key(verb), op);
    }
    let mut out = json!({"openapi": "3.0.1", "info": {"title": text(&api, "name"), "version": text(&api, "version").map_or_else(|| r.now_iso(), str::to_owned)},
        "paths": paths, "x-amazon-apigateway-importexport-version": "1.0"});
    if ext && let Some(c) = api.get("corsConfiguration") {
        crate::doc::put(&mut out, "x-amazon-apigateway-cors", c.clone());
    }
    let yaml = r.s("OutputType") == "YAML";
    let text = if yaml {
        super::to_yaml(&out)
    } else {
        serde_json::to_string_pretty(&out).unwrap_or_default()
    };
    Ok(json!({"body": aws_core::base64::encode(text.as_bytes())}).into())
}
