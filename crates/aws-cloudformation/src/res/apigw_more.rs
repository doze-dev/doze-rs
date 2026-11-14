//! The simpler REST API types, as a table (create operation, properties, the attribute the id goes under), and the updates and
//! deletes of every REST API type.

use serde_json::{Value, json};

use super::apigw::{SVC, body, call, method, stage_settings, tags};
use super::{Done, Made, Rc, document, gone, text};
use crate::j::J;
use crate::model::Res;

/// A type made with one call: its operation, its properties, and the attribute its id goes under.
struct Simple {
    ty: &'static str,
    op: &'static str,
    keys: &'static [&'static str],
    attr: &'static str,
    on_api: bool,
    tagged: bool,
}

const SIMPLE: &[Simple] = &[
    Simple {
        ty: "AWS::ApiGateway::Authorizer",
        op: "CreateAuthorizer",
        keys: &[
            "Name",
            "Type",
            "ProviderARNs",
            "AuthType",
            "AuthorizerUri",
            "AuthorizerCredentials",
            "IdentitySource",
            "IdentityValidationExpression",
            "AuthorizerResultTtlInSeconds",
        ],
        attr: "AuthorizerId",
        on_api: true,
        tagged: false,
    },
    Simple {
        ty: "AWS::ApiGateway::ApiKey",
        op: "CreateApiKey",
        keys: &[
            "Name",
            "Description",
            "Enabled",
            "GenerateDistinctId",
            "Value",
            "StageKeys",
            "CustomerId",
        ],
        attr: "APIKeyId",
        on_api: false,
        tagged: true,
    },
    Simple {
        ty: "AWS::ApiGateway::UsagePlan",
        op: "CreateUsagePlan",
        keys: &[
            "UsagePlanName",
            "Description",
            "ApiStages",
            "Throttle",
            "Quota",
        ],
        attr: "Id",
        on_api: false,
        tagged: true,
    },
    Simple {
        ty: "AWS::ApiGateway::UsagePlanKey",
        op: "CreateUsagePlanKey",
        keys: &["UsagePlanId", "KeyId", "KeyType"],
        attr: "Id",
        on_api: false,
        tagged: false,
    },
    Simple {
        ty: "AWS::ApiGateway::Model",
        op: "CreateModel",
        keys: &["Name", "Description", "ContentType"],
        attr: "",
        on_api: true,
        tagged: false,
    },
    Simple {
        ty: "AWS::ApiGateway::RequestValidator",
        op: "CreateRequestValidator",
        keys: &["Name", "ValidateRequestBody", "ValidateRequestParameters"],
        attr: "RequestValidatorId",
        on_api: true,
        tagged: false,
    },
];

pub(super) async fn create_more(rc: &Rc, p: &Value, api: Value) -> Done {
    if rc.ty == "AWS::ApiGateway::GatewayResponse" {
        let keys = [
            "ResponseType",
            "StatusCode",
            "ResponseParameters",
            "ResponseTemplates",
        ];
        call(
            rc,
            "PutGatewayResponse",
            body(p, &keys, &[("restApiId", api.clone())]),
        )
        .await?;
        let id = format!(
            "{}|{}",
            api.as_str().unwrap_or_default(),
            text(p, "ResponseType").unwrap_or_default()
        );
        return Ok(Made::new(id));
    }
    let Some(s) = SIMPLE.iter().find(|s| s.ty == rc.ty) else {
        return Err(format!("{} is not a type doze-aws makes", rc.ty));
    };
    let mut extra = Vec::new();
    if s.on_api {
        extra.push(("restApiId", api));
    }
    if s.tagged {
        extra.push(("tags", tags(p)));
    }
    if s.ty == "AWS::ApiGateway::Model" {
        extra.push(("schema", json!(document(p, "Schema"))));
    }
    let mut b = body(p, s.keys, &extra);
    if let Some(name) = b.get("usagePlanName").cloned() {
        b.put("name", name);
    }
    let made = call(rc, s.op, b).await?;
    let id = made
        .at("id")
        .as_str()
        .or_else(|| made.at("name").as_str())
        .unwrap_or_default()
        .to_owned();
    let mut m = Made::new(&id);
    if !s.attr.is_empty() {
        m = m.with(s.attr, id.clone());
    }
    Ok(m)
}

