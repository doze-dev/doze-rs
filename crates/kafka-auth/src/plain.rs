//! SASL/PLAIN (RFC 4616), as Apache Kafka's `PlainSaslServer` answers it.
//!
//! The token is `authzid NUL user NUL password`. The checks run in the order the recordings show, because the
//! order is observable: the number of parts, then an empty user, then an empty password, then the password
//! itself (compared in constant time, and compared for an unknown user too, so a wrong user and a wrong
//! password take the same time and read the same), and only then the authorization id. A client that names
//! someone else's authzid but has the wrong password is told about the password, not the authzid.

use std::fmt;

use crate::credentials::{Credentials, Secret};
use crate::crypto;
use crate::principal::Principal;

/// Why a PLAIN login was refused. The `Display` text is exactly what Kafka sends the client, and never holds
/// anything the client sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// The token did not split into three parts at the NUL bytes; `got` counts the parts found, up to four.
    Tokens {
        /// How many parts there were.
        got: usize,
    },
    /// The user part is empty.
    NoUser,
    /// The password part is empty.
    NoPassword,
    /// The user is unknown, or the password is not its password. One refusal for both, deliberately.
    BadCredentials,
    /// The password was right, but the authorization id names someone other than the user.
    AuthzId,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tokens { got } => {
                write!(f, "Invalid SASL/PLAIN response: expected 3 tokens, got {got}")
            }
            Self::NoUser => f.write_str("Authentication failed: username not specified"),
            Self::NoPassword => f.write_str("Authentication failed: password not specified"),
            Self::BadCredentials => f.write_str("Authentication failed: Invalid username or password"),
            Self::AuthzId => f.write_str(
                "Authentication failed: Client requested an authorization id that is different from username",
            ),
        }
    }
}

impl std::error::Error for Refusal {}

// fuzz:parser plain
/// Checks a PLAIN `token` against `credentials`; the principal it logs in is `User:<user>`.
///
/// The token is bytes, not text: a client may send anything, and non-UTF-8 bytes are simply a user or password
/// that matches nobody.
///
/// # Errors
/// The first check the token fails, as a [`Refusal`].
pub fn check(token: &[u8], credentials: &Credentials) -> Result<Principal, Refusal> {
    let parts: Vec<&[u8]> = token.splitn(4, |&b| b == 0).collect();
    let [authzid, user, password] = parts.as_slice() else {
        return Err(Refusal::Tokens { got: parts.len() });
    };
    if user.is_empty() {
        return Err(Refusal::NoUser);
    }
    if password.is_empty() {
        return Err(Refusal::NoPassword);
    }
    if !password_is_right(credentials, user, password) {
        return Err(Refusal::BadCredentials);
    }
    if !authzid.is_empty() && authzid != user {
        return Err(Refusal::AuthzId);
    }
    Ok(Principal::user(&String::from_utf8_lossy(user)))
}

/// Whether `password` is the password of `user`. An unknown user is compared against the password itself, so
/// the comparison is made, and takes the same time, whether or not the user exists; the answer is then
/// "no" whatever it came to.
fn password_is_right(credentials: &Credentials, user: &[u8], password: &[u8]) -> bool {
    let known = credentials.password(user);
    let expected = known.map_or(password, Secret::expose);
    let same = crypto::ct_eq(password, expected);
    same & known.is_some()
}

#[cfg(test)]
mod tests;
