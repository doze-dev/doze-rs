//! The resource tree and the methods of an imported REST API: each OpenAPI path becomes resources (made with their parents), each
//! operation a method with its parameters, models, responses, authorization and `x-amazon-apigateway-integration`.

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use super::{methods, upper};
use crate::doc::{child, obj, text};
use crate::errors::bad;
use crate::req::new_id;

pub(super) struct Ctx<'a> {
    pub(super) spec: &'a Value,
    pub(super) validators: &'a Map<String, Value>,
    pub(super) authorizers: &'a Map<String, Value>,
    pub(super) default_validator: Option<Value>,
    pub(super) warnings: &'a mut Vec<String>,
}

pub(super) fn paths(doc: &mut Value, root: &str, mut ctx: Ctx<'_>) -> Result<(), ApiError> {
    let Some(paths) = ctx.spec.get("paths").and_then(Value::as_object) else {
        return Ok(());
    };
    for (path, item) in paths {
        let rid = ensure_resource(doc, root, path)?;
        let shared = item.get("parameters");
        for (verb, op) in methods(item) {
            let method = method(&rid, verb, op, shared, &mut ctx);
            let ptr = format!("/resources/{rid}");
            if let Some(res) = doc.pointer_mut(&ptr)
                && let Some(ms) = child(res, "resourceMethods")
            {
                ms.insert(verb.to_owned(), method);
            }
        }
    }
    Ok(())
}

/// The id of the resource at `path`, made with its parents where missing.
fn ensure_resource(doc: &mut Value, root: &str, path: &str) -> Result<String, ApiError> {
    let mut parent = root.to_owned();
    let mut so_far = String::new();
    for part in path.split('/').filter(|s| !s.is_empty()) {
        if !crate::v1::valid_part(part) {
            return Err(bad(format!(
                "Invalid OpenAPI input: path part {part} of {path} is not valid"
            )));
        }
        so_far.push('/');
        so_far.push_str(part);
        let found = obj(doc, "resources")
            .and_then(|m| {
                m.values().find(|r| {
                    text(r, "parentId") == Some(parent.as_str())
                        && text(r, "pathPart") == Some(part)
                })
            })
            .and_then(|r| text(r, "id"))
            .map(str::to_owned);
        parent = match found {
            Some(id) => id,
            None => {
                let id = new_id(6);
                if let Some(m) = child(doc, "resources") {
                    m.insert(
                        id.clone(),
                        json!({"id": id, "parentId": parent, "pathPart": part, "path": so_far}),
                    );
                }
                id
            }
        };
    }
    Ok(parent)
}

fn method(rid: &str, verb: &str, op: &Value, shared: Option<&Value>, ctx: &mut Ctx<'_>) -> Value {
    let mut m = Map::new();
    m.insert("httpMethod".into(), json!(verb));
    m.insert("authorizationType".into(), json!("NONE"));
    m.insert("apiKeyRequired".into(), json!(false));
    if let Some(id) = op.get("operationId") {
        m.insert("operationName".into(), id.clone());
    }
    let security = op.get("security").or_else(|| ctx.spec.get("security"));
    for name in security
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .flat_map(|o| o.keys())
    {
        match ctx.authorizers.get(name) {
            Some(a) if text(a, "kind") == Some("API_KEY") => {
                m.insert("apiKeyRequired".into(), json!(true));
            }
            Some(a) => {
                m.insert(
                    "authorizationType".into(),
                    a.get("kind").cloned().unwrap_or(Value::Null),
                );
                if let Some(id) = a.get("id") {
                    m.insert("authorizerId".into(), id.clone());
                }
            }
            None => ctx.warnings.push(format!(
                "Security {name} of {verb} names no security scheme"
            )),
        }
    }
    let validator = text(op, "x-amazon-apigateway-request-validator")
        .and_then(|n| ctx.validators.get(n).cloned())
        .or_else(|| ctx.default_validator.clone());
    if let Some(v) = validator {
        m.insert("requestValidatorId".into(), v);
    }
    request_side(&mut m, op, shared);
    m.insert("methodResponses".into(), method_responses(op));
    match op.get("x-amazon-apigateway-integration") {
        Some(i) => {
            m.insert("methodIntegration".into(), integration(rid, i));
        }
        None => ctx
            .warnings
            .push(format!("No integration defined for method {verb}")),
    }
    Value::Object(m)
}

