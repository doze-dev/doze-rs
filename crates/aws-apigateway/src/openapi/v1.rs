//! A REST API from an OpenAPI document: `ImportRestApi` makes one, `PutRestApi` overwrites or merges into one. The resource tree is
//! made from `paths`; each method from its operation (`parameters`, `requestBody`, `responses`, `security`, `operationId`) and its
//! `x-amazon-apigateway-integration`; models from `definitions` / `components.schemas`; authorizers from the security schemes'
//! `x-amazon-apigateway-authorizer`; validators, gateway responses, binary media types, the key source and the policy from their
//! extensions.

use aws_wire::ApiError;
use serde_json::{Map, Value, json};

use super::{body_of, methods, schemes, upper};
use crate::core::Core;
use crate::doc::{Doc, child, obj, put, text};
use crate::errors::{bad, no_api};
use crate::req::{Req, Res, new_id};

pub(crate) fn import(core: &mut Core, r: &Req) -> Res {
    let spec = body_of(r, true)?;
    let name = spec
        .pointer("/info/title")
        .and_then(Value::as_str)
        .unwrap_or("imported");
    let (id, mut doc) = crate::v1::new_api(r, name);
    let warnings = apply(&mut doc, &spec, true, r)?;
    let mut shown = crate::v1::view(&doc);
    if !warnings.is_empty() {
        put(&mut shown, "warnings", json!(warnings));
    }
    core.rest.insert(id, Doc(doc));
    Ok(shown.into())
}

pub(crate) fn put_rest_api(core: &mut Core, r: &Req) -> Res {
    let spec = body_of(r, true)?;
    let id = r.s("restApiId");
    let overwrite = r.opt("mode") != Some("merge");
    let doc = core
        .rest
        .get_mut(id)
        .map(|d| &mut d.0)
        .ok_or_else(|| no_api(r.account(), id))?;
    let warnings = apply(doc, &spec, overwrite, r)?;
    let mut shown = crate::v1::view(doc);
    if !warnings.is_empty() {
        put(&mut shown, "warnings", json!(warnings));
    }
    Ok(shown.into())
}

pub(crate) fn export(core: &mut Core, r: &Req) -> Res {
    super::export::v1(core, r)
}

