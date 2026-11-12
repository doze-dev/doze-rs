//! The two SASL requests, answered: pure functions from a session and a request to the exact response Kafka
//! gives, and what happens to the connection after it.
//!
//! Nothing the client sent is ever put in an answer or a log: the texts are Kafka's own (the PLAIN and SCRAM ones
//! come from `kafka-auth`), and the mechanism list is the server's.

use bytes::Bytes;
use kafka_auth::scram::{self, Awaiting};
use kafka_auth::{Authenticator, Mechanism, Principal};
use kafka_wire::ResponseError;
use kafka_wire::StrBytes;
use kafka_wire::messages::{
    SaslAuthenticateRequest, SaslAuthenticateResponse, SaslHandshakeRequest, SaslHandshakeResponse,
};
use tracing::debug;

use super::Session;

/// What becomes of the connection once the answer is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fate {
    /// It carries on.
    Open,
    /// It is closed, after the answer has been written and flushed: a client is told why it failed.
    Closing,
    /// As [`Fate::Closing`], and the answer (SASL_AUTHENTICATION_FAILED) is held back for the failed-login delay first
    /// (SEC-192).
    Failed,
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

/// A login that is over without an answer: the connection is closed and nothing is written. Kafka does this for
/// a SCRAM proof of the wrong length, where its code throws an exception that is not a failed login.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hangup;

/// Answers a SaslAuthenticate. A right token logs in (or, for the first message of SCRAM, is answered with the
/// challenge); a wrong one is answered SASL_AUTHENTICATION_FAILED with Kafka's text for it, and the connection
/// closes.
///
/// # Errors
/// [`Hangup`]: the connection is to be closed with no answer.
pub(crate) fn authenticate(
    session: &mut Session,
    auth: &Authenticator,
    request: &SaslAuthenticateRequest,
) -> Result<(SaslAuthenticateResponse, Fate), Hangup> {
    let token = &request.auth_bytes;
    if let Some(awaiting) = take_challenge(session) {
        return match awaiting.finish(token) {
            Ok((signature, principal)) => Ok(logged_in(session, principal, signature)),
            Err(refusal) => refused(&refusal),
        };
    }
    match session {
        Session::Handshaken(Mechanism::Plain) => match auth.plain(token) {
            Ok(principal) => Ok(logged_in(session, principal, Vec::new())),
            Err(refusal) => Ok((failed(&refusal.to_string()), Fate::Failed)),
        },
        Session::Handshaken(mechanism) => match auth.scram_first(*mechanism, token) {
            Ok((challenge, awaiting)) => {
                *session = Session::Challenged(awaiting);
                Ok((succeeded(challenge), Fate::Open))
            }
            Err(refusal) => refused(&refusal),
        },
        Session::Authenticated(_) => Ok((after_login(), Fate::Open)),
        Session::Unauthenticated { .. } | Session::Challenged(_) => {
            Ok((error(ResponseError::IllegalSaslState, None), Fate::Closing))
        }
    }
}

/// The SCRAM exchange in flight, taken out of the session (which is left as it was if there is none).
fn take_challenge(session: &mut Session) -> Option<Awaiting> {
    match std::mem::replace(session, Session::new()) {
        Session::Challenged(awaiting) => Some(awaiting),
        other => {
            *session = other;
            None
        }
    }
}

fn logged_in(
    session: &mut Session,
    principal: Principal,
    reply: Vec<u8>,
) -> (SaslAuthenticateResponse, Fate) {
    *session = Session::Authenticated(principal);
    (succeeded(reply), Fate::Open)
}

/// A SCRAM refusal: the generic answer then a close, or, for a proof the wrong length, a close alone. The reason
/// goes to the debug log as an enum name; nothing the client sent does.
fn refused(refusal: &scram::Refusal) -> Result<(SaslAuthenticateResponse, Fate), Hangup> {
    debug!(reason = ?refusal, "SCRAM login refused");
    if refusal.is_silent() {
        return Err(Hangup);
    }
    Ok((failed(&refusal.to_string()), Fate::Failed))
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

/// Success: no error, an empty (not absent) message, the mechanism's reply (none for PLAIN), no session lifetime.
fn succeeded(reply: Vec<u8>) -> SaslAuthenticateResponse {
    let mut r = SaslAuthenticateResponse::default();
    r.error_message = Some(StrBytes::from_static_str(""));
    r.auth_bytes = Bytes::from(reply);
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
