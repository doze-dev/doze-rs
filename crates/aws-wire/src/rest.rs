//! The restXml protocol (S3): the operation is found from the HTTP method, the path and the query ([`route`]); the request is read
//! into the JSON the model decodes ([`request`], with [`xmlin`] for XML bodies); the answer is written from the JSON a service
//! returns ([`response`]).
//!
//! Large bodies do not go through this module's JSON: a streaming blob (an object being uploaded) reaches the service as an
//! [`Upload`] it pulls from, and one being downloaded leaves as a [`Download`] the gateway pulls from, so neither is ever held
//! whole in memory.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use serde_json::Value;

use crate::ApiError;
use crate::service::{BoxFuture, Call};

pub mod fold;
pub mod jsonin;
pub mod jsonout;
pub mod request;
pub mod response;
pub mod route;
pub mod xmlin;

/// A request body the service reads piece by piece.
pub trait Upload: Send {
    /// The next piece of the payload, or `None` at its end. An error ends the stream: the connection broke, the framing was bad, or
    /// a chunk's signature did not match.
    fn next(&mut self) -> BoxFuture<'_, Result<Option<Bytes>, ApiError>>;

    /// The trailing headers (`x-amz-checksum-crc32` and so on), lower-cased, known once the stream has ended.
    fn trailers(&self) -> Vec<(String, String)>;

    /// The hex SHA-256 the client signed for the payload (`x-amz-content-sha256`), when it gave one: the body must hash to it.
    fn sha256(&self) -> Option<String> {
        None
    }

    /// The payload's length when the client declared it (`x-amz-decoded-content-length`, or `Content-Length` for a plain body).
    fn length(&self) -> Option<u64>;
}

/// A response body the gateway reads piece by piece.
pub trait ByteStream: Send {
    /// The next piece, or `None` at the end.
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<io::Result<Bytes>>>;

    /// The whole body at once, when it is held whole in memory ([`whole`]); `None` for a body that streams.
    fn take_whole(self: Pin<&mut Self>) -> Option<Bytes> {
        None
    }
}

/// A streamed response body.
pub type Download = Pin<Box<dyn ByteStream>>;

/// A body held whole: the bytes of a payload member that is a plain blob (Lambda's `Invoke` answer), handed over beside the JSON so
/// they are never written into it as base64 and read back out.
struct Whole(Option<Bytes>);

impl ByteStream for Whole {
    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<io::Result<Bytes>>> {
        Poll::Ready(self.get_mut().0.take().map(Ok))
    }

    fn take_whole(self: Pin<&mut Self>) -> Option<Bytes> {
        self.get_mut().0.take()
    }
}

/// `bytes` as a body held whole.
#[must_use]
pub fn whole(bytes: Bytes) -> Download {
    Box::pin(Whole(Some(bytes)))
}

/// What a REST service answers.
#[derive(Default)]
pub struct RestReply {
    /// The operation's output as JSON (members bound to headers and the body alike; a streaming blob is not in it, nor a payload
    /// blob carried whole in `body`).
    pub json: Value,
    /// The status when it is not the model's (206 for a ranged read).
    pub status: Option<u16>,
    /// The payload of a streaming blob output; or the bytes of a plain blob payload, held whole ([`whole`]).
    pub body: Option<Download>,
}

impl RestReply {
    /// The bytes of a body held whole ([`whole`]), taken out of the reply; `None` (and the body left alone) for one that streams.
    pub fn take_whole(&mut self) -> Option<Bytes> {
        let bytes = self.body.as_mut()?.as_mut().take_whole()?;
        self.body = None;
        Some(bytes)
    }

    /// The reply as a caller that reads only JSON sees it: a payload held whole goes back into the payload member of operation `op`'s
    /// output, as base64 (a call on the bus, whose callers read `Payload` from the JSON). A reply with none is left as it is.
    #[must_use]
    pub fn payload_in_json(mut self, tables: &aws_schema::tables::Tables, op: u32) -> Self {
        let member = jsonout::blob_payload(tables, op);
        if let (Some(name), Value::Object(map)) = (member, &mut self.json)
            && let Some(bytes) = self.body.as_mut().and_then(|b| b.as_mut().take_whole())
        {
            map.insert(
                name.to_owned(),
                Value::String(aws_core::base64::encode(&bytes)),
            );
            self.body = None;
        }
        self
    }
}

impl std::fmt::Debug for RestReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RestReply")
            .field("json", &self.json)
            .field("status", &self.status)
            .field("body", &self.body.is_some())
            .finish()
    }
}

/// A service that speaks a REST protocol. Its `call.body` is the input as JSON; a streaming input arrives as `upload`.
pub trait RestService: Send + Sync {
    /// Handles one call.
    fn call_rest(
        &self,
        call: Call,
        upload: Option<Box<dyn Upload>>,
    ) -> BoxFuture<'_, Result<RestReply, ApiError>>;

    /// The headers to add to the answer of a request a browser page made (it carries `Origin`) for `bucket`: S3's CORS headers.
    /// Asked after the call, whether it succeeded or not; the default adds none.
    fn cors<'a>(
        &'a self,
        _region: &'a str,
        _bucket: &'a str,
        _origin: &'a str,
        _method: &'a str,
    ) -> BoxFuture<'a, Vec<(String, String)>> {
        Box::pin(async { Vec::new() })
    }

    /// How many leading bytes of `path` are the endpoint the client was given rather than the operation's path: the API Gateway
    /// management API is called at `<api>.execute-api.<region>.amazonaws.com/<stage>` (locally `/_aws/execute-api/<api>/<stage>`),
    /// and its routes start after that. The gateway routes the rest, and hands the cut prefix on in the caller's `endpoint`. The
    /// default cuts nothing.
    fn base_path(&self, _host: &str, _path: &str) -> usize {
        0
    }
}
