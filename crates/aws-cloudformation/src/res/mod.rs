//! Resource types: how each mapped type is made, updated and deleted through its service's own operations, which properties force
//! a replacement, and the class of every other type (decision D5).

mod cloudwatch;
mod coerce;
mod ddb;
mod events;
mod iam;
mod iam_more;
mod kinesis;
mod kms;
mod lambda;
mod lambda_more;
mod logs;
pub(crate) mod more;
mod naming;
pub(crate) mod policy;
mod s3;
mod s3_config;
mod secrets;
pub(crate) mod skip;
mod sns;
mod sqs;
mod ssm;
mod table;

use std::sync::Arc;

use serde_json::{Map, Value};

use aws_peers::Peers;
use aws_wire::ApiError;
use aws_wire::service::Caller;

use crate::engine::Runner;
use crate::model::Res;

pub(crate) use table::{create_only, identifiers, name_property};

/// What a resource is made with.
#[derive(Debug, Clone)]
pub(crate) struct Rc {
    pub(crate) peers: Option<Arc<Peers>>,
    pub(crate) caller: Caller,
    pub(crate) stack_name: String,
    pub(crate) stack_id: String,
    pub(crate) logical: String,
    pub(crate) ty: String,
    /// The stack's tags, given to every taggable resource.
    pub(crate) tags: Vec<(String, String)>,
    pub(crate) logical_names: bool,
    pub(crate) runner: Arc<Runner>,
    /// The id AWS quotes in a handler's message.
    pub(crate) token: String,
}

/// A resource as made.
#[derive(Debug, Clone, Default)]
pub(crate) struct Made {
    pub(crate) physical: String,
    pub(crate) attrs: Map<String, Value>,
}

impl Made {
    pub(crate) fn new(physical: impl Into<String>) -> Self {
        Self {
            physical: physical.into(),
            attrs: Map::new(),
        }
    }

    pub(crate) fn with(mut self, key: &str, v: impl Into<Value>) -> Self {
        self.attrs.insert(key.to_owned(), v.into());
        self
    }
}

/// A resource made, or the reason it was not (the status reason, in AWS's words).
pub(crate) type Done = Result<Made, String>;

const ACTING: &str = "cloudformation";

impl Rc {
    /// Calls `op` of `service` with `body`, its scalars first coerced to what the operation's model expects (a template's `"30"`
    /// where the model has an integer), as CloudFormation's resource handlers convert property types.
    ///
    /// # Errors
    /// The service's own error.
    pub(crate) async fn call(
        &self,
        service: &str,
        op: &str,
        mut body: Value,
    ) -> Result<Value, ApiError> {
        let peers = self.peers.as_ref().ok_or_else(|| {
            ApiError::internal(format!("no service {service} is running to be called"))
        })?;
        if let Some(t) = peers.tables(service) {
            coerce::input(t, op, &mut body);
        }
        peers.call(ACTING, &self.caller, service, op, &body).await
    }

    /// Like [`Rc::call`], for a REST service whose answer has a streamed body (S3).
    ///
    /// # Errors
    /// The service's own error.
    pub(crate) async fn rest(
        &self,
        service: &str,
        op: &str,
        mut body: Value,
    ) -> Result<Value, ApiError> {
        let peers = self.peers.as_ref().ok_or_else(|| {
            ApiError::internal(format!("no service {service} is running to be called"))
        })?;
        if let Some(t) = peers.tables(service) {
            coerce::input(t, op, &mut body);
        }
        peers
            .call_rest(ACTING, &self.caller, service, op, &body)
            .await
            .map(|r| r.json)
    }

