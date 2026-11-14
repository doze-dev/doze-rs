//! SAM APIs: `AWS::Serverless::Api` and `HttpApi`, and the implicit `ServerlessRestApi` and `ServerlessHttpApi` that functions' `Api` and
//! `HttpApi` events make. Each becomes the API with an OpenAPI body of its routes (or the `DefinitionBody` given), a deployment and a
//! stage, as SAM makes them.

use serde_json::{Map, Value, json};

use aws_wire::ApiError;

use super::sam::{resource, tag_list};
use super::sam_events::Out;
use crate::j::J;

/// Registers an explicit REST API; it is made once every function has added its routes.
pub(crate) fn rest_api(
    name: &str,
    r: &Value,
    _all: &Map<String, Value>,
    out: &mut Out,
) -> Result<(), ApiError> {
    out.put(name, json!({"__sam_rest": r}));
    Ok(())
}

/// Registers an explicit HTTP API.
pub(crate) fn http_api(
    name: &str,
    r: &Value,
    _all: &Map<String, Value>,
    out: &mut Out,
) -> Result<(), ApiError> {
    out.put(name, json!({"__sam_http": r}));
    Ok(())
}

/// Makes every API: the registered ones and the implicit ones the events asked for.
pub(crate) fn implicit_apis(_all: &Map<String, Value>, out: &mut Out) -> Result<(), ApiError> {
    let pending: Vec<(String, Value)> = out
        .resources
        .iter()
        .filter(|(_, v)| v.get("__sam_rest").is_some() || v.get("__sam_http").is_some())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (name, v) in pending {
        if let Some(r) = v.get("__sam_rest") {
            make_rest(&name, r, out);
        } else if let Some(r) = v.get("__sam_http") {
            make_http(&name, r, out);
        }
    }
    let implicit = json!({"Type": "AWS::Serverless::Api", "Properties": {"StageName": "Prod"}});
    if out
        .rest_routes
        .iter()
        .any(|(a, ..)| a == "ServerlessRestApi")
        && !out.resources.contains_key("ServerlessRestApi")
    {
        make_rest("ServerlessRestApi", &implicit, out);
    }
    if out
        .http_routes
        .iter()
        .any(|(a, ..)| a == "ServerlessHttpApi")
        && !out.resources.contains_key("ServerlessHttpApi")
    {
        make_http(
            "ServerlessHttpApi",
            &json!({"Type": "AWS::Serverless::HttpApi"}),
            out,
        );
    }
    Ok(())
}

fn integration(function: &str, payload: Option<&str>) -> Value {
    let mut i = json!({"httpMethod": "POST", "type": "aws_proxy",
        "uri": {"Fn::Sub": format!("arn:${{AWS::Partition}}:apigateway:${{AWS::Region}}:lambda:path/2015-03-31/functions/${{{function}.Arn}}/invocations")}});
    if let Some(p) = payload {
        i.put("payloadFormatVersion", p.into());
    }
    i
}

/// The `paths` of an OpenAPI body for `routes` of `api`.
fn paths(
    routes: &[(String, String, String, String, String)],
    api: &str,
    payload: Option<&str>,
) -> Map<String, Value> {
    let mut paths = Map::new();
    for (_, path, method, function, _) in routes.iter().filter(|(a, ..)| a == api) {
        let key = if method == "any" {
            "x-amazon-apigateway-any-method".to_owned()
        } else {
            method.clone()
        };
        let entry = paths.entry(path.clone()).or_insert_with(|| json!({}));
        entry.put(key, json!({"x-amazon-apigateway-integration": integration(function, payload), "responses": {}}));
    }
    paths
}

fn make_rest(name: &str, r: &Value, out: &mut Out) {
    let p = r.get("Properties").cloned().unwrap_or(json!({}));
    let body = p.get("DefinitionBody").cloned().unwrap_or_else(|| {
        json!({"swagger": "2.0", "info": {"version": "1.0", "title": p.get("Name").cloned().unwrap_or(json!({"Ref": "AWS::StackName"}))},
            "paths": paths(&out.rest_routes, name, None)})
    });
    let mut api = json!({"Body": body});
    for k in [
        "Name",
        "Description",
        "EndpointConfiguration",
        "BinaryMediaTypes",
        "MinimumCompressionSize",
        "ApiKeySourceType",
        "DisableExecuteApiEndpoint",
    ] {
        if let Some(v) = p.get(k) {
            api.put(
                if k == "ApiKeySourceType" {
                    "ApiKeySource"
                } else {
                    k
                },
                v.clone(),
            );
        }
    }
    if let Some(uri) = p.get("DefinitionUri") {
        let loc = super::sam::location(name, Some(uri), "DefinitionUri").unwrap_or(Value::Null);
        api.put(
            "BodyS3Location",
            json!({"Bucket": loc.at("S3Bucket"), "Key": loc.at("S3Key")}),
        );
        if let Some(o) = api.as_object_mut() {
            o.remove("Body");
        }
    }
    out.put(name, resource("AWS::ApiGateway::RestApi", api, r));
    rest_stage(name, &p, r, out);
}

