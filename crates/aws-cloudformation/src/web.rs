//! The response URL a custom resource's function answers on. AWS hands the function a presigned S3 URL in a bucket of its own; here
//! the URL has the same path-style shape (`/cloudformation-custom-resource-response-<region>/<key>?X-Amz-...`) on the local
//! endpoint, and the gateway hands a request to it to this service before it looks for a signature. The `PUT` body is delivered to
//! the operation waiting for it; the key carries a random token that only that operation knows, so nothing else can answer.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use aws_wire::service::{WebReply, WebRequest};
use serde_json::Value;
use tokio::sync::oneshot;

/// The bucket-like first segment of a response URL's path.
const PREFIX: &str = "/cloudformation-custom-resource-response-";

/// Whether a path is a response URL's.
pub(crate) fn owns(path: &str) -> bool {
    path.starts_with(PREFIX)
}

/// The operations waiting for a function's answer, by token.
#[derive(Debug, Default)]
pub(crate) struct Responses {
    waiting: Mutex<HashMap<String, oneshot::Sender<Value>>>,
}

impl Responses {
    /// A response URL and the receiver its answer arrives on. `endpoint` is the scheme and host functions reach this process at.
    pub(crate) fn expect(
        &self,
        endpoint: &str,
        region: &str,
        stack_id: &str,
        logical: &str,
    ) -> (String, oneshot::Receiver<Value>) {
        let token = aws_seal::alphanumeric(40).unwrap_or_default();
        let (tx, rx) = oneshot::channel();
        self.waiting
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(token.clone(), tx);
        let key = aws_core::uri::encode(format!("{stack_id}|{logical}|{token}").as_bytes(), false);
        let url = format!(
            "{endpoint}{PREFIX}{}/{key}?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Expires=7200&X-Amz-SignedHeaders=host&X-Amz-Signature={token}",
            region.replace('-', "")
        );
        (url, rx)
    }

    /// Stops waiting for a token (its operation gave up).
    pub(crate) fn forget(&self, url: &str) {
        if let Some(token) = token_of(url) {
            self.waiting
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&token);
        }
    }

    /// Answers a `PUT` to a response URL.
    pub(crate) fn serve(&self, request: &WebRequest) -> WebReply {
        let reply = |status: u16, body: &str| WebReply {
            status,
            headers: vec![("content-type".into(), "application/xml".into())],
            body: bytes::Bytes::from(body.to_owned()),
        };
        if request.method != "PUT" {
            return reply(
                405,
                "<Error><Code>MethodNotAllowed</Code><Message>The specified method is not allowed against this resource.</Message></Error>",
            );
        }
        let token = token_of(&format!("{}?{}", request.path, request.query));
        let sender = token.and_then(|t| {
            self.waiting
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&t)
        });
        let Some(sender) = sender else {
            // (memory) An expired or unknown presigned URL.
            return reply(
                403,
                "<Error><Code>AccessDenied</Code><Message>Request has expired</Message></Error>",
            );
        };
        let Ok(body) = serde_json::from_slice::<Value>(&request.body) else {
            drop(sender.send(Value::Null));
            return reply(200, "");
        };
        drop(sender.send(body));
        reply(200, "")
    }
}

/// The token a response URL carries (its signature).
fn token_of(url: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    query
        .split('&')
        .find_map(|kv| kv.strip_prefix("X-Amz-Signature="))
        .map(str::to_owned)
}
