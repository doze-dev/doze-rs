//! `AWS::ApiGateway::*` (REST APIs): `RestApi` (from properties, or imported from an OpenAPI `Body`/`BodyS3Location`), `Resource`,
//! `Method` (with its integration and responses), `Deployment`, `Stage` (with method settings and access logs), `Authorizer`,
//! `ApiKey`, `UsagePlan`, `UsagePlanKey`, `Model`, `RequestValidator` and `GatewayResponse`, through API Gateway's own operations
//! (signing name `apigateway`; members are lower camel case). When API Gateway is not running, the resource fails and says so.

use serde_json::{Map, Value, json};

use super::logs::camel;
use super::{Done, Made, Rc, document, text};
use crate::j::J;

pub(crate) const SVC: &str = "apigateway";

pub(super) async fn call(rc: &Rc, op: &str, body: Value) -> Result<Value, String> {
    rc.call(SVC, op, body).await.map_err(|e| rc.fail(SVC, &e))
}

/// The properties named, renamed to lower camel case, with `extra` added.
pub(super) fn body(p: &Value, keys: &[&str], extra: &[(&str, Value)]) -> Value {
    let mut out = Map::new();
    for k in keys {
        if let Some(v) = p.get(*k).filter(|v| !v.is_null()) {
            let mut name = (*k).to_owned();
            if let Some(f) = name.get_mut(..1) {
                f.make_ascii_lowercase();
            }
            out.insert(name, camel(v));
        }
    }
    for (k, v) in extra {
        out.insert((*k).to_owned(), v.clone());
    }
    Value::Object(out)
}

pub(super) fn tags(p: &Value) -> Value {
    let mut m = Map::new();
    for t in p
        .get("Tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        m.insert(
            text(t, "Key").unwrap_or_default(),
            Value::String(text(t, "Value").unwrap_or_default()),
        );
    }
    Value::Object(m)
}

pub(super) async fn create(rc: &Rc, p: &Value) -> Done {
    let api = json!(text(p, "RestApiId"));
    match rc.ty.as_str() {
        "AWS::ApiGateway::RestApi" => rest_api(rc, p).await,
        "AWS::ApiGateway::Resource" => {
            let made = call(
                rc,
                "CreateResource",
                body(p, &["ParentId", "PathPart"], &[("restApiId", api)]),
            )
            .await?;
            let id = made.at("id").as_str().unwrap_or_default().to_owned();
            Ok(Made::new(&id).with("ResourceId", id.clone()))
        }
        "AWS::ApiGateway::Method" => method(rc, p).await,
        "AWS::ApiGateway::Deployment" => {
            let made = call(
                rc,
                "CreateDeployment",
                body(
                    p,
                    &["Description", "StageName", "StageDescription"],
                    &[("restApiId", api)],
                ),
            )
            .await?;
            let id = made.at("id").as_str().unwrap_or_default().to_owned();
            Ok(Made::new(&id).with("DeploymentId", id.clone()))
        }
        "AWS::ApiGateway::Stage" => {
            let keys = [
                "StageName",
                "DeploymentId",
                "Description",
                "Variables",
                "TracingEnabled",
                "CacheClusterEnabled",
                "CacheClusterSize",
            ];
            call(
                rc,
                "CreateStage",
                body(p, &keys, &[("restApiId", api.clone()), ("tags", tags(p))]),
            )
            .await?;
            let name = text(p, "StageName").unwrap_or_default();
            stage_settings(rc, &api, &name, p).await?;
            Ok(Made::new(name))
        }
        _ => super::apigw_more::create_more(rc, p, api).await,
    }
}

async fn rest_api(rc: &Rc, p: &Value) -> Done {
    let spec = match (p.get("Body"), p.get("BodyS3Location")) {
        (Some(b), _) => Some(b.as_str().map_or_else(|| b.to_string(), str::to_owned)),
        (None, Some(loc)) => {
            let (bucket, key) = (
                loc.at("Bucket").as_str().unwrap_or_default(),
                loc.at("Key").as_str().unwrap_or_default(),
            );
            let bytes = crate::transform::include::object(
                rc.peers.as_deref(),
                &rc.caller,
                bucket,
                key,
                1 << 22,
            )
            .await
            .map_err(|e| rc.handler(&e, "NotFound"))?;
            Some(String::from_utf8_lossy(&bytes).into_owned())
        }
        (None, None) => None,
    };
    let made = match spec {
        Some(s) => call(rc, "ImportRestApi", json!({"body": aws_core::base64::encode(s.as_bytes()), "failOnWarnings": text(p, "FailOnWarnings") == Some("true".into())})).await?,
        None => {
            let name = text(p, "Name").unwrap_or_else(|| super::generated_name(rc, 128, false));
            let keys = ["Description", "BinaryMediaTypes", "MinimumCompressionSize", "ApiKeySourceType", "EndpointConfiguration", "DisableExecuteApiEndpoint", "CloneFrom"];
            let mut b = body(p, &keys, &[("name", json!(name)), ("tags", tags(p))]);
            if let Some(policy) = document(p, "Policy") {
                b.put("policy", policy.into());
            }
            if let Some(src) = b.as_object_mut().and_then(|o| o.remove("apiKeySourceType")) {
                b.put("apiKeySource", src);
            }
            call(rc, "CreateRestApi", b).await?
        }
    };
    let id = made.at("id").as_str().unwrap_or_default().to_owned();
    let root = match made.get("rootResourceId") {
        Some(r) => r.clone(),
        None => {
            let resources = call(rc, "GetResources", json!({"restApiId": id})).await?;
            resources
                .at("items")
                .as_array()
                .into_iter()
                .flatten()
                .find(|r| r.at("path") == "/")
                .map(|r| r.at("id").clone())
                .unwrap_or(Value::Null)
        }
    };
    Ok(Made::new(&id)
        .with("RestApiId", id.clone())
        .with("RootResourceId", root))
}

