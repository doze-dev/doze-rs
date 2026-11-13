//! `AWS::ApiGatewayV2::*` (HTTP and WebSocket APIs): `Api` (from properties, or imported from an OpenAPI `Body`), `Route`,
//! `Integration`, `Stage`, `Deployment`, `Authorizer`, `RouteResponse` and `IntegrationResponse`, through API Gateway v2's own
//! operations (members are lower camel case; CloudFormation's names are the same in Pascal case). When the service is not running,
//! the resource fails and says so.

use serde_json::{Map, Value, json};

use super::logs::camel;
use super::{Done, Made, Rc, gone, text};
use crate::j::J;
use crate::model::Res;

/// The name API Gateway v2 is called by on the peer bus: its model's signing name, which it shares with v1 (`apigateway`). The
/// integrator decides how the two are registered; this is the one place to change.
pub(crate) const SVC: &str = "apigateway";

async fn call(rc: &Rc, op: &str, body: Value) -> Result<Value, String> {
    rc.call(SVC, op, body).await.map_err(|e| rc.fail(SVC, &e))
}

/// Every property in lower camel case (`Tags` stays a map), plus `extra`.
fn body(p: &Value, extra: &[(&str, Value)]) -> Value {
    let mut out = match camel(p) {
        Value::Object(o) => o,
        _ => Map::new(),
    };
    for (k, v) in extra {
        out.insert((*k).to_owned(), v.clone());
    }
    Value::Object(out)
}

/// The kind, the create and update operation names, and the id member of each type.
fn kind(ty: &str) -> Option<(&'static str, &'static str)> {
    Some(match ty {
        "AWS::ApiGatewayV2::Route" => ("Route", "routeId"),
        "AWS::ApiGatewayV2::Integration" => ("Integration", "integrationId"),
        "AWS::ApiGatewayV2::Stage" => ("Stage", "stageName"),
        "AWS::ApiGatewayV2::Deployment" => ("Deployment", "deploymentId"),
        "AWS::ApiGatewayV2::Authorizer" => ("Authorizer", "authorizerId"),
        "AWS::ApiGatewayV2::RouteResponse" => ("RouteResponse", "routeResponseId"),
        "AWS::ApiGatewayV2::IntegrationResponse" => {
            ("IntegrationResponse", "integrationResponseId")
        }
        _ => return None,
    })
}

/// The attribute CloudFormation gives each type's id under.
fn id_attr(noun: &str) -> &'static str {
    match noun {
        "Route" => "RouteId",
        "Integration" => "IntegrationId",
        "Deployment" => "DeploymentId",
        "Authorizer" => "AuthorizerId",
        "RouteResponse" => "RouteResponseId",
        "IntegrationResponse" => "IntegrationResponseId",
        _ => "Id",
    }
}

pub(super) async fn create(rc: &Rc, p: &Value) -> Done {
    if rc.ty == "AWS::ApiGatewayV2::Api" {
        let made = match p.get("Body") {
            Some(b) => {
                let spec = b.as_str().map_or_else(|| b.to_string(), str::to_owned);
                call(rc, "ImportApi", json!({"body": spec, "basepath": text(p, "BasePath"), "failOnWarnings": text(p, "FailOnWarnings") == Some("true".into())})).await?
            }
            None => {
                let mut b = body(p, &[]);
                if b.get("name").is_none() {
                    b.put("name", super::generated_name(rc, 128, false).into());
                }
                call(rc, "CreateApi", b).await?
            }
        };
        let id = made.at("apiId").as_str().unwrap_or_default().to_owned();
        return Ok(Made::new(&id)
            .with("ApiId", id.clone())
            .with("ApiEndpoint", made.at("apiEndpoint").clone()));
    }
    let Some((noun, id_member)) = kind(&rc.ty) else {
        return Err(format!("{} is not a type doze-aws makes", rc.ty));
    };
    let made = call(rc, &format!("Create{noun}"), body(p, &[])).await?;
    let id = made.at(id_member).as_str().unwrap_or_default().to_owned();
    Ok(Made::new(&id).with(id_attr(noun), id.clone()))
}

pub(super) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    let id = old.physical.clone().unwrap_or_default();
    if rc.ty == "AWS::ApiGatewayV2::Api" {
        match p.get("Body") {
            Some(b) => {
                let spec = b.as_str().map_or_else(|| b.to_string(), str::to_owned);
                call(rc, "ReimportApi", json!({"apiId": id, "body": spec})).await?;
            }
            None => {
                call(rc, "UpdateApi", body(p, &[("apiId", json!(id))])).await?;
            }
        }
    } else if let Some((noun, id_member)) = kind(&rc.ty) {
        call(
            rc,
            &format!("Update{noun}"),
            body(p, &[(id_member, json!(id))]),
        )
        .await?;
    }
    let mut m = Made::new(&id);
    m.attrs = old.attrs_map();
    Ok(m)
}

pub(super) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    let id = res.physical.clone().unwrap_or_default();
    let p = res.props_value();
    let (op, body) = if rc.ty == "AWS::ApiGatewayV2::Api" {
        ("DeleteApi".to_owned(), json!({"apiId": id}))
    } else if let Some((noun, id_member)) = kind(&rc.ty) {
        let mut b = json!({"apiId": text(&p, "ApiId")});
        b.put(id_member, json!(id));
        for (k, member) in [("RouteId", "routeId"), ("IntegrationId", "integrationId")] {
            if let Some(v) = text(&p, k) {
                b.put(member, v.into());
            }
        }
        (format!("Delete{noun}"), b)
    } else {
        return Ok(());
    };
    match rc.call(SVC, &op, body).await {
        Ok(_) => Ok(()),
        Err(e) if gone(&e) || e.message.starts_with("no service ") => Ok(()),
        Err(e) => Err(rc.fail(SVC, &e)),
    }
}