/// A REST API's deployment (named for its body, so a change makes a new one) and its stage.
fn rest_stage(name: &str, p: &Value, r: &Value, out: &mut Out) {
    let tags = tag_list(p, false);
    let deployment = format!(
        "{name}Deployment{}",
        super::sam::hash10(&body_of(p, out, name))
    );
    out.put(&deployment, resource("AWS::ApiGateway::Deployment", json!({"RestApiId": {"Ref": name}, "Description": format!("RestApi deployment id: {name}")}), r));
    let stage_name = p.get("StageName").cloned().unwrap_or(json!("Prod"));
    let stage_id = format!(
        "{name}{}Stage",
        stage_name
            .as_str()
            .unwrap_or("Prod")
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect::<String>()
    );
    let mut stage = json!({"RestApiId": {"Ref": name}, "DeploymentId": {"Ref": deployment}, "StageName": stage_name});
    for k in [
        "MethodSettings",
        "AccessLogSetting",
        "Variables",
        "TracingEnabled",
        "CacheClusterEnabled",
        "CacheClusterSize",
        "CanarySetting",
    ] {
        if let Some(v) = p.get(k) {
            stage.put(k, v.clone());
        }
    }
    if !tags.is_empty() {
        stage.put("Tags", Value::Array(tags));
    }
    out.put(&stage_id, resource("AWS::ApiGateway::Stage", stage, r));
    out.aliases.insert(format!("{name}.Stage"), stage_id.into());
    out.aliases
        .insert(format!("{name}.Deployment"), deployment.into());
}

/// What a REST API's deployment depends on: its body (given or generated), so a new route makes a new deployment.
fn body_of(p: &Value, out: &Out, name: &str) -> Value {
    p.get("DefinitionBody")
        .cloned()
        .unwrap_or_else(|| Value::Object(paths(&out.rest_routes, name, None)))
}

fn make_http(name: &str, r: &Value, out: &mut Out) {
    let p = r.get("Properties").cloned().unwrap_or(json!({}));
    let mut body = p.get("DefinitionBody").cloned().unwrap_or_else(|| {
        json!({"openapi": "3.0.1", "info": {"version": "1.0", "title": p.get("Name").cloned().unwrap_or(json!({"Ref": "AWS::StackName"}))},
            "paths": paths(&out.http_routes, name, Some("2.0"))})
    });
    if let Some(cors) = p.get("CorsConfiguration") {
        body.put("x-amazon-apigateway-cors", cors.clone());
    }
    let mut api = json!({"Body": body});
    for k in ["Description", "DisableExecuteApiEndpoint", "FailOnWarnings"] {
        if let Some(v) = p.get(k) {
            api.put(k, v.clone());
        }
    }
    if let Some(Value::Object(tags)) = p.get("Tags") {
        api.put("Tags", Value::Object(tags.clone()));
    }
    out.put(name, resource("AWS::ApiGatewayV2::Api", api, r));
    let stage_name = p.get("StageName").cloned().unwrap_or(json!("$default"));
    let stage_id = if stage_name == "$default" {
        format!("{name}ApiGatewayDefaultStage")
    } else {
        format!(
            "{name}{}Stage",
            stage_name
                .as_str()
                .unwrap_or_default()
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .collect::<String>()
        )
    };
    let mut stage = json!({"ApiId": {"Ref": name}, "StageName": stage_name, "AutoDeploy": true});
    for k in [
        "AccessLogSettings",
        "DefaultRouteSettings",
        "RouteSettings",
        "StageVariables",
    ] {
        if let Some(v) = p.get(k) {
            stage.put(k, v.clone());
        }
    }
    out.put(&stage_id, resource("AWS::ApiGatewayV2::Stage", stage, r));
    out.aliases.insert(format!("{name}.Stage"), stage_id.into());
}
