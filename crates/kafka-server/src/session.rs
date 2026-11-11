//! Where a connection is in its login, and what that lets it ask for.
//!
//! The order is the one Apache Kafka 4.3.1 keeps and the `sasl-plain` recording shows. Before the handshake a
//! client may send one ApiVersions and then SaslHandshake; nothing else is answered, and a second ApiVersions is
//! not either. After the handshake only SaslAuthenticate is welcome; ApiVersions and SaslHandshake are answered
//! ILLEGAL_SASL_STATE and the connection is closed. Once logged in every API is served, except the two SASL
//! ones, which answer ILLEGAL_SASL_STATE and leave the connection as it was: a login never changes who a
//! connection is.
//!
//! The verdict is given on the request's header alone, so the body of a stranger's request is never decoded
//! for an API it may not use.

use kafka_auth::{Mechanism, Principal};
use kafka_wire::messages::ApiKey;

pub(crate) mod steps;

/// A connection's login state. A broker with SASL off has none (the connection holds no `Session`).
#[derive(Debug)]
pub(crate) enum Session {
    /// Nothing proved yet. `versions_answered` is whether the one ApiVersions this state allows has been
    /// answered; an ApiVersions that was answered with an error (a version not served, a badly named client)
    /// does not use it up, as in Kafka.
    Unauthenticated { versions_answered: bool },
    /// The mechanism is chosen and the login is in progress.
    Handshaken(Mechanism),
    /// Logged in, as this principal, for the life of the connection.
    Authenticated(Principal),
}

/// What to do with a request, given its API alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Read the body and serve it.
    Serve,
    /// Close the connection without an answer.
    Close,
    /// Answer ILLEGAL_SASL_STATE in the request's own shape, without reading its body. The connection then
    /// lives on if `stays_open` (logged in), and is closed otherwise (in the middle of a login).
    IllegalState { stays_open: bool },
}

impl Session {
    /// The state of a connection that has just arrived.
    pub(crate) fn new() -> Self {
        Self::Unauthenticated {
            versions_answered: false,
        }
    }

    /// What may be done with a request to `key` now.
    pub(crate) fn admit(&self, key: ApiKey) -> Verdict {
        use ApiKey::{ApiVersions, SaslAuthenticate, SaslHandshake};
        match (self, key) {
            (
                Self::Unauthenticated {
                    versions_answered: false,
                },
                ApiVersions,
            )
            | (Self::Unauthenticated { .. }, SaslHandshake)
            | (Self::Handshaken(_), SaslAuthenticate) => Verdict::Serve,
            (Self::Handshaken(_), ApiVersions | SaslHandshake) => {
                Verdict::IllegalState { stays_open: false }
            }
            (Self::Authenticated(_), SaslHandshake | SaslAuthenticate) => {
                Verdict::IllegalState { stays_open: true }
            }
            (Self::Authenticated(_), _) => Verdict::Serve,
            (Self::Unauthenticated { .. } | Self::Handshaken(_), _) => Verdict::Close,
        }
    }

    /// An ApiVersions was answered with the list of APIs: the one before the handshake is used.
    pub(crate) fn versions_answered(&mut self) {
        if let Self::Unauthenticated { versions_answered } = self {
            *versions_answered = true;
        }
    }

    /// Whether the login is complete.
    pub(crate) fn is_authenticated(&self) -> bool {
        matches!(self, Self::Authenticated(_))
    }

    /// Who the connection is, once it has logged in.
    pub(crate) fn principal(&self) -> Option<&Principal> {
        match self {
            Self::Authenticated(p) => Some(p),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
