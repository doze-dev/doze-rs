//! The Lambda types beside functions: `Permission`, `Version`, `Alias`, `EventSourceMapping`, `Url`, `LayerVersion`,
//! `LayerVersionPermission` and `EventInvokeConfig`.

use serde_json::{Value, json};

use super::lambda::SVC;
use super::{Done, Made, Rc, gone, text};
use crate::j::J;
use crate::model::Res;

/// The members a template's properties give an operation as they are, plus the ones named.
fn pick(p: &Value, keys: &[&str]) -> Value {
    let mut out = json!({});
    for k in keys {
        if let Some(v) = p.get(*k).filter(|v| !v.is_null()) {
            out.put(k, v.clone());
        }
    }
    out
}

async fn call(rc: &Rc, op: &str, body: Value) -> Result<Value, String> {
    rc.call(SVC, op, body).await.map_err(|e| rc.fail(SVC, &e))
}

async fn call_or_gone(rc: &Rc, op: &str, body: Value) -> Result<(), String> {
    match rc.call(SVC, op, body).await {
        Ok(_) => Ok(()),
        Err(e) if gone(&e) => Ok(()),
        Err(e) => Err(rc.fail(SVC, &e)),
    }
}

const PERMISSION: &[&str] = &[
    "FunctionName",
    "Action",
    "Principal",
    "SourceArn",
    "SourceAccount",
    "EventSourceToken",
    "PrincipalOrgID",
    "FunctionUrlAuthType",
];

const ALIAS: &[&str] = &[
    "FunctionName",
    "Name",
    "FunctionVersion",
    "Description",
    "RoutingConfig",
];

const MAPPING: &[&str] = &[
    "FunctionName",
    "EventSourceArn",
    "Enabled",
    "BatchSize",
    "FilterCriteria",
    "MaximumBatchingWindowInSeconds",
    "ParallelizationFactor",
    "StartingPosition",
    "StartingPositionTimestamp",
    "DestinationConfig",
    "MaximumRecordAgeInSeconds",
    "BisectBatchOnFunctionError",
    "MaximumRetryAttempts",
    "TumblingWindowInSeconds",
    "Topics",
    "Queues",
    "SourceAccessConfigurations",
    "SelfManagedEventSource",
    "FunctionResponseTypes",
    "ScalingConfig",
    "DocumentDBEventSourceConfig",
    "KMSKeyArn",
    "MetricsConfig",
    "ProvisionedPollerConfig",
];

pub(super) async fn create(rc: &Rc, p: &Value) -> Done {
    match rc.ty.as_str() {
        "AWS::Lambda::Permission" => {
            let id = super::generated_name(rc, 100, false);
            let mut body = pick(p, PERMISSION);
            body.put("StatementId", id.clone().into());
            call(rc, "AddPermission", body).await?;
            Ok(Made::new(id))
        }
        "AWS::Lambda::Version" => {
            let made = call(
                rc,
                "PublishVersion",
                pick(p, &["FunctionName", "Description", "CodeSha256"]),
            )
            .await?;
            let arn = made
                .at("FunctionArn")
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let function = arn
                .rsplit_once(':')
                .map_or(arn.clone(), |(f, _)| f.to_owned());
            Ok(Made::new(&arn)
                .with("Version", made.at("Version").clone())
                .with("FunctionArn", function))
        }
        "AWS::Lambda::Alias" => {
            let made = call(rc, "CreateAlias", pick(p, ALIAS)).await?;
            let arn = made.at("AliasArn").as_str().unwrap_or_default().to_owned();
            Ok(Made::new(&arn).with("AliasArn", arn.clone()))
        }
        "AWS::Lambda::EventSourceMapping" => {
            let made = call(rc, "CreateEventSourceMapping", pick(p, MAPPING)).await?;
            let id = made.at("UUID").as_str().unwrap_or_default().to_owned();
            Ok(Made::new(&id).with("Id", id.clone()).with(
                "EventSourceMappingArn",
                made.at("EventSourceMappingArn").clone(),
            ))
        }
        "AWS::Lambda::Url" => {
            let mut body = pick(p, &["Qualifier", "AuthType", "Cors", "InvokeMode"]);
            body.put("FunctionName", p.at("TargetFunctionArn").clone());
            let made = call(rc, "CreateFunctionUrlConfig", body).await?;
            let function = made
                .at("FunctionArn")
                .as_str()
                .unwrap_or_default()
                .to_owned();
            Ok(Made::new(&function)
                .with("FunctionArn", function.clone())
                .with("FunctionUrl", made.at("FunctionUrl").clone()))
        }
        _ => create_layer_or_config(rc, p).await,
    }
}