const METHOD: &[&str] = &[
    "AuthorizationType",
    "AuthorizerId",
    "ApiKeyRequired",
    "OperationName",
    "RequestParameters",
    "RequestModels",
    "RequestValidatorId",
    "AuthorizationScopes",
];

const INTEGRATION: &[&str] = &[
    "Type",
    "IntegrationHttpMethod",
    "Uri",
    "Credentials",
    "RequestParameters",
    "RequestTemplates",
    "PassthroughBehavior",
    "ContentHandling",
    "TimeoutInMillis",
    "CacheNamespace",
    "CacheKeyParameters",
    "ConnectionType",
    "ConnectionId",
];

pub(super) async fn method(rc: &Rc, p: &Value) -> Done {
    let (api, resource, verb) = (
        json!(text(p, "RestApiId")),
        json!(text(p, "ResourceId")),
        json!(text(p, "HttpMethod")),
    );
    let ids = [
        ("restApiId", api.clone()),
        ("resourceId", resource.clone()),
        ("httpMethod", verb.clone()),
    ];
    let mut m = body(p, METHOD, &ids);
    if m.get("authorizationType").is_none() {
        m.put("authorizationType", "NONE".into());
    }
    call(rc, "PutMethod", m).await?;
    for r in p
        .get("MethodResponses")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        call(
            rc,
            "PutMethodResponse",
            body(
                r,
                &["StatusCode", "ResponseParameters", "ResponseModels"],
                &ids,
            ),
        )
        .await?;
    }
    if let Some(i) = p.get("Integration") {
        call(rc, "PutIntegration", body(i, INTEGRATION, &ids)).await?;
        let keys = [
            "StatusCode",
            "SelectionPattern",
            "ResponseParameters",
            "ResponseTemplates",
            "ContentHandling",
        ];
        for r in i
            .get("IntegrationResponses")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            call(rc, "PutIntegrationResponse", body(r, &keys, &ids)).await?;
        }
    }
    let s = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    Ok(Made::new(format!(
        "{}|{}|{}",
        s(&api),
        s(&resource),
        s(&verb)
    )))
}

/// A stage's access log and method settings, as `UpdateStage` patch operations (memory: the paths API Gateway documents).
pub(super) async fn stage_settings(
    rc: &Rc,
    api: &Value,
    stage: &str,
    p: &Value,
) -> Result<(), String> {
    let mut ops: Vec<Value> = Vec::new();
    if let Some(a) = p.get("AccessLogSetting") {
        ops.push(json!({"op": "replace", "path": "/accessLogSettings/destinationArn", "value": a.at("DestinationArn")}));
        ops.push(
            json!({"op": "replace", "path": "/accessLogSettings/format", "value": a.at("Format")}),
        );
    }
    for s in p
        .get("MethodSettings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let path = text(s, "ResourcePath")
            .unwrap_or_else(|| "/*".into())
            .replace('/', "~1");
        let method = text(s, "HttpMethod").unwrap_or_else(|| "*".into());
        for (key, setting) in [
            ("LoggingLevel", "logging/loglevel"),
            ("DataTraceEnabled", "logging/dataTrace"),
            ("MetricsEnabled", "metrics/enabled"),
            ("ThrottlingBurstLimit", "throttling/burstLimit"),
            ("ThrottlingRateLimit", "throttling/rateLimit"),
            ("CachingEnabled", "caching/enabled"),
        ] {
            if let Some(v) = text(s, key) {
                let base = if path == "~1*" {
                    "/*".to_owned()
                } else {
                    path.clone()
                };
                ops.push(json!({"op": "replace", "path": format!("{base}/{method}/{setting}"), "value": v}));
            }
        }
    }
    if ops.is_empty() {
        return Ok(());
    }
    call(
        rc,
        "UpdateStage",
        json!({"restApiId": api, "stageName": stage, "patchOperations": ops}),
    )
    .await
    .map(|_| ())
}
