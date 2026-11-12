//! The bus between services. A service that needs another (SSM sealing a `SecureString` with KMS, SNS delivering to SQS) calls the
//! other's operation by name, with a JSON body, as itself acting for the caller. The call goes through the other service's own
//! model: the body is decoded and checked as a request from the wire would be, the service answers as it would, and the answer
//! is read back as JSON. There is no socket, no signature and no URL.
//!
//! A service never calls one that calls it back while it waits (that would wait forever); the order in use is SSM and Secrets
//! Manager to KMS, SSM to Secrets Manager, and, later, S3, SNS, EventBridge and Lambda to SQS, SNS and Lambda.

mod authz;
pub mod envelope;

pub use authz::{Authorizer, Mode, Need};

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use aws_schema::arena::{Arena, NIL};
use aws_schema::enc::encode;
use aws_schema::json::decode;
use aws_schema::patterns::Patterns;
use aws_wire::service::{Call, Caller, Service};
use aws_wire::{ApiError, RequestId};
use serde_json::Value;

struct Entry {
    service: Arc<dyn Service>,
    patterns: Patterns,
}

/// The services that can be called, by the name they are signed for (`kms`, `sqs`, `secretsmanager`).
#[derive(Default)]
pub struct Peers {
    entries: RwLock<HashMap<&'static str, Entry>>,
    authorizer: RwLock<Option<Arc<dyn Authorizer>>>,
    mode: authz::ModeCell,
}

impl std::fmt::Debug for Peers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Peers")
    }
}

impl Peers {
    /// An empty bus.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Makes `service` callable by its signing name.
    pub fn register(&self, service: Arc<dyn Service>) {
        let name = service.meta().signing_name;
        self.register_as(name, service);
    }

    /// Makes `service` callable as `name`, for a service whose signing name another already has (API Gateway's HTTP APIs sign as
    /// `apigateway` like its REST APIs, and are called `apigatewayv2`).
    pub fn register_as(&self, name: &'static str, service: Arc<dyn Service>) {
        let patterns = Patterns::new(service.tables());
        if let Ok(mut entries) = self.entries.write() {
            entries.insert(name, Entry { service, patterns });
        }
    }

    /// Calls operation `operation` of service `target` with `body`, as service `acting` on behalf of `caller`. The answer is the
    /// operation's output as JSON (`{}` for an operation with none).
    ///
    /// # Errors
    /// The target's own error; or an internal error if there is no such service or operation, or the answer cannot be read.
    pub async fn call(
        &self,
        acting: &str,
        caller: &Caller,
        target: &str,
        operation: &str,
        body: &Value,
    ) -> Result<Value, ApiError> {
        let (service, call) = self.prepare(acting, caller, target, operation, body)?;
        let output = service.call(call).await?;
        let tables = service.tables();
        let shape = tables
            .find_op(operation)
            .and_then(|i| tables.op(i))
            .and_then(|o| o.output);
        let Some(shape) = shape.filter(|_| output.root != NIL) else {
            return Ok(Value::Object(serde_json::Map::new()));
        };
        let mut bytes = Vec::new();
        encode(tables, shape, &output.arena, "", output.root, &mut bytes)
            .map_err(|_| ApiError::internal("a peer's answer could not be written"))?;
        serde_json::from_slice(&bytes)
            .map_err(|_| ApiError::internal("a peer's answer could not be read"))
    }

    /// Like [`Peers::call`], for a target that speaks a REST protocol (S3): the answer keeps its streamed body, which the caller reads.
    /// There is no request body stream; an operation that needs one cannot be called this way.
    ///
    /// # Errors
    /// The target's own error; or an internal error if there is no such service or operation, or it has no REST side.
    pub async fn call_rest(
        &self,
        acting: &str,
        caller: &Caller,
        target: &str,
        operation: &str,
        body: &Value,
    ) -> Result<aws_wire::rest::RestReply, ApiError> {
        let (service, call) = self.prepare(acting, caller, target, operation, body)?;
        let rest = service
            .rest()
            .ok_or_else(|| ApiError::internal(format!("{target} has no REST side")))?;
        rest.call_rest(call, None).await
    }