/// Applies a document to an API document. `overwrite` replaces the resource tree and the other parts; otherwise they are merged.
fn apply(doc: &mut Value, spec: &Value, overwrite: bool, r: &Req) -> Result<Vec<String>, ApiError> {
    let mut warnings = Vec::new();
    let api_id = doc
        .pointer("/api/id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let root = doc
        .pointer("/api/rootResourceId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    settings(doc, spec);
    if overwrite {
        put(
            doc,
            "resources",
            json!({root.clone(): {"id": root, "path": "/"}}),
        );
        for part in ["authorizers", "validators", "responses"] {
            put(doc, part, json!({}));
        }
    }
    models(doc, spec, &api_id);
    let validators = validators(doc, spec);
    let authorizers = authorizers(doc, spec, &mut warnings);
    if let Some(responses) = spec
        .get("x-amazon-apigateway-gateway-responses")
        .and_then(Value::as_object)
    {
        for (kind, v) in responses {
            let mut g = v.as_object().cloned().unwrap_or_default();
            g.insert("responseType".into(), json!(kind));
            g.insert("defaultResponse".into(), json!(false));
            if let Some(m) = child(doc, "responses") {
                m.insert(kind.clone(), Value::Object(g));
            }
        }
    }
    let default_validator = text(spec, "x-amazon-apigateway-request-validator")
        .and_then(|n| validators.get(n).cloned());
    let ctx = Ctx {
        spec,
        validators: &validators,
        authorizers: &authorizers,
        default_validator,
        warnings: &mut warnings,
    };
    paths(doc, &root, ctx)?;
    if r.flag("failOnWarnings") == Some(true) && !warnings.is_empty() {
        return Err(bad(format!(
            "Warnings found during import:\n\t{}",
            warnings.join("\n\t")
        )));
    }
    Ok(warnings)
}

fn settings(doc: &mut Value, spec: &Value) {
    let Some(api) = doc.get_mut("api").and_then(Value::as_object_mut) else {
        return;
    };
    if let Some(t) = spec.pointer("/info/title") {
        api.insert("name".into(), t.clone());
    }
    for (from, to) in [
        ("/info/description", "description"),
        ("/info/version", "version"),
    ] {
        if let Some(v) = spec.pointer(from) {
            api.insert(to.into(), v.clone());
        }
    }
    for (ext, to) in [
        ("x-amazon-apigateway-binary-media-types", "binaryMediaTypes"),
        (
            "x-amazon-apigateway-minimum-compression-size",
            "minimumCompressionSize",
        ),
    ] {
        if let Some(v) = spec.get(ext) {
            api.insert(to.into(), v.clone());
        }
    }
    if let Some(v) = upper(spec.get("x-amazon-apigateway-api-key-source")) {
        api.insert("apiKeySource".into(), json!(v));
    }
    if let Some(p) = spec.get("x-amazon-apigateway-policy") {
        api.insert(
            "policy".into(),
            json!(if p.is_string() {
                crate::doc::plain(p)
            } else {
                p.to_string()
            }),
        );
    }
}

/// `definitions` / `components.schemas` as models, their `$ref`s pointed at the API's own models as API Gateway writes them.
fn models(doc: &mut Value, spec: &Value, api_id: &str) {
    let Some(schemas) = spec
        .pointer("/components/schemas")
        .or_else(|| spec.get("definitions"))
        .and_then(Value::as_object)
    else {
        return;
    };
    for (name, schema) in schemas {
        let text = schema
            .to_string()
            .replace(
                "#/components/schemas/",
                &format!("https://apigateway.amazonaws.com/restapis/{api_id}/models/"),
            )
            .replace(
                "#/definitions/",
                &format!("https://apigateway.amazonaws.com/restapis/{api_id}/models/"),
            );
        let m = crate::v1::new_model(name, None, Some(&text), "application/json");
        if let Some(ms) = child(doc, "models") {
            ms.insert(name.clone(), m);
        }
    }
}

fn validators(doc: &mut Value, spec: &Value) -> Map<String, Value> {
    let mut ids = Map::new();
    for (name, v) in spec
        .get("x-amazon-apigateway-request-validators")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let id = new_id(6);
        let rec = json!({"id": id, "name": name,
            "validateRequestBody": v.get("validateRequestBody").and_then(Value::as_bool).unwrap_or(false),
            "validateRequestParameters": v.get("validateRequestParameters").and_then(Value::as_bool).unwrap_or(false)});
        if let Some(m) = child(doc, "validators") {
            m.insert(id.clone(), rec);
        }
        ids.insert(name.clone(), json!(id));
    }
    ids
}

/// What each security scheme means for a method: `{"kind": "CUSTOM"|"COGNITO_USER_POOLS"|"AWS_IAM"|"API_KEY", "id": authorizer}`.
fn authorizers(doc: &mut Value, spec: &Value, warnings: &mut Vec<String>) -> Map<String, Value> {
    let mut out = Map::new();
    for (name, scheme) in schemes(spec).into_iter().flatten() {
        if let Some(a) = scheme.get("x-amazon-apigateway-authorizer") {
            let kind = upper(a.get("type")).unwrap_or_else(|| "TOKEN".into());
            let id = new_id(6);
            let mut rec = Map::new();
            rec.insert("id".into(), json!(id));
            rec.insert("name".into(), json!(name));
            rec.insert("type".into(), json!(kind));
            for k in [
                "authorizerUri",
                "authorizerCredentials",
                "identityValidationExpression",
                "providerARNs",
                "identitySource",
            ] {
                if let Some(v) = a.get(k) {
                    rec.insert(k.into(), v.clone());
                }
            }
            if kind == "TOKEN" && !rec.contains_key("identitySource") {
                let header = text(scheme, "name").unwrap_or("Authorization");
                rec.insert(
                    "identitySource".into(),
                    json!(format!("method.request.header.{header}")),
                );
            }
            rec.insert(
                "authorizerResultTtlInSeconds".into(),
                a.get("authorizerResultTtlInSeconds")
                    .cloned()
                    .unwrap_or(json!(300)),
            );
            if let Some(m) = child(doc, "authorizers") {
                m.insert(id.clone(), Value::Object(rec));
            }
            let method_kind = if kind == "COGNITO_USER_POOLS" {
                "COGNITO_USER_POOLS"
            } else {
                "CUSTOM"
            };
            out.insert(name.clone(), json!({"kind": method_kind, "id": id}));
        } else if text(scheme, "x-amazon-apigateway-authtype") == Some("awsSigv4") {
            out.insert(name.clone(), json!({"kind": "AWS_IAM"}));
        } else if text(scheme, "type") == Some("apiKey") {
            out.insert(name.clone(), json!({"kind": "API_KEY"}));
        } else {
            warnings.push(format!(
                "Security scheme {name} is not an API Gateway authorizer and was ignored"
            ));
        }
    }
    out
}

struct Ctx<'a> {
    spec: &'a Value,
    validators: &'a Map<String, Value>,
    authorizers: &'a Map<String, Value>,
    default_validator: Option<Value>,
    warnings: &'a mut Vec<String>,
}

fn paths(doc: &mut Value, root: &str, mut ctx: Ctx<'_>) -> Result<(), ApiError> {
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
    let mut params = Map::new();
    for p in shared
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .chain(
            op.get("parameters")
                .and_then(Value::as_array)
                .into_iter()
                .flatten(),
        )
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
        params.insert(
            format!("method.request.{place}.{name}"),
            json!(p.get("required").and_then(Value::as_bool).unwrap_or(false)),
        );
    }
    if !params.is_empty() {
        m.insert("requestParameters".into(), Value::Object(params));
    }
    let schema_ref = |s: Option<&Value>| {
        s.and_then(|s| text(s, "$ref"))
            .map(|r| r.rsplit('/').next().unwrap_or(r).to_owned())
    };
    let mut models = Map::new();
    for (ct, c) in op
        .pointer("/requestBody/content")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        if let Some(n) = schema_ref(c.get("schema")) {
            models.insert(ct.clone(), json!(n));
        }
    }
    if !models.is_empty() {
        m.insert("requestModels".into(), Value::Object(models));
    }
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