    /// A service's error as a resource's status reason: `Resource handler returned message: "<message> (Service: <Svc>, Status Code:
    /// <n>, Request ID: <id>)" (RequestToken: <token>, HandlerErrorCode: <code>)` (memory, the shape of the recorded
    /// already-exists reason).
    pub(crate) fn fail(&self, service: &str, e: &ApiError) -> String {
        if e.message.starts_with("no service ") {
            return format!(
                "{} is not served by this doze-aws ({}); the resource was not created",
                self.ty, e.message
            );
        }
        let code = match (e.status, e.code.as_ref()) {
            (_, c) if c.contains("AlreadyExists") || c.contains("EntityAlreadyExists") => {
                "AlreadyExists"
            }
            (_, c)
                if c.contains("NotFound") || c.contains("NoSuch") || c.contains("NonExistent") =>
            {
                "NotFound"
            }
            (_, c) if c.contains("AccessDenied") || c.contains("Authorization") => "AccessDenied",
            (_, c) if c.contains("Throttl") || c.contains("LimitExceeded") => "Throttling",
            (s, _) if s >= 500 => "ServiceInternalError",
            _ => "InvalidRequest",
        };
        let request_id = aws_seal::uuid_v4().unwrap_or_default();
        self.handler(
            &format!(
                "{} (Service: {}, Status Code: {}, Request ID: {request_id})",
                e.message,
                service_label(service),
                e.status
            ),
            code,
        )
    }

    /// A handler's own message as a status reason.
    pub(crate) fn handler(&self, message: &str, code: &str) -> String {
        format!(
            "Resource handler returned message: \"{message}\" (RequestToken: {}, HandlerErrorCode: {code})",
            self.token
        )
    }

    /// The reason a create fails when the name is taken: recorded for SQS (2026-11-12); the generic sentence for the rest (memory).
    pub(crate) fn taken(&self, id: &str) -> String {
        self.handler(
            &format!(
                "Resource of type '{}' with identifier '{id}' already exists.",
                self.ty
            ),
            "AlreadyExists",
        )
    }
}

/// How a service is named in a handler's message (memory: the SDK's client name).
fn service_label(service: &str) -> String {
    match service {
        "sqs" => "Sqs".into(),
        "sns" => "Sns".into(),
        "ssm" => "Ssm".into(),
        "s3" => "S3".into(),
        "dynamodb" => "DynamoDb".into(),
        "iam" => "Iam".into(),
        "lambda" => "Lambda".into(),
        "logs" => "CloudWatchLogs".into(),
        "kinesis" => "Kinesis".into(),
        "kms" => "Kms".into(),
        "secretsmanager" => "SecretsManager".into(),
        "events" => "EventBridge".into(),
        "monitoring" => "CloudWatch".into(),
        "states" => "Sfn".into(),
        other => other.to_owned(),
    }
}

/// Whether an error says the thing is not there (a delete of something already gone succeeds).
pub(crate) fn gone(e: &ApiError) -> bool {
    e.status == 404
        || ["NotFound", "NoSuch", "NonExistent", "DoesNotExist"]
            .iter()
            .any(|w| e.code.contains(w))
}

/// Makes a resource of a mapped type.
pub(crate) async fn create(rc: &Rc, props: &Value) -> Done {
    let family = family(&rc.ty);
    match family {
        "SQS" => Box::pin(sqs::create(rc, props)).await,
        "SNS" => Box::pin(sns::create(rc, props)).await,
        "SSM" => Box::pin(ssm::create(rc, props)).await,
        "S3" => Box::pin(s3::create(rc, props)).await,
        "DynamoDB" => Box::pin(ddb::create(rc, props)).await,
        "IAM" => Box::pin(iam::create(rc, props)).await,
        "Logs" => Box::pin(logs::create(rc, props)).await,
        "Lambda" => Box::pin(lambda::create(rc, props)).await,
        "Kinesis" => Box::pin(kinesis::create(rc, props)).await,
        "KMS" => Box::pin(kms::create(rc, props)).await,
        "SecretsManager" => Box::pin(secrets::create(rc, props)).await,
        "Events" => Box::pin(events::create(rc, props)).await,
        "CloudWatch" => Box::pin(cloudwatch::create(rc, props)).await,
        _ => Box::pin(more::create(rc, props)).await,
    }
}

