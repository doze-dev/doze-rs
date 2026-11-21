//! The console as a caller on the peer bus.
//!
//! Its calls are made as the caller itself ([`aws_peers::Peers::call_as`]), with the access key [`CONSOLE`]: the target asks the
//! authorizer about it as about a request from the wire. IAM resolves a key it did not issue to the account root, which is
//! allowed everything, so `--iam enforce` lets the console do what the person running doze-aws can (the console is served on
//! loopback only, with no login: it is that person). IAM's access log says so for each of its calls.
//!
//! Each call runs as [`aws_peers::seen::asked`]: the Live record shows it, the console's reads of a queue's attributes
//! included, which a background poll's would not be.

use std::sync::Arc;

use aws_peers::Peers;
use aws_schema::meta::Protocol;
use aws_wire::ApiError;
use aws_wire::rest::{RestReply, Upload};
use aws_wire::service::Caller;
use axum::body::Bytes;
use serde_json::Value;

/// The access key the console's calls carry, and the caller Live shows them under.
pub const CONSOLE: &str = "console";

/// The console's caller on the bus.
#[derive(Debug, Clone)]
pub struct Bus {
    peers: Arc<Peers>,
    caller: Caller,
}

impl Bus {
    /// The console calling on `peers` for `account`, in `region`; answers that hold a URL begin with `endpoint`.
    #[must_use]
    pub fn new(peers: Arc<Peers>, account: &str, region: &str, endpoint: &str) -> Self {
        Self {
            peers,
            caller: Caller {
                access_key_id: CONSOLE.to_owned(),
                account_id: account.to_owned(),
                region: region.to_owned(),
                endpoint: endpoint.to_owned(),
                service: None,
                session_token: None,
            },
        }
    }

    /// The account it calls for.
    #[must_use]
    pub fn account(&self) -> &str {
        &self.caller.account_id
    }

    /// The region it calls in.
    #[must_use]
    pub fn region(&self) -> &str {
        &self.caller.region
    }

    /// Calls `operation` of `target` (its name on the bus) with `body`; the answer as JSON. A service that speaks a REST protocol
    /// is called on its REST side; its streamed body (an object's bytes) is not read: no page asks for one.
    ///
    /// # Errors
    /// The target's refusal, IAM's included, with the code and status a client of its wire sees (Query-only services are worded as their wire words them).
    pub async fn call(
        &self,
        target: &str,
        operation: &str,
        body: &Value,
    ) -> Result<Value, ApiError> {
        let answered = aws_peers::seen::asked(async {
            if self.peers.speaks_rest(target) {
                self.peers
                    .call_rest_as(&self.caller, target, operation, body, None)
                    .await
                    .map(|reply| reply.json)
            } else {
                self.peers
                    .call_as(&self.caller, target, operation, body)
                    .await
            }
        })
        .await;
        answered.map_err(|e| self.on_the_wire(target, e))
    }

    /// A refusal of a service whose only wire is the Query protocol (SNS, IAM, STS, CloudFormation), worded as that wire words it: the
    /// model's Query code and status (`InvalidParameter`, `NotFound` with 404), not the shape's name (`InvalidParameterException`).
    /// What a person reads in the console is then what the AWS CLI prints. Every other service keeps its own code.
    fn on_the_wire(&self, target: &str, mut e: ApiError) -> ApiError {
        let Some(shape) = e.shape.clone() else {
            return e;
        };
        let all = self.peers.targets();
        let Some((_, api)) = all.iter().find(|(name, _)| *name == target) else {
            return e;
        };
        if api.meta().protocols != [Protocol::AwsQuery] {
            return e;
        }
        let tables = api.tables();
        let found = (0..tables.n_errs())
            .filter_map(|i| tables.err(i))
            .find(|m| tables.shape(m.shape).is_some_and(|s| s.name == shape));
        if let Some(m) = found {
            e.code = m.code.to_owned().into();
            e.status = m.status;
        }
        e
    }

    /// Calls `operation` of the REST service `target` with `body` and the request payload `payload` (an object's bytes, held
    /// whole: the console's upload is capped); the whole answer, its streamed body included.
    ///
    /// # Errors
    /// The target's refusal, IAM's included.
    pub async fn call_rest(
        &self,
        target: &str,
        operation: &str,
        body: &Value,
        payload: Option<Bytes>,
    ) -> Result<RestReply, ApiError> {
        let upload = payload.map(|p| Box::new(Whole(Some(p))) as Box<dyn Upload>);
        aws_peers::seen::asked(self.peers.call_rest_as(
            &self.caller,
            target,
            operation,
            body,
            upload,
        ))
        .await
    }

    /// Calls `operation` of `target`, whose answer is an event stream (`StartLiveTail`): the framed messages as they come.
    ///
    /// # Errors
    /// The target's refusal, IAM's included.
    pub async fn stream(
        &self,
        target: &str,
        operation: &str,
        body: &Value,
    ) -> Result<aws_wire::rest::Download, ApiError> {
        aws_peers::seen::asked(
            self.peers
                .call_stream_as(&self.caller, target, operation, body),
        )
        .await
    }
}

/// A request payload held whole, handed to the service as one piece.
struct Whole(Option<Bytes>);

impl Upload for Whole {
    fn next(&mut self) -> aws_wire::service::BoxFuture<'_, Result<Option<Bytes>, ApiError>> {
        let piece = self.0.take();
        Box::pin(async move { Ok(piece) })
    }

    fn trailers(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    fn length(&self) -> Option<u64> {
        self.0.as_ref().map(|b| b.len() as u64)
    }
}