    /// The target service and the decoded call.
    fn prepare(
        &self,
        acting: &str,
        caller: &Caller,
        target: &str,
        operation: &str,
        body: &Value,
    ) -> Result<(Arc<dyn Service>, Call), ApiError> {
        let entries = self
            .entries
            .read()
            .map_err(|_| ApiError::internal("the peer bus is unavailable"))?;
        let entry = entries.get(target).ok_or_else(|| {
            ApiError::internal(format!("no service {target} is running to be called"))
        })?;
        let tables = entry.service.tables();
        let op = tables
            .find_op(operation)
            .ok_or_else(|| ApiError::internal(format!("{target} has no operation {operation}")))?;
        if !entry.service.serves(op) {
            return Err(ApiError::new(
                501,
                "NotImplemented",
                format!("{operation} is not built in doze-aws."),
            ));
        }
        let shape = tables.op(op).and_then(|o| o.input);
        let text = body.to_string();
        let mut arena = Arena::new();
        let root = match shape {
            Some(shape) => decode(tables, &entry.patterns, shape, &text, &mut arena)
                .map_err(|v| entry.service.invalid(&v))?,
            None => NIL,
        };
        let mut caller = caller.clone();
        caller.service = Some(acting.to_owned());
        Ok((
            Arc::clone(&entry.service),
            Call {
                op,
                id: RequestId::new(),
                caller,
                body: text,
                arena,
                root,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use aws_wire::service::Caller;
    use doze_runtime::{Ctx, SystemClock, Tasks};
    use serde_json::json;

    use super::*;

    fn caller() -> Caller {
        Caller {
            access_key_id: "test".into(),
            account_id: "000000000000".into(),
            region: "us-east-1".into(),
            endpoint: "http://localhost".into(),
            service: None,
            session_token: None,
        }
    }

    fn bus(dir: &std::path::Path) -> Arc<Peers> {
        let peers = Peers::new();
        let ctx = Ctx {
            dir: dir.to_path_buf(),
            idle: std::time::Duration::from_secs(60),
            tasks: Tasks::new(),
            clock: Arc::new(SystemClock),
            ext: None,
        };
        peers.register(Arc::new(aws_kms::Kms::new(Arc::new(ctx))));
        peers
    }

    #[tokio::test]
    async fn a_call_goes_through_the_target_model_and_back() {
        let dir = std::env::temp_dir().join(format!("doze-aws-peers-{}", std::process::id()));
        let peers = bus(&dir);
        let made = peers
            .call("test", &caller(), "kms", "CreateKey", &json!({}))
            .await
            .unwrap();
        assert_eq!(made["KeyMetadata"]["KeyState"], "Enabled");
        // The target's own checks apply to the body, as they would to a request from the wire.
        let bad = peers
            .call(
                "test",
                &caller(),
                "kms",
                "ScheduleKeyDeletion",
                &json!({"PendingWindowInDays": 9}),
            )
            .await
            .unwrap_err();
        assert_eq!(bad.code, "ValidationException");
        // An operation the target refuses by name is refused here too.
        let refused = peers
            .call("test", &caller(), "kms", "CreateCustomKeyStore", &json!({}))
            .await
            .unwrap_err();
        assert_eq!(refused.status, 501);
        assert!(
            peers
                .call("test", &caller(), "nope", "X", &json!({}))
                .await
                .unwrap_err()
                .message
                .contains("no service nope")
        );
        assert!(
            peers
                .call("test", &caller(), "kms", "Nope", &json!({}))
                .await
                .unwrap_err()
                .message
                .contains("no operation Nope")
        );
        drop(std::fs::remove_dir_all(&dir));
    }

    #[tokio::test]
    async fn state_is_kept_apart_by_the_region_a_call_was_signed_for() {
        let dir = std::env::temp_dir().join(format!("doze-aws-regions-{}", std::process::id()));
        let peers = bus(&dir);
        let in_region = |region: &str| Caller {
            region: region.into(),
            ..caller()
        };
        let made = peers
            .call(
                "test",
                &in_region("eu-west-1"),
                "kms",
                "CreateKey",
                &json!({}),
            )
            .await
            .unwrap();
        let id = made["KeyMetadata"]["KeyId"].as_str().unwrap().to_owned();
        let seen = |region: &str| {
            let (peers, caller) = (Arc::clone(&peers), in_region(region));
            async move {
                peers
                    .call("test", &caller, "kms", "ListKeys", &json!({}))
                    .await
                    .unwrap()["Keys"]
                    .as_array()
                    .unwrap()
                    .len()
            }
        };
        assert_eq!((seen("eu-west-1").await, seen("us-east-1").await), (1, 0));
        let elsewhere = peers
            .call(
                "test",
                &in_region("us-east-1"),
                "kms",
                "DescribeKey",
                &json!({"KeyId": id}),
            )
            .await
            .unwrap_err();
        assert_eq!(elsewhere.code, "NotFoundException");
        // The region the files are kept in for itself is the home region's: the others go in a directory of their own.
        assert!(dir.join("regions").join("eu-west-1").exists());
        drop(std::fs::remove_dir_all(&dir));
    }
}
