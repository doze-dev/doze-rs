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
}

/// A streamed response body.
pub type Download = Pin<Box<dyn ByteStream>>;

/// What a REST service answers.
#[derive(Default)]
pub struct RestReply {
    /// The operation's output as JSON (members bound to headers and the body alike; a streaming blob is not in it).
    pub json: Value,
    /// The status when it is not the model's (206 for a ranged read).
    pub status: Option<u16>,
    /// The payload of a streaming blob output.
    pub body: Option<Download>,
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
}
