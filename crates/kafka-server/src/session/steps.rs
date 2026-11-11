//! The two SASL requests, answered: pure functions from a session and a request to the exact response Kafka
//! gives, and what happens to the connection after it.
//!
//! Nothing the client sent is ever put in an answer or a log: the texts are Kafka's own (the PLAIN ones come
//! from `kafka-auth`), and the mechanism list is the server's.

use bytes::Bytes;
use kafka_auth::{Authenticator, Mechanism};
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::messages::{
    SaslAuthenticateRequest, SaslAuthenticateResponse, SaslHandshakeRequest, SaslHandshakeResponse,
};

use super::Session;

/// What becomes of the connection once the answer is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fate {
    /// It carries on.
    Open,
    /// It is closed, after the answer has been written and flushed: a client is told why it failed.
    Closing,
}

/// The text of the answer to a SaslAuthenticate on a connection that has already logged in.
const AFTER_LOGIN: &str = "SaslAuthenticate request received after successful authentication";

/// Answers a SaslHandshake. A mechanism that is offered moves the session to [`Session::Handshaken`] and is
/// answered with the list; one that is not (a wrong case, an empty name, a mechanism that is off) is answered
/// UNSUPPORTED_SASL_MECHANISM with the same list, and the connection closes.
pub(crate) fn handshake(
    session: &mut Session,
    auth: &Authenticator,
    request: &SaslHandshakeRequest,
) -> (SaslHandshakeResponse, Fate) {
    if !matches!(session, Session::Unauthenticated { .. }) {
        return (illegal_handshake(), Fate::Closing);
    }
    match Mechanism::from_name(&request.mechanism).filter(|&m| auth.offers(m)) {
        Some(mechanism) => {
            *session = Session::Handshaken(mechanism);
            (listing(0, auth), Fate::Open)
        }
        None => (
            listing(ResponseError::UnsupportedSaslMechanism.code(), auth),
            Fate::Closing,
        ),
    }
}

/// Answers a SaslAuthenticate. A right token logs in; a wrong one is answered SASL_AUTHENTICATION_FAILED with
/// Kafka's text for it, and the connection closes.
pub(crate) fn authenticate(
    session: &mut Session,
    auth: &Authenticator,
    request: &SaslAuthenticateRequest,
) -> (SaslAuthenticateResponse, Fate) {
    match session {
        Session::Handshaken(Mechanism::Plain) => match auth.plain(&request.auth_bytes) {
            Ok(principal) => {
                *session = Session::Authenticated(principal);
                (login_succeeded(), Fate::Open)
            }
            Err(refusal) => (failed(&refusal.to_string()), Fate::Closing),
        },
        Session::Handshaken(other) => (
            failed(&format!(
                "Authentication failed during authentication due to invalid credentials with SASL mechanism {other}"
            )),
            Fate::Closing,
        ),
        Session::Authenticated(_) => (after_login(), Fate::Open),
        Session::Unauthenticated { .. } => {
            (error(ResponseError::IllegalSaslState, None), Fate::Closing)
        }
    }
}

/// The answer to a SaslHandshake that comes at the wrong time: ILLEGAL_SASL_STATE and no mechanisms.
pub(crate) fn illegal_handshake() -> SaslHandshakeResponse {
    let mut r = SaslHandshakeResponse::default();
    r.error_code = ResponseError::IllegalSaslState.code();
    r
}

/// The answer to a SaslAuthenticate on a connection that has already logged in: ILLEGAL_SASL_STATE, whoever
/// it names and whatever the token.
pub(crate) fn after_login() -> SaslAuthenticateResponse {
    error(ResponseError::IllegalSaslState, Some(AFTER_LOGIN))
}

fn listing(code: i16, auth: &Authenticator) -> SaslHandshakeResponse {
    let mut r = SaslHandshakeResponse::default();
    r.error_code = code;
    r.mechanisms = auth
        .offered()
        .iter()
        .map(|m| StrBytes::from_static_str(m.name()))
        .collect();
    r
}

/// Success: no error, an empty (not absent) message, no bytes to send back, no session lifetime.
fn login_succeeded() -> SaslAuthenticateResponse {
    let mut r = SaslAuthenticateResponse::default();
    r.error_message = Some(StrBytes::from_static_str(""));
    r.auth_bytes = Bytes::new();
    r
}

fn failed(message: &str) -> SaslAuthenticateResponse {
    error(ResponseError::SaslAuthenticationFailed, Some(message))
}

fn error(code: ResponseError, message: Option<&str>) -> SaslAuthenticateResponse {
    let mut r = SaslAuthenticateResponse::default();
    r.error_code = code.code();
    r.error_message = message.map(|m| StrBytes::from_string(m.to_owned()));
    r
}

#[cfg(test)]
mod tests;
