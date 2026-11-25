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
    /// A refusal of one field of an action's form (minor 2): the field's id, for the form to mark. Set only by
    /// [`Error::for_field`], never for a problem in the instance's configuration (those are `diagnostics`).
    #[n(4)]
    pub field: Option<String>,
}

impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Error {
        let retryable = matches!(kind, ErrorKind::Unavailable | ErrorKind::TimedOut);
        Error {
            kind: Some(kind),
            message: message.into(),
            diagnostics: Vec::new(),
            retryable,
            field: None,
        }
    }

    /// A refusal of the form field `field`: `message` is shown next to it (and is the whole of the error's text).
    pub fn for_field(field: impl Into<String>, message: impl Into<String>) -> Error {
        Error {
            field: Some(field.into()),
            ..Error::new(ErrorKind::Invalid, message)
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

    /// The message, then each diagnostic on a line of its own: where in the block (`role[0].password`), what is
    /// wrong, and what to do about it. What a host says when a call fails: the message alone ("the postgres block
    /// `x` is not valid") names no problem.
    pub fn describe(&self) -> String {
        let mut out = self.message.clone();
        for d in &self.diagnostics {
            out.push('\n');
            let at = path(&d.path);
            if !at.is_empty() {
                out.push_str(&at);
                out.push_str(": ");
            }
            out.push_str(&d.message);
            if let Some(h) = d.hint.as_deref().filter(|h| !h.is_empty()) {
                out.push_str(" (");
                out.push_str(h);
                out.push(')');
            }
        }
        out
    }
}

/// A path into a block as it reads in HCL: `role[0].connection_limit`.
fn path(p: &[PathSeg]) -> String {
    let mut out = String::new();
    for seg in p {
        match seg {
            PathSeg::Key(k) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(k);
            }
            PathSeg::Index(i) => out.push_str(&format!("[{i}]")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_names_each_problem_where_it_is() {
        let e = Error::invalid(
            "the db block `x` is not valid",
            vec![
                Diagnostic {
                    path: vec![],
                    message: "the block is wrong".into(),
                    hint: Some("pick another label".into()),
                },
                Diagnostic {
                    path: vec![
                        PathSeg::Key("role".into()),
                        PathSeg::Index(1),
                        PathSeg::Key("connection_limit".into()),
                    ],
                    message: "a limit is -1 or more".into(),
                    hint: None,
                },
            ],
        );
        assert_eq!(
            e.describe(),
            "the db block `x` is not valid\nthe block is wrong (pick another label)\nrole[1].connection_limit: a limit is -1 or more"
        );
        let plain = Error::new(ErrorKind::Unavailable, "down");
        assert_eq!(plain.describe(), "down");
    }
}
