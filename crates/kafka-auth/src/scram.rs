//! SASL/SCRAM-SHA-256 and SCRAM-SHA-512 (RFC 5802, RFC 7677), as Apache Kafka 4.3.1's `ScramSaslServer`
//! answers them.
//!
//! The server side is two functions and no I/O. [`first`] reads the client-first message and returns the
//! server-first message with an [`Awaiting`]; [`Awaiting::finish`] reads the client-final message and returns the
//! server-final message with the [`Principal`] that logged in. Randomness is an argument, so a test can pin it.
//!
//! A refusal is a [`Refusal`]. Almost every one is answered `SASL_AUTHENTICATION_FAILED` with one generic text,
//! which says nothing of why; the two exceptions are a client that asks for an authorization id that is not its
//! own, which is told so, and a proof that is not the length of a hash, which is not answered at all (Kafka
//! throws an exception it does not catch as a login failure, and the connection just closes).

use std::fmt;
use std::sync::Arc;

use crate::crypto::base64;
use crate::principal::{Mechanism, Principal};

pub mod keys;
pub mod parse;

#[cfg(any(test, feature = "client"))]
pub mod client;

pub use keys::{Kind, Verifier};

/// Why a login was turned down, for the log and the tests. It never reaches the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Why {
    /// The client-first message is not one Kafka's pattern matches.
    BadFirst,
    /// The user has no verifier for this mechanism.
    UnknownUser,
    /// The client asked to authenticate as a delegation token, and there are none.
    TokenAuth,
    /// The client-final message is not one Kafka's pattern matches.
    BadFinal,
    /// The nonce in the client-final message is not the server's.
    WrongNonce,
    /// The proof is not the right one for the password.
    BadProof,
    /// The system gave no random bytes for the server nonce.
    NoEntropy,
}

/// A login that did not succeed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// Answered with Kafka's generic text for a failed `SaslException`, then closed.
    Rejected {
        /// The mechanism of the exchange, which the text names.
        mechanism: Mechanism,
        /// The reason, for the log.
        why: Why,
    },
    /// The first message asked for an authorization id that is not the user's, for a user that exists.
    AuthzId,
    /// The proof was not the length of a hash. Not answered: the connection is closed.
    ProofLength,
}

impl Refusal {
    /// Whether the connection is closed without an answer.
    #[must_use]
    pub fn is_silent(&self) -> bool {
        matches!(self, Self::ProofLength)
    }
}

impl fmt::Display for Refusal {
    /// Exactly what Kafka sends the client; nothing the client sent is in it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rejected { mechanism, .. } => write!(
                f,
                "Authentication failed during authentication due to invalid credentials with SASL mechanism {mechanism}"
            ),
            Self::AuthzId => f.write_str(
                "Authentication failed: Client requested an authorization id that is different from username",
            ),
            Self::ProofLength => f.write_str("the client proof is not the length of the hash"),
        }
    }
}

impl std::error::Error for Refusal {}

/// A login after its first message: the server nonce is out, and the client-final message is next.
pub struct Awaiting {
    verifier: Arc<Verifier>,
    user: String,
    /// Client nonce followed by the server's: the nonce the final message must carry.
    nonce: String,
    /// `client-first-bare "," server-first ","`: the start of the `AuthMessage`.
    auth_message: String,
}

impl fmt::Debug for Awaiting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Awaiting")
            .field("mechanism", &self.verifier.kind().mechanism())
            .finish_non_exhaustive()
    }
}

/// A fresh server nonce: 144 random bits as 24 characters of base64, which are all printable and none a comma.
/// `None` if the system has no random bytes to give.
#[must_use]
pub fn fresh_nonce() -> Option<String> {
    let mut bytes = [0u8; 18];
    getrandom::fill(&mut bytes).ok()?;
    Some(base64::encode(&bytes))
}

/// Reads a client-first `token` for the mechanism of `kind` and answers with the server-first message.
///
/// `lookup` finds the verifier of a user by name; `server_nonce` is appended to the client's. The checks run in
/// Kafka's order, which is observable: the shape of the message, the user (an unknown user is refused here and
/// not after the proof), then the authorization id.
///
/// # Errors
/// A [`Refusal`]: the message is malformed, the user is unknown, or the authorization id is another's.
pub fn first(
    kind: Kind,
    token: &[u8],
    server_nonce: &str,
    lookup: impl FnOnce(&str) -> Option<Arc<Verifier>>,
) -> Result<(Vec<u8>, Awaiting), Refusal> {
    let rejected = |why| Refusal::Rejected {
        mechanism: kind.mechanism(),
        why,
    };
    let message = parse::first(token).ok_or(rejected(Why::BadFirst))?;
    let user = parse::unescape(message.saslname);
    if message.is_token_auth() {
        return Err(rejected(Why::TokenAuth));
    }
    let verifier = lookup(&user)
        .filter(|v| v.kind() == kind)
        .ok_or(rejected(Why::UnknownUser))?;
    // Kafka compares the id as the client wrote it, escapes and all, with the unescaped name.
    if message.authzid.is_some_and(|id| id != user) {
        return Err(Refusal::AuthzId);
    }
    let nonce = format!("{}{server_nonce}", message.nonce);
    let server_first = format!(
        "r={nonce},s={},i={}",
        base64::encode(verifier.salt()),
        verifier.iterations()
    );
    let bare = message.bare();
    let awaiting = Awaiting {
        verifier,
        user,
        nonce,
        auth_message: format!("{bare},{server_first},"),
    };
    Ok((server_first.into_bytes(), awaiting))
}

impl Awaiting {
    /// Reads the client-final `token` and, if the proof is right, answers with the server-final message and the
    /// principal that is now logged in. The login is over either way.
    ///
    /// # Errors
    /// A [`Refusal`]: a malformed message, another nonce, a proof the wrong length (silent) or the wrong proof.
    pub fn finish(self, token: &[u8]) -> Result<(Vec<u8>, Principal), Refusal> {
        let rejected = |why| Refusal::Rejected {
            mechanism: self.verifier.kind().mechanism(),
            why,
        };
        let message = parse::last(token).ok_or(rejected(Why::BadFinal))?;
        if message.nonce != self.nonce {
            return Err(rejected(Why::WrongNonce));
        }
        // The binding is decoded and written back, never compared with the first message (Kafka does not).
        let auth_message = format!(
            "{}c={},r={}",
            self.auth_message,
            base64::encode(&message.channel),
            self.nonce
        );
        if message.proof.len() != self.verifier.kind().hash_len() {
            return Err(Refusal::ProofLength);
        }
        if !self
            .verifier
            .proof_is_right(auth_message.as_bytes(), &message.proof)
        {
            return Err(rejected(Why::BadProof));
        }
        let signature = self.verifier.server_signature(auth_message.as_bytes());
        let reply = format!("v={}", base64::encode(&signature)).into_bytes();
        Ok((reply, Principal::user(&self.user)))
    }
}

#[cfg(test)]
mod tests;
