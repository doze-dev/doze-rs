//! What can go wrong talking to the daemon, in the words Go's client uses.

use crate::codec::CodecError;
use crate::types::frame::Response;
use std::io;
use std::time::Duration;

/// A failed call, connection or stream.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Nothing is listening, or the socket could not be opened.
    #[error("cannot reach the daemon: {0}")]
    Connect(io::Error),
    /// Writing to or reading from an open connection failed.
    #[error("the connection to the daemon failed: {0}")]
    Io(#[from] io::Error),
    /// The daemon did not answer the hello.
    #[error(
        "the daemon did not answer the hello (it may be an older doze — run `doze down` and try again): {0}"
    )]
    NoHello(String),
    /// The daemon refused the connection and said why (a protocol mismatch, in its words).
    #[error("{0}")]
    Refused(String),
    /// The daemon greeted with another protocol version.
    #[error(
        "the daemon speaks a different control protocol than this doze ({}) — run `doze down` and try again",
        crate::PROTO_VERSION
    )]
    Protocol,
    /// The daemon answered with an error; `code` is its kind (see [`crate::fault`]), empty for plain `failed`.
    #[error("{message}")]
    Daemon { message: String, code: String },
    /// A unary request was answered with a frame that carried no response.
    #[error("the daemon answered {0:?} with no response")]
    NoResponse(String),
    /// A stream's first frame was not its acknowledgement.
    #[error("the daemon answered {0:?} with no acknowledgement")]
    NoAck(String),
    /// The connection ended while a request was waiting.
    #[error("the connection to the daemon closed: {0}")]
    Closed(String),
    /// The daemon did not answer in time. What was asked may still happen: the daemon finishes a command whose
    /// client stopped waiting.
    #[error("the daemon did not answer within {0:?}")]
    Timeout(Duration),
    /// A frame that could not be encoded or decoded.
    #[error(transparent)]
    Codec(#[from] CodecError),
}

impl Error {
    /// The daemon's error kind, if it sent one (`not_found`, `timeout`, …).
    pub fn code(&self) -> Option<&str> {
        match self {
            Error::Daemon { code, .. } if !code.is_empty() => Some(code),
            _ => None,
        }
    }

    /// The error a response that failed carries.
    pub fn from_response(r: &Response) -> Error {
        Error::Daemon {
            message: r.error.clone(),
            code: r.code.clone(),
        }
    }

    /// Whether the daemon was never reached or the connection is gone: the caller may try again later.
    pub fn is_unreachable(&self) -> bool {
        matches!(
            self,
            Error::Connect(_) | Error::Io(_) | Error::Closed(_) | Error::NoHello(_)
        )
    }
}