/// Updates a resource in place (no create-only property changed).
pub(crate) async fn update(rc: &Rc, old: &Res, props: &Value) -> Done {
    match family(&rc.ty) {
        "SQS" => Box::pin(sqs::update(rc, old, props)).await,
        "SNS" => Box::pin(sns::update(rc, old, props)).await,
        "SSM" => Box::pin(ssm::update(rc, old, props)).await,
        "S3" => Box::pin(s3::update(rc, old, props)).await,
        "DynamoDB" => Box::pin(ddb::update(rc, old, props)).await,
        "IAM" => Box::pin(iam::update(rc, old, props)).await,
        "Logs" => Box::pin(logs::update(rc, old, props)).await,
        "Lambda" => Box::pin(lambda::update(rc, old, props)).await,
        "Kinesis" => Box::pin(kinesis::update(rc, old, props)).await,
        "KMS" => Box::pin(kms::update(rc, old, props)).await,
        "SecretsManager" => Box::pin(secrets::update(rc, old, props)).await,
        "Events" => Box::pin(events::update(rc, old, props)).await,
        "CloudWatch" => Box::pin(cloudwatch::update(rc, old, props)).await,
        _ => Box::pin(more::update(rc, old, props)).await,
    }
}

/// Deletes a resource. One already gone counts as deleted.
pub(crate) async fn delete(rc: &Rc, res: &Res) -> Result<(), String> {
    match family(&rc.ty) {
        "SQS" => Box::pin(sqs::delete(rc, res)).await,
        "SNS" => Box::pin(sns::delete(rc, res)).await,
        "SSM" => Box::pin(ssm::delete(rc, res)).await,
        "S3" => Box::pin(s3::delete(rc, res)).await,
        "DynamoDB" => Box::pin(ddb::delete(rc, res)).await,
        "IAM" => Box::pin(iam::delete(rc, res)).await,
        "Logs" => Box::pin(logs::delete(rc, res)).await,
        "Lambda" => Box::pin(lambda::delete(rc, res)).await,
        "Kinesis" => Box::pin(kinesis::delete(rc, res)).await,
        "KMS" => Box::pin(kms::delete(rc, res)).await,
        "SecretsManager" => Box::pin(secrets::delete(rc, res)).await,
        "Events" => Box::pin(events::delete(rc, res)).await,
        "CloudWatch" => Box::pin(cloudwatch::delete(rc, res)).await,
        _ => Box::pin(more::delete(rc, res)).await,
    }
}

/// `SQS` of `AWS::SQS::Queue`.
pub(crate) fn family(ty: &str) -> &str {
    ty.strip_prefix("AWS::")
        .and_then(|t| t.split("::").next())
        .unwrap_or(ty)
}

/// A text property.
pub(crate) fn text(p: &Value, key: &str) -> Option<String> {
    match p.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

/// A property that may be given as a JSON document or as its text (a policy, a redrive policy), as text.
pub(crate) fn document(p: &Value, key: &str) -> Option<String> {
    match p.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

/// CloudFormation's `[{Key, Value}]` tags with the stack's added (the template's win).
pub(crate) fn tag_list(rc: &Rc, p: &Value) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = rc.tags.clone();
    match p.get("Tags") {
        Some(Value::Array(a)) => {
            for t in a {
                let k = text(t, "Key").unwrap_or_default();
                let v = text(t, "Value").unwrap_or_default();
                out.retain(|(x, _)| *x != k);
                out.push((k, v));
            }
        }
        Some(Value::Object(o)) => {
            for (k, v) in o {
                out.retain(|(x, _)| x != k);
                out.push((
                    k.clone(),
                    v.as_str().map_or_else(|| v.to_string(), str::to_owned),
                ));
            }
        }
        _ => {}
    }
    out
}

pub(crate) use naming::generated_name;