async fn create_layer_or_config(rc: &Rc, p: &Value) -> Done {
    match rc.ty.as_str() {
        "AWS::Lambda::LayerVersion" => {
            let name =
                text(p, "LayerName").unwrap_or_else(|| super::generated_name(rc, 140, false));
            let mut body = pick(
                p,
                &[
                    "Description",
                    "Content",
                    "CompatibleRuntimes",
                    "CompatibleArchitectures",
                    "LicenseInfo",
                ],
            );
            body.put("LayerName", name.into());
            let made = call(rc, "PublishLayerVersion", body).await?;
            let arn = made
                .at("LayerVersionArn")
                .as_str()
                .unwrap_or_default()
                .to_owned();
            Ok(Made::new(&arn).with("LayerVersionArn", arn.clone()))
        }
        "AWS::Lambda::LayerVersionPermission" => {
            let arn = text(p, "LayerVersionArn").unwrap_or_default();
            let (layer, version) = layer_parts(&arn);
            let id = super::generated_name(rc, 100, false);
            let mut body = pick(p, &["Action", "Principal", "OrganizationId"]);
            for (k, v) in [
                ("LayerName", json!(layer)),
                ("VersionNumber", json!(version)),
                ("StatementId", json!(id)),
            ] {
                body.put(k, v);
            }
            call(rc, "AddLayerVersionPermission", body).await?;
            Ok(Made::new(format!("{arn}#{id}")))
        }
        "AWS::Lambda::EventInvokeConfig" => {
            let body = pick(
                p,
                &[
                    "FunctionName",
                    "Qualifier",
                    "MaximumRetryAttempts",
                    "MaximumEventAgeInSeconds",
                    "DestinationConfig",
                ],
            );
            call(rc, "PutFunctionEventInvokeConfig", body).await?;
            Ok(Made::new(super::generated_name(rc, 128, false)))
        }
        other => Err(format!("{other} is not a type doze-aws makes")),
    }
}

/// The layer name (or ARN without its version) and the version number of a layer version ARN.
fn layer_parts(arn: &str) -> (String, i64) {
    match arn.rsplit_once(':') {
        Some((layer, v)) => (layer.to_owned(), v.parse().unwrap_or(0)),
        None => (arn.to_owned(), 0),
    }
}

pub(super) async fn update(rc: &Rc, old: &Res, p: &Value) -> Done {
    let physical = old.physical.clone().unwrap_or_default();
    match rc.ty.as_str() {
        "AWS::Lambda::Alias" => {
            call(rc, "UpdateAlias", pick(p, ALIAS)).await?;
        }
        "AWS::Lambda::EventSourceMapping" => {
            let mut body = pick(p, MAPPING);
            if let Some(o) = body.as_object_mut() {
                for k in [
                    "EventSourceArn",
                    "StartingPosition",
                    "StartingPositionTimestamp",
                    "Topics",
                    "Queues",
                    "SelfManagedEventSource",
                ] {
                    o.remove(k);
                }
            }
            body.put("UUID", physical.clone().into());
            call(rc, "UpdateEventSourceMapping", body).await?;
        }
        "AWS::Lambda::Url" => {
            let mut body = pick(p, &["Qualifier", "AuthType", "Cors", "InvokeMode"]);
            body.put("FunctionName", p.at("TargetFunctionArn").clone());
            call(rc, "UpdateFunctionUrlConfig", body).await?;
        }
        "AWS::Lambda::EventInvokeConfig" => {
            let body = pick(
                p,
                &[
                    "FunctionName",
                    "Qualifier",
                    "MaximumRetryAttempts",
                    "MaximumEventAgeInSeconds",
                    "DestinationConfig",
                ],
            );
            call(rc, "PutFunctionEventInvokeConfig", body).await?;
        }
        _ => return create(rc, p).await,
    }
    let mut made = Made::new(physical);
    made.attrs = old.attrs_map();
    Ok(made)
}

pub(super) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    let id = res.physical.clone().unwrap_or_default();
    let p = res.props_value();
    match rc.ty.as_str() {
        "AWS::Lambda::Permission" => {
            call_or_gone(
                rc,
                "RemovePermission",
                json!({"FunctionName": p.at("FunctionName"), "StatementId": id}),
            )
            .await
        }
        "AWS::Lambda::Version" => {
            let (function, version) = id.rsplit_once(':').unwrap_or((&id, ""));
            call_or_gone(
                rc,
                "DeleteFunction",
                json!({"FunctionName": function, "Qualifier": version}),
            )
            .await
        }
        "AWS::Lambda::Alias" => {
            call_or_gone(
                rc,
                "DeleteAlias",
                json!({"FunctionName": p.at("FunctionName"), "Name": p.at("Name")}),
            )
            .await
        }
        "AWS::Lambda::EventSourceMapping" => {
            call_or_gone(rc, "DeleteEventSourceMapping", json!({"UUID": id})).await
        }
        "AWS::Lambda::Url" => {
            let mut body = pick(&p, &["Qualifier"]);
            body.put("FunctionName", p.at("TargetFunctionArn").clone());
            call_or_gone(rc, "DeleteFunctionUrlConfig", body).await
        }
        "AWS::Lambda::LayerVersion" => {
            let (layer, version) = layer_parts(&id);
            call_or_gone(
                rc,
                "DeleteLayerVersion",
                json!({"LayerName": layer, "VersionNumber": version}),
            )
            .await
        }
        "AWS::Lambda::LayerVersionPermission" => {
            let (arn, sid) = id.split_once('#').unwrap_or((&id, ""));
            let (layer, version) = layer_parts(arn);
            call_or_gone(
                rc,
                "RemoveLayerVersionPermission",
                json!({"LayerName": layer, "VersionNumber": version, "StatementId": sid}),
            )
            .await
        }
        "AWS::Lambda::EventInvokeConfig" => {
            call_or_gone(
                rc,
                "DeleteFunctionEventInvokeConfig",
                pick(&p, &["FunctionName", "Qualifier"]),
            )
            .await
        }
        _ => Ok(()),
    }
}