/// A method's `requestParameters` (from the path's and the operation's `parameters`) and `requestModels` (from `requestBody`).
fn request_side(m: &mut Map<String, Value>, op: &Value, shared: Option<&Value>) {
    let mut params = Map::new();
    let listed = |v: Option<&Value>| v.and_then(Value::as_array).cloned().unwrap_or_default();
    for p in listed(shared)
        .iter()
        .chain(listed(op.get("parameters")).iter())
    {
        let (Some(name), Some(place)) = (text(p, "name"), text(p, "in")) else {
            continue;
        };
        let place = match place {
            "query" => "querystring",
            "header" => "header",
            "path" => "path",
            _ => continue,
        };
        let required = p.get("required").and_then(Value::as_bool).unwrap_or(false);
        params.insert(format!("method.request.{place}.{name}"), json!(required));
    }
    if !params.is_empty() {
        m.insert("requestParameters".into(), Value::Object(params));
    }
    let mut models = Map::new();
    for (ct, c) in op
        .pointer("/requestBody/content")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        if let Some(r) = c.get("schema").and_then(|s| text(s, "$ref")) {
            models.insert(ct.clone(), json!(r.rsplit('/').next().unwrap_or(r)));
        }
    }
    if !models.is_empty() {
        m.insert("requestModels".into(), Value::Object(models));
    }
}

fn method_responses(op: &Value) -> Value {
    let mut out = Map::new();
    for (code, resp) in op
        .get("responses")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        if code.parse::<u16>().is_err() {
            continue;
        }
        let mut r = Map::new();
        r.insert("statusCode".into(), json!(code));
        let headers: Map<String, Value> = resp
            .get("headers")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|(h, _)| (format!("method.response.header.{h}"), json!(false)))
            .collect();
        if !headers.is_empty() {
            r.insert("responseParameters".into(), Value::Object(headers));
        }
        let schema = resp
            .pointer("/content/application~1json/schema")
            .or_else(|| resp.get("schema"));
        if let Some(n) = schema
            .and_then(|s| text(s, "$ref"))
            .map(|r| r.rsplit('/').next().unwrap_or(r))
        {
            r.insert("responseModels".into(), json!({"application/json": n}));
        }
        out.insert(code.clone(), Value::Object(r));
    }
    Value::Object(out)
}

/// `x-amazon-apigateway-integration` as an integration record.
fn integration(rid: &str, i: &Value) -> Value {
    let mut out = Map::new();
    out.insert(
        "type".into(),
        json!(upper(i.get("type")).unwrap_or_else(|| "MOCK".into())),
    );
    for k in [
        "uri",
        "httpMethod",
        "credentials",
        "requestParameters",
        "requestTemplates",
        "connectionId",
        "cacheNamespace",
        "cacheKeyParameters",
        "tlsConfig",
    ] {
        if let Some(v) = i.get(k) {
            out.insert(k.into(), v.clone());
        }
    }
    for k in ["contentHandling", "connectionType"] {
        if let Some(v) = upper(i.get(k)) {
            out.insert(k.into(), json!(v));
        }
    }
    out.insert(
        "passthroughBehavior".into(),
        json!(upper(i.get("passthroughBehavior")).unwrap_or_else(|| "WHEN_NO_MATCH".into())),
    );
    out.insert(
        "timeoutInMillis".into(),
        i.get("timeoutInMillis").cloned().unwrap_or(json!(29_000)),
    );
    out.entry("cacheNamespace").or_insert_with(|| json!(rid));
    out.entry("cacheKeyParameters").or_insert_with(|| json!([]));
    let mut responses = Map::new();
    for (pattern, r) in i
        .get("responses")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let mut ir = r.as_object().cloned().unwrap_or_default();
        let code = ir
            .get("statusCode")
            .map(crate::doc::plain)
            .unwrap_or_else(|| "200".into());
        ir.insert("statusCode".into(), json!(code));
        if pattern != "default" {
            ir.insert("selectionPattern".into(), json!(pattern));
        }
        responses.insert(code, Value::Object(ir));
    }
    out.insert("integrationResponses".into(), Value::Object(responses));
    Value::Object(out)
}
