//! The `@connections` management API (`apigatewaymanagementapi`, signing name `execute-api`): `PostToConnection`, `GetConnection`
//! and `DeleteConnection` on a WebSocket API's open connections. A backend calls it at the stage's address
//! (`https://<api>.execute-api.<region>.amazonaws.com/<stage>`, locally `<endpoint>/_aws/execute-api/<api>/<stage>`); the gateway
//! routes and checks it from the model like any REST service, the base path cut off and handed on in the caller's endpoint
//! ([`RestService::base_path`]), and IAM is asked for `execute-api:ManageConnections` on the connection's ARN.

use std::sync::Arc;

use aws_model_apigatewaymanagementapi::Operation;
use aws_model_apigatewaymanagementapi::meta::META;
use aws_model_apigatewaymanagementapi::tables::TABLES;
use aws_peers::{Mode, Need};
use aws_schema::json::Violation;
use aws_schema::meta::ServiceMeta;
use aws_schema::tables::Tables;
use aws_wire::ApiError;
use aws_wire::rest::{RestReply, RestService, Upload};
use aws_wire::service::{BoxFuture, Call, Reply, Service};
use bytes::Bytes;
use serde_json::Value;

use crate::Inner;
use crate::plane::ws::{Manage, manage};

/// The management API, over the same connections as the WebSocket APIs it manages.
#[derive(Debug, Clone)]
pub struct Management {
    pub(crate) inner: Arc<Inner>,
}

impl Management {
    async fn run(&self, call: Call) -> Result<RestReply, ApiError> {
        let (api, stage, region) = stage_of(&call.caller.endpoint)
            .ok_or_else(|| ApiError::new(404, "NotFoundException", "Not Found"))?;
        let region = region.unwrap_or_else(|| call.caller.region.clone());
        let input: Value = serde_json::from_str(&call.body).unwrap_or_default();
        let id = input
            .get("ConnectionId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let (what, method) = match Operation::from_index(call.op) {
            Some(Operation::PostToConnection) => {
                let data = input
                    .get("Data")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let bytes = aws_core::base64::decode(data).unwrap_or_default();
                (Manage::Post(Bytes::from(bytes)), "POST")
            }
            Some(Operation::GetConnection) => (Manage::Get, "GET"),
            Some(Operation::DeleteConnection) => (Manage::Delete, "DELETE"),
            None => return Err(ApiError::internal("not an operation of the model")),
        };
        if let Some(peers) = self.inner.peers.as_ref().filter(|p| p.mode() != Mode::Off) {
            let arn = format!(
                "arn:aws:execute-api:{region}:{}:{api}/{stage}/{method}/@connections/{id}",
                call.caller.account_id
            );
            peers
                .authorize(
                    &call.caller,
                    vec![Need::new("execute-api:ManageConnections", arn)],
                )
                .await
                .map_err(forbidden)?;
        }
        let json = manage(&self.inner, &api, &id, what)?;
        Ok(RestReply {
            json,
            status: None,
            body: None,
        })
    }
}

/// The API, stage and (for an execute-api host) region a caller's endpoint names: `http://<host>/_aws/execute-api/<api>/<stage>` or
/// `http://<api>.execute-api.<region>.<domain>/<stage>`.
fn stage_of(endpoint: &str) -> Option<(String, String, Option<String>)> {
    let rest = endpoint.split_once("://").map_or(endpoint, |(_, r)| r);
    let (host, path) = rest.split_once('/').map_or((rest, ""), |(h, p)| (h, p));
    let addr = crate::plane::addr(host, &format!("/{path}"))?;
    let stage = addr
        .rest
        .split('/')
        .next()
        .filter(|s| !s.is_empty())?
        .to_owned();
    Some((addr.api, stage, addr.region))
}

/// The management API calls a refusal `ForbiddenException` (403). (memory)
fn forbidden(mut e: ApiError) -> ApiError {
    if e.code.starts_with("AccessDenied") {
        e.code = "ForbiddenException".into();
        e.status = 403;
    }
    e
}

impl Service for Management {
    fn meta(&self) -> &'static ServiceMeta {
        &META
    }

    fn tables(&self) -> &'static Tables {
        &TABLES
    }

    fn serves(&self, op: u32) -> bool {
        Operation::from_index(op).is_some()
    }

    /// A call from a peer: the connection lives with the gateway, so the management API is served over HTTP only.
    fn call(&self, call: Call) -> BoxFuture<'_, Reply> {
        Box::pin(async move {
            Err(ApiError::new(
                501,
                "NotImplemented",
                format!("{} is served over HTTP only.", call.id),
            ))
        })
    }

    fn rest(&self) -> Option<&dyn RestService> {
        Some(self)
    }

    fn invalid(&self, violation: &Violation) -> ApiError {
        let mut e = aws_wire::service::default_invalid(&TABLES, violation);
        e.code = "BadRequestException".into();
        e
    }
}

impl RestService for Management {
    fn call_rest(
        &self,
        call: Call,
        _upload: Option<Box<dyn Upload>>,
    ) -> BoxFuture<'_, Result<RestReply, ApiError>> {
        Box::pin(self.run(call))
    }

    fn base_path(&self, _host: &str, path: &str) -> usize {
        path.find("/@connections/").unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_endpoint_names_the_api_and_stage() {
        assert_eq!(
            stage_of("http://127.0.0.1:4566/_aws/execute-api/abc123/prod"),
            Some(("abc123".into(), "prod".into(), None))
        );
        assert_eq!(
            stage_of("http://abc123.execute-api.eu-west-1.localhost:4566/dev"),
            Some(("abc123".into(), "dev".into(), Some("eu-west-1".into())))
        );
        assert_eq!(stage_of("http://127.0.0.1:4566"), None);
    }
}
