//! Structured errors on every call (protocol.md row 5).

use super::value::PathSeg;
use minicbor::{Decode, Encode};

/// What kind of failure. Carried as `Option` so a kind added later reads as `None` (treated as internal) by an older
/// build instead of failing to decode.
/// Encoded as a one-item array, not `index_only`: only that form decodes an unknown variant as `None` inside `Option`
/// (tests/compat.rs found `index_only` fails instead).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
pub enum ErrorKind {
    /// The input is wrong (config, arguments); `diagnostics` say where.
    #[n(0)]
    Invalid,
    /// The module does not do this (a capability it did not declare, a platform it does not support).
    #[n(1)]
    Unsupported,
    /// Something named does not exist.
    #[n(2)]
    NotFound,
    /// A dependency is not there right now (the engine is down, the network is gone); retrying may help.
    #[n(3)]
    Unavailable,
    /// The call's deadline passed.
    #[n(4)]
    TimedOut,
    /// The caller cancelled.
    #[n(5)]
    Cancelled,
    /// A bug or an unexpected state.
    #[n(6)]
    Internal,
}

/// One problem at one place in the instance's configuration.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
pub struct Diagnostic {
    /// Where, as a path into the instance's spec (empty: the block as a whole).
    #[n(0)]
    pub path: Vec<PathSeg>,
    #[n(1)]
    pub message: String,
    /// What to do about it.
    #[n(2)]
    pub hint: Option<String>,
}

/// A failed call.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, thiserror::Error)]
#[error("{message}")]
pub struct Error {
    #[n(0)]
    pub kind: Option<ErrorKind>,
    #[n(1)]
    pub message: String,
    #[n(2)]
    #[cbor(default)]
    pub diagnostics: Vec<Diagnostic>,
    #[n(3)]
    #[cbor(default)]
    pub retryable: bool,
}

impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Error {
        let retryable = matches!(kind, ErrorKind::Unavailable | ErrorKind::TimedOut);
        Error {
            kind: Some(kind),
            message: message.into(),
            diagnostics: Vec::new(),
            retryable,
        }
    }

    /// An invalid-input error with diagnostics.
    pub fn invalid(message: impl Into<String>, diagnostics: Vec<Diagnostic>) -> Error {
        Error {
            diagnostics,
            ..Error::new(ErrorKind::Invalid, message)
        }
    }

    /// The kind, with an unknown one read as internal.
    pub fn kind(&self) -> ErrorKind {
        self.kind.unwrap_or(ErrorKind::Internal)
    }
}