pub(super) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    let id = old.physical.clone().unwrap_or_default();
    let api = json!(text(p, "RestApiId"));
    match rc.ty.as_str() {
        "AWS::ApiGateway::Stage" => {
            let mut ops = vec![
                json!({"op": "replace", "path": "/deploymentId", "value": text(p, "DeploymentId")}),
            ];
            if let Some(d) = text(p, "Description") {
                ops.push(json!({"op": "replace", "path": "/description", "value": d}));
            }
            if let Some(Value::Object(vars)) = p.get("Variables") {
                ops.extend(vars.iter().map(|(k, v)| json!({"op": "replace", "path": format!("/variables/{k}"), "value": v})));
            }
            call(
                rc,
                "UpdateStage",
                json!({"restApiId": api, "stageName": id, "patchOperations": ops}),
            )
            .await?;
            stage_settings(rc, &api, &id, p).await?;
        }
        "AWS::ApiGateway::Method" => {
            delete(rc, old).await?;
            return method(rc, p).await;
        }
        "AWS::ApiGateway::RestApi" if p.get("Body").is_some() => {
            let spec = p
                .get("Body")
                .map(|b| b.as_str().map_or_else(|| b.to_string(), str::to_owned))
                .unwrap_or_default();
            let b = json!({"restApiId": id, "mode": "overwrite", "body": aws_core::base64::encode(spec.as_bytes())});
            call(rc, "PutRestApi", b).await?;
        }
        "AWS::ApiGateway::GatewayResponse" => return create_more(rc, p, api).await,
        _ => {}
    }
    let mut m = Made::new(&id);
    m.attrs = old.attrs_map();
    Ok(m)
}

/// The operation that deletes a type, and its request.
fn deletion(ty: &str, id: &str, p: &Value) -> Option<(&'static str, Value)> {
    let api = json!(text(p, "RestApiId"));
    Some(match ty {
        "AWS::ApiGateway::RestApi" => ("DeleteRestApi", json!({"restApiId": id})),
        "AWS::ApiGateway::Resource" => (
            "DeleteResource",
            json!({"restApiId": api, "resourceId": id}),
        ),
        "AWS::ApiGateway::Method" => (
            "DeleteMethod",
            json!({"restApiId": api, "resourceId": text(p, "ResourceId"), "httpMethod": text(p, "HttpMethod")}),
        ),
        "AWS::ApiGateway::Deployment" => (
            "DeleteDeployment",
            json!({"restApiId": api, "deploymentId": id}),
        ),
        "AWS::ApiGateway::Stage" => ("DeleteStage", json!({"restApiId": api, "stageName": id})),
        "AWS::ApiGateway::Authorizer" => (
            "DeleteAuthorizer",
            json!({"restApiId": api, "authorizerId": id}),
        ),
        "AWS::ApiGateway::ApiKey" => ("DeleteApiKey", json!({"apiKey": id})),
        "AWS::ApiGateway::UsagePlan" => ("DeleteUsagePlan", json!({"usagePlanId": id})),
        "AWS::ApiGateway::UsagePlanKey" => (
            "DeleteUsagePlanKey",
            json!({"usagePlanId": text(p, "UsagePlanId"), "keyId": id}),
        ),
        "AWS::ApiGateway::Model" => ("DeleteModel", json!({"restApiId": api, "modelName": id})),
        "AWS::ApiGateway::RequestValidator" => (
            "DeleteRequestValidator",
            json!({"restApiId": api, "requestValidatorId": id}),
        ),
        "AWS::ApiGateway::GatewayResponse" => (
            "DeleteGatewayResponse",
            json!({"restApiId": api, "responseType": text(p, "ResponseType")}),
        ),
        _ => return None,
    })
}

pub(super) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    let id = res.physical.clone().unwrap_or_default();
    let Some((op, b)) = deletion(&rc.ty, &id, &res.props_value()) else {
        return Ok(());
    };
    match rc.call(SVC, op, b).await {
        Ok(_) => Ok(()),
        Err(e) if gone(&e) || e.message.starts_with("no service ") => Ok(()),
        Err(e) => Err(rc.fail(SVC, &e)),
    }
}
